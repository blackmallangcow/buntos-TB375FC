// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 세션: 셸 프로세스, 로그아웃, 응용 프로그램 강제 종료, logind 신호(화면 잠금, 잠자기).
//!
//! - 셸이 끝나면 [session] exit-with-shell일 때 컴포지터도 끝낸다. 로그인 화면은 bunt-greeter가 끝나야 greetd가
//!   예약한 사용자 세션을 연다.
//! - 로그아웃(macOS의 "<이름> 로그아웃…"): 셸의 확인 창(60초 뒤 자동)이나 ⌥⇧⌘Q로 시작한다. 모든 앱 창에 닫기를
//!   요청하고, 다 닫히면 컴포지터가 끝나 세션이 끝난다(greetd가 로그인 화면을 다시 띄운다). 저장하지 않은
//!   문서를 묻는 앱처럼 LOGOUT_WAIT 안에 닫히지 않는 창이 있으면 그만두고 알린다(macOS도 앱이 로그아웃을 멈춘다).
//! - 응용 프로그램 강제 종료(⌥⌘⎋): 셸이 창을 가진 앱 목록과 응답 여부를 받고(GetApps) 고른 앱을 끝낸다
//!   (ForceQuit, SIGKILL). 응답 없는 앱은 핑(xdg_wm_base.ping)에 NOT_RESPONDING 넘게 답하지 않은 앱이다.
//! - 셸 전용 D-Bus 메서드(로그아웃, 앱 목록, 강제 종료)는 컴포지터가 띄운 셸 프로세스만 부른다(dbus.rs).

use std::{
    cell::Cell,
    error::Error,
    process::ExitStatus,
    sync::mpsc,
    time::{Duration, Instant},
};

use bunt_i18n::{FluentArgs, Sources, Translations};
use smithay::{
    desktop::Window,
    reexports::{
        calloop::{
            LoopHandle, channel,
            timer::{TimeoutAction, Timer},
        },
        wayland_server::{Resource, backend::ClientId},
    },
    utils::SERIAL_COUNTER,
    wayland::{
        compositor::with_states,
        shell::{
            PingError,
            xdg::{ShellClient, ToplevelSurface, XdgToplevelSurfaceData},
        },
    },
};

use crate::{
    config::Config,
    dbus::Command,
    logind::{self, Logind},
    state::{BuntState, ClientState},
};

/// 로그아웃할 때 앱 창이 닫히기를 기다리는 시간. 넘으면 로그아웃을 그만둔다
const LOGOUT_WAIT: Duration = Duration::from_secs(10);
/// 창이 다 닫혔는지 다시 보는 간격
const LOGOUT_STEP: Duration = Duration::from_millis(200);
/// 핑에 이만큼 답하지 않은 앱은 응답하지 않는 것으로 본다 (macOS도 몇 초 뒤 "응답 없음"으로 보인다)
const NOT_RESPONDING: Duration = Duration::from_secs(3);
/// 로그아웃 취소 알림
const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../i18n/ko/bunt-comp.ftl")),
    ("en", include_str!("../../i18n/en/bunt-comp.ftl")),
];

/// 세션 상태 (BuntState::session)
pub struct Session {
    /// 컴포지터가 띄운 셸의 pid
    shell: Option<u32>,
    /// 로그아웃을 시작한 때
    logging_out: Option<Instant>,
    /// 화면을 잠그는 세션만 logind를 듣는다 (로그인 화면은 듣지 않는다)
    logind: Option<Logind>,
}

impl Session {
    pub fn new(
        loop_handle: &LoopHandle<'static, BuntState>,
        config: &Config,
    ) -> Result<Self, Box<dyn Error>> {
        let logind = if config.session.lock.is_empty() {
            None
        } else {
            let (events, receiver) = channel::channel();
            loop_handle
                .insert_source(receiver, |event, _, state| {
                    if let channel::Event::Msg(event) = event {
                        state.logind_event(event);
                    }
                })
                .map_err(|err| err.error)?;
            Some(Logind::start(events))
        };
        Ok(Self {
            shell: None,
            logging_out: None,
            logind,
        })
    }

    /// 세션의 LockedHint를 고친다 (session_lock.rs)
    pub fn set_locked_hint(&self, locked: bool) {
        if let Some(logind) = &self.logind {
            logind.set_locked_hint(locked);
        }
    }

    /// 잠긴 장면을 낸 뒤 잠자기를 놓는다 (session_lock.rs)
    pub fn release_sleep_inhibitor(&self) {
        if let Some(logind) = &self.logind {
            logind.release_sleep_inhibitor();
        }
    }
}

/// D-Bus에서 이벤트 루프로 넘기는 세션 요청 (dbus.rs의 Request::Session)
#[derive(Debug)]
pub enum SessionRequest {
    /// 화면을 잠근다 (Lock)
    Lock,
    /// 셸의 확인 창에서 로그아웃을 골랐다 (LogOut)
    LogOut,
    /// 창을 가진 앱 (GetApps)
    Apps(mpsc::Sender<Vec<RunningApp>>),
    /// 그 pid의 앱을 강제 종료한다 (ForceQuit)
    ForceQuit {
        pid: u32,
        reply: mpsc::Sender<Result<(), String>>,
    },
}

/// 응용 프로그램 강제 종료 창에 보이는 앱 하나 (GetApps)
#[derive(Clone, Debug, PartialEq)]
pub struct RunningApp {
    pub pid: u32,
    pub app_id: String,
    /// 맨 위 창의 제목. 셸이 데스크톱 항목을 찾지 못하면 이름으로 쓴다
    pub title: String,
    /// 핑에 제때 답한다. 아니면 "응답 없음"이다
    pub responding: bool,
}

/// 앱에 마지막으로 보낸 핑의 때 (그 앱의 xdg_wm_base 데이터)
struct PingSent(Cell<Option<Instant>>);

impl BuntState {
    /// 셸을 특권 클라이언트로 띄운다. 이 연결에만 창 목록·창 조작 프로토콜이 보인다(toplevels.rs). 셸이 끝나면
    /// [session] exit-with-shell을 따른다. 전에는 state.rs의 spawn_privileged가 셸을 띄우고 끝나기만 기다렸다
    pub fn spawn_shell(&mut self) {
        let command = self.config.session.shell.clone();
        if command.is_empty() {
            return;
        }
        let client = ClientState {
            privileged: true,
            ..ClientState::default()
        };
        let Some(mut child) = self.spawn_trusted(&command, client) else {
            self.shell_exited(None);
            return;
        };
        let pid = child.id();
        self.session.shell = Some(pid);
        self.dbus.update(|snapshot| snapshot.shell_pid = pid);
        let (exited, receiver) = channel::channel();
        let source = self.loop_handle.insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(status) = event {
                state.shell_exited(status);
            }
        });
        if let Err(err) = source {
            tracing::warn!("failed to watch the shell process: {}", err.error);
        }
        std::thread::spawn(move || {
            let status = child.wait().ok();
            let _ = exited.send(status);
        });
    }

    /// 셸이 끝났거나 뜨지 못했다(`status`가 None)
    fn shell_exited(&mut self, status: Option<ExitStatus>) {
        self.session.shell = None;
        self.dbus.update(|snapshot| snapshot.shell_pid = 0);
        let status = status.map_or_else(|| "did not start".to_owned(), |status| status.to_string());
        if self.config.session.exit_with_shell {
            tracing::info!("the shell ended ({status}); ending the compositor");
            self.loop_signal.stop();
        } else {
            tracing::warn!("the shell ended ({status})");
        }
    }

    pub fn session_request(&mut self, request: SessionRequest) {
        match request {
            SessionRequest::Lock => self.lock_screen("asked over D-Bus"),
            SessionRequest::LogOut => self.log_out(),
            SessionRequest::Apps(reply) => {
                let _ = reply.send(self.running_apps());
            }
            SessionRequest::ForceQuit { pid, reply } => {
                let _ = reply.send(self.force_quit(pid));
            }
        }
    }

    fn logind_event(&mut self, event: logind::Event) {
        match event {
            logind::Event::Lock => self.lock_screen("logind asked to lock the session"),
            logind::Event::PrepareForSleep(true) => self.lock_before_sleep(),
            logind::Event::PrepareForSleep(false) => {
                if let Some(logind) = &self.session.logind {
                    logind.take_sleep_inhibitor();
                }
            }
        }
    }

    /// 로그아웃: 모든 앱 창에 닫기를 요청하고, 다 닫히면 컴포지터를 끝낸다. 잠긴 동안에는 하지 않는다
    pub fn log_out(&mut self) {
        if self.session.logging_out.is_some() {
            return;
        }
        if self.session_lock.is_locked() {
            tracing::warn!("refused to log out while the screen is locked");
            return;
        }
        tracing::info!("logging out: asking every window to close");
        self.session.logging_out = Some(Instant::now());
        for window in self.all_windows() {
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_close();
            }
        }
        let timer = self
            .loop_handle
            .insert_source(Timer::immediate(), |_, _, state| state.logout_step());
        if let Err(err) = timer {
            tracing::warn!("failed to watch the windows close: {}", err.error);
            self.session.logging_out = None;
        }
    }

    fn logout_step(&mut self) -> TimeoutAction {
        let Some(started) = self.session.logging_out else {
            return TimeoutAction::Drop;
        };
        let Some(window) = self.all_windows().next().cloned() else {
            tracing::info!("every window closed; ending the session");
            self.loop_signal.stop();
            return TimeoutAction::Drop;
        };
        if started.elapsed() < LOGOUT_WAIT {
            return TimeoutAction::ToDuration(LOGOUT_STEP);
        }
        // 닫히지 않은 창이 있다(저장하지 않은 문서를 묻는 앱 등). 그만두고 그 창을 알린다
        self.session.logging_out = None;
        let name = window.toplevel().map(window_name).unwrap_or_default();
        tracing::warn!("logout cancelled: {name:?} did not close");
        let tr = Translations::load(TRANSLATIONS);
        let mut args = FluentArgs::new();
        args.set("app", name);
        self.dbus.send(Command::Notify {
            title: tr.get("logout-cancelled-title"),
            body: tr.get_with("logout-cancelled-body", &args),
        });
        TimeoutAction::Drop
    }

    /// 창을 가진 앱. 맨 위 창이 앞이다. 부를 때마다 앱에 핑을 보내 응답 여부를 잰다
    pub fn running_apps(&self) -> Vec<RunningApp> {
        let mut apps: Vec<(ClientId, RunningApp)> = Vec::new();
        let windows = self
            .space
            .elements()
            .rev()
            .chain(self.hidden.iter().map(|hidden| &hidden.window));
        for window in windows {
            let (Some((client, pid)), Some(toplevel)) =
                (self.app_client(window), window.toplevel())
            else {
                continue;
            };
            if apps.iter().any(|(known, _)| *known == client) {
                continue;
            }
            let (title, app_id) = title_and_app_id(toplevel);
            apps.push((
                client,
                RunningApp {
                    pid,
                    app_id,
                    title,
                    responding: responding(&toplevel.client()),
                },
            ));
        }
        apps.into_iter().map(|(_, app)| app).collect()
    }

    /// 창의 클라이언트가 앱이면 (클라이언트, pid). 컴포지터가 띄운 셸, 입력기, 잠금 화면은 앱이 아니다
    fn app_client(&self, window: &Window) -> Option<(ClientId, u32)> {
        let toplevel = window.toplevel()?;
        let client = self
            .display_handle
            .get_client(toplevel.wl_surface().id())
            .ok()?;
        let trusted = client
            .get_data::<ClientState>()
            .is_some_and(|data| data.privileged || data.input_method || data.lock_screen);
        if trusted {
            return None;
        }
        let pid = client.get_credentials(&self.display_handle).ok()?.pid;
        u32::try_from(pid)
            .ok()
            .filter(|pid| *pid > 0)
            .map(|pid| (client.id(), pid))
    }

    /// 그 pid의 앱을 강제 종료한다(macOS의 강제 종료처럼 SIGKILL). 창을 가진 앱의 pid만 받는다: 셸이 다른
    /// 프로세스(컴포지터, 사용자 서비스)를 끝내지 못하게 한다
    pub fn force_quit(&mut self, pid: u32) -> Result<(), String> {
        let known = self
            .all_windows()
            .any(|window| self.app_client(window).is_some_and(|(_, app)| app == pid));
        if !known {
            return Err(format!("no app with windows has pid {pid}"));
        }
        let Ok(target) = libc::pid_t::try_from(pid) else {
            return Err(format!("pid {pid} is out of range"));
        };
        tracing::info!("force quitting pid {pid}");
        // 안전: 지금 창을 가진 채 연결된 클라이언트의 pid다(SO_PEERCRED). 연결이 살아 있으므로 끝난 프로세스의
        // 번호가 다른 프로세스에 다시 쓰였을 틈은 거의 없다
        if unsafe { libc::kill(target, libc::SIGKILL) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error().to_string())
        }
    }
}

/// 창의 (제목, 앱 ID)
fn title_and_app_id(toplevel: &ToplevelSurface) -> (String, String) {
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .map(|data| {
                let data = data.lock().unwrap();
                (
                    data.title.clone().unwrap_or_default(),
                    data.app_id.clone().unwrap_or_default(),
                )
            })
            .unwrap_or_default()
    })
}

/// 알림에 쓰는 창 이름: 제목, 없으면 앱 ID
fn window_name(toplevel: &ToplevelSurface) -> String {
    let (title, app_id) = title_and_app_id(toplevel);
    if title.is_empty() { app_id } else { title }
}

/// 앱이 핑에 답하는지. 지난 핑에 답했으면 새 핑을 보내고 답하는 것으로 본다. 지난 핑이 아직 답을 기다리면 보낸 지
/// NOT_RESPONDING이 지났는지 본다
fn responding(client: &ShellClient) -> bool {
    let now = Instant::now();
    match client.send_ping(SERIAL_COUNTER.next_serial()) {
        Ok(()) => {
            let _ = client.with_data(|data| {
                data.insert_if_missing(|| PingSent(Cell::new(None)));
                if let Some(sent) = data.get::<PingSent>() {
                    sent.0.set(Some(now));
                }
            });
            true
        }
        Err(PingError::PingAlreadyPending(_)) => client
            .with_data(|data| data.get::<PingSent>().and_then(|sent| sent.0.get()))
            .ok()
            .flatten()
            .is_none_or(|sent| answered_in_time(sent, now)),
        Err(_) => true,
    }
}

/// `sent`에 보낸 핑을 `now`까지 기다린 것이 아직 NOT_RESPONDING 안인지
fn answered_in_time(sent: Instant, now: Instant) -> bool {
    now.saturating_duration_since(sent) < NOT_RESPONDING
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_stop_responding_after_a_few_seconds() {
        let sent = Instant::now();
        assert!(answered_in_time(sent, sent));
        assert!(answered_in_time(sent, sent + Duration::from_millis(2900)));
        assert!(!answered_in_time(sent, sent + NOT_RESPONDING));
        // 시계가 거꾸로 가도 응답 없음으로 보지 않는다
        assert!(answered_in_time(sent + Duration::from_secs(1), sent));
    }

    #[test]
    fn logout_cancel_messages_exist_in_every_language() {
        for (language, source) in TRANSLATIONS {
            let tr = Translations::from_source(language, source);
            let mut args = FluentArgs::new();
            args.set("app", "TextEdit");
            assert_ne!(tr.get("logout-cancelled-title"), "logout-cancelled-title");
            let body = tr.get_with("logout-cancelled-body", &args);
            assert!(body.contains("TextEdit"), "{language}: {body}");
        }
    }
}
