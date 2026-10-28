// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터 제어 D-Bus 서비스 (세션 버스 `org.buntos.Compositor`). 시스템 설정이 화면(해상도,
//! 배율), 키 반복, 마우스를 읽고 바꾸고 연결된 포인터 장치를 읽으며, 셸이 메뉴 막대·Dock 가리기
//! 설정을 알린다.
//! 버스 일은 따로 도는 스레드가 맡는다. 읽기는 컴포지터가 미리
//! 적어 둔 사본(Snapshot)으로 답하고, 바꾸기는 값을 확인한 뒤 이벤트 루프로 넘긴다.
//! 입력 소스 전환처럼 컴포지터가 다른 프로그램을 부르는 일도 이 스레드가 한다.

use std::{
    collections::HashMap,
    os::unix::fs::MetadataExt,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use smithay::{input::keyboard::xkb, reexports::calloop::channel::Sender as LoopSender};

use crate::{
    cursor,
    panels::{Autohide, Panels},
    prefs::PointerPrefs,
};
use zbus::{
    Message, blocking, fdo, interface,
    message::Header,
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

/// 에이전트 전용 메서드(창 목록, 창 활성화, 키·포인터 입력)를 부를 수 있는 프로그램
const AGENT_PATH: &str = "/usr/bin/buntd-agent";
/// 이벤트 루프가 에이전트 요청에 답하기를 기다리는 시간. 소프트웨어 렌더링으로 바쁜 VM에서는 프레임
/// 하나에 수백 ms가 걸려, 500ms로는 창 목록의 답을 놓쳤다
const AGENT_REPLY: Duration = Duration::from_secs(2);

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

/// 연결된 포인터 장치 하나 (마우스, 터치패드 등)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointerDevice {
    /// 장치가 알린 이름. 모르면 빈 문자열
    pub name: String,
    /// USB 제조사·제품 번호. 모르면 0
    pub vendor: u32,
    pub product: u32,
    /// 연결 방식 (udev의 ID_BUS: usb, bluetooth 등). 모르면 빈 문자열
    pub bus: String,
    pub touchpad: bool,
}

/// 키보드 포커스를 가진 클라이언트와 그 최상위 표면. 에이전트(buntd-agent)가 활성 창을 고를 때
/// 쓴다. 셸의 레이어 창은 팝업이 닫힌 뒤에도 GTK가 활성으로 알릴 때가 있어 컴포지터에 묻는다
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Focus {
    pub pid: u32,
    /// 창이면 앱 ID, 레이어 표면이면 그 이름(namespace)
    pub app_id: String,
    /// 창 제목. 레이어 표면은 빈 문자열
    pub title: String,
    /// 레이어 표면(셸의 메뉴, 모든 앱 등)이다
    pub layer: bool,
}

/// 컴포지터 상태의 사본. 컴포지터가 바뀔 때마다 고쳐 쓴다
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub outputs: Vec<OutputInfo>,
    pub repeat_delay: u32,
    pub repeat_rate: u32,
    pub pointer: PointerPrefs,
    pub pointer_devices: Vec<PointerDevice>,
    pub focus: Option<Focus>,
}

/// 에이전트가 보는 창 하나 (GetWindows)
#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub pid: u32,
    pub app_id: String,
    pub title: String,
    /// 창(xdg 창 영역)의 자리와 크기 (논리 좌표)
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// 키보드 포커스를 가진 창이다
    pub focused: bool,
}

/// 에이전트의 입력을 받지 않은 까닭
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Refused {
    /// 관리자 인증 창이 떠 있다. 그 창은 사람만 답한다
    AuthDialog,
    /// 키 배열에 없는 글자 (글 입력을 켜지 않은 앱에 글을 칠 때)
    NotOnLayout(char),
}

/// 에이전트 입력 요청의 답을 기다린다
fn answer<T>(answer: mpsc::Receiver<Result<T, Refused>>) -> fdo::Result<T> {
    match answer.recv_timeout(AGENT_REPLY) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(Refused::AuthDialog)) => Err(fdo::Error::AccessDenied(
            "an authentication dialog is open; only the user can answer it".into(),
        )),
        Ok(Err(Refused::NotOnLayout(missing))) => Err(fdo::Error::InvalidArgs(format!(
            "{missing:?} is not on the keyboard layout; fill the field instead"
        ))),
        Err(_) => Err(fdo::Error::Failed("the compositor did not answer".into())),
    }
}

/// 에이전트가 친 글이 간 길
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Typed {
    /// 앱의 글 입력(text-input-v3)에 입력기처럼 통째로 확정했다
    TextInput,
    /// 글자마다 키를 쳤다
    Keys,
}

/// 함께 누르는 키 하나: 수정 키들(Control_L 등)과 마지막 키
#[derive(Clone, Debug, PartialEq)]
pub struct Chord {
    pub modifiers: Vec<xkb::Keysym>,
    pub key: xkb::Keysym,
}

/// 에이전트(buntd-agent)의 요청. 답이 있는 것은 이벤트 루프가 보낸다
#[derive(Debug)]
pub enum AgentRequest {
    Windows(mpsc::Sender<Vec<WindowInfo>>),
    /// 키보드 포커스를 가진 앱이 글 입력(text-input-v3)을 켜 두었는지
    TextInput(mpsc::Sender<bool>),
    /// 그 pid의 창을 앞으로 가져와 키보드 포커스를 준다. 제목이 같은 창을 먼저 고른다
    Activate {
        pid: u32,
        title: String,
        reply: mpsc::Sender<Result<bool, Refused>>,
    },
    /// 키보드 포커스에 키를 차례로 누른다 (컴포지터 단축키는 거치지 않는다)
    Keys {
        chords: Vec<Chord>,
        reply: mpsc::Sender<Result<(), Refused>>,
    },
    /// 글을 친다. 글 입력이 켜진 앱이 아니고 키 배열에 없는 글자가 있으면 치지 않는다
    Type {
        text: String,
        reply: mpsc::Sender<Result<Typed, Refused>>,
    },
    /// 그 자리로 포인터를 옮겨 누른다 (linux 단추 번호)
    Click {
        x: f64,
        y: f64,
        button: u32,
        reply: mpsc::Sender<Result<(), Refused>>,
    },
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
    Pointer(PointerPrefs),
    Agent(AgentRequest),
}

/// 컴포지터가 D-Bus 스레드에 시키는 일
#[derive(Debug)]
pub enum Command {
    /// OutputsChanged 신호를 보낸다
    OutputsChanged,
    /// PointerDevicesChanged 신호를 보낸다
    PointerDevicesChanged,
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
            Command::PointerDevicesChanged => {
                if let Err(err) = connection.emit_signal(
                    None::<&str>,
                    OBJECT_PATH,
                    INTERFACE,
                    "PointerDevicesChanged",
                    &(),
                ) {
                    tracing::warn!("failed to emit PointerDevicesChanged: {err}");
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

/// 에이전트 전용 메서드는 buntd-agent만 부른다. 키와 포인터를 넣고 모든 창 제목을 읽기 때문이다.
/// 경로가 아니라 파일(장치·inode)을 견준다: 사용자 이름공간에서 다른 파일을 그 경로에 bind mount하면
/// /proc/<pid>/exe의 경로는 같게 보인다
async fn check_agent(connection: &zbus::Connection, header: &Header<'_>) -> fdo::Result<()> {
    let denied = || fdo::Error::AccessDenied(format!("only {AGENT_PATH} may call this"));
    let sender = header.sender().ok_or_else(denied)?;
    let pid = fdo::DBusProxy::new(connection)
        .await?
        .get_connection_unix_process_id(sender.clone().into())
        .await?;
    match (
        std::fs::metadata(format!("/proc/{pid}/exe")),
        std::fs::metadata(AGENT_PATH),
    ) {
        (Ok(running), Ok(agent))
            if running.dev() == agent.dev() && running.ino() == agent.ino() =>
        {
            Ok(())
        }
        _ => Err(denied()),
    }
}

/// "ctrl+s", "shift+Tab", "Return" 같은 키 이름. 수정 키는 ctrl, shift, alt(option), super(cmd)다
fn parse_chord(text: &str) -> Result<Chord, String> {
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key = parts
        .pop()
        .filter(|key| !key.is_empty())
        .ok_or("empty key")?;
    let modifiers = parts
        .iter()
        .map(|name| match name.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Ok(xkb::Keysym::Control_L),
            "shift" => Ok(xkb::Keysym::Shift_L),
            "alt" | "option" => Ok(xkb::Keysym::Alt_L),
            "super" | "cmd" | "meta" | "logo" => Ok(xkb::Keysym::Super_L),
            other => Err(format!("unknown modifier {other:?}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let name = match key.to_ascii_lowercase().as_str() {
        "enter" => "Return",
        "esc" => "Escape",
        "del" => "Delete",
        "pageup" => "Prior",
        "pagedown" => "Next",
        _ => key,
    };
    let key = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
    if key.raw() == 0 {
        return Err(format!("unknown key {name:?}"));
    }
    Ok(Chord { modifiers, key })
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

    /// 에이전트 전용: 키보드 포커스를 가진 앱이 글 입력(text-input-v3)을 켜 두었는지. 켜져 있으면
    /// TypeText가 글을 통째로 확정한다. 에이전트는 글 칸에 포커스를 준 뒤 이것이 켜지기를 기다린다
    async fn text_input_active(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<bool> {
        check_agent(connection, &header).await?;
        let (reply, answer) = mpsc::channel();
        self.request(Request::Agent(AgentRequest::TextInput(reply)))?;
        answer
            .recv_timeout(AGENT_REPLY)
            .map_err(|_| fdo::Error::Failed("the compositor did not answer".into()))
    }

    /// 에이전트 전용: 창 목록 (pid, 앱 ID, 제목, x, y, 너비, 높이, 키보드 포커스). 쌓인 순서(아래부터)다
    #[allow(clippy::type_complexity)]
    async fn get_windows(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<Vec<(u32, String, String, i32, i32, i32, i32, bool)>> {
        check_agent(connection, &header).await?;
        let (reply, answer) = mpsc::channel();
        self.request(Request::Agent(AgentRequest::Windows(reply)))?;
        let windows = answer
            .recv_timeout(AGENT_REPLY)
            .map_err(|_| fdo::Error::Failed("the compositor did not answer".into()))?;
        Ok(windows
            .into_iter()
            .map(|window| {
                (
                    window.pid,
                    window.app_id,
                    window.title,
                    window.x,
                    window.y,
                    window.width,
                    window.height,
                    window.focused,
                )
            })
            .collect())
    }

    /// 에이전트 전용: 그 pid의 창을 앞으로 가져와 키보드 포커스를 준다. 제목이 같은 창을 먼저 고르고,
    /// 없으면 맨 위 창이다. 창이 없으면 false
    async fn activate_window(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        pid: u32,
        title: String,
    ) -> fdo::Result<bool> {
        check_agent(connection, &header).await?;
        let (reply, receiver) = mpsc::channel();
        self.request(Request::Agent(AgentRequest::Activate { pid, title, reply }))?;
        answer(receiver)
    }

    /// 에이전트 전용: 키보드 포커스에 키를 차례로 누른다 ("ctrl+a", "BackSpace", "Return").
    /// 컴포지터 단축키는 거치지 않는다
    async fn press_keys(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        keys: Vec<String>,
    ) -> fdo::Result<()> {
        check_agent(connection, &header).await?;
        let chords = keys
            .iter()
            .map(|key| parse_chord(key))
            .collect::<Result<Vec<_>, _>>()
            .map_err(fdo::Error::InvalidArgs)?;
        let (reply, receiver) = mpsc::channel();
        self.request(Request::Agent(AgentRequest::Keys { chords, reply }))?;
        answer(receiver)
    }

    /// 에이전트 전용: 글을 친다. 키보드 포커스를 가진 앱이 글 입력(text-input-v3)을 켜 두었으면
    /// 입력기가 확정하듯 통째로 넣는다("text-input"). 한글 같은 글자도 되고 입력기가 한글 모드여도 글이
    /// 바뀌지 않는다. 아니면 글자마다 키를 친다("keys", 지금 키 배열에 있는 글자만)
    async fn type_text(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        text: String,
    ) -> fdo::Result<String> {
        check_agent(connection, &header).await?;
        let (reply, receiver) = mpsc::channel();
        self.request(Request::Agent(AgentRequest::Type { text, reply }))?;
        Ok(match answer(receiver)? {
            Typed::TextInput => "text-input",
            Typed::Keys => "keys",
        }
        .into())
    }

    /// 에이전트 전용: 논리 좌표 (x, y)로 포인터를 옮겨 누른다. button은 linux 번호(왼쪽 272)다
    async fn click(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        x: f64,
        y: f64,
        button: u32,
    ) -> fdo::Result<()> {
        check_agent(connection, &header).await?;
        if !x.is_finite() || !y.is_finite() {
            return Err(fdo::Error::InvalidArgs("x and y must be numbers".into()));
        }
        let (reply, receiver) = mpsc::channel();
        self.request(Request::Agent(AgentRequest::Click {
            x,
            y,
            button,
            reply,
        }))?;
        answer(receiver)
    }

    /// 키보드 포커스: (클라이언트 pid, 앱 ID나 레이어 이름, 창 제목, 레이어 표면인지).
    /// 포커스가 없으면 pid가 0이다
    fn get_focus(&self) -> (u32, String, String, bool) {
        let focus = self
            .snapshot
            .lock()
            .unwrap()
            .focus
            .clone()
            .unwrap_or_default();
        (focus.pid, focus.app_id, focus.title, focus.layer)
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

    /// 마우스 설정: (이동 속도 -1~1, 자연스러운 스크롤, 보조 클릭을 왼쪽 단추로, 스크롤 속도 배수,
    /// 커서 크기 논리 픽셀)
    fn get_pointer(&self) -> (f64, bool, bool, f64, u32) {
        let pointer = self.snapshot.lock().unwrap().pointer;
        (
            pointer.speed,
            pointer.natural_scroll,
            pointer.left_handed,
            pointer.scroll_speed,
            pointer.cursor_size,
        )
    }

    /// 마우스 설정을 바꾸고 다음 로그인에도 쓴다. 연결된 장치와 나중에 연결하는 장치에 모두 쓴다
    fn set_pointer(
        &self,
        speed: f64,
        natural_scroll: bool,
        left_handed: bool,
        scroll_speed: f64,
        cursor_size: u32,
    ) -> fdo::Result<()> {
        let (min_size, max_size) = cursor::SIZE_RANGE;
        if !(-1.0..=1.0).contains(&speed)
            || !(0.25..=4.0).contains(&scroll_speed)
            || !(min_size..=max_size).contains(&cursor_size)
        {
            return Err(fdo::Error::InvalidArgs(format!(
                "speed must be -1..=1, scroll speed 0.25..=4 and cursor size \
                 {min_size}..={max_size} (got {speed}, {scroll_speed}, {cursor_size})"
            )));
        }
        self.request(Request::Pointer(PointerPrefs {
            speed,
            natural_scroll,
            left_handed,
            scroll_speed,
            cursor_size,
        }))
    }

    /// 연결된 포인터 장치. 장치마다 name(s), vendor(u), product(u), bus(s: usb, bluetooth 등.
    /// 모르면 빈 문자열), touchpad(b)
    fn get_pointer_devices(&self) -> Vec<HashMap<String, Value<'static>>> {
        let snapshot = self.snapshot.lock().unwrap();
        snapshot
            .pointer_devices
            .iter()
            .map(|device| {
                HashMap::from([
                    ("name".to_owned(), Value::from(device.name.clone())),
                    ("vendor".to_owned(), Value::from(device.vendor)),
                    ("product".to_owned(), Value::from(device.product)),
                    ("bus".to_owned(), Value::from(device.bus.clone())),
                    ("touchpad".to_owned(), Value::from(device.touchpad)),
                ])
            })
            .collect()
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

    /// 포인터 장치를 연결하거나 뺀 뒤 보낸다
    #[zbus(signal)]
    async fn pointer_devices_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_parse_modifiers_and_key_names() {
        assert_eq!(
            parse_chord("ctrl+shift+Tab").unwrap(),
            Chord {
                modifiers: vec![xkb::Keysym::Control_L, xkb::Keysym::Shift_L],
                key: xkb::Keysym::Tab,
            }
        );
        assert_eq!(parse_chord("enter").unwrap().key, xkb::Keysym::Return);
        assert_eq!(
            parse_chord("backspace").unwrap().key,
            xkb::Keysym::BackSpace
        );
        assert_eq!(
            parse_chord("cmd+a").unwrap().modifiers,
            [xkb::Keysym::Super_L]
        );
        assert!(parse_chord("hyper+a").is_err());
        assert!(parse_chord("notakey").is_err());
    }
}
