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
//!   읽지 않는다. 메뉴 막대에서 사람이 방금 고른 네트워크([`expect_join`])가 아니면 창을 눌러야 키보드를
//!   받는다. 저절로 뜬 창이 다른 앱에 치던 글을 받아 암호로 보내지 않게 한다. NetworkManager의 USER_REQUESTED는
//!   D-Bus로 연결을 켠 프로그램이면 어느 것이든 붙으므로 그것만으로는 키보드를 주지 않는다.
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
    time::{Duration, Instant},
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::KeyboardMode;

use crate::{
    network::ssid_text,
    service_dialog::{self, Frame, Secret},
};

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
/// 어떤 프로그램이 D-Bus로 켠 연결이다. 없으면 NetworkManager가 저절로 연결하다 물었다
const USER_REQUESTED: u32 = 0x4;
/// NetworkManager가 GetSecrets의 답을 기다리는 시간은 120초다(요청을 보낸 때부터). 그보다 조금 일찍 창을 닫는다
const PROMPT_TIMEOUT: Duration = Duration::from_secs(115);
/// 메뉴 막대에서 고른 네트워크를 묻는 창이 키보드를 바로 받는 동안. 처음 물음과, 틀린 암호를 다시 묻는 물음까지
const EXPECT_JOIN: Duration = Duration::from_secs(150);

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
    /// 메뉴 막대에서 사람이 방금 고른 네트워크와, 그 네트워크를 묻는 창이 키보드를 바로 받아도 되는 마지막 때
    static EXPECTED: RefCell<Option<(String, Instant)>> = const { RefCell::new(None) };
}

/// 이 에이전트가 NetworkManager에 등록돼 있는지
pub fn registered() -> bool {
    REGISTERED.with(Cell::get)
}

/// 메뉴 막대가 사람이 고른 네트워크를 켜기 직전에 알린다. 그 네트워크를 묻는 창은 키보드를 바로 받는다
pub fn expect_join(network: &str) {
    EXPECTED.with(|expected| {
        expected.replace(Some((network.to_owned(), Instant::now() + EXPECT_JOIN)));
    });
}

/// 이 네트워크를 메뉴 막대에서 방금 골랐는지
fn join_expected(network: &str) -> bool {
    EXPECTED.with(|expected| {
        expected
            .borrow()
            .as_ref()
            .is_some_and(|(name, until)| name == network && Instant::now() < *until)
    })
}

/// 제목에 쓰는 보안 방식
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Security {
    /// WPA·WPA2 개인용 (wpa-psk)
    Wpa2,
    /// WPA3 개인용 (sae)
    Wpa3,
    /// WEP
    Wep { key_type: WepKeyType },
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

/// WEP 키의 꼴 (NMWepKeyType). 빈 설정으로 만든 새 연결은 모른다(0)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WepKeyType {
    Unknown,
    /// 16진수 10·26자리나 ASCII 5·13자
    Key,
    /// 1~64바이트의 암호 문구 (NetworkManager가 키로 바꾼다)
    Passphrase,
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
                security: Security::Wep { key_type },
                ..
            } => wep_key_valid(secret, *key_type),
            _ => !secret.is_empty(),
        }
    }

    /// 바이트 길이만으로 받을 수 있는지 정해지면 Some. 글자를 보아야 하면 None(64자리 WPA 키, WEP 키).
    /// 창은 칸의 글을 꺼내지 않고 이것으로 [연결]을 켜고 끈다
    pub fn accepts_length(&self, bytes: usize) -> Option<bool> {
        match self {
            Self::Password {
                security: Security::Wpa2,
                ..
            } => match bytes {
                8..=63 => Some(true),
                64 => None,
                _ => Some(false),
            },
            Self::Password {
                security:
                    Security::Wep {
                        key_type: WepKeyType::Key,
                    },
                ..
            } => match bytes {
                5 | 10 | 13 | 26 => None,
                _ => Some(false),
            },
            // 암호 문구이거나 꼴을 모르면 1~64바이트는 모두 된다(키의 꼴도 그 안에 든다)
            Self::Password {
                security: Security::Wep { .. },
                ..
            } => Some((1..=64).contains(&bytes)),
            _ => Some(bytes > 0),
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

/// WEP: 키는 10·26자리 16진수나 5·13자 ASCII, 암호 문구는 1~64바이트. 꼴을 모르면 둘 가운데 하나면 된다
/// (예전에는 모르는 꼴을 키로만 보아, 빈 설정으로 만든 새 연결에서 암호 문구를 받지 않았다)
fn wep_key_valid(key: &str, key_type: WepKeyType) -> bool {
    let as_key = || match key.len() {
        10 | 26 => key.bytes().all(|byte| byte.is_ascii_hexdigit()),
        5 | 13 => key
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' '),
        _ => false,
    };
    let as_passphrase = || (1..=64).contains(&key.len());
    match key_type {
        WepKeyType::Key => as_key(),
        WepKeyType::Passphrase => as_passphrase(),
        WepKeyType::Unknown => as_key() || as_passphrase(),
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
    /// 사람이 메뉴 막대에서 방금 고른 네트워크다: USER_REQUESTED가 붙었고 메뉴 막대가 그 네트워크를 켰다
    /// ([`expect_join`]). 이때만 창이 키보드를 바로 받는다
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
    // 제어 문자(NUL 등)는 바꿔 둔다. 메뉴 막대의 네트워크 이름과 같은 규칙이라 expect_join과 견줄 수 있다
    let network = value(WIRELESS, "ssid")
        .and_then(|ssid| ssid.get::<Vec<u8>>())
        .map(|ssid| ssid_text(&ssid))
        .filter(|ssid| !ssid.is_empty())
        .unwrap_or_else(|| ssid_text(text("connection", "id").as_bytes()));
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
                    // NMWepKeyType: 1은 키, 2는 암호 문구, 0(빈 설정으로 만든 연결)은 모른다
                    let key_type = match value(WIRELESS_SECURITY, "wep-key-type")
                        .and_then(|kind| kind.get::<u32>())
                    {
                        Some(1) => WepKeyType::Key,
                        Some(2) => WepKeyType::Passphrase,
                        _ => WepKeyType::Unknown,
                    };
                    Some(Fields::Password {
                        security: Security::Wep { key_type },
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
            // 인증서 칸이 PKCS#11(스마트카드)이면 NetworkManager는 그 PIN(…-cert-password)을 묻는다
            let pkcs11 = |key: &str| {
                value(IEEE_8021X, key)
                    .and_then(|cert| cert.get::<Vec<u8>>())
                    .is_some_and(|cert| cert.starts_with(b"pkcs11:"))
            };
            let phase2 = Some(text(IEEE_8021X, "phase2-auth"))
                .filter(|auth| !auth.is_empty())
                .unwrap_or_else(|| text(IEEE_8021X, "phase2-autheap"));
            // 이 창이 받는 비밀은 암호와 개인 키 암호뿐이다. 다른 비밀을 엉뚱한 칸으로 받아 답하면 NetworkManager가
            // 틀렸다고 다시 묻기를 되풀이하므로, 받지 못하는 비밀이면 묻지 않는다
            if !hints.is_empty() {
                // NetworkManager가 필요한 비밀을 알려 주면 그대로 따른다 (유선 802.1X. Wi-Fi는 힌트가 없다)
                if hinted("password") {
                    Some(Fields::Credentials { identity })
                } else if hinted("private-key-password") {
                    Some(Fields::PrivateKey { identity })
                } else {
                    None
                }
            } else {
                // 힌트가 없으면 NetworkManager의 need_secrets처럼 방식으로 정한다
                match method.as_str() {
                    "tls" if !pkcs11("client-cert") && !pkcs11("ca-cert") => {
                        Some(Fields::PrivateKey { identity })
                    }
                    // 터널 안이 TLS면 phase2 개인 키 암호다
                    "peap" | "ttls" if phase2 == "tls" => None,
                    method if PASSWORD_METHODS.contains(&method) => {
                        Some(Fields::Credentials { identity })
                    }
                    _ => None,
                }
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
    /// NetworkManager가 요청을 보낸 때. 기다리는 시간은 여기서부터 센다
    arrived: Instant,
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
        let Some(mut prompt) = request.prompt.clone() else {
            invocation.return_dbus_error(ERROR_NO_SECRETS, "this agent has no such secrets");
            return;
        };
        // 키보드를 바로 주는 것은 메뉴 막대에서 사람이 고른 네트워크뿐이다 (머리 주석)
        prompt.user_requested &= join_expected(&prompt.network);
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.queue.borrow_mut().push_back(Pending {
            id,
            request,
            prompt,
            invocation,
            arrived: Instant::now(),
        });
        self.show_next();
    }

    /// 떠 있는 물음이 없으면 다음 요청을 묻는다
    fn show_next(self: &Rc<Self>) {
        if self.current.borrow().is_some() {
            return;
        }
        // 줄을 서 있던 시간은 빼고 센다. NetworkManager는 요청을 보낸 때부터 120초를 기다리고, 그 뒤로는
        // CancelGetSecrets 없이 답을 버린다. 예전에는 창이 뜬 때부터 다시 115초를 세어, 줄을 서 있던 요청은
        // NetworkManager가 그만둔 뒤에도 창이 남아 사람이 친 암호가 버려졌다
        let (pending, left) = loop {
            let Some(pending) = self.queue.borrow_mut().pop_front() else {
                return;
            };
            let left = self.timeout.saturating_sub(pending.arrived.elapsed());
            // 남은 시간이 사람이 칠 만큼이 아니면 묻지 않는다
            if left >= self.timeout / 10 {
                break (pending, left);
            }
            pending
                .invocation
                .return_dbus_error(ERROR_NO_SECRETS, "the request waited too long");
        };
        let id = pending.id;
        let prompt = pending.prompt.clone();
        let weak = Rc::downgrade(self);
        // 이 스레드의 기본 컨텍스트에서 센다 (시험은 따로 만든 컨텍스트를 돌린다)
        let timer = glib::spawn_future_local(async move {
            glib::timeout_future(left).await;
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
                // 틀린 암호를 다시 묻는 창도 키보드를 바로 받게 한다
                if prompt.user_requested {
                    expect_join(&prompt.network);
                }
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
        // 메뉴 막대에서 사람이 고른 네트워크면 바로 칠 수 있게 키보드를 받고, 그 밖의 창은 눌러야 받는다
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
        dialog.password.connect_changed(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.join.set_sensitive(dialog.acceptable());
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
        let weak = Rc::downgrade(&dialog);
        dialog.frame.on_dismiss(move || {
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

    /// 칸의 글이 받을 수 있는 암호인지. 길이로 정해지면 글을 꺼내지 않는다. text()는 GTK가 보안 메모리에 둔
    /// 암호를 지워지지 않는 사본으로 꺼내므로, 예전처럼 글자를 칠 때마다 꺼내지 않는다
    fn acceptable(&self) -> bool {
        let bytes = self
            .password
            .delegate()
            .and_downcast::<gtk::Text>()
            .map(|text| text.buffer().bytes());
        match bytes.and_then(|bytes| self.fields.accepts_length(bytes)) {
            Some(decided) => decided,
            None => self.fields.accepts(&self.password.text()),
        }
    }

    /// 연결: 칸이 받을 수 있는 암호일 때만 (Enter도 기본 단추를 누른다)
    fn submit(&self) {
        if !self.acceptable() {
            return;
        }
        let secret = Secret::new(&self.password.text());
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
                security: Security::Wep {
                    key_type: WepKeyType::Passphrase
                },
                key: "wep-key2".into()
            }
        );
        // 빈 설정으로 만든 새 연결은 키의 꼴을 모른다
        let unknown = wifi("Old", &[("key-mgmt", "none".to_variant())]);
        assert_eq!(
            prompt_for(&unknown, WIRELESS_SECURITY, &[], ALLOW_INTERACTION)
                .unwrap()
                .fields,
            Fields::Password {
                security: Security::Wep {
                    key_type: WepKeyType::Unknown
                },
                key: "wep-key0".into()
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
        // 이 창이 받지 못하는 비밀을 알려 오면 묻지 않는다 (엉뚱한 칸으로 답하면 NetworkManager가 다시 묻기를 되풀이한다)
        let pin = prompt_for(
            &tls,
            IEEE_8021X,
            &["client-cert-password".into()],
            ALLOW_INTERACTION,
        );
        assert!(pin.is_none());
        let sim = enterprise(WIRELESS, &["sim"], "");
        assert!(prompt_for(&sim, IEEE_8021X, &[], ALLOW_INTERACTION).is_none());
    }

    #[test]
    fn enterprise_secrets_this_dialog_cannot_take_are_not_asked() {
        // 터널 안이 TLS면 phase2 개인 키 암호가 필요하다
        let mut inner_tls = enterprise(WIRELESS, &["ttls"], "hong");
        inner_tls
            .get_mut(IEEE_8021X)
            .unwrap()
            .insert("phase2-autheap".into(), "tls".to_variant());
        assert!(prompt_for(&inner_tls, IEEE_8021X, &[], ALLOW_INTERACTION).is_none());
        let mut inner_password = enterprise(WIRELESS, &["peap"], "hong");
        inner_password
            .get_mut(IEEE_8021X)
            .unwrap()
            .insert("phase2-auth".into(), "mschapv2".to_variant());
        assert!(matches!(
            prompt_for(&inner_password, IEEE_8021X, &[], ALLOW_INTERACTION)
                .unwrap()
                .fields,
            Fields::Credentials { .. }
        ));
        // 스마트카드(PKCS#11) 인증서의 PIN은 개인 키 암호 칸으로 받지 않는다
        let mut smartcard = enterprise(WIRELESS, &["tls"], "hong");
        smartcard.get_mut(IEEE_8021X).unwrap().insert(
            "client-cert".into(),
            b"pkcs11:token=card".to_vec().to_variant(),
        );
        assert!(prompt_for(&smartcard, IEEE_8021X, &[], ALLOW_INTERACTION).is_none());
        let mut file_key = enterprise(WIRELESS, &["tls"], "hong");
        file_key.get_mut(IEEE_8021X).unwrap().insert(
            "client-cert".into(),
            b"file:///etc/cert.pem\0".to_vec().to_variant(),
        );
        assert!(matches!(
            prompt_for(&file_key, IEEE_8021X, &[], ALLOW_INTERACTION)
                .unwrap()
                .fields,
            Fields::PrivateKey { .. }
        ));
    }

    #[test]
    fn network_names_lose_control_characters() {
        let nul = wifi("Lab\0Guest", &[("key-mgmt", "wpa-psk".to_variant())]);
        let prompt = prompt_for(&nul, WIRELESS_SECURITY, &[], ALLOW_INTERACTION).unwrap();
        assert_eq!(prompt.network, "Lab\u{FFFD}Guest");
    }

    #[test]
    fn only_networks_picked_in_the_menu_take_the_keyboard() {
        expect_join("집 Wi-Fi");
        assert!(join_expected("집 Wi-Fi"));
        assert!(!join_expected("Lab"));
        expect_join("Lab");
        assert!(!join_expected("집 Wi-Fi"));
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
        let wep_field = |key_type| Fields::Password {
            security: Security::Wep { key_type },
            key: "wep-key0".into(),
        };
        let wep = wep_field(WepKeyType::Key);
        assert!(wep.accepts("abcde"));
        assert!(wep.accepts("0123456789"));
        assert!(!wep.accepts("012345678g"));
        assert!(!wep.accepts("abcdef"));
        let wep_phrase = wep_field(WepKeyType::Passphrase);
        assert!(wep_phrase.accepts("x"));
        assert!(!wep_phrase.accepts(""));
        assert!(!wep_phrase.accepts(&"x".repeat(65)));
        // 꼴을 모르면 키나 암호 문구 가운데 하나면 된다 (nm_utils_wep_key_valid의 UNKNOWN)
        let wep_unknown = wep_field(WepKeyType::Unknown);
        assert!(wep_unknown.accepts("abcde"));
        assert!(wep_unknown.accepts("my old router"));
        assert!(!wep_unknown.accepts(""));
        assert!(!wep_unknown.accepts(&"x".repeat(65)));
        let sae = Fields::Password {
            security: Security::Wpa3,
            key: "psk".into(),
        };
        assert!(sae.accepts("x"));
        assert!(!sae.accepts(""));
    }

    /// 창은 길이로 정해지면 칸의 글을 꺼내지 않는다. 길이로 정한 것이 글을 보고 정한 것과 같아야 한다
    #[test]
    fn length_checks_agree_with_full_checks() {
        let fields = [
            Fields::Password {
                security: Security::Wpa2,
                key: "psk".into(),
            },
            Fields::Password {
                security: Security::Wpa3,
                key: "psk".into(),
            },
            Fields::Password {
                security: Security::Wep {
                    key_type: WepKeyType::Key,
                },
                key: "wep-key0".into(),
            },
            Fields::Password {
                security: Security::Wep {
                    key_type: WepKeyType::Passphrase,
                },
                key: "wep-key0".into(),
            },
            Fields::Password {
                security: Security::Wep {
                    key_type: WepKeyType::Unknown,
                },
                key: "wep-key0".into(),
            },
            Fields::Credentials {
                identity: "hong".into(),
            },
        ];
        let samples = [
            String::new(),
            "abcde".into(),
            "0123456789".into(),
            "12345678".into(),
            "한글암".into(),
            "0f".repeat(32),
            "g".repeat(64),
            "x".repeat(65),
        ];
        for field in &fields {
            for sample in &samples {
                if let Some(decided) = field.accepts_length(sample.len()) {
                    assert_eq!(decided, field.accepts(sample), "{field:?} {sample:?}");
                }
            }
        }
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
                    // 메뉴 막대에서 고른 네트워크다
                    expect_join("Home");
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

                    // 다른 프로그램이 켠 연결은 USER_REQUESTED가 붙어도 키보드를 바로 받지 않는다
                    let lab = wifi("Lab", &[("key-mgmt", "wpa-psk".to_variant())]);
                    answers.borrow_mut().push_back(Some("lab password"));
                    get_secrets(&manager, &name, &lab, path, WIRELESS_SECURITY, flags)
                        .await
                        .unwrap();
                    assert!(!asked.borrow()[2].user_requested);
                })
            })
            .unwrap();
        bus.down();
    }

    /// 줄을 서 있던 요청도 NetworkManager가 보낸 때부터 센다. 남은 시간이 너무 짧으면 묻지 않고 NoSecrets로 답한다
    #[test]
    fn queued_requests_keep_network_managers_deadline() {
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
                    let manager = connect(&address).await;
                    let fake = Fake::default();
                    let (asked, answers) = (fake.asked.clone(), fake.answers.clone());
                    let agent = Agent::new(Box::new(fake), Duration::from_millis(400));
                    agent.export(&agent_bus).unwrap();
                    agent
                        .manager
                        .replace(manager.unique_name().map(|name| name.to_string()));
                    let name = agent_bus.unique_name().unwrap().to_string();
                    let home = wifi("Home", &[("key-mgmt", "wpa-psk".to_variant())]);
                    let lab = wifi("Lab", &[("key-mgmt", "wpa-psk".to_variant())]);
                    // 첫 물음은 아무도 답하지 않는다. 그동안 둘째 요청이 줄을 선다
                    answers.borrow_mut().extend([None, Some("late")]);
                    let first = get_secrets(
                        &manager,
                        &name,
                        &home,
                        "/org/freedesktop/NetworkManager/Settings/1",
                        WIRELESS_SECURITY,
                        ALLOW_INTERACTION,
                    );
                    let second = get_secrets(
                        &manager,
                        &name,
                        &lab,
                        "/org/freedesktop/NetworkManager/Settings/2",
                        WIRELESS_SECURITY,
                        ALLOW_INTERACTION,
                    );
                    let (first, second, ()) = futures_join(first, second, async {}).await;
                    assert_eq!(dbus_error_name(&first.unwrap_err()), ERROR_NO_SECRETS);
                    assert_eq!(dbus_error_name(&second.unwrap_err()), ERROR_NO_SECRETS);
                    // 둘째 요청은 창을 띄우지 않았다
                    assert_eq!(asked.borrow().len(), 1);
                })
            })
            .unwrap();
        bus.down();
    }

    /// CancelGetSecrets는 떠 있는 물음을 닫고 AgentCanceled로 답하게 한다. 기다리던 다음 요청은 그 뒤에 묻는다
    #[test]
    fn cancel_closes_the_prompt_and_asks_the_next_one() {
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
