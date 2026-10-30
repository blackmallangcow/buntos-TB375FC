// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 잠금 (ext-session-lock-v1). macOS의 화면 잠금(⌃⌘Q)처럼 앱과 셸을 가리고, 사람이 암호를 칠 때까지
//! 아무것도 보이거나 받지 않는다.
//!
//! - 잠그는 길: ⌃⌘Q, 로고 메뉴의 화면 잠금(D-Bus Lock), logind의 Lock 신호(loginctl lock-session), 잠자기 직전
//!   (session.rs). 컴포지터가 먼저 잠긴 상태가 되어 그 순간부터 앱 창, 셸, 배경화면을 그리지 않고 입력도 보내지
//!   않는다. 그다음 잠금 화면(comp.toml의 [session] lock, bunt-lock)을 띄운다.
//! - 잠금 프로토콜은 컴포지터가 띄운 잠금 화면의 연결에만 보인다. 같은 사용자로 도는 다른 프로그램(AI 도구 포함)이
//!   잠금 화면을 죽인 뒤 스스로 잠갔다 풀어 암호 없이 들어오지 못하게 한다.
//! - 푸는 길은 잠금 화면이 사람이 친 암호를 PAM으로 확인하고 unlock_and_destroy를 보내는 것뿐이다. 컴포지터의
//!   D-Bus에는 푸는 메서드가 없고, logind의 Unlock 신호도 듣지 않는다.
//! - 잠금 화면이 죽어도 화면은 잠긴 채로 남는다(프로토콜 규칙). 컴포지터가 잠금 화면을 다시 띄운다.
//! - 잠긴 동안 화면마다 그 화면의 잠금 표면과 검은 바탕만 그린다. 키보드는 잠금 표면에만, 포인터는 그 아래의
//!   잠금 표면에만 간다. 컴포지터 단축키는 가상 터미널 전환, 입력 소스 전환, 세션 끝내기만 남는다.
//! - 에이전트의 요청은 모두 거절한다(agent.rs). 잠긴 상태는 D-Bus(GetLocked, LockedChanged)와 logind의
//!   LockedHint로 알린다.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use smithay::{
    backend::renderer::{
        element::{
            Kind,
            solid::{SolidColorBuffer, SolidColorRenderElement},
            surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
        },
        gles::GlesRenderer,
        utils::with_renderer_surface_state,
    },
    desktop::{
        WindowSurfaceType,
        utils::{send_frames_surface_tree, under_from_surface_tree},
    },
    output::Output,
    reexports::{
        calloop::{
            RegistrationToken, channel,
            timer::{TimeoutAction, Timer},
        },
        wayland_protocols::ext::session_lock::v1::server::{
            ext_session_lock_manager_v1::ExtSessionLockManagerV1,
            ext_session_lock_surface_v1::ExtSessionLockSurfaceV1,
            ext_session_lock_v1::{self, ExtSessionLockV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, Resource,
            backend::ClientId,
            delegate_dispatch, delegate_global_dispatch,
            protocol::{wl_output::WlOutput, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, SERIAL_COUNTER, Scale, Size},
    wayland::{
        input_method::InputMethodSeat,
        session_lock::{
            ExtLockSurfaceUserData, LockSurface, SessionLockHandler, SessionLockManagerGlobalData,
            SessionLockManagerState, SessionLockState, SessionLocker,
        },
    },
};

use crate::{
    dbus::Command,
    render::OutputRenderElements,
    shortcuts::{Builtin, Hit},
    state::{BuntState, ClientState},
};

/// 잠금 표면이 아직 없거나 비치는 곳의 바탕
const BACKDROP: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// 잠긴 동안 잠금 화면이 끝나면 다시 띄우기까지 기다리는 시간. 거듭 끝나면 두 배씩 늘리되 상한까지만 늘린다
const RESTART_DELAY: Duration = Duration::from_millis(500);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(10);
/// 이만큼 돈 뒤에 끝났으면 기다리는 시간을 처음 값으로 되돌린다
const STABLE_RUN: Duration = Duration::from_secs(60);
/// 띄운 잠금 화면이 이 안에 잠금 표면을 보이지 않으면 멈춘 것으로 보고 끝낸 뒤 다시 띄운다
const START_TIMEOUT: Duration = Duration::from_secs(20);
/// 잠자기 직전 잠긴 장면을 그리지 못해도 이만큼 뒤에는 잠자기를 놓는다. logind는 늦어도 InhibitDelayMaxSec
/// (기본 5초) 뒤에는 잠든다
const SLEEP_WAIT: Duration = Duration::from_secs(3);

/// 화면 잠금 상태 (BuntState::session_lock)
pub struct SessionLock {
    manager: SessionLockManagerState,
    /// 잠긴 동안 Some
    locked: Option<Locked>,
    client: LockClient,
    /// 화면(출력 이름)마다 검은 바탕. 요소 ID가 프레임마다 같아야 다시 그릴 손상이 생기지 않는다
    backdrops: HashMap<String, SolidColorBuffer>,
    /// 잠긴 장면을 모든 화면에 낸 뒤 logind의 잠자기 지연 잠금을 놓는다
    release_sleep: bool,
}

struct Locked {
    since: Instant,
    /// 잠금 화면이 보낸 잠금 객체. 잠금 화면이 다시 뜨면 새 것으로 바뀐다
    lock: Option<ExtSessionLockV1>,
    /// 이 잠금 객체에 잠겼다고 알렸다(locked). 알린 잠금 객체만 잠금을 풀 수 있다
    confirmed: bool,
    /// 잠긴 장면을 모든 화면에 낸 뒤에 보낼 잠금 확인
    pending: Option<SessionLocker>,
    /// 화면마다 잠금 표면
    surfaces: Vec<(Output, LockSurface)>,
    /// 잠긴 장면을 내보낸 화면 (출력 이름)
    blanked: HashSet<String>,
    /// 이번 프레임에 잠긴 장면을 그리는 화면
    drawing: HashSet<String>,
    /// 잠그기 전의 키보드 포커스. 풀면 돌려준다
    previous_focus: Option<WlSurface>,
}

impl Locked {
    /// 그 화면의 살아 있는 잠금 표면
    fn surface_on(&self, output: &Output) -> Option<&LockSurface> {
        self.surfaces
            .iter()
            .find(|(on, surface)| on == output && surface.alive())
            .map(|(_, surface)| surface)
    }
}

/// 잠금 화면 프로세스
#[derive(Default)]
struct LockClient {
    /// 떠 있는 잠금 화면의 (pid, 세대)
    running: Option<(u32, u64)>,
    generation: u64,
    started: Option<Instant>,
    /// 잠긴 동안 거듭 끝난 횟수. 다시 띄우기까지 기다리는 시간을 정한다
    failures: u32,
    restart: Option<RegistrationToken>,
    /// 잠금 화면이 끝나면 세대를 보낸다
    exited: Option<channel::Sender<u64>>,
}

impl SessionLock {
    /// 잠금 프로토콜은 컴포지터가 띄운 잠금 화면(ClientState::lock_screen)에만 보인다
    pub fn new(display: &DisplayHandle) -> Self {
        Self {
            manager: SessionLockManagerState::new::<BuntState, _>(display, |client| {
                client
                    .get_data::<ClientState>()
                    .is_some_and(|data| data.lock_screen)
            }),
            locked: None,
            client: LockClient::default(),
            backdrops: HashMap::new(),
            release_sleep: false,
        }
    }

    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// 잠겼다고 알린 지금의 잠금 객체인지
    fn holds_screen(&self, lock: &ExtSessionLockV1) -> bool {
        self.locked
            .as_ref()
            .is_some_and(|locked| locked.confirmed && locked.lock.as_ref() == Some(lock))
    }

    /// 버퍼를 붙여 보이는 잠금 표면이 있는지
    fn has_visible_surface(&self) -> bool {
        self.locked.as_ref().is_some_and(|locked| {
            locked.surfaces.iter().any(|(_, surface)| {
                surface.alive()
                    && with_renderer_surface_state(surface.wl_surface(), |state| {
                        state.buffer().is_some()
                    })
                    .unwrap_or(false)
            })
        })
    }

    /// 잠긴 동안 그 화면에 그릴 것 (udev.rs의 render_output): 잠금 표면과 그 뒤의 검은 바탕. 잠기지 않았으면
    /// None이고 평소대로 그린다. `size`는 그 화면의 논리 크기다
    pub fn scene(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &Output,
        size: Size<i32, Logical>,
        scale: Scale<f64>,
    ) -> Option<Vec<OutputRenderElements>> {
        let locked = self.locked.as_mut()?;
        let mut elements = Vec::new();
        if let Some(surface) = locked.surface_on(output) {
            // 화면 크기나 배율이 바뀌면 새 크기를 알린다. 알린 크기와 같으면 보내지 않는다
            surface.with_pending_state(|state| state.size = Some(lock_size(size)));
            surface.send_configure();
            elements.extend(
                render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
                    renderer,
                    surface.wl_surface(),
                    (0, 0),
                    scale,
                    1.0,
                    Kind::Unspecified,
                )
                .into_iter()
                .map(OutputRenderElements::Surface),
            );
        }
        let backdrop = self
            .backdrops
            .entry(output.name())
            .or_insert_with(|| SolidColorBuffer::new(size, BACKDROP));
        backdrop.update(size, BACKDROP);
        elements.push(OutputRenderElements::Solid(
            SolidColorRenderElement::from_buffer(backdrop, (0, 0), scale, 1.0, Kind::Unspecified),
        ));
        locked.drawing.insert(output.name());
        Some(elements)
    }

    /// 그 화면의 잠금 표면에 프레임 알림을 보낸다 (udev.rs의 render_output)
    pub fn send_frames(&self, output: &Output, time: Duration) {
        let Some(surface) = self
            .locked
            .as_ref()
            .and_then(|locked| locked.surface_on(output))
        else {
            return;
        };
        send_frames_surface_tree(surface.wl_surface(), output, time, None, |_, _| {
            Some(output.clone())
        });
    }
}

/// 잠금 표면의 크기: 화면의 논리 크기
fn lock_size(size: Size<i32, Logical>) -> Size<u32, Logical> {
    Size::from((size.w.max(1) as u32, size.h.max(1) as u32))
}

/// 잠긴 동안에도 컴포지터가 하는 일: 입력 소스 전환, 가상 터미널 전환, 세션 끝내기(Ctrl+Alt+Backspace).
/// 세션 끝내기는 잠금을 푸는 것이 아니라 저장하지 않은 앱과 함께 세션을 끝내고 로그인 화면으로 가는 것이다.
/// 잠금 화면이 멈추거나 거듭 죽을 때 사람이 빠져나갈 길로 남긴다
pub fn runs_while_locked(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::SwitchInputSource | Builtin::SwitchVt(_) | Builtin::Quit
    )
}

impl BuntState {
    /// 화면을 잠근다. `why`는 기록에 남기는 까닭이다. 잠금 화면이 설정에 없으면(로그인 화면) 잠그지 않는다
    pub fn lock_screen(&mut self, why: &str) {
        if self.config.session.lock.is_empty() {
            tracing::info!("no lock screen is configured; not locking ({why})");
            return;
        }
        if self.session_lock.locked.is_none() {
            tracing::info!("locking the screen ({why})");
            self.enter_locked();
        }
        self.spawn_lock_client();
    }

    /// 잠자기 직전 (logind PrepareForSleep): 잠그고, 잠긴 장면을 모든 화면에 낸 뒤 잠자기를 늦추던 logind 잠금을
    /// 놓는다. 깨어난 화면에 잠그기 전의 앱이 보이지 않게 한다
    pub fn lock_before_sleep(&mut self) {
        self.lock_screen("going to sleep");
        if !self.session_lock.is_locked() {
            self.session.release_sleep_inhibitor();
            return;
        }
        self.session_lock.release_sleep = true;
        self.confirm_lock_if_blanked();
        // 화면을 그리지 못하는 동안(다른 가상 터미널에 있을 때 등)에도 잠자기를 너무 늦추지 않는다
        let timer =
            self.loop_handle
                .insert_source(Timer::from_duration(SLEEP_WAIT), |_, _, state| {
                    if std::mem::take(&mut state.session_lock.release_sleep) {
                        tracing::warn!("could not show the locked screen before sleeping");
                        state.session.release_sleep_inhibitor();
                    }
                    TimeoutAction::Drop
                });
        if let Err(err) = timer {
            tracing::warn!("failed to schedule the sleep wait: {}", err.error);
        }
    }

    /// 컴포지터 쪽 잠금: 하던 일(앱 전환, 스크린샷 영역, 메뉴, 창 끌기)을 그만두고 키보드와 포인터를 앱에서 뗀다.
    /// 이 순간부터 앱 창과 셸을 그리지 않는다
    fn enter_locked(&mut self) {
        self.finish_switching();
        self.cancel_screenshot();
        self.screenshot_capture = None;
        // 잠금 화면(암호를 치는 화면)은 기록하지 않는다 (recording.rs)
        self.stop_recording();
        self.stop_key_repeat();
        let serial = SERIAL_COUNTER.next_serial();
        let time = self.start_time.elapsed().as_millis() as u32;
        let keyboard = self.seat.get_keyboard();
        let previous_focus = keyboard
            .as_ref()
            .and_then(|keyboard| keyboard.current_focus());
        // 메뉴(팝업)의 그랩과 창 끌기를 푼다. 팝업 그랩은 풀면서 키보드를 메뉴의 주인 표면에 돌려주므로 포커스를
        // 비우기 전에 푼다. 입력기(fcitx5)가 잡은 키보드는 그대로 둔다: 포커스가 잠금 표면으로 옮겨 가면 입력기가 놓는다
        if let Some(keyboard) = &keyboard
            && keyboard.is_grabbed()
            && !self.seat.input_method().keyboard_grabbed()
        {
            keyboard.unset_grab(self);
        }
        if let Some(pointer) = self.seat.get_pointer()
            && pointer.is_grabbed()
        {
            pointer.unset_grab(self, serial, time);
        }
        self.session_lock.locked = Some(Locked {
            since: Instant::now(),
            lock: None,
            confirmed: false,
            pending: None,
            surfaces: Vec::new(),
            blanked: HashSet::new(),
            drawing: HashSet::new(),
            previous_focus,
        });
        // 잠금 표면이 생길 때까지는 아무도 키를 받지 않는다. 조합하던 글자는 다른 포커스 이동처럼 앱에 확정한다
        // (ime.rs). 잠그기 전에 사람이 친 글이다
        if let Some(keyboard) = keyboard {
            self.commit_preedit_before_focus(None);
            keyboard.set_focus(self, None, serial);
        }
        self.refresh_pointer_focus();
        self.publish_lock(true);
        // logind의 Lock 신호처럼 입력이나 커밋 없이 잠글 때도 바로 가린다
        self.queue_redraw_all();
    }

    /// 잠금을 푼다. 잠금을 확인받은 잠금 객체의 unlock_and_destroy로만 온다
    fn unlock_screen(&mut self) {
        let Some(locked) = self.session_lock.locked.take() else {
            return;
        };
        tracing::info!("unlocked the screen after {:?}", locked.since.elapsed());
        self.session_lock.release_sleep = false;
        if let Some(token) = self.session_lock.client.restart.take() {
            self.loop_handle.remove(token);
        }
        self.session_lock.client.failures = 0;
        if let Some(keyboard) = self.seat.get_keyboard() {
            let previous = locked.previous_focus.filter(Resource::is_alive);
            keyboard.set_focus(self, previous, SERIAL_COUNTER.next_serial());
        }
        // 잠긴 동안 떠서 키보드를 독점하는 레이어(인증 창)나 사라진 창을 맞춘다
        self.refresh_layer_focus();
        self.refresh_pointer_focus();
        self.publish_lock(false);
        self.queue_redraw_all();
    }

    /// 잠긴 상태를 D-Bus 사본과 신호, logind의 LockedHint로 알린다
    fn publish_lock(&mut self, locked: bool) {
        self.dbus.update(|snapshot| snapshot.locked = locked);
        self.dbus.send(Command::LockedChanged(locked));
        self.session.set_locked_hint(locked);
    }

    /// 포인터 아래의 표면을 다시 정한다 (잠그거나 풀 때)
    fn refresh_pointer_focus(&mut self) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let time = self.start_time.elapsed().as_millis() as u32;
        self.pointer_moved(pointer.current_location(), time);
    }

    /// 잠금 화면을 띄운다. 이미 떠 있거나 다시 띄우기를 기다리면 그대로 둔다
    fn spawn_lock_client(&mut self) {
        if self.session_lock.client.running.is_some() || self.session_lock.client.restart.is_some()
        {
            return;
        }
        let exited = self.lock_client_exited_sender();
        let command = self.config.session.lock.clone();
        let client = ClientState {
            lock_screen: true,
            ..ClientState::default()
        };
        let Some(mut child) = self.spawn_trusted(&command, client) else {
            // 띄우지 못했다(실행 파일이 없는 등). 화면은 잠긴 채로 두고 잠시 뒤 다시 해 본다
            self.schedule_lock_client_restart();
            return;
        };
        let lock_client = &mut self.session_lock.client;
        lock_client.generation += 1;
        let generation = lock_client.generation;
        lock_client.running = Some((child.id(), generation));
        lock_client.started = Some(Instant::now());
        std::thread::spawn(move || {
            let _ = child.wait();
            let _ = exited.send(generation);
        });
        let timer = self.loop_handle.insert_source(
            Timer::from_duration(START_TIMEOUT),
            move |_, _, state| {
                state.check_lock_client_started(generation);
                TimeoutAction::Drop
            },
        );
        if let Err(err) = timer {
            tracing::warn!("failed to watch the lock screen start: {}", err.error);
        }
    }

    /// 잠금 화면이 끝났다는 알림을 받는 통로. 처음 쓸 때 이벤트 루프에 단다
    fn lock_client_exited_sender(&mut self) -> channel::Sender<u64> {
        if let Some(sender) = &self.session_lock.client.exited {
            return sender.clone();
        }
        let (sender, receiver) = channel::channel();
        let source = self.loop_handle.insert_source(receiver, |event, _, state| {
            if let channel::Event::Msg(generation) = event {
                state.lock_client_exited(generation);
            }
        });
        if let Err(err) = source {
            tracing::warn!("failed to watch the lock screen process: {}", err.error);
        }
        self.session_lock.client.exited = Some(sender.clone());
        sender
    }

    fn lock_client_exited(&mut self, generation: u64) {
        let locked = self.session_lock.is_locked();
        let client = &mut self.session_lock.client;
        if client.running.map(|(_, running)| running) != Some(generation) {
            return;
        }
        client.running = None;
        let ran = client.started.take().map(|started| started.elapsed());
        if !locked {
            // 잠금을 풀고 끝났다
            client.failures = 0;
            return;
        }
        if ran.is_some_and(|ran| ran >= STABLE_RUN) {
            client.failures = 0;
        }
        tracing::warn!("the lock screen exited while the screen is locked; starting it again");
        self.schedule_lock_client_restart();
    }

    fn schedule_lock_client_restart(&mut self) {
        let client = &mut self.session_lock.client;
        if client.restart.is_some() {
            return;
        }
        let delay = RESTART_DELAY
            .saturating_mul(1 << client.failures.min(5))
            .min(MAX_RESTART_DELAY);
        client.failures += 1;
        let token = self
            .loop_handle
            .insert_source(Timer::from_duration(delay), |_, _, state| {
                state.session_lock.client.restart = None;
                if state.session_lock.is_locked() {
                    state.spawn_lock_client();
                }
                TimeoutAction::Drop
            });
        match token {
            Ok(token) => self.session_lock.client.restart = Some(token),
            Err(err) => tracing::warn!("failed to schedule the lock screen restart: {}", err.error),
        }
    }

    /// 띄운 잠금 화면이 START_TIMEOUT 안에 잠금 표면을 보이지 않았으면 끝낸다. 끝나면 다시 띄운다
    fn check_lock_client_started(&mut self, generation: u64) {
        let Some((pid, running)) = self.session_lock.client.running else {
            return;
        };
        if running != generation
            || !self.session_lock.is_locked()
            || self.session_lock.has_visible_surface()
        {
            return;
        }
        tracing::warn!("the lock screen showed nothing in {START_TIMEOUT:?}; restarting it");
        // 안전: 우리가 띄운 자식 프로세스다. 끝난 자식을 거둔 스레드의 알림을 아직 받지 않았을 때만 여기 오므로,
        // 그사이 다른 프로세스가 같은 번호를 받을 틈은 거의 없다
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }

    /// 화면 하나를 그린 뒤 (udev.rs의 render_output). 잠긴 장면을 모든 화면에 낸 뒤에야 잠금 화면에 잠겼다고
    /// 알리고(locked) 잠자기를 늦추던 logind 잠금을 놓는다. 그 전에는 잠그기 전의 화면이 남아 있을 수 있다
    pub fn lock_frame_done(&mut self, output: &Output, queued: bool) {
        let Some(locked) = self.session_lock.locked.as_mut() else {
            return;
        };
        let name = output.name();
        if locked.drawing.remove(&name) && queued {
            locked.blanked.insert(name);
        }
        self.confirm_lock_if_blanked();
    }

    fn confirm_lock_if_blanked(&mut self) {
        let outputs: Vec<String> = self.space.outputs().map(Output::name).collect();
        let Some(locked) = self.session_lock.locked.as_mut() else {
            return;
        };
        if !outputs.iter().all(|name| locked.blanked.contains(name)) {
            return;
        }
        if let Some(confirmation) = locked.pending.take() {
            confirmation.lock();
            locked.confirmed = true;
            tracing::info!("the lock screen holds the screen");
        }
        if std::mem::take(&mut self.session_lock.release_sleep) {
            self.session.release_sleep_inhibitor();
        }
    }

    /// 잠긴 동안 그 자리 아래의 표면: 그 화면의 잠금 표면(과 그 하위 표면)뿐이다 (state.rs의 surface_under)
    pub fn lock_surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let locked = self.session_lock.locked.as_ref()?;
        let output = self.space.output_under(pos).next()?;
        let output_geo = self.space.output_geometry(output)?;
        let surface = locked.surface_on(output)?;
        under_from_surface_tree(
            surface.wl_surface(),
            pos - output_geo.loc.to_f64(),
            (0, 0),
            WindowSurfaceType::ALL,
        )
        .map(|(surface, loc)| (surface, (loc + output_geo.loc).to_f64()))
    }

    /// 잠긴 동안 키보드를 잠금 표면에 둔다 (state.rs의 refresh_layer_focus). 이미 잠금 표면에 있으면 그대로, 아니면
    /// 첫 화면의 잠금 표면이다. 잠금 화면은 첫 화면에 암호 칸을 둔다. 잠금 표면이 아직 없으면 아무도 키를 받지 않는다
    pub fn focus_lock_surface(&mut self) {
        let Some(locked) = &self.session_lock.locked else {
            return;
        };
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let surfaces: Vec<WlSurface> = self
            .space
            .outputs()
            .filter_map(|output| locked.surface_on(output))
            .map(|surface| surface.wl_surface().clone())
            .collect();
        let focus = keyboard.current_focus();
        if focus.as_ref().is_some_and(|focus| surfaces.contains(focus)) {
            return;
        }
        let target = surfaces.into_iter().next();
        if focus != target {
            keyboard.set_focus(self, target, SERIAL_COUNTER.next_serial());
        }
    }

    /// 잠긴 동안 누른 화면의 잠금 표면에 키보드를 준다 (화면이 여럿일 때, input.rs의 press_button)
    pub fn focus_lock_surface_at(&mut self, pos: Point<f64, Logical>) {
        let Some(surface) = self.session_lock.locked.as_ref().and_then(|locked| {
            let output = self.space.output_under(pos).next()?;
            locked
                .surface_on(output)
                .map(|surface| surface.wl_surface().clone())
        }) else {
            return;
        };
        if let Some(keyboard) = self.seat.get_keyboard()
            && keyboard.current_focus().as_ref() != Some(&surface)
        {
            keyboard.set_focus(self, Some(surface), SERIAL_COUNTER.next_serial());
        }
    }

    /// 잠긴 동안 그 단축키를 할지 (input.rs의 on_keyboard). 하지 않는 키는 단축키로 옮기지 않고 잠금 화면에
    /// 그대로 보낸다
    pub fn allowed_while_locked(&self, hit: &Hit) -> bool {
        !self.session_lock.is_locked()
            || matches!(hit, Hit::Run(builtin) if runs_while_locked(*builtin))
    }
}

impl SessionLockHandler for BuntState {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.session_lock.manager
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        // 잠금 화면은 컴포지터가 잠근 뒤에 띄우므로 대개 이미 잠겨 있다. 잠기지 않았으면 지금 잠근다
        if self.session_lock.locked.is_none() {
            if self.config.session.lock.is_empty() {
                // 받지 않는다 (confirmation을 버리면 finished가 간다)
                return;
            }
            tracing::info!("locking the screen (the lock screen asked)");
            self.enter_locked();
        }
        let Some(locked) = self.session_lock.locked.as_mut() else {
            return;
        };
        // 화면을 쥔 잠금 객체가 살아 있으면 두 번째 잠금은 받지 않는다
        if locked.lock.as_ref().is_some_and(Resource::is_alive) {
            tracing::warn!("refused a second lock while another lock holds the screen");
            return;
        }
        locked.lock = Some(confirmation.ext_session_lock().clone());
        locked.confirmed = false;
        locked.pending = Some(confirmation);
        self.confirm_lock_if_blanked();
    }

    fn unlock(&mut self) {
        self.unlock_screen();
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            return;
        };
        let Some(size) = self.space.output_geometry(&output).map(|geo| geo.size) else {
            return;
        };
        let Some(locked) = self.session_lock.locked.as_mut() else {
            return;
        };
        // smithay가 이 함수 바로 뒤에 이 크기로 첫 configure를 보낸다
        surface.with_pending_state(|state| state.size = Some(lock_size(size)));
        output.enter(surface.wl_surface());
        locked
            .surfaces
            .retain(|(on, known)| known.alive() && *on != output);
        locked.surfaces.push((output, surface));
        self.focus_lock_surface();
    }
}

// 잠금 관리자와 잠금 표면은 smithay의 처리에 맡기고, 잠금 객체(ext_session_lock_v1)는 아래에서 손본다
delegate_global_dispatch!(BuntState: [ExtSessionLockManagerV1: SessionLockManagerGlobalData] => SessionLockManagerState);
delegate_dispatch!(BuntState: [ExtSessionLockManagerV1: ()] => SessionLockManagerState);
delegate_dispatch!(BuntState: [ExtSessionLockSurfaceV1: ExtLockSurfaceUserData] => SessionLockManagerState);

impl Dispatch<ExtSessionLockV1, SessionLockState> for BuntState {
    /// smithay(0.7)는 잠겼다고 알리지 않은 잠금 객체가 unlock_and_destroy를 보내도 오류를 보낸 뒤 unlock을 부른다.
    /// 잠겼다고 알린 지금의 잠금 객체만 잠금을 풀 수 있게 여기서 막는다
    fn request(
        state: &mut Self,
        client: &Client,
        lock: &ExtSessionLockV1,
        request: ext_session_lock_v1::Request,
        data: &SessionLockState,
        display: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if matches!(request, ext_session_lock_v1::Request::UnlockAndDestroy)
            && !state.session_lock.holds_screen(lock)
        {
            lock.post_error(
                ext_session_lock_v1::Error::InvalidUnlock,
                "this lock does not hold the screen",
            );
            return;
        }
        <SessionLockManagerState as Dispatch<ExtSessionLockV1, SessionLockState, Self>>::request(
            state, client, lock, request, data, display, data_init,
        );
    }

    /// 잠금 화면이 풀지 않고 사라졌으면(죽었거나 연결이 끊겼다) 화면은 잠긴 채로 둔다. 끝난 프로세스는
    /// lock_client_exited가 다시 띄운다
    fn destroyed(
        state: &mut Self,
        client: ClientId,
        lock: &ExtSessionLockV1,
        data: &SessionLockState,
    ) {
        <SessionLockManagerState as Dispatch<ExtSessionLockV1, SessionLockState, Self>>::destroyed(
            state, client, lock, data,
        );
        if let Some(locked) = state.session_lock.locked.as_mut()
            && locked.lock.as_ref() == Some(lock)
        {
            tracing::warn!("the lock screen went away without unlocking; the screen stays locked");
            locked.lock = None;
            locked.confirmed = false;
            locked.pending = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_escape_hatches_run_while_locked() {
        assert!(runs_while_locked(Builtin::SwitchInputSource));
        assert!(runs_while_locked(Builtin::SwitchVt(2)));
        assert!(runs_while_locked(Builtin::Quit));
        for builtin in [
            Builtin::LockScreen,
            Builtin::LogOut { confirm: true },
            Builtin::LogOut { confirm: false },
            Builtin::ForceQuit,
            Builtin::OpenTerminal,
            Builtin::ShowLauncher,
            Builtin::NextApp,
            Builtin::QuitApp,
            Builtin::CloseWindow,
            Builtin::Screenshot {
                area: false,
                clipboard: false,
            },
        ] {
            assert!(!runs_while_locked(builtin), "{builtin:?}");
        }
    }

    #[test]
    fn lock_surfaces_cover_the_logical_screen() {
        assert_eq!(lock_size(Size::from((1024, 640))), Size::from((1024, 640)));
        // 크기를 모르는 화면에도 0이 아닌 크기를 알린다 (0은 클라이언트가 고르라는 뜻이다)
        assert_eq!(lock_size(Size::from((0, 0))), Size::from((1, 1)));
    }
}
