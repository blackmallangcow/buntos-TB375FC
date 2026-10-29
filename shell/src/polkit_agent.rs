// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! polkit 인증 에이전트 (org.freedesktop.PolicyKit1.AuthenticationAgent).
//!
//! 이 세션의 관리자 인증(pkexec, 시간대, 기기 이름, AI 에이전트 권한 등)을 polkit이 이 에이전트에게
//! 맡긴다. macOS 인증 창처럼 화면 가운데에 뜨는 창이 키보드를 독점하고 사용자 이름과 암호를 받는다.
//! 암호가 틀리면 창을 흔들고 다시 묻는다.
//!
//! 암호 확인은 polkit의 polkit-agent-helper-1이 PAM(polkit-1)으로 한다. 순서는 polkitagentsession.c와 같다.
//! - polkit 126부터는 root로 도는 소켓 활성화 도우미(/run/polkit/agent-helper.socket)에 붙어 사용자 이름과
//!   쿠키를 한 줄씩 쓴다. 소켓이 없으면 setuid 도우미를 `polkit-agent-helper-1 <사용자>`로 실행해 쿠키만 쓴다.
//! - 도우미는 `PAM_PROMPT_ECHO_OFF 글`, `PAM_PROMPT_ECHO_ON 글`, `PAM_ERROR_MSG 글`, `PAM_TEXT_INFO 글`,
//!   `SUCCESS`, `FAILURE`를 한 줄씩 낸다(g_strescape로 이스케이프). 물음에는 답을 한 줄로 쓴다.
//! - SUCCESS는 도우미가 polkitd에 인증을 알렸다는 뜻이다. 그때 BeginAuthentication에 답한다.
//!   취소하면 Cancelled 오류로 답한다(pkexec는 126으로 끝난다).

use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    ffi::OsStr,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

/// 인증 창의 레이어 이름이자 접근성 이름. 컴포지터(comp/src/agent.rs)와 에이전트 데몬
/// (agent/src/atspi.rs)이 이 이름의 창을 에이전트가 건드리지 못하게 막는다. 사용자만 답해야 하는 AI
/// 접근 요청 창(agent_access.rs)도 이 이름을 쓴다
pub(crate) const NAMESPACE: &str = "bunt-polkit-agent";
const AGENT_PATH: &str = "/org/buntos/PolicyKit1/AuthenticationAgent";
const AGENT_INTERFACE: &str = "org.freedesktop.PolicyKit1.AuthenticationAgent";
const AUTHORITY_NAME: &str = "org.freedesktop.PolicyKit1";
const AUTHORITY_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const AUTHORITY_INTERFACE: &str = "org.freedesktop.PolicyKit1.Authority";
/// polkit 126부터의 소켓 활성화 도우미 (polkit-agent-helper.socket)
const HELPER_SOCKET: &str = "/run/polkit/agent-helper.socket";
/// 소켓이 없을 때 실행하는 setuid 도우미
const HELPER: &str = "/usr/lib/polkit-1/polkit-agent-helper-1";
const ERROR_CANCELLED: &str = "org.freedesktop.PolicyKit1.Error.Cancelled";
const ERROR_FAILED: &str = "org.freedesktop.PolicyKit1.Error.Failed";
/// 앱을 찾지 못했을 때의 그림
const LOCK_ICON: &str = "system-lock-screen-symbolic";
const INTROSPECTION: &str = r#"<node>
  <interface name="org.freedesktop.PolicyKit1.AuthenticationAgent">
    <method name="BeginAuthentication">
      <arg name="action_id" type="s" direction="in"/>
      <arg name="message" type="s" direction="in"/>
      <arg name="icon_name" type="s" direction="in"/>
      <arg name="details" type="a{ss}" direction="in"/>
      <arg name="cookie" type="s" direction="in"/>
      <arg name="identities" type="a(sa{sv})" direction="in"/>
    </method>
    <method name="CancelAuthentication">
      <arg name="cookie" type="s" direction="in"/>
    </method>
  </interface>
</node>"#;

/// 세션의 polkit 인증 에이전트가 된다. polkitd가 (다시) 뜰 때마다 등록한다
pub fn start(app: &gtk::Application, tr: &Rc<Translations>) {
    let agent = Rc::new(Agent {
        app: app.clone(),
        tr: tr.clone(),
        authority: RefCell::default(),
        queue: RefCell::default(),
        dialog: RefCell::default(),
    });
    glib::spawn_future_local(async move {
        let bus = match gio::bus_get_future(gio::BusType::System).await {
            Ok(bus) => bus,
            Err(err) => {
                eprintln!("bunt-shell: no system bus, polkit authentication is off: {err}");
                return;
            }
        };
        if let Err(err) = agent.export(&bus) {
            eprintln!("bunt-shell: could not export the polkit authentication agent: {err}");
            return;
        }
        let appeared = agent.clone();
        let vanished = agent.clone();
        // 감시는 셸이 끝날 때까지 둔다
        let _watch = gio::bus_watch_name_on_connection(
            &bus,
            AUTHORITY_NAME,
            gio::BusNameWatcherFlags::AUTO_START,
            move |bus, _, owner| {
                appeared.authority.replace(Some(owner.to_owned()));
                glib::spawn_future_local(async move {
                    if let Err(err) = register(&bus).await {
                        eprintln!(
                            "bunt-shell: could not register the polkit authentication agent: {err}"
                        );
                    }
                });
            },
            move |_, _| {
                vanished.authority.replace(None);
            },
        );
    });
}

/// 세션을 주체로 등록한다 (RegisterAuthenticationAgent). 메시지는 이 로캘로 번역돼 온다
async fn register(bus: &gio::DBusConnection) -> Result<(), glib::Error> {
    let session = session_id(bus).await?;
    let details = HashMap::from([("session-id".to_owned(), session.to_variant())]);
    let subject = ("unix-session", details);
    bus.call_future(
        Some(AUTHORITY_NAME),
        AUTHORITY_PATH,
        AUTHORITY_INTERFACE,
        "RegisterAuthenticationAgent",
        Some(&(subject, locale(), AGENT_PATH).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
    )
    .await?;
    Ok(())
}

/// logind 세션 ID. 세션이 주는 XDG_SESSION_ID가 없으면 logind에 이 프로세스의 세션을 묻는다
async fn session_id(bus: &gio::DBusConnection) -> Result<String, glib::Error> {
    if let Ok(id) = std::env::var("XDG_SESSION_ID")
        && !id.is_empty()
    {
        return Ok(id);
    }
    let reply = bus
        .call_future(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "GetSessionByPID",
            Some(&(std::process::id(),).to_variant()),
            Some(glib::VariantTy::new("(o)").expect("valid type")),
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await?;
    let path = reply.child_value(0).str().unwrap_or_default().to_owned();
    let reply = bus
        .call_future(
            Some("org.freedesktop.login1"),
            &path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&("org.freedesktop.login1.Session", "Id").to_variant()),
            Some(glib::VariantTy::new("(v)").expect("valid type")),
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await?;
    reply
        .child_value(0)
        .as_variant()
        .and_then(|id| id.str().map(str::to_owned))
        .ok_or_else(|| glib::Error::new(gio::IOErrorEnum::Failed, "logind gave no session ID"))
}

fn locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_else(|| "C".to_owned())
}

/// 인증할 수 있는 사용자 (polkit의 unix-user 신원)
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identity {
    uid: u32,
    login: String,
    /// 실명 (GECOS의 첫 칸). 없으면 사용자 이름
    display: String,
}

impl Identity {
    /// /etc/passwd의 로컬 계정. 이 셸의 사용자는 passwd에 없어도 GLib에서 이름을 얻는다
    fn for_uid(uid: u32) -> Option<Identity> {
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        passwd
            .lines()
            .find_map(|line| parse_passwd_line(line).filter(|identity| identity.uid == uid))
            .or_else(|| {
                (Some(uid) == current_uid()).then(|| {
                    let login = glib::user_name().to_string_lossy().into_owned();
                    let real = glib::real_name().to_string_lossy().into_owned();
                    Identity {
                        uid,
                        display: if real.is_empty() || real == "Unknown" {
                            login.clone()
                        } else {
                            real
                        },
                        login,
                    }
                })
            })
    }

    /// 사용자 이름 칸에 쓴 글이 이 사람인지 (사용자 이름이나 실명)
    fn is_named(&self, text: &str) -> bool {
        let text = text.trim();
        self.login.eq_ignore_ascii_case(text) || self.display == text
    }
}

/// `이름:암호:uid:gid:GECOS:홈:셸`
fn parse_passwd_line(line: &str) -> Option<Identity> {
    let fields: Vec<&str> = line.split(':').collect();
    let [login, _, uid, _, gecos, ..] = fields.as_slice() else {
        return None;
    };
    let uid = uid.parse().ok()?;
    let real = gecos.split(',').next().unwrap_or_default().trim();
    Some(Identity {
        uid,
        login: (*login).to_owned(),
        display: if real.is_empty() {
            (*login).to_owned()
        } else {
            real.to_owned()
        },
    })
}

/// 이 셸의 uid (/proc/self의 주인)
fn current_uid() -> Option<u32> {
    std::fs::metadata("/proc/self").ok().map(|meta| meta.uid())
}

/// polkitd가 보낸 인증 요청 하나
struct Request {
    cookie: String,
    action_id: String,
    message: String,
    icon_name: String,
    /// polkit 동작의 세부 (pkexec는 program, command_line 등을 준다)
    details: HashMap<String, String>,
    /// 인증을 요청한 프로세스 (pkexec면 pkexec를 실행한 앱)
    subject_pid: Option<u32>,
    identities: Vec<Identity>,
    /// 끝나면(성공, 취소) 이 호출에 답한다
    invocation: gio::DBusMethodInvocation,
}

impl Request {
    fn parse(
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) -> Result<Request, gio::DBusMethodInvocation> {
        type Args = (
            String,
            String,
            String,
            HashMap<String, String>,
            String,
            Vec<(String, HashMap<String, glib::Variant>)>,
        );
        let Some((action_id, message, icon_name, details, cookie, identities)) =
            params.get::<Args>()
        else {
            return Err(invocation);
        };
        let mut users: Vec<Identity> = identities
            .iter()
            .filter(|(kind, _)| kind == "unix-user")
            .filter_map(|(_, details)| details.get("uid")?.get::<u32>())
            .filter_map(Identity::for_uid)
            .collect();
        users.dedup();
        if users.is_empty() {
            return Err(invocation);
        }
        Ok(Request {
            cookie,
            action_id,
            message,
            icon_name,
            subject_pid: details
                .get("polkit.subject-pid")
                .and_then(|pid| pid.parse().ok()),
            details,
            identities: users,
            invocation,
        })
    }
}

struct Agent {
    app: gtk::Application,
    tr: Rc<Translations>,
    /// polkitd의 고유 버스 이름. 이 이름이 부른 메서드만 받는다
    authority: RefCell<Option<String>>,
    /// 보이는 창 다음에 물을 요청
    queue: RefCell<VecDeque<Request>>,
    dialog: RefCell<Option<Rc<Dialog>>>,
}

impl Agent {
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
        // 버스 정책도 polkitd만 이 인터페이스를 부르게 하지만, 가짜 암호 창을 띄우지 못하게 한 번 더 본다
        if sender.is_none() || self.authority.borrow().as_deref() != sender {
            invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.AccessDenied",
                "only polkitd may call the authentication agent",
            );
            return;
        }
        match method {
            "BeginAuthentication" => match Request::parse(params, invocation) {
                Ok(request) => {
                    self.queue.borrow_mut().push_back(request);
                    self.show_next();
                }
                Err(invocation) => {
                    invocation.return_dbus_error(ERROR_FAILED, "no identity to authenticate")
                }
            },
            "CancelAuthentication" => {
                if let Some((cookie,)) = params.get::<(String,)>() {
                    self.cancel(&cookie);
                }
                invocation.return_value(None);
            }
            _ => invocation
                .return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
        }
    }

    /// 보이는 창이 없으면 다음 요청을 묻는다
    fn show_next(self: &Rc<Self>) {
        if self.dialog.borrow().is_some() {
            return;
        }
        let Some(request) = self.queue.borrow_mut().pop_front() else {
            return;
        };
        let dialog = Dialog::new(self, request);
        self.dialog.replace(Some(dialog.clone()));
        dialog.present();
    }

    fn cancel(self: &Rc<Self>, cookie: &str) {
        let active = self.dialog.borrow().clone();
        if let Some(dialog) = active.filter(|dialog| dialog.cookie == cookie) {
            dialog.finish(false);
            return;
        }
        let mut queue = self.queue.borrow_mut();
        if let Some(index) = queue.iter().position(|request| request.cookie == cookie)
            && let Some(request) = queue.remove(index)
        {
            request
                .invocation
                .return_dbus_error(ERROR_CANCELLED, "Authentication was cancelled");
        }
    }

    /// 창이 닫혔다
    fn closed(self: &Rc<Self>) {
        self.dialog.replace(None);
        self.show_next();
    }
}

/// 도우미가 보낸 줄
#[derive(Debug, PartialEq, Eq)]
enum Event {
    /// 답을 기다린다. echo면 보이는 글을 묻는다
    Prompt {
        echo: bool,
        text: String,
    },
    Error(String),
    Info(String),
    /// 인증이 끝났다 (성공인지)
    Done(bool),
}

impl Event {
    /// 모르는 줄은 실패다 (polkitagentsession.c와 같다)
    fn parse(line: &str) -> Event {
        let line = unescape(line);
        if let Some(text) = line.strip_prefix("PAM_PROMPT_ECHO_OFF ") {
            Event::Prompt {
                echo: false,
                text: text.to_owned(),
            }
        } else if let Some(text) = line.strip_prefix("PAM_PROMPT_ECHO_ON ") {
            Event::Prompt {
                echo: true,
                text: text.to_owned(),
            }
        } else if let Some(text) = line.strip_prefix("PAM_ERROR_MSG ") {
            Event::Error(text.to_owned())
        } else if let Some(text) = line.strip_prefix("PAM_TEXT_INFO ") {
            Event::Info(text.to_owned())
        } else {
            Event::Done(line.starts_with("SUCCESS"))
        }
    }
}

/// g_strcompress: \b \f \n \r \t \v, 8진수 \ooo, 그 밖의 \x는 x
fn unescape(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        index += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let Some(&next) = bytes.get(index) else {
            break;
        };
        index += 1;
        match next {
            b'0'..=b'7' => {
                let mut value = u32::from(next - b'0');
                for _ in 0..2 {
                    match bytes.get(index) {
                        Some(&digit @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(digit - b'0');
                            index += 1;
                        }
                        _ => break,
                    }
                }
                out.push((value & 0xff) as u8);
            }
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            other => out.push(other),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 암호 사본. 버릴 때 0으로 덮는다
struct Secret(Vec<u8>);

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&self.0);
    }
}

/// 도우미 쪽을 닫으면 도우미가 FAILURE로 끝난다
enum Helper {
    Socket(gio::SocketConnection),
    Process(gio::Subprocess),
}

/// 도우미와 나누는 대화 하나 (암호 한 번 확인)
struct Session {
    task: glib::JoinHandle<()>,
    /// 도우미 쪽으로 쓰는 스트림. 연결되기 전에는 None
    output: RefCell<Option<gio::OutputStream>>,
    helper: RefCell<Option<Helper>>,
    /// 첫 물음에 넣을 답 (확인을 누를 때 받은 암호)
    answer: RefCell<Option<Secret>>,
    /// 도우미가 사용자의 답을 기다린다
    asking: Cell<bool>,
}

impl Session {
    fn write(&self, mut line: Vec<u8>) {
        line.push(b'\n');
        let Some(output) = self.output.borrow().clone() else {
            return;
        };
        glib::spawn_future_local(async move {
            // 버퍼는 돌려받아 덮는다
            let buffer = match output.write_all_future(line, glib::Priority::DEFAULT).await {
                Ok((buffer, ..)) | Err((buffer, _)) => buffer,
            };
            drop(Secret(buffer));
        });
    }

    fn close(&self) {
        self.task.abort();
        self.answer.replace(None);
        match self.helper.take() {
            Some(Helper::Socket(connection)) => {
                let _ = connection.close(gio::Cancellable::NONE);
            }
            Some(Helper::Process(process)) => process.force_exit(),
            None => {}
        }
    }
}

/// 도우미를 연다: (쓰기, 읽기, 닫을 것). 사용자 이름과 쿠키는 여기서 쓴다
async fn open_helper(
    login: &str,
    cookie: &str,
) -> Result<(gio::OutputStream, gio::DataInputStream, Helper), glib::Error> {
    if Path::new(HELPER_SOCKET).exists() {
        let address = gio::UnixSocketAddress::new(Path::new(HELPER_SOCKET));
        match gio::SocketClient::new().connect_future(&address).await {
            Ok(connection) => {
                let output = connection.output_stream();
                let input = gio::DataInputStream::new(&connection.input_stream());
                let first = format!("{login}\n{cookie}\n").into_bytes();
                output
                    .write_all_future(first, glib::Priority::DEFAULT)
                    .await
                    .map_err(|(_, err)| err)?;
                return Ok((output, input, Helper::Socket(connection)));
            }
            Err(err) => eprintln!(
                "bunt-shell: cannot connect to {HELPER_SOCKET}, trying the setuid helper: {err}"
            ),
        }
    }
    let process = gio::Subprocess::newv(
        &[OsStr::new(HELPER), OsStr::new(login)],
        gio::SubprocessFlags::STDIN_PIPE | gio::SubprocessFlags::STDOUT_PIPE,
    )?;
    let missing = || glib::Error::new(gio::IOErrorEnum::Failed, "helper pipes are missing");
    let output = process.stdin_pipe().ok_or_else(missing)?;
    let input = gio::DataInputStream::new(&process.stdout_pipe().ok_or_else(missing)?);
    output
        .write_all_future(format!("{cookie}\n").into_bytes(), glib::Priority::DEFAULT)
        .await
        .map_err(|(_, err)| err)?;
    Ok((output, input, Helper::Process(process)))
}

/// 인증을 요청한 앱: (이름, 아이콘). 프로세스의 실행 파일을 Exec로 실행하는 앱을 찾는다
fn requesting_app(pid: u32) -> Option<(String, Option<gio::Icon>)> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    gio::AppInfo::all().into_iter().find_map(|app| {
        let executable = app.executable();
        let path = if executable.is_absolute() {
            Some(executable)
        } else {
            glib::find_program_in_path(&executable)
        }?;
        let path: PathBuf = std::fs::canonicalize(path).ok()?;
        (path == exe).then(|| (app.display_name().to_string(), app.icon()))
    })
}

/// 동작의 설명. polkitd는 systemd 동작(시간대, 기기 이름)의 번역(gettext-domain)을 쓰지 않아 영어로
/// 보내므로, 시스템 설정이 부르는 동작은 셸의 문구(auth-action-<동작 ID의 .을 -로>)로 바꾼다
fn action_message(
    tr: &Translations,
    action_id: &str,
    message: &str,
    details: &HashMap<String, String>,
    app: Option<&str>,
) -> String {
    let id = format!("auth-action-{}", action_id.replace('.', "-"));
    let mut args = FluentArgs::new();
    args.set(
        "app",
        app.map_or_else(|| tr.get("auth-someone"), str::to_owned),
    );
    // polkitd(128)는 pkexec가 넣은 세부(program 등)를 인증 에이전트에 넘기지 않는다. 실행할 프로그램은
    // polkitd가 그 세부로 만든 설명("… run `/usr/bin/true' as …")이나 pkexec의 명령줄에서 읽는다
    let program = details
        .get("program")
        .cloned()
        .or_else(|| quoted_program(message))
        .or_else(|| {
            details
                .get("polkit.caller-pid")
                .and_then(|pid| pkexec_program(pid))
        });
    let needs_program = action_id == "org.freedesktop.policykit.exec";
    match program {
        Some(program) => args.set("program", program),
        None if needs_program => return message.to_owned(),
        None => {}
    }
    let text = tr.get_with(&id, &args);
    // 없는 메시지는 ID가 그대로 돌아온다
    if text == id { message.to_owned() } else { text }
}

/// polkit 설명 안의 `프로그램' (pkexec의 설명 형식)
fn quoted_program(message: &str) -> Option<String> {
    let (_, rest) = message.split_once('`')?;
    let (program, _) = rest.split_once('\'')?;
    (!program.is_empty()).then(|| program.to_owned())
}

/// pkexec가 실행할 프로그램: pkexec 명령줄에서 옵션(--user 이름 등)을 뺀 첫 인자
fn pkexec_program(pid: &str) -> Option<String> {
    let pid: u32 = pid.parse().ok()?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let mut args = cmdline
        .split(|byte| *byte == 0)
        .skip(1)
        .map(|arg| String::from_utf8_lossy(arg).into_owned());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--user" | "-u" => {
                args.next();
            }
            _ if arg.starts_with('-') => {}
            _ => return Some(arg),
        }
    }
    None
}

/// 인증 창
struct Dialog {
    agent: Weak<Agent>,
    tr: Rc<Translations>,
    cookie: String,
    identities: Vec<Identity>,
    /// 답할 호출. 끝내면 비운다
    invocation: RefCell<Option<gio::DBusMethodInvocation>>,
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    user: gtk::Entry,
    password: gtk::PasswordEntry,
    /// PAM이 보이는 글(일회용 코드 등)을 물을 때 쓰는 칸
    answer: gtk::Entry,
    info: gtk::Label,
    confirm: gtk::Button,
    /// 도우미가 보이는 글을 묻는 중이다 (answer 칸에 답한다)
    echo: Cell<bool>,
    session: RefCell<Option<Session>>,
    /// 흔들기 애니메이션을 다시 시작하려고 두 클래스를 번갈아 쓴다
    shakes: Cell<u32>,
}

impl Dialog {
    fn new(agent: &Rc<Agent>, request: Request) -> Rc<Dialog> {
        let tr = agent.tr.clone();
        let window = crate::layer_window(&agent.app, NAMESPACE);
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        if crate::interface_settings()
            .is_some_and(|settings| settings.string("color-scheme") == "prefer-dark")
        {
            window.add_css_class("dark");
        }

        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["auth-panel"])
            .build();
        let app = request.subject_pid.and_then(requesting_app);
        panel.append(&requester_icon(app.as_ref(), &request.icon_name));
        let title = app
            .as_ref()
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| tr.get("auth-title"));
        panel.append(
            &gtk::Label::builder()
                .label(title)
                .justify(gtk::Justification::Center)
                .wrap(true)
                .max_width_chars(30)
                .css_classes(["auth-title"])
                .build(),
        );
        let me = current_uid();
        let default = request
            .identities
            .iter()
            .position(|identity| Some(identity.uid) == me)
            .unwrap_or(0);
        let only_me = request.identities.len() == 1 && Some(request.identities[0].uid) == me;
        let hint = tr.get(if only_me {
            "auth-hint"
        } else {
            "auth-hint-admin"
        });
        panel.append(
            &gtk::Label::builder()
                .label(format!(
                    "{}\n{hint}",
                    action_message(
                        &tr,
                        &request.action_id,
                        &request.message,
                        &request.details,
                        app.as_ref().map(|(name, _)| name.as_str())
                    )
                ))
                .justify(gtk::Justification::Center)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .max_width_chars(34)
                .css_classes(["auth-message"])
                .build(),
        );

        let fields = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["auth-fields"])
            .build();
        let user = gtk::Entry::builder()
            .text(&request.identities[default].display)
            .placeholder_text(tr.get("auth-user-name"))
            .activates_default(true)
            .css_classes(["auth-field"])
            .build();
        user.update_property(&[gtk::accessible::Property::Label(&tr.get("auth-user-name"))]);
        let password = gtk::PasswordEntry::builder()
            .placeholder_text(tr.get("auth-password"))
            .activates_default(true)
            .css_classes(["auth-field"])
            .build();
        password.update_property(&[gtk::accessible::Property::Label(&tr.get("auth-password"))]);
        let answer = gtk::Entry::builder()
            .activates_default(true)
            .visible(false)
            .css_classes(["auth-field"])
            .build();
        fields.append(&user);
        fields.append(&password);
        fields.append(&answer);
        panel.append(&fields);
        let info = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .justify(gtk::Justification::Center)
            .max_width_chars(34)
            .visible(false)
            .css_classes(["auth-info"])
            .build();
        panel.append(&info);

        let buttons = gtk::Box::builder()
            .spacing(8)
            .homogeneous(true)
            .css_classes(["auth-buttons"])
            .build();
        let cancel = gtk::Button::with_label(&tr.get("auth-cancel"));
        let confirm = gtk::Button::builder()
            .label(tr.get("auth-confirm"))
            .css_classes(["suggested-action"])
            .build();
        buttons.append(&cancel);
        buttons.append(&confirm);
        panel.append(&buttons);
        window.set_child(Some(&panel));
        window.set_default_widget(Some(&confirm));

        let dialog = Rc::new(Dialog {
            agent: Rc::downgrade(agent),
            tr,
            cookie: request.cookie,
            identities: request.identities,
            invocation: RefCell::new(Some(request.invocation)),
            window,
            panel,
            user,
            password,
            answer,
            info,
            confirm,
            echo: Cell::new(false),
            session: RefCell::default(),
            shakes: Cell::new(0),
        });

        let weak = Rc::downgrade(&dialog);
        dialog.confirm.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.submit();
            }
        });
        let weak = Rc::downgrade(&dialog);
        cancel.connect_clicked(move |_| {
            if let Some(dialog) = weak.upgrade() {
                dialog.finish(false);
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&dialog);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if let Some(dialog) = weak.upgrade() {
                dialog.finish(false);
            }
            glib::Propagation::Stop
        });
        dialog.window.add_controller(keys);
        dialog
    }

    fn present(&self) {
        // 창을 띄우기 전에 포커스를 정해야 이름 칸의 글이 선택된 채로 남지 않는다
        let focus: &gtk::Widget = if self.user.text().is_empty() {
            self.user.upcast_ref()
        } else {
            self.password.upcast_ref()
        };
        GtkWindowExt::set_focus(&self.window, Some(focus));
        self.window.present();
        self.user.select_region(0, 0);
    }

    /// 확인: 도우미가 물음을 기다리면 답하고, 아니면 새로 확인을 시작한다
    fn submit(self: &Rc<Self>) {
        if let Some(session) = self.session.borrow().as_ref() {
            if session.asking.replace(false) {
                let field: &gtk::Editable = if self.echo.get() {
                    self.answer.upcast_ref()
                } else {
                    self.password.upcast_ref()
                };
                let text = field.text();
                field.set_text("");
                self.set_busy(true);
                session.write(text.as_bytes().to_vec());
            }
            return;
        }
        let entered = self.user.text();
        let Some(identity) = self
            .identities
            .iter()
            .find(|identity| identity.is_named(&entered))
            .cloned()
        else {
            self.failed();
            return;
        };
        let password = Secret(self.password.text().as_bytes().to_vec());
        self.password.set_text("");
        self.info.set_visible(false);
        self.set_busy(true);

        let weak = Rc::downgrade(self);
        let cookie = self.cookie.clone();
        let task = glib::spawn_future_local(async move {
            let opened = open_helper(&identity.login, &cookie).await;
            let Some(dialog) = weak.upgrade() else {
                return;
            };
            let input = match opened {
                Ok((output, input, helper)) => {
                    if let Some(session) = dialog.session.borrow().as_ref() {
                        session.output.replace(Some(output));
                        session.helper.replace(Some(helper));
                    }
                    input
                }
                Err(err) => {
                    eprintln!("bunt-shell: could not start the polkit helper: {err}");
                    dialog.on_event(Event::Done(false));
                    return;
                }
            };
            drop(dialog);
            loop {
                let line = input.read_line_utf8_future(glib::Priority::DEFAULT).await;
                let Some(dialog) = weak.upgrade() else {
                    return;
                };
                let event = match line {
                    Ok(Some(line)) => Event::parse(&line),
                    // 끝(EOF)이나 오류
                    _ => Event::Done(false),
                };
                let done = matches!(event, Event::Done(_));
                dialog.on_event(event);
                if done {
                    return;
                }
            }
        });
        self.session.replace(Some(Session {
            task,
            output: RefCell::default(),
            helper: RefCell::default(),
            answer: RefCell::new(Some(password)),
            asking: Cell::new(false),
        }));
    }

    fn on_event(self: &Rc<Self>, event: Event) {
        match event {
            Event::Prompt { echo, text } => {
                let session = self.session.borrow();
                let Some(session) = session.as_ref() else {
                    return;
                };
                if let Some(answer) = session.answer.take() {
                    session.write(answer.0.clone());
                    return;
                }
                // PAM이 더 묻는다 (일회용 코드, 새 암호 등): 그 물음을 칸에 보이고 답을 기다린다
                session.asking.set(true);
                let text = text.trim().trim_end_matches(':').trim().to_owned();
                self.echo.set(echo);
                self.answer.set_visible(echo);
                if echo {
                    self.answer.set_placeholder_text(Some(&text));
                } else {
                    self.password.set_placeholder_text(Some(&text));
                }
                self.set_busy(false);
                self.user.set_sensitive(false);
                if echo {
                    self.answer.grab_focus();
                } else {
                    self.password.grab_focus();
                }
            }
            Event::Error(text) => self.show_info(&text, true),
            Event::Info(text) => self.show_info(&text, false),
            Event::Done(true) => self.finish(true),
            Event::Done(false) => self.failed(),
        }
    }

    fn show_info(&self, text: &str, error: bool) {
        self.info.set_text(text.trim());
        self.info.set_visible(!text.trim().is_empty());
        if error {
            self.info.add_css_class("error");
        } else {
            self.info.remove_css_class("error");
        }
    }

    /// 틀렸다: 창을 흔들고 암호를 다시 묻는다
    fn failed(&self) {
        if let Some(session) = self.session.take() {
            session.close();
        }
        self.echo.set(false);
        self.answer.set_visible(false);
        self.answer.set_text("");
        self.password
            .set_placeholder_text(Some(&self.tr.get("auth-password")));
        self.password.set_text("");
        self.set_busy(false);
        self.user.set_sensitive(true);
        let shake = self.shakes.get();
        self.shakes.set(shake.wrapping_add(1));
        let (old, new) = if shake.is_multiple_of(2) {
            ("shake-b", "shake-a")
        } else {
            ("shake-a", "shake-b")
        };
        self.panel.remove_css_class(old);
        self.panel.add_css_class(new);
        self.password.grab_focus();
    }

    fn set_busy(&self, busy: bool) {
        self.user.set_sensitive(!busy);
        self.password.set_sensitive(!busy);
        self.answer.set_sensitive(!busy);
        self.confirm.set_sensitive(!busy);
    }

    /// 성공했거나 취소했다: polkitd에 답하고 창을 닫는다
    fn finish(&self, authenticated: bool) {
        if let Some(session) = self.session.take() {
            session.close();
        }
        let Some(invocation) = self.invocation.take() else {
            return;
        };
        if authenticated {
            invocation.return_value(None);
        } else {
            invocation.return_dbus_error(ERROR_CANCELLED, "Authentication was cancelled");
        }
        self.window.destroy();
        if let Some(agent) = self.agent.upgrade() {
            // 이 창을 닫는 처리기 안에서 다음 창을 만들지 않게 미룬다
            glib::idle_add_local_once(move || agent.closed());
        }
    }
}

/// 요청한 앱의 아이콘에 작은 자물쇠를 붙인다. 앱을 모르면 polkit 동작의 아이콘이나 자물쇠를 그린다
pub(crate) fn requester_icon(
    app: Option<&(String, Option<gio::Icon>)>,
    icon_name: &str,
) -> gtk::Widget {
    let overlay = gtk::Overlay::builder()
        .halign(gtk::Align::Center)
        .css_classes(["auth-icon"])
        .build();
    if let Some(icon) = app.and_then(|(_, icon)| icon.as_ref()) {
        let image = gtk::Image::from_gicon(icon);
        image.set_pixel_size(64);
        overlay.set_child(Some(&image));
        let lock = gtk::Image::from_icon_name(LOCK_ICON);
        lock.set_pixel_size(12);
        lock.set_hexpand(true);
        let badge = gtk::Box::builder()
            .width_request(22)
            .height_request(22)
            .halign(gtk::Align::End)
            .valign(gtk::Align::End)
            .css_classes(["auth-lock-badge"])
            .build();
        badge.append(&lock);
        overlay.add_overlay(&badge);
        return overlay.upcast();
    }
    let theme = gdk::Display::default().map(|display| gtk::IconTheme::for_display(&display));
    let name = if !icon_name.is_empty() && theme.is_some_and(|theme| theme.has_icon(icon_name)) {
        icon_name
    } else {
        LOCK_ICON
    };
    let image = gtk::Image::from_icon_name(name);
    image.set_pixel_size(34);
    image.set_hexpand(true);
    image.set_vexpand(true);
    let tile = gtk::Box::builder()
        .width_request(64)
        .height_request(64)
        .css_classes(["auth-lock-tile"])
        .build();
    tile.append(&image);
    overlay.set_child(Some(&tile));
    overlay.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_helper_lines() {
        assert_eq!(
            Event::parse("PAM_PROMPT_ECHO_OFF Password: "),
            Event::Prompt {
                echo: false,
                text: "Password: ".into()
            }
        );
        // 한글은 8진수로 이스케이프돼 온다 ("암호:")
        assert_eq!(
            Event::parse(r"PAM_PROMPT_ECHO_OFF \354\225\224\355\230\270:"),
            Event::Prompt {
                echo: false,
                text: "암호:".into()
            }
        );
        assert_eq!(
            Event::parse(r#"PAM_ERROR_MSG Account \"x\"\nlocked"#),
            Event::Error("Account \"x\"\nlocked".into())
        );
        assert_eq!(Event::parse("PAM_TEXT_INFO hi"), Event::Info("hi".into()));
        assert_eq!(Event::parse("SUCCESS"), Event::Done(true));
        assert_eq!(Event::parse("FAILURE"), Event::Done(false));
        assert_eq!(Event::parse("garbage"), Event::Done(false));
    }

    #[test]
    fn replaces_known_action_messages() {
        let ko = Translations::from_source("ko", crate::TRANSLATIONS[0].1);
        let en = Translations::from_source("en", crate::TRANSLATIONS[1].1);
        let details = HashMap::from([("program".to_owned(), "/usr/bin/true".to_owned())]);
        let english = "Authentication is required to set the system timezone.";
        assert_eq!(
            action_message(
                &ko,
                "org.freedesktop.timedate1.set-timezone",
                english,
                &details,
                None
            ),
            "시간대를 바꾸려고 합니다."
        );
        assert_eq!(
            action_message(
                &en,
                "org.freedesktop.policykit.exec",
                "x",
                &details,
                Some("Terminal")
            ),
            "Terminal wants to run /usr/bin/true as an administrator."
        );
        assert_eq!(
            action_message(
                &en,
                "org.freedesktop.timedate1.set-time",
                english,
                &details,
                None
            ),
            "A program wants to change the system time."
        );
        // 모르는 동작은 polkit의 설명 그대로
        assert_eq!(
            action_message(&ko, "org.example.unknown", english, &details, None),
            english
        );
        // polkitd가 실제로 넘기는 세부에는 program이 없다. 설명에서 읽는다
        let real = HashMap::from([
            ("polkit.caller-pid".to_owned(), "0".to_owned()),
            ("polkit.subject-pid".to_owned(), "0".to_owned()),
        ]);
        let pkexec = "Authentication is needed to run `/usr/bin/true' as the super user";
        assert_eq!(
            action_message(&ko, "org.freedesktop.policykit.exec", pkexec, &real, None),
            "/usr/bin/true 프로그램을 관리자 권한으로 실행하려고 합니다."
        );
        // 프로그램을 알 수 없으면 {$program}을 그대로 보이지 않고 polkit의 설명을 쓴다
        assert_eq!(
            action_message(&ko, "org.freedesktop.policykit.exec", "x", &real, None),
            "x"
        );
    }

    #[test]
    fn unescapes_like_g_strcompress() {
        assert_eq!(unescape(r"a\tb\\c\101\7"), "a\tb\\cA\u{7}");
        assert_eq!(unescape("trailing\\"), "trailing");
    }

    #[test]
    fn reads_passwd_lines() {
        let identity =
            parse_passwd_line("buntos:x:1000:1000:홍길동,,,:/home/buntos:/bin/bash").unwrap();
        assert_eq!(identity.uid, 1000);
        assert_eq!(identity.login, "buntos");
        assert_eq!(identity.display, "홍길동");
        assert!(identity.is_named("BUNTOS"));
        assert!(identity.is_named(" 홍길동 "));
        assert!(!identity.is_named("root"));
        let nameless = parse_passwd_line("root:x:0:0::/root:/bin/bash").unwrap();
        assert_eq!(nameless.display, "root");
        assert!(parse_passwd_line("broken").is_none());
    }
}
