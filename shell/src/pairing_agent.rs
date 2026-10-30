// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! BlueZ 페어링 에이전트 (org.bluez.Agent1). 셸이 세션의 기본 에이전트가 된다(RegisterAgent,
//! RequestDefaultAgent). 시스템 설정의 Bluetooth 페이지와 셸의 Bluetooth 메뉴가 부르는 Pair는 자기
//! 에이전트를 두지 않으므로 BlueZ가 이 기본 에이전트에게 묻는다. 상대 기기가 먼저 페어링을 청할 때도 같다.
//!
//! 능력은 KeyboardDisplay다(화면과 키보드가 있다). BlueZ가 상대 기기에 맞춰 고르는 방식마다 macOS의 페어링
//! 창처럼 묻는다.
//! - RequestConfirmation: 두 기기에 보이는 6자리 코드가 같은지 견준다 [취소] [연결]
//! - DisplayPasskey, DisplayPinCode: 키보드에서 칠 코드를 보인다. 상대가 친 자릿수만큼 흐리게 한다 [취소]
//! - RequestPasskey: 상대 기기에 보이는 6자리 코드를 받는다. RequestPinCode: 옛 기기의 PIN(1~16자)을 받는다
//! - RequestAuthorization: 상대가 먼저 청한 페어링(확인 없는 방식)을 허락할지 [거부] [연결]
//! - AuthorizeService: 믿는 기기가 아닌 기기가 서비스(오디오 등)에 연결할지 [거부] [허용]. 허용은 이번 연결만이다.
//!   믿는 기기(Trusted)로 두면 그 기기의 다른 서비스(키보드 입력 등)도 묻지 않게 되므로 두지 않는다. 시스템
//!   설정과 메뉴 막대에서 짝지은 기기는 짝지을 때 믿는 기기가 된다
//! - Cancel: BlueZ가 요청을 거뒀다(시간 초과, 상대가 취소). Release: BlueZ가 에이전트를 내렸다
//!
//! 순서는 BlueZ의 src/agent.c, src/device.c와 같다.
//! - 묻는 요청은 60초 안에 답해야 한다. 그 전에 창을 닫는다. 한 번에 하나만 온다.
//! - 코드를 보이는 요청(Display*)은 바로 답해야 페어링이 이어진다. BlueZ는 끝났다고 알리지 않으므로 기기가
//!   페어링되거나(Paired), 사람이 취소하거나(Device1.CancelPairing), 시간이 지나면 창을 닫는다. 이미 페어링된
//!   기기에는 CancelPairing을 부르지 않는다. BlueZ는 진행 중인 페어링이 없으면 그 호출로 페어링을 지운다.
//! - 코드를 보이는 요청은 답을 기다리는 다른 요청이 있어도 온다. 다른 기기의 답을 기다리는 창이 떠 있으면 그 창을
//!   지키고 코드는 보이지 않는다(상대가 한 자리 칠 때마다 다시 온다).
//! - 창은 사람만 답한다(NAMESPACE). 상대 기기가 먼저 청할 수 있으므로 창은 눌러야 키보드를 받는다. 저절로 뜬
//!   PIN 창이 다른 앱에 치던 글을 받아 상대 기기로 보내지 않게 한다. 받은 PIN·코드는 기록에 남기지 않는다.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::{Rc, Weak},
    time::Duration,
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::KeyboardMode;

use crate::{
    bluetooth::device_icon,
    service_dialog::{self, Frame, Secret},
};

/// 창의 레이어 이름이자 접근성 이름. 사람만 답하는 창의 목록(comp/src/agent.rs, agent/src/atspi.rs)에 있다
pub(crate) const NAMESPACE: &str = "bunt-pairing-agent";
const BLUEZ: &str = "org.bluez";
const MANAGER_PATH: &str = "/org/bluez";
const MANAGER_INTERFACE: &str = "org.bluez.AgentManager1";
const AGENT_PATH: &str = "/org/buntos/Shell/BluetoothAgent";
const AGENT_INTERFACE: &str = "org.bluez.Agent1";
const DEVICE_INTERFACE: &str = "org.bluez.Device1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// 화면과 키보드가 있다: 코드를 보이고, 받고, 견줄 수 있다
const CAPABILITY: &str = "KeyboardDisplay";
const ERROR_REJECTED: &str = "org.bluez.Error.Rejected";
const ERROR_CANCELED: &str = "org.bluez.Error.Canceled";
const ERROR_ACCESS_DENIED: &str = "org.freedesktop.DBus.Error.AccessDenied";
const ERROR_INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";
const ERROR_UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";
/// BlueZ는 요청의 답을 60초 기다린다(REQUEST_TIMEOUT). 그보다 조금 일찍 창을 닫는다
const PROMPT_TIMEOUT: Duration = Duration::from_secs(58);
/// 코드를 보이는 창을 페어링이 끝났다는 알림 없이 두는 시간
const DISPLAY_TIMEOUT: Duration = Duration::from_secs(60);
/// 기기 이름을 묻는 시간. 답이 없으면 주소로 부른다
const LOOKUP_TIMEOUT_MS: i32 = 2_000;

const INTROSPECTION: &str = r#"<node>
  <interface name="org.bluez.Agent1">
    <method name="Release"/>
    <method name="RequestPinCode">
      <arg name="device" type="o" direction="in"/>
      <arg name="pincode" type="s" direction="out"/>
    </method>
    <method name="DisplayPinCode">
      <arg name="device" type="o" direction="in"/>
      <arg name="pincode" type="s" direction="in"/>
    </method>
    <method name="RequestPasskey">
      <arg name="device" type="o" direction="in"/>
      <arg name="passkey" type="u" direction="out"/>
    </method>
    <method name="DisplayPasskey">
      <arg name="device" type="o" direction="in"/>
      <arg name="passkey" type="u" direction="in"/>
      <arg name="entered" type="q" direction="in"/>
    </method>
    <method name="RequestConfirmation">
      <arg name="device" type="o" direction="in"/>
      <arg name="passkey" type="u" direction="in"/>
    </method>
    <method name="RequestAuthorization">
      <arg name="device" type="o" direction="in"/>
    </method>
    <method name="AuthorizeService">
      <arg name="device" type="o" direction="in"/>
      <arg name="uuid" type="s" direction="in"/>
    </method>
    <method name="Cancel"/>
  </interface>
</node>"#;

/// BlueZ가 청한 일
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kind {
    /// 옛 기기의 PIN(1~16자)을 받는다
    PinCode,
    /// 상대 기기에 보이는 6자리 코드를 받는다
    Passkey,
    /// 두 기기에 보이는 코드가 같은지 견준다
    Confirm { passkey: u32 },
    /// 상대가 먼저 청한 페어링을 허락할지
    Authorize,
    /// 서비스(UUID) 연결을 허락할지
    Service { uuid: String },
    /// 상대 기기에서 칠 코드를 보인다. entered는 상대가 친 자릿수다
    DisplayPasskey { passkey: u32, entered: u16 },
    /// 옛 키보드에서 칠 PIN을 보인다
    DisplayPin { pin: String },
}

impl Kind {
    /// 답을 기다리는 요청인지. 보이기만 하는 요청은 바로 답한다
    fn waits(&self) -> bool {
        !matches!(self, Self::DisplayPasskey { .. } | Self::DisplayPin { .. })
    }

    /// (메서드 이름, 인자)에서 요청과 기기 경로를 읽는다
    fn parse(method: &str, params: &glib::Variant) -> Option<(String, Kind)> {
        type Path = glib::variant::ObjectPath;
        let (device, kind) = match method {
            "RequestPinCode" => (params.get::<(Path,)>()?.0, Self::PinCode),
            "RequestPasskey" => (params.get::<(Path,)>()?.0, Self::Passkey),
            "RequestAuthorization" => (params.get::<(Path,)>()?.0, Self::Authorize),
            "RequestConfirmation" => {
                let (device, passkey) = params.get::<(Path, u32)>()?;
                (device, Self::Confirm { passkey })
            }
            "AuthorizeService" => {
                let (device, uuid) = params.get::<(Path, String)>()?;
                (device, Self::Service { uuid })
            }
            "DisplayPasskey" => {
                let (device, passkey, entered) = params.get::<(Path, u32, u16)>()?;
                (device, Self::DisplayPasskey { passkey, entered })
            }
            "DisplayPinCode" => {
                let (device, pin) = params.get::<(Path, String)>()?;
                (device, Self::DisplayPin { pin })
            }
            _ => return None,
        };
        Some((device.as_str().to_owned(), kind))
    }
}

/// 6자리 코드. BlueZ는 0으로 앞을 채워 보이라고 한다 ("012345")
pub(crate) fn passkey_text(passkey: u32) -> String {
    format!("{passkey:06}")
}

/// PIN은 1~16자(BlueZ). 사람이 칠 수 있는 ASCII만 받는다
pub(crate) fn pin_valid(pin: &str) -> bool {
    (1..=16).contains(&pin.len()) && pin.bytes().all(|byte| byte.is_ascii_graphic())
}

/// 6자리까지의 숫자 코드 (0~999999)
pub(crate) fn parse_passkey(text: &str) -> Option<u32> {
    let text = text.trim();
    if text.is_empty() || text.len() > 6 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// 기기 경로(/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF)의 주소. 이름을 모를 때 쓴다
fn address_of(path: &str) -> String {
    path.rsplit('/')
        .next()
        .and_then(|leaf| leaf.strip_prefix("dev_"))
        .map(|address| address.replace('_', ":"))
        .unwrap_or_else(|| path.to_owned())
}

/// 잘 알려진 Bluetooth 서비스 UUID(16비트)의 이름 메시지 ID. 모르면 None
fn service_id(uuid: &str) -> Option<&'static str> {
    let uuid = uuid.to_ascii_lowercase();
    let short = uuid
        .strip_prefix("0000")
        .and_then(|rest| rest.strip_suffix("-0000-1000-8000-00805f9b34fb"))?;
    Some(match short {
        // A2DP(소스·싱크), AVRCP
        "110a" | "110b" | "110c" | "110d" | "110e" | "110f" => "pairing-service-audio",
        // 헤드셋, 핸즈프리
        "1108" | "1112" | "111e" | "111f" | "1131" => "pairing-service-handsfree",
        // HID
        "1124" | "1812" => "pairing-service-input",
        // OBEX 파일 보내기·받기, 전화번호부, 메시지
        "1105" | "1106" | "112f" | "1132" | "1133" | "1134" => "pairing-service-files",
        // PAN 네트워크
        "1115" | "1116" | "1117" => "pairing-service-network",
        // 직렬 포트, 전화 접속
        "1101" | "1103" => "pairing-service-serial",
        _ => return None,
    })
}

/// 상대 기기
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Device {
    pub path: String,
    pub name: String,
    /// BlueZ의 Icon 속성 (audio-headset 등)
    pub icon: String,
    pub paired: bool,
}

/// 사람에게 물을 것
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub device: Device,
    pub kind: Kind,
}

/// 사람의 답
pub(crate) enum Answer {
    /// PIN
    Text(Secret),
    /// 6자리 코드
    Number(u32),
    /// 연결, 허용
    Accept,
    /// 취소, 거부
    Reject,
}

/// 물음에 답을 돌려주는 곳. 물음마다 번호가 있어 지난 물음의 늦은 답은 버린다
pub(crate) struct Responder {
    agent: Weak<Agent>,
    id: u64,
}

impl Responder {
    pub fn respond(&self, answer: Answer) {
        if let Some(agent) = self.agent.upgrade() {
            agent.answered(self.id, answer);
        }
    }
}

/// 사람에게 묻는 쪽. 셸에서는 GTK 창이고, 시험에서는 가짜다
pub(crate) trait Asker {
    /// 묻는다(보이기만 하는 요청도). 사람이 답하면 `responder`로 돌려준다
    fn ask(&self, prompt: &Prompt, responder: Responder);
    /// 보이는 코드 창의 상대가 친 자릿수를 바꾼다
    fn update_entered(&self, entered: u16);
    /// 떠 있는 창을 닫는다
    fn close(&self);
}

struct Current {
    id: u64,
    device: String,
    kind: Kind,
    /// 답을 기다리는 호출. 보이기만 하는 요청은 이미 답했다
    invocation: Option<gio::DBusMethodInvocation>,
    /// 창을 띄웠는지 (기기 이름을 읽는 동안은 아직이다)
    shown: bool,
    /// 시간이 지나면 닫는다
    timer: glib::JoinHandle<()>,
}

pub(crate) struct Agent {
    asker: Box<dyn Asker>,
    /// BlueZ(bluetoothd)의 고유 버스 이름. 이 이름이 부른 메서드만 받는다
    bluez: RefCell<Option<String>>,
    /// 기기 정보를 묻고 기기를 바꾸는 연결
    bus: RefCell<Option<gio::DBusConnection>>,
    current: RefCell<Option<Current>>,
    /// 보이는 코드 창의 기기가 페어링됐는지 지켜본다
    watch: RefCell<Option<gio::SignalSubscription>>,
    next_id: Cell<u64>,
    prompt_timeout: Duration,
    display_timeout: Duration,
}

impl Agent {
    pub fn new(
        asker: Box<dyn Asker>,
        prompt_timeout: Duration,
        display_timeout: Duration,
    ) -> Rc<Self> {
        Rc::new(Self {
            asker,
            bluez: RefCell::default(),
            bus: RefCell::default(),
            current: RefCell::default(),
            watch: RefCell::default(),
            next_id: Cell::new(1),
            prompt_timeout,
            display_timeout,
        })
    }

    fn export(self: &Rc<Self>, bus: &gio::DBusConnection) -> Result<(), glib::Error> {
        let node = gio::DBusNodeInfo::for_xml(INTROSPECTION)?;
        let interface = node
            .lookup_interface(AGENT_INTERFACE)
            .expect("interface in XML");
        let agent = self.clone();
        bus.register_object(AGENT_PATH, &interface)
            .method_call(move |_, sender, _, _, method, params, invocation| {
                agent.handle_call(sender, method, &params, invocation);
            })
            .build()?;
        self.bus.replace(Some(bus.clone()));
        Ok(())
    }

    fn handle_call(
        self: &Rc<Self>,
        sender: Option<&str>,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        // 버스 정책도 bluetoothd(root)만 이 인터페이스를 부르게 하지만, 가짜 페어링 창을 띄우지 못하게 한 번 더 본다
        if sender.is_none() || self.bluez.borrow().as_deref() != sender {
            invocation
                .return_dbus_error(ERROR_ACCESS_DENIED, "only BlueZ may call the pairing agent");
            return;
        }
        match method {
            "Release" => {
                // BlueZ가 이미 에이전트를 내렸다. 다시 뜨면 등록한다
                self.close_current(ERROR_CANCELED);
                invocation.return_value(None);
            }
            "Cancel" => {
                self.close_current(ERROR_CANCELED);
                invocation.return_value(None);
            }
            _ => match Kind::parse(method, params) {
                Some((device, kind)) => self.request(device, kind, invocation),
                None if method.starts_with("Request")
                    || method.starts_with("Display")
                    || method == "AuthorizeService" =>
                {
                    invocation.return_dbus_error(ERROR_INVALID_ARGS, "unexpected arguments");
                }
                None => invocation.return_dbus_error(ERROR_UNKNOWN_METHOD, "unknown method"),
            },
        }
    }

    fn request(self: &Rc<Self>, device: String, kind: Kind, invocation: gio::DBusMethodInvocation) {
        // 같은 기기의 코드를 보이는 중에 친 자릿수만 바뀌었다
        if let Kind::DisplayPasskey { passkey, entered } = &kind {
            let same = self.current.borrow().as_ref().is_some_and(|current| {
                current.device == device
                    && matches!(&current.kind, Kind::DisplayPasskey { passkey: shown, .. } if shown == passkey)
            });
            if same {
                invocation.return_value(None);
                if let Some(current) = self.current.borrow_mut().as_mut()
                    && let Kind::DisplayPasskey { entered: shown, .. } = &mut current.kind
                {
                    *shown = *entered;
                }
                self.asker.update_entered(*entered);
                return;
            }
        }
        // BlueZ는 답을 기다리는 요청을 하나씩 보내지만, 코드 보이기(Display*)는 그것과 따로 온다. 다른 기기의 답을
        // 기다리는 창이 떠 있으면 그 창을 지킨다. 예전에는 무엇이 오든 떠 있던 요청을 취소해, 사람이 고르지도 않은
        // 서비스 요청이 거절되거나 다른 기기의 코드 입력이 끊겼다
        let keep_waiting = !kind.waits()
            && self
                .current
                .borrow()
                .as_ref()
                .is_some_and(|current| current.kind.waits() && current.device != device);
        if keep_waiting {
            invocation.return_value(None);
            return;
        }
        // 앞의 창(코드를 보이던 창, 같은 기기의 물음)은 닫는다
        self.close_current(ERROR_CANCELED);
        let invocation = if kind.waits() {
            Some(invocation)
        } else {
            // 보이기만 하는 요청은 바로 답해야 페어링이 이어진다
            invocation.return_value(None);
            None
        };
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let timeout = if kind.waits() {
            self.prompt_timeout
        } else {
            self.display_timeout
        };
        let weak = Rc::downgrade(self);
        let timer = glib::spawn_future_local(async move {
            glib::timeout_future(timeout).await;
            if let Some(agent) = weak.upgrade() {
                agent.expire(id);
            }
        });
        self.current.replace(Some(Current {
            id,
            device: device.clone(),
            kind,
            invocation,
            shown: false,
            timer,
        }));
        let agent = self.clone();
        glib::spawn_future_local(async move {
            let info = agent.lookup(&device).await;
            agent.show(id, info);
        });
    }

    /// 기기의 이름과 종류. 읽지 못하면 주소로 부른다
    async fn lookup(&self, path: &str) -> Device {
        let mut device = Device {
            path: path.to_owned(),
            name: address_of(path),
            icon: String::new(),
            paired: false,
        };
        let (Some(bus), Some(bluez)) = (self.bus.borrow().clone(), self.bluez.borrow().clone())
        else {
            return device;
        };
        let reply = bus
            .call_future(
                Some(&bluez),
                path,
                PROPERTIES,
                "GetAll",
                Some(&(DEVICE_INTERFACE,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                LOOKUP_TIMEOUT_MS,
            )
            .await;
        let Some(props) = reply
            .ok()
            .and_then(|reply| reply.child_value(0).get::<HashMap<String, glib::Variant>>())
        else {
            return device;
        };
        let text = |key: &str| {
            props
                .get(key)
                .and_then(|value| value.str().map(str::to_owned))
                .filter(|value| !value.is_empty())
        };
        if let Some(name) = text("Alias").or_else(|| text("Name")) {
            device.name = name;
        }
        device.icon = text("Icon").unwrap_or_default();
        let flag = |key: &str| {
            props
                .get(key)
                .and_then(|value| value.get::<bool>())
                .unwrap_or(false)
        };
        // 키만 남은 기기(Bonded)도 페어링된 것으로 본다
        device.paired = flag("Paired") || flag("Bonded");
        device
    }

    /// 기기 정보를 읽었다. 그사이 요청이 거둬지지 않았으면 창을 띄운다
    fn show(self: &Rc<Self>, id: u64, device: Device) {
        let kind = {
            let mut current = self.current.borrow_mut();
            let Some(current) = current.as_mut().filter(|current| current.id == id) else {
                return;
            };
            current.shown = true;
            current.kind.clone()
        };
        if !kind.waits() {
            self.watch_paired(&device.path);
        }
        self.asker.ask(
            &Prompt { device, kind },
            Responder {
                agent: Rc::downgrade(self),
                id,
            },
        );
    }

    /// 코드를 보이는 동안 그 기기가 페어링되면 창을 닫는다. BlueZ는 끝났다고 알리지 않는다
    fn watch_paired(self: &Rc<Self>, path: &str) {
        let (Some(bus), Some(bluez)) = (self.bus.borrow().clone(), self.bluez.borrow().clone())
        else {
            return;
        };
        let weak = Rc::downgrade(self);
        let subscription = bus.subscribe_to_signal(
            Some(&bluez),
            Some(PROPERTIES),
            Some("PropertiesChanged"),
            Some(path),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let Some((interface, changed, _)) =
                    signal
                        .parameters
                        .get::<(String, HashMap<String, glib::Variant>, Vec<String>)>()
                else {
                    return;
                };
                let paired = ["Paired", "Bonded"].iter().any(|key| {
                    changed
                        .get(*key)
                        .and_then(|value| value.get::<bool>())
                        .unwrap_or(false)
                });
                if interface == DEVICE_INTERFACE
                    && paired
                    && let Some(agent) = weak.upgrade()
                {
                    agent.close_current(ERROR_CANCELED);
                }
            },
        );
        self.watch.replace(Some(subscription));
    }

    fn take_current(&self, id: u64) -> Option<Current> {
        let mut current = self.current.borrow_mut();
        if current.as_ref()?.id != id {
            return None;
        }
        current.take()
    }

    fn answered(self: &Rc<Self>, id: u64, answer: Answer) {
        let Some(current) = self.take_current(id) else {
            return;
        };
        current.timer.abort();
        self.watch.replace(None);
        let Some(invocation) = current.invocation else {
            // 코드를 보이던 창에서 사람이 취소했다: 페어링을 멈춘다
            if matches!(answer, Answer::Reject) {
                self.cancel_pairing_if_unpaired(&current.device);
            }
            return;
        };
        match (&current.kind, answer) {
            (_, Answer::Reject) => {
                invocation.return_dbus_error(ERROR_REJECTED, "the user rejected the request")
            }
            (Kind::PinCode, Answer::Text(pin)) if pin_valid(pin.as_str()) => {
                invocation.return_value(Some(&(pin.as_str(),).to_variant()))
            }
            (Kind::Passkey, Answer::Number(passkey)) if passkey <= 999_999 => {
                invocation.return_value(Some(&(passkey,).to_variant()))
            }
            (Kind::Confirm { .. } | Kind::Authorize, Answer::Accept) => {
                invocation.return_value(None)
            }
            // 이번 연결만 허용한다. 예전에는 믿는 기기로 두어, 오디오를 허용한 기기가 키보드 입력(HID) 같은 다른
            // 서비스로도 묻지 않고 붙을 수 있었다
            (Kind::Service { .. }, Answer::Accept) => invocation.return_value(None),
            // 요청에 맞지 않는 답은 거절로 본다
            _ => invocation.return_dbus_error(ERROR_REJECTED, "unexpected answer"),
        }
    }

    /// 떠 있는 요청을 닫는다. 답을 기다리던 호출에는 `error`로 답한다
    fn close_current(&self, error: &str) {
        let Some(current) = self.current.take() else {
            return;
        };
        current.timer.abort();
        self.watch.replace(None);
        if current.shown {
            self.asker.close();
        }
        if let Some(invocation) = current.invocation {
            invocation.return_dbus_error(error, "the request was canceled");
        }
    }

    /// 아무도 제때 답하지 않았다(코드를 보이던 창은 페어링이 끝났다는 알림이 없었다)
    fn expire(&self, id: u64) {
        // 이 일은 시간을 센 future 안에서 돈다. 그 future는 여기서 끝나므로 멈추지 않는다
        let Some(current) = self.take_current(id) else {
            return;
        };
        self.watch.replace(None);
        if current.shown {
            self.asker.close();
        }
        if let Some(invocation) = current.invocation {
            invocation.return_dbus_error(ERROR_CANCELED, "nobody answered in time");
        }
    }

    /// Device1.CancelPairing: 코드를 보이던 페어링을 사람이 취소했다. 이미 페어링된 기기(다시 짝짓다 끝난 기기 등)에는
    /// 부르지 않는다. BlueZ는 진행 중인 페어링이 없으면 이 호출로 페어링을 지운다(device.c cancel_pairing →
    /// btd_adapter_remove_bonding). 예전에는 늘 불러, 다시 짝지은 키보드의 코드 창을 닫다가 짝짓기를 지울 수 있었다
    fn cancel_pairing_if_unpaired(self: &Rc<Self>, device: &str) {
        let agent = self.clone();
        let device = device.to_owned();
        glib::spawn_future_local(async move {
            if !agent.lookup(&device).await.paired {
                agent.call_device(&device, DEVICE_INTERFACE, "CancelPairing", None);
            }
        });
    }

    /// 기기에 메서드를 부른다. 결과는 기다리지 않는다
    fn call_device(
        &self,
        device: &str,
        interface: &str,
        method: &'static str,
        args: Option<glib::Variant>,
    ) {
        let (Some(bus), Some(bluez)) = (self.bus.borrow().clone(), self.bluez.borrow().clone())
        else {
            return;
        };
        bus.call(
            Some(&bluez),
            device,
            interface,
            method,
            args.as_ref(),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |result| {
                if let Err(err) = result {
                    eprintln!("bunt-shell: Bluetooth {method}: {err}");
                }
            },
        );
    }
}

/// 세션의 기본 Bluetooth 페어링 에이전트가 된다. bluetoothd가 (다시) 뜰 때마다 등록한다
pub fn start(app: &gtk::Application, tr: &Rc<Translations>) {
    let dialogs = Dialogs {
        app: app.clone(),
        tr: tr.clone(),
        open: RefCell::default(),
    };
    let agent = Agent::new(Box::new(dialogs), PROMPT_TIMEOUT, DISPLAY_TIMEOUT);
    glib::spawn_future_local(async move {
        let bus = match gio::bus_get_future(gio::BusType::System).await {
            Ok(bus) => bus,
            Err(err) => {
                eprintln!("bunt-shell: no system bus, Bluetooth pairing is off: {err}");
                return;
            }
        };
        if let Err(err) = agent.export(&bus) {
            eprintln!("bunt-shell: could not export the Bluetooth pairing agent: {err}");
            return;
        }
        let (appeared, vanished) = (agent.clone(), agent.clone());
        // 감시는 셸이 끝날 때까지 둔다. 어댑터가 없으면 bluetoothd를 깨우지 않는다. gio-rs의 이름 감시를
        // 그대로 쓰면 버스 데몬이 갑자기 끝날 때 GLib이 사라짐 처리기에 준 NULL 연결을 unwrap해 셸이 죽었다.
        // 그 일을 막는 dbus::watch_name_owner로 옮겼다
        crate::dbus::watch_name_owner(
            &bus,
            BLUEZ,
            gio::BusNameWatcherFlags::NONE,
            move |bus, owner| {
                appeared.bluez.replace(Some(owner.to_owned()));
                glib::spawn_future_local(async move {
                    if let Err(err) = register(&bus).await {
                        eprintln!(
                            "bunt-shell: could not register the Bluetooth pairing agent: {err}"
                        );
                    }
                });
            },
            move || {
                vanished.close_current(ERROR_CANCELED);
                vanished.bluez.replace(None);
            },
        );
    });
}

/// AgentManager1.RegisterAgent와 RequestDefaultAgent
async fn register(bus: &gio::DBusConnection) -> Result<(), glib::Error> {
    let path = glib::variant::ObjectPath::try_from(AGENT_PATH).expect("valid object path");
    let call = |method: &'static str, args: glib::Variant| {
        bus.call_future(
            Some(BLUEZ),
            MANAGER_PATH,
            MANAGER_INTERFACE,
            method,
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
    };
    if let Err(err) = call("RegisterAgent", (path.clone(), CAPABILITY).to_variant()).await {
        // 같은 연결로 이미 등록했다면(bluetoothd가 그대로인데 이름 감시가 다시 알렸다) 기본 에이전트만 청한다
        let exists = gio::DBusError::remote_error(&err)
            .is_some_and(|name| name == "org.bluez.Error.AlreadyExists");
        if !exists {
            return Err(err);
        }
    }
    call("RequestDefaultAgent", (path,).to_variant()).await?;
    Ok(())
}

/// 셸의 Bluetooth 페어링 창들. 한 번에 하나만 뜬다
struct Dialogs {
    app: gtk::Application,
    tr: Rc<Translations>,
    open: RefCell<Option<Rc<PairingDialog>>>,
}

impl Asker for Dialogs {
    fn ask(&self, prompt: &Prompt, responder: Responder) {
        let dialog = PairingDialog::new(&self.app, &self.tr, prompt, responder);
        if let Some(old) = self.open.replace(Some(dialog.clone())) {
            old.frame.close();
        }
        dialog.present();
    }

    fn update_entered(&self, entered: u16) {
        if let Some(dialog) = self.open.borrow().as_ref() {
            dialog.set_entered(entered);
        }
    }

    fn close(&self) {
        if let Some(dialog) = self.open.take() {
            dialog.frame.close();
        }
    }
}

/// 6자리 코드의 마크업. 상대가 친 자릿수만큼 앞을 흐리게 한다 (macOS 키보드 페어링처럼 진행을 보인다)
fn code_markup(code: &str, entered: u16) -> String {
    let split = code
        .char_indices()
        .nth(usize::from(entered))
        .map_or(code.len(), |(index, _)| index);
    let (typed, rest) = code.split_at(split);
    if typed.is_empty() {
        glib::markup_escape_text(rest).to_string()
    } else {
        format!(
            "<span alpha=\"35%\">{}</span>{}",
            glib::markup_escape_text(typed),
            glib::markup_escape_text(rest)
        )
    }
}

/// 설명과 단추 이름 (메시지 ID)
struct Wording {
    title: &'static str,
    message: &'static str,
    /// 취소·거부
    cancel: &'static str,
    /// 연결·허용. 보이기만 하는 창에는 없다
    confirm: Option<&'static str>,
}

fn wording(kind: &Kind) -> Wording {
    let pairing = |message, confirm| Wording {
        title: "pairing-title",
        message,
        cancel: "pairing-cancel",
        confirm,
    };
    match kind {
        Kind::Confirm { .. } => pairing("pairing-confirm", Some("pairing-connect")),
        Kind::DisplayPasskey { .. } | Kind::DisplayPin { .. } => pairing("pairing-display", None),
        Kind::PinCode => pairing("pairing-pin", Some("pairing-connect")),
        Kind::Passkey => pairing("pairing-passkey", Some("pairing-connect")),
        Kind::Authorize => Wording {
            title: "pairing-title",
            message: "pairing-authorize",
            cancel: "pairing-deny",
            confirm: Some("pairing-connect"),
        },
        Kind::Service { .. } => Wording {
            title: "pairing-service-title",
            message: "pairing-service",
            cancel: "pairing-deny",
            confirm: Some("pairing-allow"),
        },
    }
}

/// 설명. $name은 기기 이름, $service는 서비스 이름
fn message(tr: &Translations, prompt: &Prompt) -> String {
    let mut args = FluentArgs::new();
    args.set("name", prompt.device.name.clone());
    if let Kind::Service { uuid } = &prompt.kind {
        let service = service_id(uuid).map_or_else(|| uuid.clone(), |id| tr.get(id));
        args.set("service", service);
    }
    tr.get_with(wording(&prompt.kind).message, &args)
}

/// Bluetooth 페어링 창 (macOS의 페어링 요청): 기기 그림, 제목, 설명, 코드나 칸, 단추
struct PairingDialog {
    frame: Frame,
    kind: Kind,
    /// 보이는 코드 (견주기, 보이기)
    code: Option<gtk::Label>,
    /// PIN이나 코드를 받는 칸
    entry: Option<gtk::Entry>,
    confirm: Option<gtk::Button>,
    responder: RefCell<Option<Responder>>,
}

impl PairingDialog {
    fn new(
        app: &gtk::Application,
        tr: &Translations,
        prompt: &Prompt,
        responder: Responder,
    ) -> Rc<PairingDialog> {
        // 상대 기기가 먼저 청할 수 있으므로 늘 창을 눌러야 키보드를 받는다
        let frame = Frame::new(app, NAMESPACE, KeyboardMode::OnDemand);
        frame.tile(device_icon(&prompt.device.icon), "service-tile");
        let words = wording(&prompt.kind);
        frame.title(&tr.get(words.title));
        frame.message(&message(tr, prompt));
        let shown_code = match &prompt.kind {
            Kind::Confirm { passkey } | Kind::DisplayPasskey { passkey, .. } => {
                Some((passkey_text(*passkey), 0))
            }
            Kind::DisplayPin { pin } => Some((pin.clone(), 0)),
            _ => None,
        };
        let code = shown_code.map(|(text, entered)| {
            let label = gtk::Label::builder()
                .use_markup(true)
                .label(code_markup(&text, entered))
                .selectable(false)
                .css_classes(["pairing-code"])
                .build();
            frame.panel.append(&label);
            label
        });
        let entry = matches!(prompt.kind, Kind::PinCode | Kind::Passkey).then(|| {
            let fields = frame.fields();
            let placeholder = if prompt.kind == Kind::PinCode {
                tr.get("pairing-pin-placeholder")
            } else {
                tr.get("pairing-passkey-placeholder")
            };
            let entry = service_dialog::text_field(&fields, &placeholder, "");
            if prompt.kind == Kind::Passkey {
                entry.set_input_purpose(gtk::InputPurpose::Digits);
                entry.set_max_length(6);
            } else {
                entry.set_max_length(16);
            }
            entry
        });
        let (cancel, confirm) = frame.buttons(
            &tr.get(words.cancel),
            words.confirm.map(|id| tr.get(id)).as_deref(),
        );
        if let (Some(confirm), Some(_)) = (&confirm, &entry) {
            confirm.set_sensitive(false);
        }
        let dialog = Rc::new(PairingDialog {
            frame,
            kind: prompt.kind.clone(),
            code,
            entry,
            confirm,
            responder: RefCell::new(Some(responder)),
        });
        if let Some(entry) = &dialog.entry {
            let weak = Rc::downgrade(&dialog);
            entry.connect_changed(move |entry| {
                if let Some(dialog) = weak.upgrade()
                    && let Some(confirm) = &dialog.confirm
                {
                    confirm.set_sensitive(dialog.answer_from(&entry.text()).is_some());
                }
            });
        }
        if let Some(confirm) = &dialog.confirm {
            let weak = Rc::downgrade(&dialog);
            confirm.connect_clicked(move |_| {
                if let Some(dialog) = weak.upgrade() {
                    dialog.submit();
                }
            });
        }
        let weak = Rc::downgrade(&dialog);
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.respond(Answer::Reject);
            }
        });
        let weak = Rc::downgrade(&dialog);
        dialog.frame.on_escape(move || {
            if let Some(dialog) = weak.upgrade() {
                dialog.respond(Answer::Reject);
            }
        });
        // 컴포지터가 창을 닫으면(화면을 뺐을 때) 거절로 답해 BlueZ가 60초를 기다리지 않게 한다
        let weak = Rc::downgrade(&dialog);
        dialog.frame.on_dismiss(move || {
            if let Some(dialog) = weak.upgrade() {
                dialog.respond(Answer::Reject);
            }
        });
        dialog
    }

    fn present(&self) {
        let focus = self
            .entry
            .as_ref()
            .map(|entry| entry.upcast_ref::<gtk::Widget>());
        self.frame.present(focus);
    }

    fn set_entered(&self, entered: u16) {
        if let (Some(code), Kind::DisplayPasskey { passkey, .. }) = (&self.code, &self.kind) {
            code.set_markup(&code_markup(&passkey_text(*passkey), entered));
        }
    }

    /// 칸의 글을 답으로 바꾼다. 받을 수 없는 글이면 None
    fn answer_from(&self, text: &str) -> Option<Answer> {
        match self.kind {
            Kind::PinCode => pin_valid(text).then(|| Answer::Text(Secret::new(text))),
            Kind::Passkey => parse_passkey(text).map(Answer::Number),
            _ => Some(Answer::Accept),
        }
    }

    fn submit(&self) {
        let answer = match &self.entry {
            Some(entry) => {
                let Some(answer) = self.answer_from(&entry.text()) else {
                    return;
                };
                entry.set_text("");
                answer
            }
            None => Answer::Accept,
        };
        self.respond(answer);
    }

    fn respond(&self, answer: Answer) {
        if let Some(responder) = self.responder.take() {
            self.frame.close();
            responder.respond(answer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_read_from_method_arguments() {
        let device =
            glib::variant::ObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap();
        let parse = |method: &str, params: glib::Variant| Kind::parse(method, &params);
        assert_eq!(
            parse("RequestConfirmation", (device.clone(), 42u32).to_variant()),
            Some((device.as_str().to_owned(), Kind::Confirm { passkey: 42 }))
        );
        assert_eq!(
            parse("DisplayPasskey", (device.clone(), 7u32, 3u16).to_variant())
                .map(|(_, kind)| kind),
            Some(Kind::DisplayPasskey {
                passkey: 7,
                entered: 3
            })
        );
        assert_eq!(
            parse("RequestPinCode", (device.clone(),).to_variant()).map(|(_, kind)| kind),
            Some(Kind::PinCode)
        );
        assert_eq!(
            parse(
                "AuthorizeService",
                (device.clone(), "0000110b-0000-1000-8000-00805f9b34fb").to_variant()
            )
            .map(|(_, kind)| kind),
            Some(Kind::Service {
                uuid: "0000110b-0000-1000-8000-00805f9b34fb".into()
            })
        );
        // 인자 형식이 다르면 받지 않는다
        assert_eq!(
            parse("RequestConfirmation", (device.clone(),).to_variant()),
            None
        );
        assert_eq!(parse("Release", ().to_variant()), None);
        assert!(!Kind::DisplayPin { pin: "0000".into() }.waits());
        assert!(Kind::Authorize.waits());
    }

    #[test]
    fn codes_are_six_digits_and_pins_are_short_ascii() {
        assert_eq!(passkey_text(42), "000042");
        assert_eq!(passkey_text(999_999), "999999");
        assert_eq!(parse_passkey("012345"), Some(12_345));
        assert_eq!(parse_passkey(" 7 "), Some(7));
        assert_eq!(parse_passkey("1234567"), None);
        assert_eq!(parse_passkey("12a4"), None);
        assert_eq!(parse_passkey(""), None);
        assert!(pin_valid("0000"));
        assert!(pin_valid(&"9".repeat(16)));
        assert!(!pin_valid(&"9".repeat(17)));
        assert!(!pin_valid(""));
        assert!(!pin_valid("12 34"));
        assert!(!pin_valid("한글"));
    }

    #[test]
    fn devices_without_names_are_called_by_address() {
        assert_eq!(
            address_of("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"),
            "AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(address_of("/odd"), "/odd");
    }

    #[test]
    fn well_known_services_have_names() {
        assert_eq!(
            service_id("0000110B-0000-1000-8000-00805F9B34FB"),
            Some("pairing-service-audio")
        );
        assert_eq!(
            service_id("0000111e-0000-1000-8000-00805f9b34fb"),
            Some("pairing-service-handsfree")
        );
        assert_eq!(service_id("00001234-0000-1000-8000-00805f9b34fb"), None);
        assert_eq!(service_id("not-a-uuid"), None);
    }

    #[test]
    fn typed_digits_are_dimmed() {
        assert_eq!(code_markup("123456", 0), "123456");
        assert_eq!(
            code_markup("123456", 2),
            "<span alpha=\"35%\">12</span>3456"
        );
        // 다 쳤거나 넘으면 모두 흐리게
        assert_eq!(
            code_markup("123456", 9),
            "<span alpha=\"35%\">123456</span>"
        );
    }

    #[test]
    fn messages_name_the_device() {
        let ko = Translations::from_source("ko", crate::TRANSLATIONS[0].1);
        let prompt = |kind| Prompt {
            device: Device {
                path: "/org/bluez/hci0/dev_AA".into(),
                name: "Magic Keyboard".into(),
                icon: "input-keyboard".into(),
                paired: false,
            },
            kind,
        };
        assert!(message(&ko, &prompt(Kind::Confirm { passkey: 1 })).contains("“Magic Keyboard”"));
        let service = message(
            &ko,
            &prompt(Kind::Service {
                uuid: "0000110b-0000-1000-8000-00805f9b34fb".into(),
            }),
        );
        assert!(service.contains("오디오"), "{service}");
        // 모르는 서비스는 UUID를 그대로 보인다
        let unknown = message(
            &ko,
            &prompt(Kind::Service {
                uuid: "abcd".into(),
            }),
        );
        assert!(unknown.contains("abcd"), "{unknown}");
    }

    /// 가짜 묻는 쪽: 물은 것을 적어 두고, 정해 둔 답이 있으면 바로 답한다
    #[derive(Default)]
    struct Fake {
        asked: Rc<RefCell<Vec<Prompt>>>,
        closed: Rc<Cell<u32>>,
        entered: Rc<RefCell<Vec<u16>>>,
        /// 다음 물음의 답 (None이면 기다린다)
        answers: Rc<RefCell<std::collections::VecDeque<Option<Answer>>>>,
        waiting: Rc<RefCell<Option<Responder>>>,
    }

    impl Asker for Fake {
        fn ask(&self, prompt: &Prompt, responder: Responder) {
            self.asked.borrow_mut().push(prompt.clone());
            match self.answers.borrow_mut().pop_front().flatten() {
                Some(answer) => responder.respond(answer),
                None => {
                    self.waiting.replace(Some(responder));
                }
            }
        }

        fn update_entered(&self, entered: u16) {
            self.entered.borrow_mut().push(entered);
        }

        fn close(&self) {
            self.closed.set(self.closed.get() + 1);
            self.waiting.replace(None);
        }
    }

    async fn connect(address: &str) -> gio::DBusConnection {
        gio::DBusConnection::for_address_future(
            address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None,
        )
        .await
        .expect("a connection to the test bus")
    }

    fn dbus_error_name(error: &glib::Error) -> String {
        gio::DBusError::remote_error(error)
            .map(|name| name.to_string())
            .unwrap_or_default()
    }

    async fn call(
        caller: &gio::DBusConnection,
        agent: &str,
        method: &str,
        args: glib::Variant,
    ) -> Result<glib::Variant, glib::Error> {
        caller
            .call_future(
                Some(agent),
                AGENT_PATH,
                AGENT_INTERFACE,
                method,
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                5_000,
            )
            .await
    }

    const DEVICE: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF";

    /// BlueZ처럼 부르는 쪽에서 요청마다 맞는 답이 돌아온다. 기기 정보를 읽지 못하면 주소로 부른다
    #[test]
    fn requests_round_trip_over_dbus() {
        let Some((_serial, bus)) = crate::service_dialog::test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let agent_bus = connect(&address).await;
                    let bluez = connect(&address).await;
                    let stranger = connect(&address).await;
                    let fake = Fake::default();
                    let (asked, answers) = (fake.asked.clone(), fake.answers.clone());
                    let agent = Agent::new(Box::new(fake), PROMPT_TIMEOUT, DISPLAY_TIMEOUT);
                    agent.export(&agent_bus).unwrap();
                    agent
                        .bluez
                        .replace(bluez.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let device = glib::variant::ObjectPath::try_from(DEVICE).unwrap();

                    answers.borrow_mut().push_back(Some(Answer::Accept));
                    call(
                        &bluez,
                        &name,
                        "RequestConfirmation",
                        (device.clone(), 123_456u32).to_variant(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(asked.borrow()[0].device.name, "AA:BB:CC:DD:EE:FF");
                    assert_eq!(asked.borrow()[0].kind, Kind::Confirm { passkey: 123_456 });

                    answers.borrow_mut().push_back(Some(Answer::Reject));
                    let error = call(
                        &bluez,
                        &name,
                        "RequestAuthorization",
                        (device.clone(),).to_variant(),
                    )
                    .await
                    .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_REJECTED);

                    answers
                        .borrow_mut()
                        .push_back(Some(Answer::Text(Secret::new("0000"))));
                    let reply = call(
                        &bluez,
                        &name,
                        "RequestPinCode",
                        (device.clone(),).to_variant(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(reply.get::<(String,)>(), Some(("0000".to_owned(),)));

                    answers.borrow_mut().push_back(Some(Answer::Number(42)));
                    let reply = call(
                        &bluez,
                        &name,
                        "RequestPasskey",
                        (device.clone(),).to_variant(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(reply.get::<(u32,)>(), Some((42,)));

                    // 요청에 맞지 않는 답(코드를 물었는데 PIN 글)은 거절이다
                    answers
                        .borrow_mut()
                        .push_back(Some(Answer::Text(Secret::new("abc"))));
                    let error = call(
                        &bluez,
                        &name,
                        "RequestPasskey",
                        (device.clone(),).to_variant(),
                    )
                    .await
                    .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_REJECTED);

                    // BlueZ가 아닌 쪽은 창을 띄우지 못한다
                    let before = asked.borrow().len();
                    let error = call(
                        &stranger,
                        &name,
                        "RequestAuthorization",
                        (device.clone(),).to_variant(),
                    )
                    .await
                    .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_ACCESS_DENIED);
                    assert_eq!(asked.borrow().len(), before);
                })
            })
            .unwrap();
        bus.down();
    }

    /// 코드를 보이는 요청은 바로 답하고, 친 자릿수만 바뀌면 같은 창을 고친다. Cancel은 기다리던 요청을
    /// Canceled로 끝내고 창을 닫는다
    #[test]
    fn displays_answer_at_once_and_cancel_closes() {
        let Some((_serial, bus)) = crate::service_dialog::test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let agent_bus = connect(&address).await;
                    let bluez = connect(&address).await;
                    let fake = Fake::default();
                    let (asked, closed, entered, waiting, answers) = (
                        fake.asked.clone(),
                        fake.closed.clone(),
                        fake.entered.clone(),
                        fake.waiting.clone(),
                        fake.answers.clone(),
                    );
                    let agent = Agent::new(Box::new(fake), PROMPT_TIMEOUT, DISPLAY_TIMEOUT);
                    agent.export(&agent_bus).unwrap();
                    agent
                        .bluez
                        .replace(bluez.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let device = glib::variant::ObjectPath::try_from(DEVICE).unwrap();

                    answers.borrow_mut().push_back(None);
                    call(
                        &bluez,
                        &name,
                        "DisplayPasskey",
                        (device.clone(), 1234u32, 0u16).to_variant(),
                    )
                    .await
                    .unwrap();
                    while waiting.borrow().is_none() {
                        glib::timeout_future(Duration::from_millis(10)).await;
                    }
                    call(
                        &bluez,
                        &name,
                        "DisplayPasskey",
                        (device.clone(), 1234u32, 4u16).to_variant(),
                    )
                    .await
                    .unwrap();
                    assert_eq!(asked.borrow().len(), 1);
                    assert_eq!(*entered.borrow(), [4]);

                    // 새 요청은 코드 창을 닫고 묻는다
                    answers.borrow_mut().push_back(None);
                    let confirmation = call(
                        &bluez,
                        &name,
                        "RequestConfirmation",
                        (device.clone(), 1u32).to_variant(),
                    );
                    let cancel = async {
                        while asked.borrow().len() < 2 || waiting.borrow().is_none() {
                            glib::timeout_future(Duration::from_millis(10)).await;
                        }
                        call(&bluez, &name, "Cancel", ().to_variant())
                            .await
                            .unwrap();
                    };
                    let (confirmation, ()) = join(confirmation, cancel).await;
                    assert_eq!(dbus_error_name(&confirmation.unwrap_err()), ERROR_CANCELED);
                    assert_eq!(closed.get(), 2);
                })
            })
            .unwrap();
        bus.down();
    }

    /// 다른 기기의 답을 기다리는 창이 떠 있는 동안 온 코드 보이기는 그 창을 닫지 않는다
    #[test]
    fn displays_do_not_cancel_another_devices_prompt() {
        let Some((_serial, bus)) = crate::service_dialog::test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let agent_bus = connect(&address).await;
                    let bluez = connect(&address).await;
                    let fake = Fake::default();
                    let (asked, closed, waiting, answers) = (
                        fake.asked.clone(),
                        fake.closed.clone(),
                        fake.waiting.clone(),
                        fake.answers.clone(),
                    );
                    let agent = Agent::new(Box::new(fake), PROMPT_TIMEOUT, DISPLAY_TIMEOUT);
                    agent.export(&agent_bus).unwrap();
                    agent
                        .bluez
                        .replace(bluez.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let headset = glib::variant::ObjectPath::try_from(DEVICE).unwrap();
                    let keyboard = glib::variant::ObjectPath::try_from(
                        "/org/bluez/hci0/dev_11_22_33_44_55_66",
                    )
                    .unwrap();

                    answers.borrow_mut().push_back(None);
                    let service = call(
                        &bluez,
                        &name,
                        "AuthorizeService",
                        (headset.clone(), "0000110b-0000-1000-8000-00805f9b34fb").to_variant(),
                    );
                    let display_then_answer = async {
                        while waiting.borrow().is_none() {
                            glib::timeout_future(Duration::from_millis(10)).await;
                        }
                        call(
                            &bluez,
                            &name,
                            "DisplayPasskey",
                            (keyboard.clone(), 4321u32, 0u16).to_variant(),
                        )
                        .await
                        .unwrap();
                        // 헤드폰의 물음은 그대로 떠 있다
                        assert_eq!(closed.get(), 0);
                        assert_eq!(asked.borrow().len(), 1);
                        let responder = waiting.borrow_mut().take().unwrap();
                        responder.respond(Answer::Accept);
                    };
                    let (service, ()) = join(service, display_then_answer).await;
                    service.unwrap();
                })
            })
            .unwrap();
        bus.down();
    }

    /// 아무도 답하지 않으면 시간이 지나 Canceled로 답하고 창을 닫는다
    #[test]
    fn unanswered_requests_expire() {
        let Some((_serial, bus)) = crate::service_dialog::test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let agent_bus = connect(&address).await;
                    let bluez = connect(&address).await;
                    let fake = Fake::default();
                    let (closed, answers) = (fake.closed.clone(), fake.answers.clone());
                    let agent = Agent::new(
                        Box::new(fake),
                        Duration::from_millis(300),
                        Duration::from_millis(300),
                    );
                    agent.export(&agent_bus).unwrap();
                    agent
                        .bluez
                        .replace(bluez.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let device = glib::variant::ObjectPath::try_from(DEVICE).unwrap();
                    answers.borrow_mut().push_back(None);
                    let error = call(&bluez, &name, "RequestPasskey", (device,).to_variant())
                        .await
                        .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_CANCELED);
                    assert_eq!(closed.get(), 1);
                })
            })
            .unwrap();
        bus.down();
    }

    /// 두 future를 함께 돌린다. a를 먼저 처음 돌린다
    async fn join<A, B>(
        a: impl std::future::Future<Output = A>,
        b: impl std::future::Future<Output = B>,
    ) -> (A, B) {
        use std::task::Poll;
        let (mut a, mut b) = (std::pin::pin!(a), std::pin::pin!(b));
        let (mut ra, mut rb) = (None, None);
        std::future::poll_fn(|cx| {
            if ra.is_none()
                && let Poll::Ready(value) = a.as_mut().poll(cx)
            {
                ra = Some(value);
            }
            if rb.is_none()
                && let Poll::Ready(value) = b.as_mut().poll(cx)
            {
                rb = Some(value);
            }
            if ra.is_some() && rb.is_some() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        (ra.unwrap(), rb.unwrap())
    }
}
