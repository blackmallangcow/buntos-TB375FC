// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터 제어 D-Bus 서비스 (세션 버스 `org.buntos.Compositor`). 시스템 설정이 화면(해상도,
//! 배율)과 키 반복을 읽고 바꾸고, 셸이 메뉴 막대·Dock 가리기 설정을 알린다.
//! 버스 일은 따로 도는 스레드가 맡는다. 읽기는 컴포지터가 미리
//! 적어 둔 사본(Snapshot)으로 답하고, 바꾸기는 값을 확인한 뒤 이벤트 루프로 넘긴다.
//! 입력 소스 전환처럼 컴포지터가 다른 프로그램을 부르는 일도 이 스레드가 한다.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, mpsc},
};

use smithay::reexports::calloop::channel::Sender as LoopSender;

use crate::panels::{Autohide, Panels};
use zbus::{
    Message, blocking, fdo, interface,
    object_server::SignalEmitter,
    zvariant::{DynamicType, Value},
};

const BUS_NAME: &str = "org.buntos.Compositor";
const OBJECT_PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";

/// fcitx5 제어 인터페이스
const FCITX_NAME: &str = "org.fcitx.Fcitx5";
const FCITX_PATH: &str = "/controller";
const FCITX_INTERFACE: &str = "org.fcitx.Fcitx.Controller1";

/// 시스템 설정에 보이는 출력 하나
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OutputInfo {
    /// 커넥터 이름 (예: eDP-1)
    pub name: String,
    /// 제조사와 모델 (EDID). 모르면 빈 문자열
    pub description: String,
    /// 기기에 붙은 화면 (eDP, LVDS, DSI)
    pub builtin: bool,
    pub width: i32,
    pub height: i32,
    /// mHz
    pub refresh: i32,
    pub scale: f64,
    /// (너비, 높이, mHz). 화면이 권하는 모드가 먼저
    pub modes: Vec<(i32, i32, i32)>,
    /// 시스템 설정에 보일 배율 (작은 것부터, 많으면 다섯 개)
    pub scales: Vec<f64>,
    /// 화면 크기로 고른 기본 배율 (macOS의 "기본값")
    pub preferred_scale: f64,
    /// mm. 모르면 0
    pub physical_width: i32,
    pub physical_height: i32,
}

/// 컴포지터 상태의 사본. 컴포지터가 바뀔 때마다 고쳐 쓴다
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub outputs: Vec<OutputInfo>,
    pub repeat_delay: u32,
    pub repeat_rate: u32,
}

/// D-Bus에서 이벤트 루프로 넘기는 변경 요청 (값은 이미 확인했다)
#[derive(Debug)]
pub enum Request {
    OutputMode {
        name: String,
        width: i32,
        height: i32,
        refresh: i32,
    },
    OutputScale {
        name: String,
        scale: f64,
    },
    KeyboardRepeat {
        delay: u32,
        rate: u32,
    },
    Panels(Panels),
}

/// 컴포지터가 D-Bus 스레드에 시키는 일
#[derive(Debug)]
pub enum Command {
    /// OutputsChanged 신호를 보낸다
    OutputsChanged,
    /// 입력 소스를 바꾼다 (한/영 전환)
    ToggleInputMethod,
}

pub struct Service {
    snapshot: Arc<Mutex<Snapshot>>,
    commands: mpsc::Sender<Command>,
}

impl Service {
    pub fn start(requests: LoopSender<Request>) -> Self {
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let (commands, receiver) = mpsc::channel();
        let api = Api {
            snapshot: snapshot.clone(),
            requests: Mutex::new(requests),
        };
        if let Err(err) = std::thread::Builder::new()
            .name("bunt-comp-dbus".into())
            .spawn(move || run(api, receiver))
        {
            tracing::warn!("failed to start the D-Bus thread: {err}");
        }
        Self { snapshot, commands }
    }

    pub fn update(&self, f: impl FnOnce(&mut Snapshot)) {
        f(&mut self.snapshot.lock().unwrap());
    }

    pub fn send(&self, command: Command) {
        // 스레드가 없으면(버스 연결 실패) 버린다
        let _ = self.commands.send(command);
    }
}

fn run(api: Api, commands: mpsc::Receiver<Command>) {
    let connection = blocking::connection::Builder::session()
        .and_then(|builder| builder.name(BUS_NAME))
        .and_then(|builder| builder.serve_at(OBJECT_PATH, api))
        .and_then(|builder| builder.build());
    let connection = match connection {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!("failed to provide {BUS_NAME} on the session bus: {err}");
            for _ in commands {}
            return;
        }
    };
    for command in commands {
        match command {
            Command::OutputsChanged => {
                if let Err(err) = connection.emit_signal(
                    None::<&str>,
                    OBJECT_PATH,
                    INTERFACE,
                    "OutputsChanged",
                    &(),
                ) {
                    tracing::warn!("failed to emit OutputsChanged: {err}");
                }
            }
            Command::ToggleInputMethod => toggle_input_method(&connection),
        }
    }
}

/// 입력 소스를 바꾼다. 입력기가 아직 입력 칸을 만난 적이 없으면 fcitx5의 Toggle이 아무 일도
/// 하지 않으므로, 현재 그룹의 다음 입력기를 직접 고른다
fn toggle_input_method(connection: &blocking::Connection) {
    let current: Option<String> = fcitx(connection, "CurrentInputMethod", &())
        .ok()
        .and_then(|reply| reply.body().deserialize::<String>().ok());
    let group: Option<String> = fcitx(connection, "CurrentInputMethodGroup", &())
        .ok()
        .and_then(|reply| reply.body().deserialize::<String>().ok());
    let items: Vec<String> = group
        .and_then(|group| fcitx(connection, "InputMethodGroupInfo", &(group,)).ok())
        .and_then(|reply| {
            reply
                .body()
                .deserialize::<(String, Vec<(String, String)>)>()
                .ok()
        })
        .map(|(_layout, items)| items.into_iter().map(|(name, _)| name).collect())
        .unwrap_or_default();
    let next = current
        .as_ref()
        .and_then(|current| items.iter().position(|item| item == current))
        .map(|index| items[(index + 1) % items.len()].clone())
        .or_else(|| items.get(1).cloned());
    let result = match next {
        Some(next) => fcitx(connection, "SetCurrentIM", &(next,)),
        None => fcitx(connection, "Toggle", &()),
    };
    if let Err(err) = result {
        tracing::warn!("failed to switch the input method: {err}");
    }
}

fn fcitx<B: serde::Serialize + DynamicType>(
    connection: &blocking::Connection,
    method: &str,
    body: &B,
) -> zbus::Result<Message> {
    connection.call_method(
        Some(FCITX_NAME),
        FCITX_PATH,
        Some(FCITX_INTERFACE),
        method,
        body,
    )
}

struct Api {
    snapshot: Arc<Mutex<Snapshot>>,
    requests: Mutex<LoopSender<Request>>,
}

impl Api {
    fn request(&self, request: Request) -> fdo::Result<()> {
        self.requests
            .lock()
            .unwrap()
            .send(request)
            .map_err(|_| fdo::Error::Failed("the compositor is shutting down".into()))
    }

    fn output(&self, name: &str) -> fdo::Result<OutputInfo> {
        self.snapshot
            .lock()
            .unwrap()
            .outputs
            .iter()
            .find(|output| output.name == name)
            .cloned()
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no output named {name}")))
    }
}

#[interface(name = "org.buntos.Compositor1")]
impl Api {
    /// 연결된 출력 목록
    fn get_outputs(&self) -> Vec<HashMap<String, Value<'static>>> {
        let snapshot = self.snapshot.lock().unwrap();
        snapshot
            .outputs
            .iter()
            .map(|output| {
                HashMap::from([
                    ("name".to_owned(), Value::from(output.name.clone())),
                    (
                        "description".to_owned(),
                        Value::from(output.description.clone()),
                    ),
                    ("builtin".to_owned(), Value::from(output.builtin)),
                    ("width".to_owned(), Value::from(output.width)),
                    ("height".to_owned(), Value::from(output.height)),
                    ("refresh".to_owned(), Value::from(output.refresh)),
                    ("scale".to_owned(), Value::from(output.scale)),
                    ("modes".to_owned(), Value::new(output.modes.clone())),
                    ("scales".to_owned(), Value::new(output.scales.clone())),
                    (
                        "preferred-scale".to_owned(),
                        Value::from(output.preferred_scale),
                    ),
                    (
                        "physical-width".to_owned(),
                        Value::from(output.physical_width),
                    ),
                    (
                        "physical-height".to_owned(),
                        Value::from(output.physical_height),
                    ),
                ])
            })
            .collect()
    }

    /// 출력의 모드를 바꾸고 다음 로그인에도 쓴다. 모드 목록에 없는 값이면 오류
    fn set_output_mode(
        &self,
        name: &str,
        width: i32,
        height: i32,
        refresh: i32,
    ) -> fdo::Result<()> {
        let output = self.output(name)?;
        if !output.modes.contains(&(width, height, refresh)) {
            return Err(fdo::Error::InvalidArgs(format!(
                "{name} has no mode {width}x{height}@{refresh}"
            )));
        }
        self.request(Request::OutputMode {
            name: name.to_owned(),
            width,
            height,
            refresh,
        })
    }

    /// 출력의 배율을 바꾸고 다음 로그인에도 쓴다. 고를 수 있는 배율이 아니면 오류
    fn set_output_scale(&self, name: &str, scale: f64) -> fdo::Result<()> {
        let output = self.output(name)?;
        let Some(&scale) = output
            .scales
            .iter()
            .find(|allowed| (*allowed - scale).abs() < 0.001)
        else {
            return Err(fdo::Error::InvalidArgs(format!(
                "{name} does not support scale {scale}"
            )));
        };
        self.request(Request::OutputScale {
            name: name.to_owned(),
            scale,
        })
    }

    /// (반복 시작까지 ms, 초당 반복 횟수)
    fn get_keyboard_repeat(&self) -> (u32, u32) {
        let snapshot = self.snapshot.lock().unwrap();
        (snapshot.repeat_delay, snapshot.repeat_rate)
    }

    /// 키 반복을 바꾸고 다음 로그인에도 쓴다. 횟수가 0이면 반복하지 않는다
    fn set_keyboard_repeat(&self, delay_ms: u32, rate: u32) -> fdo::Result<()> {
        if !(100..=3000).contains(&delay_ms) || rate > 100 {
            return Err(fdo::Error::InvalidArgs(format!(
                "repeat delay must be 100..=3000 ms and rate at most 100 (got {delay_ms}, {rate})"
            )));
        }
        self.request(Request::KeyboardRepeat {
            delay: delay_ms,
            rate,
        })
    }

    /// 셸이 메뉴 막대와 Dock 설정을 알린다 (org.buntos.shell의 menu-bar-autohide, dock-autohide,
    /// menu-bar-background). 가리기 값은 never, fullscreen, maximized, always다.
    /// 최대화·전체 화면 창이 채우는 영역과 메뉴 막대 흐림이 바뀐다. 다음 로그인에는 셸이 다시 알린다
    fn set_panels(
        &self,
        menu_bar_autohide: &str,
        dock_autohide: &str,
        menu_bar_background: bool,
    ) -> fdo::Result<()> {
        let parse = |value: &str| {
            Autohide::parse(value).ok_or_else(|| {
                fdo::Error::InvalidArgs(format!(
                    "autohide must be never, fullscreen, maximized or always (got {value})"
                ))
            })
        };
        self.request(Request::Panels(Panels {
            menu_bar: parse(menu_bar_autohide)?,
            dock: parse(dock_autohide)?,
            menu_bar_background,
        }))
    }

    /// 모드, 배율, 연결이 바뀐 뒤 보낸다
    #[zbus(signal)]
    async fn outputs_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
