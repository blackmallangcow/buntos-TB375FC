// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! logind (org.freedesktop.login1). 이 세션의 Lock 신호(loginctl lock-session)와 잠자기 직전·깬 뒤 신호
//! (PrepareForSleep)를 받아 이벤트 루프로 넘기고, 잠긴 상태를 세션의 LockedHint로 알린다. 잠자기는 잠긴 장면을
//! 화면에 낼 때까지 늦춘다(delay 잠금). 버스 일은 따로 도는 스레드가 맡는다.
//!
//! Unlock 신호(loginctl unlock-session)는 듣지 않는다. 화면 잠금을 푸는 것은 잠금 화면에서 사람이 친 암호뿐이다
//! (session_lock.rs).

use std::{os::fd::OwnedFd, sync::mpsc, thread};

use smithay::reexports::calloop::channel::Sender as LoopSender;
use zbus::{
    MatchRule, Message, blocking,
    message::Type as MessageType,
    zvariant::{self, OwnedObjectPath},
};

const LOGIN1: &str = "org.freedesktop.login1";
const MANAGER_PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";
const SESSION: &str = "org.freedesktop.login1.Session";

/// logind가 알린 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// 이 세션을 잠그라고 했다 (loginctl lock-session, lock-sessions)
    Lock,
    /// 잠자기 직전(true)과 깬 뒤(false)
    PrepareForSleep(bool),
}

/// 이벤트 루프가 logind 스레드에 시키는 일
enum Request {
    LockedHint(bool),
    TakeSleepInhibitor,
    ReleaseSleepInhibitor,
    DarkenScreen(bool),
}

pub struct Logind {
    requests: mpsc::Sender<Request>,
}

impl Logind {
    /// 시스템 버스에 붙어 이 세션을 찾고, 잠자기를 늦추는 잠금을 잡고 신호를 듣기 시작한다
    pub fn start(events: LoopSender<Event>) -> Self {
        let (requests, receiver) = mpsc::channel();
        if let Err(err) = thread::Builder::new()
            .name("bunt-comp-logind".into())
            .spawn(move || run(events, receiver))
        {
            tracing::warn!("failed to start the logind thread: {err}");
        }
        Self { requests }
    }

    /// 세션의 LockedHint (loginctl show-session -p LockedHint)
    pub fn set_locked_hint(&self, locked: bool) {
        let _ = self.requests.send(Request::LockedHint(locked));
    }

    /// 깬 뒤 다음 잠자기를 위해 다시 잡는다
    pub fn take_sleep_inhibitor(&self) {
        let _ = self.requests.send(Request::TakeSleepInhibitor);
    }

    /// 잠들기 직전에 백라이트를 끈다(true). 앱이 잠금 화면으로 바뀌는 모습이 보이지 않아
    /// 화면이 바로 꺼진 것처럼 보인다. 깬 뒤 원래 밝기로 돌린다(false)
    pub fn darken_screen(&self, dark: bool) {
        let _ = self.requests.send(Request::DarkenScreen(dark));
    }

    /// 잠긴 장면을 낸 뒤 놓으면 logind가 잠든다
    pub fn release_sleep_inhibitor(&self) {
        let _ = self.requests.send(Request::ReleaseSleepInhibitor);
    }
}

fn run(events: LoopSender<Event>, requests: mpsc::Receiver<Request>) {
    let connection = match blocking::Connection::system() {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!("no system bus; logind lock and sleep signals are off: {err}");
            for _ in requests {}
            return;
        }
    };
    let session = session_path(&connection)
        .inspect_err(|err| {
            tracing::warn!("logind does not know this session; its lock signal is off: {err}")
        })
        .ok();
    listen(
        &connection,
        MANAGER_PATH,
        MANAGER,
        "PrepareForSleep",
        events.clone(),
        |message| {
            message
                .body()
                .deserialize::<bool>()
                .ok()
                .map(Event::PrepareForSleep)
        },
    );
    if let Some(session) = &session {
        listen(
            &connection,
            session.as_str(),
            SESSION,
            "Lock",
            events,
            |_| Some(Event::Lock),
        );
    }
    let mut inhibitor = take_inhibitor(&connection);
    let mut darkened: Option<(String, u32)> = None;
    for request in requests {
        match request {
            Request::LockedHint(locked) => {
                let Some(session) = &session else {
                    continue;
                };
                if let Err(err) = connection.call_method(
                    Some(LOGIN1),
                    session,
                    Some(SESSION),
                    "SetLockedHint",
                    &(locked,),
                ) {
                    tracing::warn!("failed to set the session's LockedHint: {err}");
                }
            }
            Request::TakeSleepInhibitor => {
                if inhibitor.is_none() {
                    inhibitor = take_inhibitor(&connection);
                }
            }
            // 파일을 닫으면 놓인다
            Request::ReleaseSleepInhibitor => inhibitor = None,
            Request::DarkenScreen(dark) => {
                let Some(session) = &session else {
                    continue;
                };
                if !dark {
                    if let Some((name, level)) = darkened.take() {
                        set_brightness(&connection, session, &name, level);
                    }
                } else if darkened.is_none()
                    && let Some((name, level)) = backlight()
                    && set_brightness(&connection, session, &name, 0)
                {
                    darkened = Some((name, level));
                }
            }
        }
    }
}

/// 첫 백라이트의 이름과 밝기
fn backlight() -> Option<(String, u32)> {
    let device = std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .next()?;
    let level = std::fs::read_to_string(device.path().join("brightness")).ok()?;
    Some((
        device.file_name().into_string().ok()?,
        level.trim().parse().ok()?,
    ))
}

fn set_brightness(
    connection: &blocking::Connection,
    session: &OwnedObjectPath,
    name: &str,
    level: u32,
) -> bool {
    let result = connection.call_method(
        Some(LOGIN1),
        session,
        Some(SESSION),
        "SetBrightness",
        &("backlight", name, level),
    );
    if let Err(err) = &result {
        tracing::warn!("failed to set the brightness: {err}");
    }
    result.is_ok()
}

/// 이 프로세스의 세션. greetd가 연 세션의 프로세스(세션 스크립트가 exec한 컴포지터)다
fn session_path(connection: &blocking::Connection) -> zbus::Result<OwnedObjectPath> {
    let reply = connection.call_method(
        Some(LOGIN1),
        MANAGER_PATH,
        Some(MANAGER),
        "GetSessionByPID",
        &(std::process::id(),),
    )?;
    reply.body().deserialize()
}

/// 잠자기를 늦추는 잠금. logind는 PrepareForSleep(true)를 보낸 뒤 이 파일이 닫힐 때까지(늦어도
/// InhibitDelayMaxSec) 잠들기를 기다린다
fn take_inhibitor(connection: &blocking::Connection) -> Option<OwnedFd> {
    let reply = connection.call_method(
        Some(LOGIN1),
        MANAGER_PATH,
        Some(MANAGER),
        "Inhibit",
        &(
            "sleep",
            "bunt-comp",
            "Lock the screen before sleeping",
            "delay",
        ),
    );
    match reply.and_then(|reply| reply.body().deserialize::<zvariant::OwnedFd>()) {
        Ok(fd) => Some(fd.into()),
        Err(err) => {
            tracing::warn!("cannot delay sleep to lock the screen first: {err}");
            None
        }
    }
}

/// logind의 신호 하나를 받아 이벤트 루프로 넘기는 스레드
fn listen(
    connection: &blocking::Connection,
    path: &str,
    interface: &str,
    member: &str,
    events: LoopSender<Event>,
    event: fn(&Message) -> Option<Event>,
) {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(LOGIN1)
        .and_then(|builder| builder.path(path.to_owned()))
        .and_then(|builder| builder.interface(interface.to_owned()))
        .and_then(|builder| builder.member(member.to_owned()))
        .map(|builder| builder.build());
    let messages =
        rule.and_then(|rule| blocking::MessageIterator::for_match_rule(rule, connection, None));
    let messages = match messages {
        Ok(messages) => messages,
        Err(err) => {
            tracing::warn!("cannot listen to logind's {member}: {err}");
            return;
        }
    };
    let name = format!("bunt-comp-logind-{member}");
    let listener = thread::Builder::new().name(name).spawn(move || {
        for message in messages.flatten() {
            if let Some(event) = event(&message)
                && events.send(event).is_err()
            {
                return;
            }
        }
    });
    if let Err(err) = listener {
        tracing::warn!("failed to listen to logind's {member}: {err}");
    }
}
