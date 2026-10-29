// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! NetworkManager 비밀 에이전트 (org.freedesktop.NetworkManager.SecretAgent).
//!
//! NetworkManager가 연결에 필요한 비밀(Wi-Fi 암호, 802.1X 암호)이 없거나 틀렸을 때 이 에이전트에게
//! 묻는다. macOS의 Wi-Fi 암호 창처럼 화면 가운데에 "Wi-Fi 네트워크 “이름”에 WPA2 암호가 필요합니다."를
//! 띄우고 사람이 답한다.
//! - 메뉴 막대에서 고른 새 네트워크, 암호가 바뀐 저장된 네트워크, 틀린 암호를 다시 묻는 때(REQUEST_NEW)에
//!   뜬다. 다시 물을 때는 암호가 올바르지 않다고 알린다.
//! - 받은 비밀은 NetworkManager에 돌려줄 뿐 저장하지 않는다. 시스템 연결의 비밀(psk-flags 0)은
//!   NetworkManager가 연결 파일에 둔다. 그때 NetworkManager는 이 에이전트의 사용자에게
//!   settings.modify.system 권한이 있는지 polkit에 묻는다(관리자 인증 창이 뜰 수 있다).
//! - 비밀은 기록에 남기지 않고, 답한 뒤 사본을 0으로 덮는다(service_dialog::Secret).
//! - 창은 사람만 답한다. 레이어 이름(NAMESPACE)을 보고 컴포지터가 에이전트 입력을 거절하고 buntd-agent가
//!   읽지 않는다. 사람이 고른 연결(USER_REQUESTED)이 아니면 창을 눌러야 키보드를 받는다. 저절로 뜬 창이
//!   다른 앱에 치던 글을 받아 암호로 보내지 않게 한다.
//!
//! 순서와 오류는 NetworkManager의 nm-agent-manager.c, nm-secret-agent.c와 같다.
//! - GetSecrets는 120초 안에 답해야 한다. 그 뒤의 답은 버려지므로 115초에 창을 닫는다.
//! - 사람이 취소하면 UserCanceled로 답한다. NetworkManager는 다른 에이전트에게 묻지 않고 연결을 멈춘다.
//! - 묻지 않는 요청(상호작용 금지, 다루지 않는 설정)에는 NoSecrets로 답한다. 다음 에이전트로 넘어간다.
//! - CancelGetSecrets가 오면 그 물음을 닫고 AgentCanceled로 답한다.

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::{Rc, Weak},
    time::Duration,
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::KeyboardMode;

use crate::service_dialog::{self, Frame, Secret};

/// 창의 레이어 이름이자 접근성 이름. 사람만 답하는 창의 목록(comp/src/agent.rs, agent/src/atspi.rs)에 있다
pub(crate) const NAMESPACE: &str = "bunt-secret-agent";
const NM_NAME: &str = "org.freedesktop.NetworkManager";
const AGENT_MANAGER_PATH: &str = "/org/freedesktop/NetworkManager/AgentManager";
const AGENT_MANAGER_INTERFACE: &str = "org.freedesktop.NetworkManager.AgentManager";
const AGENT_PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
const AGENT_INTERFACE: &str = "org.freedesktop.NetworkManager.SecretAgent";
/// 에이전트 식별자. NetworkManager는 사용자마다 같은 식별자의 에이전트를 하나만 받는다
const IDENTIFIER: &str = "org.buntos.Shell";
const ERROR_USER_CANCELED: &str = "org.freedesktop.NetworkManager.SecretAgent.UserCanceled";
const ERROR_AGENT_CANCELED: &str = "org.freedesktop.NetworkManager.SecretAgent.AgentCanceled";
const ERROR_NO_SECRETS: &str = "org.freedesktop.NetworkManager.SecretAgent.NoSecrets";
const ERROR_ACCESS_DENIED: &str = "org.freedesktop.DBus.Error.AccessDenied";
const ERROR_INVALID_ARGS: &str = "org.freedesktop.DBus.Error.InvalidArgs";
const ERROR_UNKNOWN_METHOD: &str = "org.freedesktop.DBus.Error.UnknownMethod";

/// NMSecretAgentGetSecretsFlags: 사람에게 물어도 된다
const ALLOW_INTERACTION: u32 = 0x1;
/// 가진 비밀이 틀렸으니 새로 물어라 (물어도 된다는 뜻도 있다)
const REQUEST_NEW: u32 = 0x2;
/// 사람이 D-Bus로 고른 연결이다. 없으면 NetworkManager가 저절로 연결하다 물었다
const USER_REQUESTED: u32 = 0x4;
/// NetworkManager가 GetSecrets의 답을 기다리는 시간은 120초다. 그보다 조금 일찍 창을 닫는다
const PROMPT_TIMEOUT: Duration = Duration::from_secs(115);

const WIRELESS: &str = "802-11-wireless";
const WIRELESS_SECURITY: &str = "802-11-wireless-security";
const IEEE_8021X: &str = "802-1x";
/// 사용자 이름과 암호로 인증하는 EAP 방식 (NetworkManager의 need_secrets_password, need_secrets_phase2)
const PASSWORD_METHODS: [&str; 6] = ["peap", "ttls", "md5", "leap", "fast", "pwd"];

const INTROSPECTION: &str = r#"<node>
  <interface name="org.freedesktop.NetworkManager.SecretAgent">
    <method name="GetSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
      <arg name="setting_name" type="s" direction="in"/>
      <arg name="hints" type="as" direction="in"/>
      <arg name="flags" type="u" direction="in"/>
      <arg name="secrets" type="a{sa{sv}}" direction="out"/>
    </method>
    <method name="CancelGetSecrets">
      <arg name="connection_path" type="o" direction="in"/>
      <arg name="setting_name" type="s" direction="in"/>
    </method>
    <method name="SaveSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
    </method>
    <method name="DeleteSecrets">
      <arg name="connection" type="a{sa{sv}}" direction="in"/>
      <arg name="connection_path" type="o" direction="in"/>
    </method>
  </interface>
</node>"#;

/// a{sa{sv}} 연결 설정
type Settings = HashMap<String, HashMap<String, glib::Variant>>;

thread_local! {
    /// NetworkManager에 등록됐는지. 메뉴 막대는 등록돼 있을 때만 새 네트워크의 암호를 이 에이전트에게 맡긴다
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// 이 에이전트가 NetworkManager에 등록돼 있는지
pub fn registered() -> bool {
    REGISTERED.with(Cell::get)
}

/// 제목에 쓰는 보안 방식
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Security {
    /// WPA·WPA2 개인용 (wpa-psk)
    Wpa2,
    /// WPA3 개인용 (sae)
    Wpa3,
    /// WEP. passphrase는 wep-key-type이 2(암호 문구)인지
    Wep { passphrase: bool },
    /// Cisco LEAP
    Leap,
}

impl Security {
    /// 제목의 Fluent 선택자 값
    fn id(self) -> &'static str {
        match self {
            Self::Wpa2 => "wpa2",
            Self::Wpa3 => "wpa3",
            Self::Wep { .. } => "wep",
            Self::Leap => "leap",
        }
    }
}

/// 사람에게 받을 비밀
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fields {
    /// 무선 보안 설정의 암호 하나. key는 psk, wep-key0~3, leap-password다
    Password { security: Security, key: String },
    /// 802.1X 암호. 사용자 이름은 연결에 있는 것을 보여 준다(비밀이 아니라 NetworkManager가 답에서 받지 않는다)
    Credentials { identity: String },
    /// 802.1X TLS의 개인 키 암호
    PrivateKey { identity: String },
}

impl Fields {
    /// 답에 넣는 비밀의 키
    fn key(&self) -> &str {
        match self {
            Self::Password { key, .. } => key,
            Self::Credentials { .. } => "password",
            Self::PrivateKey { .. } => "private-key-password",
        }
    }

    /// 비밀이 NetworkManager가 받는 꼴인지 (nm_utils_wpa_psk_valid, nm_utils_wep_key_valid와 같다)
    pub fn accepts(&self, secret: &str) -> bool {
        match self {
            Self::Password {
                security: Security::Wpa2,
                ..
            } => wpa_psk_valid(secret),
            Self::Password {
                security: Security::Wep { passphrase },
                ..
            } => wep_key_valid(secret, *passphrase),
            _ => !secret.is_empty(),
        }
    }
}

/// WPA 암호: 8~63바이트의 암호 문구나 64자리 16진수 키
fn wpa_psk_valid(psk: &str) -> bool {
    match psk.len() {
        8..=63 => true,
        64 => psk.bytes().all(|byte| byte.is_ascii_hexdigit()),
        _ => false,
    }
}

/// WEP: 키는 10·26자리 16진수나 5·13자 ASCII, 암호 문구는 1~64바이트. 형식을 모르면 키로 본다
fn wep_key_valid(key: &str, passphrase: bool) -> bool {
    if passphrase {
        return (1..=64).contains(&key.len());
    }
    match key.len() {
        10 | 26 => key.bytes().all(|byte| byte.is_ascii_hexdigit()),
        5 | 13 => key
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' '),
        _ => false,
    }
}

/// 사람에게 물을 것
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prompt {
    /// Wi-Fi는 SSID, 아니면 연결 이름
    pub network: String,
    pub wireless: bool,
    pub fields: Fields,
    /// 전에 준 비밀이 틀렸다 (REQUEST_NEW)
    pub retry: bool,
    /// 사람이 고른 연결이다 (USER_REQUESTED)
    pub user_requested: bool,
}

/// GetSecrets 요청 하나
#[derive(Debug, Clone)]
pub(crate) struct Request {
    pub connection_path: String,
    pub setting: String,
    /// None이면 묻지 않고 비밀이 없다고 답한다
    pub prompt: Option<Prompt>,
}

impl Request {
    /// (a{sa{sv}} connection, o connection_path, s setting_name, as hints, u flags)
    pub fn parse(params: &glib::Variant) -> Option<Request> {
        type Args = (
            Settings,
            glib::variant::ObjectPath,
            String,
            Vec<String>,
            u32,
        );
        let (connection, path, setting, hints, flags): Args = params.get()?;
        Some(Request {
            prompt: prompt_for(&connection, &setting, &hints, flags),
            connection_path: path.as_str().to_owned(),
            setting,
        })
    }
}

/// 연결 설정에서 물을 것을 정한다. 묻지 말아야 하거나 다루지 않는 설정이면 None
fn prompt_for(
    connection: &Settings,
    setting: &str,
    hints: &[String],
    flags: u32,
) -> Option<Prompt> {
    if flags & (ALLOW_INTERACTION | REQUEST_NEW) == 0 {
        return None;
    }
    let value = |section: &str, key: &str| connection.get(section)?.get(key).cloned();
    let text = |section: &str, key: &str| {
        value(section, key)
            .and_then(|value| value.str().map(str::to_owned))
            .unwrap_or_default()
    };
    let wireless = text("connection", "type") == WIRELESS;
    let network = value(WIRELESS, "ssid")
        .and_then(|ssid| ssid.get::<Vec<u8>>())
        .map(|ssid| String::from_utf8_lossy(&ssid).into_owned())
        .filter(|ssid| !ssid.is_empty())
        .unwrap_or_else(|| text("connection", "id"));
    let fields = match setting {
        WIRELESS_SECURITY => {
            let password = |security| {
                Some(Fields::Password {
                    security,
                    key: "psk".to_owned(),
                })
            };
            match text(WIRELESS_SECURITY, "key-mgmt").as_str() {
                "wpa-psk" | "wpa-none" => password(Security::Wpa2),
                "sae" => password(Security::Wpa3),
                // LEAP는 key-mgmt가 ieee8021x이고 auth-alg가 leap다
                "ieee8021x" if text(WIRELESS_SECURITY, "auth-alg") == "leap" => {
                    Some(Fields::Password {
                        security: Security::Leap,
                        key: "leap-password".to_owned(),
                    })
                }
                "none" => {
                    let index = value(WIRELESS_SECURITY, "wep-tx-keyidx")
                        .and_then(|index| index.get::<u32>())
                        .unwrap_or(0)
                        .min(3);
                    // NMWepKeyType: 2는 암호 문구
                    let passphrase = value(WIRELESS_SECURITY, "wep-key-type")
                        .and_then(|kind| kind.get::<u32>())
                        == Some(2);
                    Some(Fields::Password {
                        security: Security::Wep { passphrase },
                        key: format!("wep-key{index}"),
                    })
                }
                _ => None,
            }
        }
        IEEE_8021X => {
            let hinted = |hint: &str| hints.iter().any(|item| item == hint);
            let method = value(IEEE_8021X, "eap")
                .and_then(|methods| methods.get::<Vec<String>>())
                .and_then(|methods| methods.into_iter().next())
                .unwrap_or_default();
            let identity = text(IEEE_8021X, "identity");
            // 힌트가 있으면 그것을 따르고, 없으면 첫 EAP 방식으로 정한다 (GNOME 셸과 같다)
            if hinted("private-key-password") || (!hinted("password") && method == "tls") {
                Some(Fields::PrivateKey { identity })
            } else if hinted("password") || PASSWORD_METHODS.contains(&method.as_str()) {
                Some(Fields::Credentials { identity })
            } else {
                None
            }
        }
        _ => None,
    }?;
    Some(Prompt {
        network,
        wireless,
        fields,
        retry: flags & REQUEST_NEW != 0,
        user_requested: flags & USER_REQUESTED != 0,
    })
}

/// GetSecrets의 답 (a{sa{sv}}): {설정 이름: {키: 비밀}}
fn secrets_reply(setting: &str, key: &str, secret: &Secret) -> glib::Variant {
    let values = HashMap::from([(key.to_owned(), secret.as_str().to_variant())]);
    let settings: Settings = HashMap::from([(setting.to_owned(), values)]);
    (settings,).to_variant()
}

/// 사람의 답
pub(crate) enum Answer {
    Secret(Secret),
    Cancel,
}

/// 물음에 답을 돌려주는 곳. 물음마다 번호가 있어 지난 물음의 늦은 답은 버린다
pub(crate) struct Responder {
    agent: Weak<Agent>,
    id: u64,
}

impl Responder {
    pub fn respond(self, answer: Answer) {
        if let Some(agent) = self.agent.upgrade() {
            agent.answered(self.id, answer);
        }
    }
}

/// 사람에게 묻는 쪽. 셸에서는 GTK 창이고, 시험에서는 가짜다
pub(crate) trait Asker {
    /// 묻는다. 사람이 답하면 `responder`로 돌려준다
    fn ask(&self, prompt: &Prompt, responder: Responder);
    /// 떠 있는 물음을 닫는다 (NetworkManager가 취소했거나 시간이 지났다)
    fn close(&self);
}

struct Pending {
    id: u64,
    request: Request,
    prompt: Prompt,
    invocation: gio::DBusMethodInvocation,
}

struct Current {
    pending: Pending,
    /// 시간이 지나면 닫는다. 끝나기 전에 답하면 멈춘다
    timer: glib::JoinHandle<()>,
}

pub(crate) struct Agent {
    asker: Box<dyn Asker>,
    /// NetworkManager의 고유 버스 이름. 이 이름이 부른 메서드만 받는다
    manager: RefCell<Option<String>>,
    /// 지금 묻는 것 다음에 물을 요청
    queue: RefCell<VecDeque<Pending>>,
    current: RefCell<Option<Current>>,
    next_id: Cell<u64>,
    timeout: Duration,
}

impl Agent {
    pub fn new(asker: Box<dyn Asker>, timeout: Duration) -> Rc<Self> {
        Rc::new(Self {
            asker,
            manager: RefCell::default(),
            queue: RefCell::default(),
            current: RefCell::default(),
            next_id: Cell::new(1),
            timeout,
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
        Ok(())
    }

    fn handle_call(
        self: &Rc<Self>,
        sender: Option<&str>,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        // 버스 정책도 NetworkManager(root)만 이 인터페이스를 부르게 하지만, 가짜 암호 창을 띄우지 못하게
        // 한 번 더 본다
        if sender.is_none() || self.manager.borrow().as_deref() != sender {
            invocation.return_dbus_error(
                ERROR_ACCESS_DENIED,
                "only NetworkManager may call the secret agent",
            );
            return;
        }
        match method {
            "GetSecrets" => self.get_secrets(params, invocation),
            "CancelGetSecrets" => {
                if let Some((path, setting)) = params.get::<(glib::variant::ObjectPath, String)>() {
                    self.cancel(path.as_str(), &setting);
                }
                invocation.return_value(None);
            }
            // 이 에이전트는 비밀을 저장하지 않는다. 시스템 연결의 비밀은 NetworkManager가 연결 파일에 둔다
            "SaveSecrets" | "DeleteSecrets" => invocation.return_value(None),
            _ => invocation.return_dbus_error(ERROR_UNKNOWN_METHOD, "unknown method"),
        }
    }

    fn get_secrets(self: &Rc<Self>, params: &glib::Variant, invocation: gio::DBusMethodInvocation) {
        let Some(request) = Request::parse(params) else {
            invocation.return_dbus_error(ERROR_INVALID_ARGS, "unexpected arguments");
            return;
        };
        let Some(prompt) = request.prompt.clone() else {
            invocation.return_dbus_error(ERROR_NO_SECRETS, "this agent has no such secrets");
            return;
        };
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.queue.borrow_mut().push_back(Pending {
            id,
            request,
            prompt,
            invocation,
        });
        self.show_next();
    }

    /// 떠 있는 물음이 없으면 다음 요청을 묻는다
    fn show_next(self: &Rc<Self>) {
        if self.current.borrow().is_some() {
            return;
        }
        let Some(pending) = self.queue.borrow_mut().pop_front() else {
            return;
        };
        let id = pending.id;
        let prompt = pending.prompt.clone();
        let (weak, timeout) = (Rc::downgrade(self), self.timeout);
        // 이 스레드의 기본 컨텍스트에서 센다 (시험은 따로 만든 컨텍스트를 돌린다)
        let timer = glib::spawn_future_local(async move {
            glib::timeout_future(timeout).await;
            if let Some(agent) = weak.upgrade() {
                agent.expire(id);
            }
        });
        self.current.replace(Some(Current { pending, timer }));
        self.asker.ask(
            &prompt,
            Responder {
                agent: Rc::downgrade(self),
                id,
            },
        );
    }

    /// 답한 처리기 안에서 다음 창을 만들지 않게 미룬다
    fn show_next_later(self: &Rc<Self>) {
        let agent = self.clone();
        glib::spawn_future_local(async move { agent.show_next() });
    }

    /// 지금 묻는 것이 `id`이면 꺼낸다
    fn take_current(&self, id: u64) -> Option<Current> {
        let mut current = self.current.borrow_mut();
        if current.as_ref()?.pending.id != id {
            return None;
        }
        current.take()
    }

    fn answered(self: &Rc<Self>, id: u64, answer: Answer) {
        let Some(current) = self.take_current(id) else {
            return;
        };
        current.timer.abort();
        let Pending {
            request,
            prompt,
            invocation,
            ..
        } = current.pending;
        match answer {
            Answer::Secret(secret) => {
                let reply = secrets_reply(&request.setting, prompt.fields.key(), &secret);
                invocation.return_value(Some(&reply));
            }
            Answer::Cancel => {
                invocation.return_dbus_error(ERROR_USER_CANCELED, "the user canceled the request");
            }
        }
        self.show_next_later();
    }

    /// NetworkManager가 그 연결·설정의 요청을 거뒀다
    fn cancel(self: &Rc<Self>, path: &str, setting: &str) {
        let matches = |pending: &Pending| {
            pending.request.connection_path == path && pending.request.setting == setting
        };
        let current = self
            .current
            .borrow()
            .as_ref()
            .filter(|current| matches(&current.pending))
            .map(|current| current.pending.id);
        if let Some(current) = current.and_then(|id| self.take_current(id)) {
            current.timer.abort();
            self.asker.close();
            current
                .pending
                .invocation
                .return_dbus_error(ERROR_AGENT_CANCELED, "NetworkManager canceled the request");
            self.show_next_later();
        }
        let canceled: VecDeque<Pending> = {
            let mut queue = self.queue.borrow_mut();
            let (canceled, kept) = queue.drain(..).partition(|pending| matches(pending));
            *queue = kept;
            canceled
        };
        for pending in canceled {
            pending
                .invocation
                .return_dbus_error(ERROR_AGENT_CANCELED, "NetworkManager canceled the request");
        }
    }

    /// 아무도 제때 답하지 않았다. NetworkManager는 이미 기다리기를 그만뒀다
    fn expire(self: &Rc<Self>, id: u64) {
        // 이 일은 시간을 센 future 안에서 돈다. 그 future는 여기서 끝나므로 멈추지 않는다
        let Some(current) = self.take_current(id) else {
            return;
        };
        self.asker.close();
        current
            .pending
            .invocation
            .return_dbus_error(ERROR_NO_SECRETS, "nobody answered in time");
        self.show_next_later();
    }

    /// NetworkManager가 사라졌다. 묻던 것과 기다리던 것을 모두 닫는다
    fn forget_all(self: &Rc<Self>) {
        if let Some(current) = self.current.take() {
            current.timer.abort();
            self.asker.close();
        }
        self.queue.borrow_mut().clear();
    }
}

/// 세션의 NetworkManager 비밀 에이전트가 된다. NetworkManager가 (다시) 뜰 때마다 등록한다
pub fn start(app: &gtk::Application, tr: &Rc<Translations>) {
    let dialogs = Dialogs {
        app: app.clone(),
        tr: tr.clone(),
        open: RefCell::default(),
    };
    let agent = Agent::new(Box::new(dialogs), PROMPT_TIMEOUT);
    glib::spawn_future_local(async move {
        let bus = match gio::bus_get_future(gio::BusType::System).await {
            Ok(bus) => bus,
            Err(err) => {
                eprintln!("bunt-shell: no system bus, the Wi-Fi password dialog is off: {err}");
                return;
            }
        };
        if let Err(err) = agent.export(&bus) {
            eprintln!("bunt-shell: could not export the NetworkManager secret agent: {err}");
            return;
        }
        let (appeared, vanished) = (agent.clone(), agent.clone());
        // 감시는 셸이 끝날 때까지 둔다
        let _watch = gio::bus_watch_name_on_connection(
            &bus,
            NM_NAME,
            gio::BusNameWatcherFlags::NONE,
            move |bus, _, owner| {
                appeared.manager.replace(Some(owner.to_owned()));
                glib::spawn_future_local(async move {
                    match register(&bus).await {
                        Ok(()) => REGISTERED.with(|registered| registered.set(true)),
                        Err(err) => eprintln!(
                            "bunt-shell: could not register the NetworkManager secret agent: {err}"
                        ),
                    }
                });
            },
            move |_, _| {
                REGISTERED.with(|registered| registered.set(false));
                vanished.manager.replace(None);
                vanished.forget_all();
            },
        );
    });
}

/// AgentManager.RegisterWithCapabilities. VPN 힌트는 다루지 않는다(능력 0)
async fn register(bus: &gio::DBusConnection) -> Result<(), glib::Error> {
    bus.call_future(
        Some(NM_NAME),
        AGENT_MANAGER_PATH,
        AGENT_MANAGER_INTERFACE,
        "RegisterWithCapabilities",
        Some(&(IDENTIFIER, 0u32).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
    )
    .await?;
    Ok(())
}

/// 셸의 Wi-Fi 암호 창들. 한 번에 하나만 뜬다
struct Dialogs {
    app: gtk::Application,
    tr: Rc<Translations>,
    open: RefCell<Option<Rc<SecretDialog>>>,
}

impl Asker for Dialogs {
    fn ask(&self, prompt: &Prompt, responder: Responder) {
        let dialog = SecretDialog::new(&self.app, &self.tr, prompt, responder);
        if let Some(old) = self.open.replace(Some(dialog.clone())) {
            old.frame.close();
        }
        dialog.present();
    }

    fn close(&self) {
        if let Some(dialog) = self.open.take() {
            dialog.frame.close();
        }
    }
}

/// 제목: "Wi-Fi 네트워크 “이름”에 WPA2 암호가 필요합니다." 등
fn title(tr: &Translations, prompt: &Prompt) -> String {
    let mut args = FluentArgs::new();
    args.set("name", prompt.network.clone());
    match &prompt.fields {
        Fields::Password { security, .. } => {
            args.set("security", security.id());
            tr.get_with("secret-wifi-title", &args)
        }
        Fields::Credentials { .. } if prompt.wireless => {
            tr.get_with("secret-wifi-enterprise-title", &args)
        }
        Fields::Credentials { .. } => tr.get_with("secret-wired-title", &args),
        Fields::PrivateKey { .. } => tr.get_with("secret-private-key-title", &args),
    }
}

/// Wi-Fi 암호 창 (macOS의 Wi-Fi 암호 창): Wi-Fi 그림, 제목, (사용자 이름), 암호, 암호 보기, 취소·연결
struct SecretDialog {
    frame: Frame,
    fields: Fields,
    password: gtk::PasswordEntry,
    join: gtk::Button,
    /// 답하면 비운다
    responder: RefCell<Option<Responder>>,
    retry: bool,
}

impl SecretDialog {
    fn new(
        app: &gtk::Application,
        tr: &Translations,
        prompt: &Prompt,
        responder: Responder,
    ) -> Rc<SecretDialog> {
        // 사람이 고른 연결이면 바로 칠 수 있게 키보드를 받고, 저절로 뜬 창은 눌러야 받는다
        let keyboard = if prompt.user_requested {
            KeyboardMode::Exclusive
        } else {
            KeyboardMode::OnDemand
        };
        let frame = Frame::new(app, NAMESPACE, keyboard);
        let icon = if prompt.wireless {
            "network-wireless-symbolic"
        } else {
            "network-wired-symbolic"
        };
        frame.tile(icon, "service-tile");
        frame.title(&title(tr, prompt));
        let fields = frame.fields();
        match &prompt.fields {
            Fields::Credentials { identity } | Fields::PrivateKey { identity }
                if !identity.is_empty() =>
            {
                // 사용자 이름은 연결 설정에 있는 값이다. NetworkManager는 GetSecrets의 답으로 바꾸지 않는다
                let user =
                    service_dialog::text_field(&fields, &tr.get("secret-user-name"), identity);
                user.set_editable(false);
                user.set_can_focus(false);
            }
            _ => {}
        }
        let placeholder = match prompt.fields {
            Fields::PrivateKey { .. } => tr.get("secret-private-key-password"),
            _ => tr.get("secret-password"),
        };
        let password = service_dialog::password_field(
            &fields,
            &placeholder,
            Some(&tr.get("secret-show-password")),
        );
        let info = frame.info();
        if prompt.retry {
            info.set_text(&tr.get("secret-retry"));
            info.add_css_class("error");
            info.set_visible(true);
        }
        let (cancel, join) = frame.buttons(&tr.get("secret-cancel"), Some(&tr.get("secret-join")));
        let join = join.expect("a join button");
        join.set_sensitive(false);

        let dialog = Rc::new(SecretDialog {
            frame,
            fields: prompt.fields.clone(),
            password,
            join,
            responder: RefCell::new(Some(responder)),
            retry: prompt.retry,
        });
        let weak = Rc::downgrade(&dialog);
        dialog.password.connect_changed(move |entry| {
            if let Some(dialog) = weak.upgrade() {
                dialog
                    .join
                    .set_sensitive(dialog.fields.accepts(&entry.text()));
            }
        });
        let weak = Rc::downgrade(&dialog);
        dialog.join.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.submit();
            }
        });
        let weak = Rc::downgrade(&dialog);
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.respond(Answer::Cancel);
            }
        });
        let weak = Rc::downgrade(&dialog);
        dialog.frame.on_escape(move || {
            if let Some(dialog) = weak.upgrade() {
                dialog.respond(Answer::Cancel);
            }
        });
        dialog
    }

    fn present(&self) {
        self.frame.present(Some(self.password.upcast_ref()));
        if self.retry {
            self.frame.shake();
        }
    }

    /// 연결: 칸이 받을 수 있는 암호일 때만 (Enter도 기본 단추를 누른다)
    fn submit(&self) {
        let text = self.password.text();
        if !self.fields.accepts(&text) {
            return;
        }
        let secret = Secret::new(&text);
        self.password.set_text("");
        self.respond(Answer::Secret(secret));
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

    fn section(entries: &[(&str, glib::Variant)]) -> HashMap<String, glib::Variant> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    /// Wi-Fi 연결 설정 (NetworkManager가 GetSecrets에 넘기는 꼴)
    fn wifi(ssid: &str, security: &[(&str, glib::Variant)]) -> Settings {
        HashMap::from([
            (
                "connection".to_owned(),
                section(&[("id", "Home".to_variant()), ("type", WIRELESS.to_variant())]),
            ),
            (
                WIRELESS.to_owned(),
                section(&[("ssid", ssid.as_bytes().to_vec().to_variant())]),
            ),
            (WIRELESS_SECURITY.to_owned(), section(security)),
        ])
    }

    fn enterprise(kind: &str, eap: &[&str], identity: &str) -> Settings {
        let eap: Vec<String> = eap.iter().map(|method| (*method).to_owned()).collect();
        HashMap::from([
            (
                "connection".to_owned(),
                section(&[("id", "Office".to_variant()), ("type", kind.to_variant())]),
            ),
            (
                IEEE_8021X.to_owned(),
                section(&[
                    ("eap", eap.to_variant()),
                    ("identity", identity.to_variant()),
                ]),
            ),
        ])
    }

    #[test]
    fn wifi_passwords_follow_key_management() {
        let psk = wifi("집 Wi-Fi", &[("key-mgmt", "wpa-psk".to_variant())]);
        let prompt = prompt_for(&psk, WIRELESS_SECURITY, &[], ALLOW_INTERACTION).unwrap();
        assert_eq!(
            prompt,
            Prompt {
                network: "집 Wi-Fi".into(),
                wireless: true,
                fields: Fields::Password {
                    security: Security::Wpa2,
                    key: "psk".into()
                },
                retry: false,
                user_requested: false,
            }
        );
        let sae = wifi("Lab", &[("key-mgmt", "sae".to_variant())]);
        let prompt = prompt_for(
            &sae,
            WIRELESS_SECURITY,
            &[],
            ALLOW_INTERACTION | REQUEST_NEW | USER_REQUESTED,
        )
        .unwrap();
        assert_eq!(
            prompt.fields,
            Fields::Password {
                security: Security::Wpa3,
                key: "psk".into()
            }
        );
        assert!(prompt.retry && prompt.user_requested);
        // WEP는 보내는 키 번호(wep-tx-keyidx)의 키를 묻는다
        let wep = wifi(
            "Old",
            &[
                ("key-mgmt", "none".to_variant()),
                ("wep-tx-keyidx", 2u32.to_variant()),
                ("wep-key-type", 2u32.to_variant()),
            ],
        );
        assert_eq!(
            prompt_for(&wep, WIRELESS_SECURITY, &[], ALLOW_INTERACTION)
                .unwrap()
                .fields,
            Fields::Password {
                security: Security::Wep { passphrase: true },
                key: "wep-key2".into()
            }
        );
        let leap = wifi(
            "Cisco",
            &[
                ("key-mgmt", "ieee8021x".to_variant()),
                ("auth-alg", "leap".to_variant()),
            ],
        );
        assert_eq!(
            prompt_for(&leap, WIRELESS_SECURITY, &[], ALLOW_INTERACTION)
                .unwrap()
                .fields
                .key(),
            "leap-password"
        );
    }

    #[test]
    fn nothing_is_asked_without_interaction_or_for_unknown_settings() {
        let psk = wifi("Home", &[("key-mgmt", "wpa-psk".to_variant())]);
        // 상호작용이 없는 요청은 저장한 비밀을 찾는 것이다. 이 에이전트는 저장하지 않는다
        assert!(prompt_for(&psk, WIRELESS_SECURITY, &[], 0).is_none());
        assert!(prompt_for(&psk, WIRELESS_SECURITY, &[], USER_REQUESTED).is_none());
        // REQUEST_NEW는 물어도 된다는 뜻을 담는다
        assert!(prompt_for(&psk, WIRELESS_SECURITY, &[], REQUEST_NEW).is_some());
        assert!(prompt_for(&psk, "vpn", &[], ALLOW_INTERACTION).is_none());
        let owe = wifi("Cafe", &[("key-mgmt", "owe".to_variant())]);
        assert!(prompt_for(&owe, WIRELESS_SECURITY, &[], ALLOW_INTERACTION).is_none());
        // 설정이 비어 있어도 넘어지지 않는다
        assert!(prompt_for(&Settings::new(), WIRELESS_SECURITY, &[], ALLOW_INTERACTION).is_none());
    }

    #[test]
    fn hidden_ssids_fall_back_to_the_connection_name() {
        let hidden = wifi("", &[("key-mgmt", "wpa-psk".to_variant())]);
        let prompt = prompt_for(&hidden, WIRELESS_SECURITY, &[], ALLOW_INTERACTION).unwrap();
        assert_eq!(prompt.network, "Home");
    }

    #[test]
    fn enterprise_credentials_follow_hints_then_methods() {
        let peap = enterprise(WIRELESS, &["peap"], "hong@example.com");
        let prompt = prompt_for(&peap, IEEE_8021X, &[], ALLOW_INTERACTION).unwrap();
        assert_eq!(
            prompt.fields,
            Fields::Credentials {
                identity: "hong@example.com".into()
            }
        );
        assert!(prompt.wireless);
        assert_eq!(prompt.network, "Office");
        let tls = enterprise("802-3-ethernet", &["tls"], "hong");
        let prompt = prompt_for(&tls, IEEE_8021X, &[], ALLOW_INTERACTION).unwrap();
        assert_eq!(
            prompt.fields,
            Fields::PrivateKey {
                identity: "hong".into()
            }
        );
        assert!(!prompt.wireless);
        // 힌트가 방식보다 앞선다
        let hinted = prompt_for(&tls, IEEE_8021X, &["password".into()], ALLOW_INTERACTION);
        assert!(matches!(hinted.unwrap().fields, Fields::Credentials { .. }));
        let sim = enterprise(WIRELESS, &["sim"], "");
        assert!(prompt_for(&sim, IEEE_8021X, &[], ALLOW_INTERACTION).is_none());
    }

    #[test]
    fn passwords_are_checked_like_network_manager() {
        let wpa = Fields::Password {
            security: Security::Wpa2,
            key: "psk".into(),
        };
        assert!(!wpa.accepts("short"));
        assert!(wpa.accepts("12345678"));
        assert!(wpa.accepts(&"a".repeat(63)));
        // 64자는 16진수 키만 된다
        assert!(wpa.accepts(&"0f".repeat(32)));
        assert!(!wpa.accepts(&"g".repeat(64)));
        assert!(!wpa.accepts(&"a".repeat(65)));
        // 한글은 글자마다 3바이트다: 3글자(9바이트)는 된다
        assert!(wpa.accepts("한글암"));
        let wep = Fields::Password {
            security: Security::Wep { passphrase: false },
            key: "wep-key0".into(),
        };
        assert!(wep.accepts("abcde"));
        assert!(wep.accepts("0123456789"));
        assert!(!wep.accepts("012345678g"));
        assert!(!wep.accepts("abcdef"));
        let wep_phrase = Fields::Password {
            security: Security::Wep { passphrase: true },
            key: "wep-key0".into(),
        };
        assert!(wep_phrase.accepts("x"));
        assert!(!wep_phrase.accepts(""));
        assert!(!wep_phrase.accepts(&"x".repeat(65)));
        let sae = Fields::Password {
            security: Security::Wpa3,
            key: "psk".into(),
        };
        assert!(sae.accepts("x"));
        assert!(!sae.accepts(""));
    }

    #[test]
    fn replies_carry_only_the_asked_secret() {
        let reply = secrets_reply(WIRELESS_SECURITY, "psk", &Secret::new("correct horse"));
        let (settings,): (Settings,) = reply.get().unwrap();
        assert_eq!(settings.len(), 1);
        let security = &settings[WIRELESS_SECURITY];
        assert_eq!(security.len(), 1);
        assert_eq!(security["psk"].str(), Some("correct horse"));
        assert_eq!(reply.type_().as_str(), "(a{sa{sv}})");
    }

    #[test]
    fn titles_name_the_network_and_security() {
        let ko = Translations::from_source("ko", crate::TRANSLATIONS[0].1);
        let en = Translations::from_source("en", crate::TRANSLATIONS[1].1);
        let prompt = Prompt {
            network: "집".into(),
            wireless: true,
            fields: Fields::Password {
                security: Security::Wpa2,
                key: "psk".into(),
            },
            retry: false,
            user_requested: true,
        };
        assert_eq!(
            title(&ko, &prompt),
            "Wi-Fi 네트워크 “집”에 WPA2 암호가 필요합니다."
        );
        assert_eq!(
            title(&en, &prompt),
            "The Wi-Fi network “집” requires a WPA2 password."
        );
        let wpa3 = Prompt {
            fields: Fields::Password {
                security: Security::Wpa3,
                key: "psk".into(),
            },
            ..prompt.clone()
        };
        assert_eq!(
            title(&ko, &wpa3),
            "Wi-Fi 네트워크 “집”에 WPA3 암호가 필요합니다."
        );
        let office = Prompt {
            fields: Fields::Credentials {
                identity: "hong".into(),
            },
            ..prompt
        };
        assert!(title(&ko, &office).contains("엔터프라이즈"));
    }

    /// 가짜 묻는 쪽: 물은 것을 적어 두고, 정해 둔 답이 있으면 바로 답한다
    #[derive(Default)]
    struct Fake {
        asked: Rc<RefCell<Vec<Prompt>>>,
        closed: Rc<Cell<u32>>,
        /// 다음 물음에 줄 답 (None이면 답하지 않고 기다린다)
        answers: Rc<RefCell<VecDeque<Option<&'static str>>>>,
        waiting: Rc<RefCell<Option<Responder>>>,
    }

    impl Asker for Fake {
        fn ask(&self, prompt: &Prompt, responder: Responder) {
            self.asked.borrow_mut().push(prompt.clone());
            match self.answers.borrow_mut().pop_front().flatten() {
                Some("") => responder.respond(Answer::Cancel),
                Some(secret) => responder.respond(Answer::Secret(Secret::new(secret))),
                None => {
                    self.waiting.replace(Some(responder));
                }
            }
        }

        fn close(&self) {
            self.closed.set(self.closed.get() + 1);
            self.waiting.replace(None);
        }
    }

    /// dbus-daemon으로 시험용 버스를 띄운다. 없으면 None (시험을 건너뛴다)
    fn test_bus() -> Option<gio::TestDBus> {
        glib::find_program_in_path("dbus-daemon")?;
        let bus = gio::TestDBus::new(gio::TestDBusFlags::NONE);
        bus.up();
        Some(bus)
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

    async fn get_secrets(
        caller: &gio::DBusConnection,
        agent_name: &str,
        connection: &Settings,
        path: &str,
        setting: &str,
        flags: u32,
    ) -> Result<Settings, glib::Error> {
        let path = glib::variant::ObjectPath::try_from(path.to_owned()).unwrap();
        let hints: Vec<String> = Vec::new();
        let reply = caller
            .call_future(
                Some(agent_name),
                AGENT_PATH,
                AGENT_INTERFACE,
                "GetSecrets",
                Some(&(connection.clone(), path, setting, hints, flags).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                5_000,
            )
            .await?;
        Ok(reply.child_value(0).get().unwrap())
    }

    fn dbus_error_name(error: &glib::Error) -> String {
        gio::DBusError::remote_error(error)
            .map(|name| name.to_string())
            .unwrap_or_default()
    }

    /// NetworkManager처럼 부르는 쪽에서 GetSecrets를 거쳐 사람의 답이 a{sa{sv}}로 돌아온다
    #[test]
    fn get_secrets_round_trip_over_dbus() {
        let Some(bus) = test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let agent_bus = connect(&address).await;
                    let manager = connect(&address).await;
                    let stranger = connect(&address).await;
                    let fake = Fake::default();
                    let (asked, answers) = (fake.asked.clone(), fake.answers.clone());
                    let agent = Agent::new(Box::new(fake), PROMPT_TIMEOUT);
                    agent.export(&agent_bus).unwrap();
                    agent
                        .manager
                        .replace(manager.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let connection = wifi("Home", &[("key-mgmt", "wpa-psk".to_variant())]);
                    let path = "/org/freedesktop/NetworkManager/Settings/1";

                    answers.borrow_mut().push_back(Some("correct horse"));
                    let flags = ALLOW_INTERACTION | USER_REQUESTED;
                    let secrets =
                        get_secrets(&manager, &name, &connection, path, WIRELESS_SECURITY, flags)
                            .await
                            .unwrap();
                    assert_eq!(
                        secrets[WIRELESS_SECURITY]["psk"].str(),
                        Some("correct horse")
                    );
                    assert_eq!(asked.borrow().len(), 1);
                    assert!(asked.borrow()[0].user_requested);

                    // 취소하면 UserCanceled
                    answers.borrow_mut().push_back(Some(""));
                    let error =
                        get_secrets(&manager, &name, &connection, path, WIRELESS_SECURITY, flags)
                            .await
                            .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_USER_CANCELED);

                    // 상호작용이 없는 요청은 묻지 않고 NoSecrets
                    let error =
                        get_secrets(&manager, &name, &connection, path, WIRELESS_SECURITY, 0)
                            .await
                            .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_NO_SECRETS);
                    assert_eq!(asked.borrow().len(), 2);

                    // NetworkManager가 아닌 쪽은 창을 띄우지 못한다
                    let error = get_secrets(
                        &stranger,
                        &name,
                        &connection,
                        path,
                        WIRELESS_SECURITY,
                        flags,
                    )
                    .await
                    .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_ACCESS_DENIED);
                    assert_eq!(asked.borrow().len(), 2);
                })
            })
            .unwrap();
        bus.down();
    }

    /// CancelGetSecrets는 떠 있는 물음을 닫고 AgentCanceled로 답하게 한다. 기다리던 다음 요청은 그 뒤에 묻는다
    #[test]
    fn cancel_closes_the_prompt_and_asks_the_next_one() {
        let Some(bus) = test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let agent_bus = connect(&address).await;
                    let manager = connect(&address).await;
                    let fake = Fake::default();
                    let (asked, closed, waiting, answers) = (
                        fake.asked.clone(),
                        fake.closed.clone(),
                        fake.waiting.clone(),
                        fake.answers.clone(),
                    );
                    let agent = Agent::new(Box::new(fake), PROMPT_TIMEOUT);
                    agent.export(&agent_bus).unwrap();
                    agent
                        .manager
                        .replace(manager.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let home = wifi("Home", &[("key-mgmt", "wpa-psk".to_variant())]);
                    let lab = wifi("Lab", &[("key-mgmt", "sae".to_variant())]);
                    let (home_path, lab_path) = (
                        "/org/freedesktop/NetworkManager/Settings/1",
                        "/org/freedesktop/NetworkManager/Settings/2",
                    );
                    // 첫 물음은 답하지 않고 기다리고, 둘째 물음에는 답한다
                    answers.borrow_mut().extend([None, Some("lab password")]);
                    let first = get_secrets(
                        &manager,
                        &name,
                        &home,
                        home_path,
                        WIRELESS_SECURITY,
                        ALLOW_INTERACTION,
                    );
                    let second = get_secrets(
                        &manager,
                        &name,
                        &lab,
                        lab_path,
                        WIRELESS_SECURITY,
                        ALLOW_INTERACTION,
                    );
                    let cancel = async {
                        // 첫 물음이 뜰 때까지 기다린 뒤 거둔다
                        while waiting.borrow().is_none() {
                            glib::timeout_future(Duration::from_millis(10)).await;
                        }
                        let path = glib::variant::ObjectPath::try_from(home_path).unwrap();
                        manager
                            .call_future(
                                Some(&name),
                                AGENT_PATH,
                                AGENT_INTERFACE,
                                "CancelGetSecrets",
                                Some(&(path, WIRELESS_SECURITY).to_variant()),
                                None,
                                gio::DBusCallFlags::NONE,
                                5_000,
                            )
                            .await
                            .unwrap();
                    };
                    let (first, second, ()) = futures_join(first, second, cancel).await;
                    assert_eq!(dbus_error_name(&first.unwrap_err()), ERROR_AGENT_CANCELED);
                    assert_eq!(
                        second.unwrap()[WIRELESS_SECURITY]["psk"].str(),
                        Some("lab password")
                    );
                    assert_eq!(closed.get(), 1);
                    let networks: Vec<String> =
                        asked.borrow().iter().map(|p| p.network.clone()).collect();
                    assert_eq!(networks, ["Home", "Lab"]);
                })
            })
            .unwrap();
        bus.down();
    }

    /// 아무도 답하지 않으면 시간이 지나 창을 닫고 NoSecrets로 답한다. 늦은 답은 버린다
    #[test]
    fn unanswered_prompts_expire() {
        let Some(bus) = test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let agent_bus = connect(&address).await;
                    let manager = connect(&address).await;
                    let fake = Fake::default();
                    let (closed, waiting, answers) = (
                        fake.closed.clone(),
                        fake.waiting.clone(),
                        fake.answers.clone(),
                    );
                    let agent = Agent::new(Box::new(fake), Duration::from_millis(200));
                    agent.export(&agent_bus).unwrap();
                    agent
                        .manager
                        .replace(manager.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let home = wifi("Home", &[("key-mgmt", "wpa-psk".to_variant())]);
                    answers.borrow_mut().push_back(None);
                    let error = get_secrets(
                        &manager,
                        &name,
                        &home,
                        "/org/freedesktop/NetworkManager/Settings/1",
                        WIRELESS_SECURITY,
                        ALLOW_INTERACTION,
                    )
                    .await
                    .unwrap_err();
                    assert_eq!(dbus_error_name(&error), ERROR_NO_SECRETS);
                    assert_eq!(closed.get(), 1);
                    assert!(waiting.borrow().is_none());
                })
            })
            .unwrap();
        bus.down();
    }

    /// 세 future를 함께 돌린다. a, b, c 차례로 처음 돌리므로 a의 D-Bus 호출이 b보다 먼저 나간다
    async fn futures_join<A, B, C>(
        a: impl std::future::Future<Output = A>,
        b: impl std::future::Future<Output = B>,
        c: impl std::future::Future<Output = C>,
    ) -> (A, B, C) {
        use std::task::Poll;
        let (mut a, mut b, mut c) = (std::pin::pin!(a), std::pin::pin!(b), std::pin::pin!(c));
        let (mut ra, mut rb, mut rc) = (None, None, None);
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
            if rc.is_none()
                && let Poll::Ready(value) = c.as_mut().poll(cx)
            {
                rc = Some(value);
            }
            if ra.is_some() && rb.is_some() && rc.is_some() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
        (ra.unwrap(), rb.unwrap(), rc.unwrap())
    }
}
