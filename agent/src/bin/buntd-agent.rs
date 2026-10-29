// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! buntd-agent: buntOS 에이전트 데몬 (ARCHITECTURE 6.3). 사용자 서비스로 돌며 접근성 버스를 읽어
//! 에이전트에게 앱의 스냅샷을 준다. 소켓(`$XDG_RUNTIME_DIR/buntos/agent.sock`)은 systemd가 열어
//! 넘기고(buntd-agent.socket), 직접 띄우면 스스로 연다. 요청은 한 번에 하나씩 처리한다.
//!
//! 접근 정책(bunt-access, ARCHITECTURE 6.5): 연결마다 건너편 프로그램을 알아보고 시스템 설정에서
//! 허용한 AI의 요청만 받는다. 사용자가 막은 앱은 목록에서 빼고 읽거나 조작하지 않는다.
//!
//! 트리에 나오지 않는 UI(캔버스, 직접 그리는 화면)는 창 스크린샷과 그 픽셀 좌표의 포인터 누르기로
//! 다룬다. 컴포지터가 그 창만 따로 그리므로 다른 앱과 셸의 인증 창은 그림에 들지 않는다.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        io::{AsRawFd, FromRawFd},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bunt_access::{Client, Policy};
use bunt_agent::{
    atspi::{self, A11y, AUTH_WINDOW, AppRef, Focus, Placed, ROOT, WindowRef},
    desktop::{self, DesktopApp},
    diff,
    protocol::{
        self, ActParams, Acted, Installed, LaunchParams, Launched, Request, Response, RpcError,
        Screenshot, ScreenshotParams, Snapshot, SnapshotParams, Untracked,
    },
    session::Sessions,
    tree::{self, Item, Options, Tree, iface, role, state},
};
use futures_util::future::{Either, select};
use serde_json::{Value, json};

/// (JSON-RPC 오류 번호, 알림)
type Failure = (i64, String);

/// 동작 뒤 화면이 멈췄는지 다시 읽는 간격. 바뀐 뒤 이만큼 그대로면 멈춘 것이다
const SETTLE_STEP: Duration = Duration::from_millis(50);
const SETTLE_QUIET: Duration = Duration::from_millis(150);
/// 첫 변화를 기다리는 시간. 계산기처럼 결과를 늦게 그리는 앱이 있고, 가려진 창은 컴포지터가 프레임을
/// 드물게 줘서 GTK가 접근성 정보도 늦게 고친다
const SETTLE_FIRST: Duration = Duration::from_secs(1);
/// 기다리는 시간의 기본 상한. 접속자 수처럼 늘 바뀌는 화면은 이만큼 기다리게 되므로 동작마다 줄일 수 있다
const SETTLE_MAX: Duration = Duration::from_secs(3);
/// 동작마다 고르는 상한의 최댓값. 요청은 하나씩 처리하므로 한 동작이 데몬을 오래 붙잡지 않게 한다
const SETTLE_CAP: Duration = Duration::from_secs(30);
/// 트리 전체(숨은 요소 빼고)를 읽는 범위: 동작 전후의 diff에 쓴다
const FULL: Options = Options {
    interactive: false,
    hidden: false,
};
/// 누르기에 쓰는 동작 이름 (GTK: 단추 click, 스위치 toggle, 글자 칸 activate)
const CLICK_ACTIONS: [&str; 6] = ["click", "press", "toggle", "activate", "jump", "open"];
/// 연 앱의 창이 뜨기를 기다리는 시간과 다시 보는 간격
const LAUNCH_WAIT: Duration = Duration::from_secs(20);
const LAUNCH_STEP: Duration = Duration::from_millis(250);
/// 연 앱이 이만큼 지나도록 창을 띄우지 못했으면 다시 열 때 새로 띄운다
const RELAUNCH_AFTER: Duration = Duration::from_secs(60);
/// 연 앱의 창이 컴포지터에 뜬 뒤 접근성 버스에도 보이기를 기다리는 시간. 지나면 트리 없이 알린다
const LAUNCH_TREE_WAIT: Duration = Duration::from_secs(4);
/// 포커스를 받은 글 칸이 컴포지터에 글 입력(text-input)을 켜기를 기다리는 시간과 다시 보는 간격.
/// 포털을 기다리느라 바쁜 앱은 늦게 켠다
const TEXT_INPUT_WAIT: Duration = Duration::from_secs(1);
const TEXT_INPUT_STEP: Duration = Duration::from_millis(20);
/// 화면 밖 요소를 휠로 보이게 할 때 돌리는 횟수의 상한과 그 사이의 쉼
const SCROLL_TURNS: usize = 30;
const SCROLL_PAUSE: Duration = Duration::from_millis(120);
/// 한 번에 돌리는 눈금 수의 상한과 눈금 하나로 어림하는 거리(논리 픽셀). GTK는 눈금 하나에 페이지
/// 크기에 따라 수십 픽셀을 움직이므로 넉넉히 어림해 지나치지 않게 한다
const SCROLL_NOTCHES: f64 = 5.0;
const SCROLL_PIXELS: f64 = 60.0;
/// 스크롤한 뒤 요소가 보이기(SHOWING)를 기다리는 시간
const SHOWN_WAIT: Duration = Duration::from_secs(1);
/// 스크린샷 폴더($XDG_RUNTIME_DIR, 메모리)에 남기는 파일 수
const SCREENSHOTS_KEPT: usize = 20;
/// 세션을 열 때 접근성을 켜며 버스의 답을 기다리는 시간. 로그인 중의 VM에서는 접근성 버스가 뜨는 데
/// 5초 넘게 걸렸다. 컴포지터는 기다리지 않는다
const PREPARE_WAIT: Duration = Duration::from_secs(30);

/// 동작의 대상: 앱(접근성 버스 이름), 창, 요소와 그 부모. press는 요소 없이 창에만 보낼 수 있다
struct Target {
    bus: String,
    window: String,
    window_title: String,
    item: Option<Item>,
    parent: Option<Item>,
    /// 요소가 웹 페이지에서 스크롤해 가려졌다 (동작 전에 보이게 스크롤한다)
    off_screen: bool,
    /// 요소가 웹 페이지 안에 있다 (Firefox는 그 동작과 자리를 늦게 알린다)
    web: bool,
}

/// ref 없는 동작(press, type, scroll)을 받을 창
enum Chosen {
    /// 접근성 트리가 있는 창
    Tree(Box<Target>),
    /// 트리가 없는 창 (pid, 제목). 키와 휠은 보내고 diff는 없다
    Blind(u32, String),
}

/// 그린 범위: 머리줄을 포함한 트리 글과 그 앱의 보이는 창 (경로, 제목)
struct Rendered {
    text: String,
    lines: usize,
    items: usize,
    windows: Vec<(String, String)>,
    /// 범위 안의 문서가 불러오는 중이다 (BUSY). 멈췄다고 보지 않는다
    busy: bool,
}

#[derive(Default)]
struct Agent {
    a11y: Option<A11y>,
    sessions: Sessions,
    /// 지금 요청에 쓰는 접근 정책. 설정에서 바꾼 것이 바로 듣도록 요청마다 다시 읽는다
    policy: Policy,
    /// 열었지만 아직 창이 뜨지 않은 앱 (데스크톱 ID, 연 시각)
    launching: HashMap<String, Instant>,
}

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    if std::env::args().nth(1).as_deref() == Some("--prepare-session") {
        prepare_session();
        return;
    }
    let listener = match listener() {
        Ok(listener) => listener,
        Err(err) => {
            tracing::error!("buntd-agent: cannot open the socket: {err}");
            std::process::exit(1);
        }
    };
    let agent = Arc::new(Mutex::new(Agent::default()));
    loop {
        // 받기 전에 본다: 끝나면 기다리는 연결은 소켓에 남고 systemd가 새 데몬을 띄워 넘긴다
        wait_for_connection(&listener);
        if replaced() {
            tracing::info!("buntd-agent was updated; exiting so the new one serves the socket");
            std::process::exit(0);
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let agent = agent.clone();
                std::thread::spawn(move || serve(stream, &agent));
            }
            Err(err) => tracing::warn!("accept failed: {err}"),
        }
    }
}

/// `buntd-agent --prepare-session`: 사용자가 허용한 AI가 있으면 접근성을 켜고 끝난다. 컴포지터가 세션을
/// 열 때 실행한다(comp.toml의 session.startup). Chromium·Electron 앱은 뜰 때만 접근성이 켜졌는지 보므로,
/// 데몬이 첫 요청 때 켜면 그 전에 뜬 앱은 다시 띄울 때까지 트리를 내놓지 않는다. 데몬 자체를 로그인 때
/// 띄우지 않는 것은 그때는 아직 세션 환경(WAYLAND_DISPLAY, LANG)이 없어 데몬이 연 앱이 화면에 붙지
/// 못하기 때문이다
fn prepare_session() {
    if !Policy::load().clients.iter().any(|client| client.enabled) {
        return;
    }
    let enable = async {
        let session = zbus::Connection::session().await?;
        atspi::enable_accessibility(&session).await
    };
    // 버스가 끝내 답하지 않아도 남아 있지 않는다
    let timeout = async_io::Timer::after(PREPARE_WAIT);
    match async_io::block_on(select(std::pin::pin!(enable), timeout)) {
        Either::Left((Ok(()), _)) => {
            tracing::info!("turned on accessibility for the apps of this session")
        }
        Either::Left((Err(err), _)) => tracing::warn!("cannot turn on accessibility: {err}"),
        Either::Right(_) => tracing::warn!("cannot turn on accessibility: the bus did not answer"),
    }
}

/// 듣는 소켓에 연결이 올 때까지 기다린다
fn wait_for_connection(listener: &UnixListener) {
    let mut fd = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // 안전: pollfd 하나를 넘긴다. 신호로 깨면 accept가 그냥 다시 기다린다
    unsafe { libc::poll(&raw mut fd, 1, -1) };
}

/// 도는 동안 실행 파일이 바뀌었는지 (꾸러미 갱신). 컴포지터는 지금 그 경로의 파일인 프로세스만
/// 에이전트로 받으므로 옛 데몬은 더 쓸모가 없다
fn replaced() -> bool {
    fs::read_link("/proc/self/exe").is_ok_and(|exe| exe.to_string_lossy().ends_with(" (deleted)"))
}

/// systemd가 넘긴 소켓(fd 3)이 있으면 그것을 쓰고, 없으면 새로 연다
fn listener() -> io::Result<UnixListener> {
    let ours = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|pid| pid.parse::<u32>().ok())
        == Some(std::process::id());
    if ours && std::env::var("LISTEN_FDS").as_deref() == Ok("1") {
        // 안전: systemd가 이 프로세스에 넘긴 듣는 소켓이고 다른 곳에서 쓰지 않는다
        return Ok(unsafe { UnixListener::from_raw_fd(3) });
    }
    let path =
        protocol::socket_path().ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// 연결 하나: 한 줄 요청마다 한 줄로 답한다. 요청한 프로그램은 연결마다 한 번 알아본다
fn serve(stream: UnixStream, agent: &Mutex<Agent>) {
    let client = peer_pid(&stream).map(bunt_access::identify);
    let mut writer = &stream;
    for line in BufReader::new(&stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(mut reply) = serde_json::to_string(&handle(&line, agent, client.as_ref())) else {
            return;
        };
        reply.push('\n');
        if writer.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

/// 소켓 건너편 프로세스의 pid (SO_PEERCRED)
fn peer_pid(stream: &UnixStream) -> Option<u32> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // 안전: 커널이 크기를 알려 준 ucred 하나에 쓴다
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &mut length,
        )
    };
    (result == 0 && credentials.pid > 0).then_some(credentials.pid as u32)
}

/// 이 컴퓨터를 제어하도록 사용자가 허용한 클라이언트인지. 허용하지 않은 클라이언트는 설정 페이지가
/// 더할 AI로 보여 주도록 요청 기록에 남긴다
fn admit(client: Option<&Client>, policy: &Policy) -> Result<(), Failure> {
    let Some(client) = client.filter(|client| client.admin || !client.is_unknown()) else {
        return Err((
            protocol::NOT_ALLOWED,
            "cannot tell which program sent this request".into(),
        ));
    };
    if policy.allows(client) {
        return Ok(());
    }
    tracing::warn!(
        client = %client.name,
        exe = %client.exe.display(),
        "refused a client the user has not allowed"
    );
    let script = client.script.as_deref();
    let refusal = if policy.find(&client.exe, script).is_some() {
        Refusal::TurnedOff
    } else {
        // 처음 거절한 AI는 셸의 AI 접근 요청 창이 사용자에게 묻는다 (shell/src/agent_access.rs)
        let asked = bunt_access::was_asked(&client.exe, script);
        bunt_access::record_request(client);
        if asked {
            Refusal::Asked
        } else {
            Refusal::Asking
        }
    };
    Err((protocol::NOT_ALLOWED, refusal.message(&client.name)))
}

/// 허용하지 않은 AI에게 알리는 것. AI가 사용자에게 무엇을 해 달라고 할지 알 수 있게 쓴다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// 목록에 있지만 사용자가 꺼 두었다
    TurnedOff,
    /// 셸이 지금 사용자에게 묻는다
    Asking,
    /// 사용자가 이미 물음을 받았다
    Asked,
}

impl Refusal {
    fn message(self, name: &str) -> String {
        match self {
            Self::TurnedOff => format!(
                "{name} is turned off in System Settings > AI Agents; only the user can turn it \
                 back on"
            ),
            Self::Asking => format!(
                "{name} is not allowed to control this computer. buntOS is asking the user now: \
                 ask them to choose \"Open System Settings\" in the alert and then \"Add\" \
                 (System Settings > AI Agents)"
            ),
            Self::Asked => format!(
                "{name} is not allowed to control this computer. The user was already asked; \
                 they can allow it in System Settings > AI Agents"
            ),
        }
    }
}

fn auth_dialog() -> Failure {
    (
        protocol::NOT_ALLOWED,
        "an authentication dialog is open; only the user can answer it".into(),
    )
}

fn blocked_app() -> Failure {
    (
        protocol::BLOCKED_APP,
        "the user has blocked AI agents from this app (System Settings > AI Agents)".into(),
    )
}

fn handle(line: &str, agent: &Mutex<Agent>, client: Option<&Client>) -> Response {
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(err) => {
            return reply(
                Value::Null,
                Err((
                    protocol::PARSE_ERROR,
                    format!("not a JSON-RPC request: {err}"),
                )),
            );
        }
    };
    let policy = Policy::load();
    if let Err(failure) = admit(client, &policy) {
        return reply(request.id, Err(failure));
    }
    let started = Instant::now();
    let mut agent = agent
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    agent.policy = policy;
    let result = async_io::block_on(agent.dispatch(&request.method, request.params));
    tracing::info!(
        client = client.map_or("", |client| client.name.as_str()),
        method = %request.method,
        ms = started.elapsed().as_millis() as u64,
        ok = result.is_ok(),
        "request"
    );
    reply(request.id, result)
}

fn reply(id: Value, result: Result<Value, Failure>) -> Response {
    let (result, error) = match result {
        Ok(value) => (Some(value), None),
        Err((code, message)) => (None, Some(RpcError { code, message })),
    };
    Response {
        jsonrpc: "2.0".into(),
        id,
        result,
        error,
    }
}

fn agent_error(message: impl Into<String>) -> Failure {
    (protocol::AGENT_ERROR, message.into())
}

impl Agent {
    async fn dispatch(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
        match method {
            "apps" => self.apps().await,
            "snapshot" => {
                let params: SnapshotParams = serde_json::from_value(params)
                    .map_err(|err| (protocol::INVALID_PARAMS, err.to_string()))?;
                let snapshot = self.snapshot(params).await?;
                serde_json::to_value(snapshot).map_err(|err| agent_error(err.to_string()))
            }
            "installed" => {
                let apps: Vec<Installed> = desktop::installed(&desktop::language())
                    .into_iter()
                    .filter(|app| !self.policy.blocks(&app.id))
                    .map(|app| Installed {
                        id: app.id,
                        name: app.name,
                    })
                    .collect();
                Ok(json!({ "apps": apps }))
            }
            "launch" => {
                let params: LaunchParams = serde_json::from_value(params)
                    .map_err(|err| (protocol::INVALID_PARAMS, err.to_string()))?;
                let launched = self.launch(params).await?;
                serde_json::to_value(launched).map_err(|err| agent_error(err.to_string()))
            }
            "act" => {
                let params: ActParams = serde_json::from_value(params)
                    .map_err(|err| (protocol::INVALID_PARAMS, err.to_string()))?;
                let acted = self.act(params).await?;
                serde_json::to_value(acted).map_err(|err| agent_error(err.to_string()))
            }
            "screenshot" => {
                let params: ScreenshotParams = serde_json::from_value(params)
                    .map_err(|err| (protocol::INVALID_PARAMS, err.to_string()))?;
                let shot = self.screenshot(params).await?;
                serde_json::to_value(shot).map_err(|err| agent_error(err.to_string()))
            }
            _ => Err((
                protocol::METHOD_NOT_FOUND,
                format!("unknown method {method}"),
            )),
        }
    }

    /// 접근성 버스. 처음 쓸 때 붙고, 호출이 실패하면 다음 요청에서 다시 붙는다
    async fn a11y(&mut self) -> Result<A11y, Failure> {
        if let Some(a11y) = &self.a11y {
            return Ok(a11y.clone());
        }
        let a11y = A11y::connect()
            .await
            .map_err(|err| agent_error(format!("cannot reach the accessibility bus: {err}")))?;
        self.a11y = Some(a11y.clone());
        Ok(a11y)
    }

    /// 앱 목록. 관리자 인증 창은 빼서 스냅샷 범위에도 들지 않는다
    async fn list(&mut self, a11y: &A11y) -> Result<Vec<AppRef>, Failure> {
        let mut apps = a11y.apps().await.map_err(|err| {
            self.a11y = None;
            agent_error(format!("cannot list apps: {err}"))
        })?;
        for app in &mut apps {
            app.windows.retain(|window| window.title != AUTH_WINDOW);
        }
        Ok(apps)
    }

    /// 사용자가 막은 앱의 창을 가진 pid. 막은 앱은 컴포지터 창의 app_id(= 데스크톱 ID)로 안다
    async fn blocked_pids(&self, a11y: &A11y) -> Result<HashSet<u32>, Failure> {
        if self.policy.blocked_apps.is_empty() {
            return Ok(HashSet::new());
        }
        let windows = a11y
            .windows()
            .await
            .map_err(|err| agent_error(format!("cannot check blocked apps: {err}")))?;
        Ok(windows
            .iter()
            .filter(|window| self.policy.blocks(&window.app_id))
            .map(|window| window.pid)
            .collect())
    }

    /// 사용자가 막은 앱이면 오류. 앱의 pid를 모르면 막은 앱이 아니라고 할 수 없다
    async fn check_app(&self, a11y: &A11y, bus: &str) -> Result<(), Failure> {
        let blocked = self.blocked_pids(a11y).await?;
        if blocked.is_empty() {
            return Ok(());
        }
        match a11y.pid(bus).await {
            Some(pid) if !blocked.contains(&pid) => Ok(()),
            _ => Err(blocked_app()),
        }
    }

    async fn apps(&mut self) -> Result<Value, Failure> {
        let a11y = self.a11y().await?;
        let blocked = self.blocked_pids(&a11y).await?;
        let apps: Vec<AppRef> = self
            .list(&a11y)
            .await?
            .into_iter()
            .filter(|app| blocked.is_empty() || app.pid.is_some_and(|pid| !blocked.contains(&pid)))
            .collect();
        let focus = a11y.focus().await;
        let active = active_window(&apps, focus.as_ref()).map(|(_, window)| window.path.clone());
        let placed = a11y.windows().await.unwrap_or_else(|err| {
            tracing::warn!("cannot read the window list: {err}");
            Vec::new()
        });
        let untracked: Vec<Untracked> =
            untracked(&apps, &placed, &desktop::installed(&desktop::language()))
                .into_iter()
                .filter(|window| !blocked.contains(&window.pid))
                .collect();
        let apps: Vec<protocol::App> = apps
            .into_iter()
            .map(|app| protocol::App {
                windows: app
                    .windows
                    .into_iter()
                    .map(|window| protocol::Window {
                        active: active.as_deref() == Some(window.path.as_str()),
                        title: window.title,
                    })
                    .collect(),
                name: app.name,
                bus: app.bus,
                pid: app.pid,
            })
            .collect();
        Ok(json!({ "apps": apps, "untracked": untracked }))
    }

    /// 키보드 포커스를 가진 창을 접근성 버스에서 찾지 못했을 때의 오류: 어느 앱인지와 다른 길을 알린다
    async fn no_active_window(&self, a11y: &A11y, focus: Option<&Focus>) -> Failure {
        let Some(focus) = focus.filter(|focus| !focus.layer) else {
            return agent_error(
                "no window has the keyboard focus; pass --app (see `bunt agent apps`)",
            );
        };
        match self.blocked_pids(a11y).await {
            Ok(blocked) if blocked.contains(&focus.pid) => blocked_app(),
            Ok(_) => agent_error(no_tree(
                &format!("the active window {:?}", focus.title),
                &focus.app_id,
                focus.pid,
            )),
            Err(failure) => failure,
        }
    }

    async fn snapshot(&mut self, params: SnapshotParams) -> Result<Snapshot, Failure> {
        let a11y = self.a11y().await?;
        let session_name = params.session.clone().unwrap_or_else(|| "default".into());
        // 범위: ref, 앱, 활성 창 차례
        let (bus, roots, scope) = if let Some(reference) = &params.scope {
            let (bus, path) = self
                .sessions
                .get(&session_name)
                .target(reference)
                .map(|(bus, path)| (bus.to_string(), path.to_string()))
                .ok_or_else(|| {
                    (
                        protocol::INVALID_PARAMS,
                        format!("unknown ref {reference} in session {session_name:?}; take a new snapshot"),
                    )
                })?;
            let scope = format!("ref {bus} {path}");
            (bus, vec![path], scope)
        } else {
            let apps = self.list(&a11y).await?;
            match &params.app {
                Some(query) => {
                    let placed = a11y.windows().await.unwrap_or_else(|err| {
                        tracing::warn!("cannot read the window list: {err}");
                        Vec::new()
                    });
                    let installed = desktop::installed(&desktop::language());
                    let app = match find_app(&apps, &placed, &installed, query) {
                        Ok(app) => app,
                        Err(failure) => {
                            return Err(self
                                .why_unmatched(&a11y, &apps, &placed, &installed, query)
                                .await
                                .unwrap_or(failure));
                        }
                    };
                    let roots = app
                        .windows
                        .iter()
                        .map(|window| window.path.clone())
                        .collect();
                    (app.bus.clone(), roots, format!("app {}", app.bus))
                }
                None => {
                    if a11y.auth_dialog_focused().await {
                        return Err(auth_dialog());
                    }
                    let focus = a11y.focus().await;
                    let Some((app, window)) = active_window(&apps, focus.as_ref()) else {
                        return Err(self.no_active_window(&a11y, focus.as_ref()).await);
                    };
                    let scope = format!("window {} {}", app.bus, window.path);
                    (app.bus.clone(), vec![window.path.clone()], scope)
                }
            }
        };

        self.check_app(&a11y, &bus).await?;

        let options = Options {
            interactive: params.interactive,
            hidden: params.hidden,
        };
        let rendered = self
            .render(&a11y, &session_name, &bus, &roots, options)
            .await?;
        let key = scope_key(&scope, options);
        let previous = self
            .sessions
            .get(&session_name)
            .remember(key, &rendered.text);
        let diff = previous
            .as_deref()
            .map(|old| diff::unified(old, &rendered.text))
            .unwrap_or_default();
        tracing::info!(
            items = rendered.items,
            lines = rendered.lines,
            diff = diff.lines().count(),
            "snapshot of {bus}"
        );
        Ok(Snapshot {
            tree: rendered.text,
            diff,
            first: previous.is_none(),
        })
    }

    /// `roots` 아래를 읽어 트리 글로 만든다 (앱 머리줄 포함). ref는 세션이 준다
    async fn render(
        &mut self,
        a11y: &A11y,
        session: &str,
        bus: &str,
        roots: &[String],
        options: Options,
    ) -> Result<Rendered, Failure> {
        let mut items = a11y.tree(bus).await.map_err(|err| {
            self.a11y = None;
            agent_error(format!("cannot read {bus}: {err}"))
        })?;
        let wanted = in_scope(&items, roots, options.hidden);
        let busy = wanted
            .iter()
            .any(|&at| items[at].busy() && items[at].shown());
        a11y.enrich(bus, &mut items, wanted).await;
        let pid = a11y.pid(bus).await;

        let tree = Tree::new(&items);
        let mut text = String::new();
        if options.interactive {
            text.push_str("# note: interactive elements only\n");
        }
        let app_name = tree.get(ROOT).map_or("", |app| app.name.as_str());
        text.push_str(&format!("- app: \"{app_name}\""));
        if let Some(pid) = pid {
            text.push_str(&format!(" [pid={pid}]"));
        }
        text.push('\n');
        let session = self.sessions.get(session);
        let root_paths: Vec<&str> = roots.iter().map(String::as_str).collect();
        let lines = tree::render(&tree, &root_paths, &options, &mut |path: &str| {
            session.ref_for(bus, path)
        });
        for line in &lines {
            text.push_str(line);
            text.push('\n');
        }
        let windows = tree
            .children(ROOT)
            .iter()
            .filter(|window| window.shown())
            .map(|window| (window.path.clone(), window.name.clone()))
            .collect();
        Ok(Rendered {
            text,
            lines: lines.len(),
            items: items.len(),
            windows,
            busy,
        })
    }

    /// 설치된 앱을 열고 새 창이 뜨기를 기다려 그 창의 상호작용 요소 트리를 돌려준다. 이미 창이 있는
    /// 앱은 새로 띄우지 않고 그 창을 앞으로 가져온다 (macOS의 `open -a`처럼)
    async fn launch(&mut self, params: LaunchParams) -> Result<Launched, Failure> {
        let a11y = self.a11y().await?;
        let session = params.session.clone().unwrap_or_else(|| "default".into());
        let apps = desktop::installed(&desktop::language());
        let app = desktop::find(&apps, &params.app)
            .map_err(|message| (protocol::INVALID_PARAMS, message))?
            .clone();
        if self.policy.blocks(&app.id) {
            return Err(blocked_app());
        }
        let placed = a11y.windows().await.unwrap_or_else(|err| {
            tracing::warn!("cannot read the window list: {err}");
            Vec::new()
        });
        // 쌓인 순서의 끝(맨 위)부터
        if let Some(open) = placed.iter().rev().find(|window| app.owns(&window.app_id)) {
            a11y.activate_window(open.pid, &open.title)
                .await
                .map_err(|err| agent_error(format!("cannot bring {} forward: {err}", app.name)))?;
            let running = self.list(&a11y).await?;
            let tree = match accessible_window(&running, open) {
                Some((bus, path)) => self.window_tree(&a11y, &session, &bus, &path).await?,
                None => String::new(),
            };
            return Ok(Launched {
                name: app.name,
                id: app.id,
                window: Some(open.title.clone()),
                running: true,
                tree,
            });
        }

        let started = Instant::now();
        // 막 연 앱이 아직 창을 띄우지 못했으면 다시 띄우지 않고 그 창을 기다린다. 첫 창이 늦은 앱(포털을
        // 기다리는 앱)을 다시 열면 둘이 떴다
        let pending = self
            .launching
            .get(&app.id)
            .is_some_and(|at| at.elapsed() < RELAUNCH_AFTER);
        // D-Bus로 뜨는 앱은 Activate의 답이 앱이 뜬 뒤에 온다. 답을 기다리는 동안에도 창을 살피고, 답이
        // 늦으면(포털을 기다리는 앱 등) 오류가 아니라 아직 창이 없는 것이다
        let mut activation = (app.dbus && !pending).then(|| {
            let a11y = a11y.clone();
            let id = app.id.clone();
            Box::pin(async move { a11y.activate_app(&id, LAUNCH_WAIT + LAUNCH_STEP).await })
        });
        if !app.dbus && !pending {
            desktop::launch(&app.path)
                .map_err(|err| agent_error(format!("cannot open {}: {err}", app.name)))?;
        }
        if !pending {
            self.launching.insert(app.id.clone(), Instant::now());
        }
        // 컴포지터에 창이 처음 보인 때. 그 뒤로도 접근성 버스에 창이 없으면 트리를 내놓지 않는 앱이다
        let mut appeared: Option<Instant> = None;
        while started.elapsed() < LAUNCH_WAIT {
            let tick = async_io::Timer::after(LAUNCH_STEP);
            match activation.as_mut() {
                Some(call) => match select(call.as_mut(), tick).await {
                    Either::Left((Ok(()), tick)) => {
                        activation = None;
                        tick.await;
                    }
                    Either::Left((Err(err), _)) => {
                        self.launching.remove(&app.id);
                        return Err(agent_error(format!("cannot open {}: {err}", app.name)));
                    }
                    Either::Right(_) => {}
                },
                None => {
                    tick.await;
                }
            }
            // 컴포지터에 이 앱의 창이 뜨고 접근성 버스에도 그 창이 보이면 끝난다
            let Ok(placed) = a11y.windows().await else {
                continue;
            };
            let Some(open) = placed.iter().rev().find(|window| app.owns(&window.app_id)) else {
                continue;
            };
            let running = self.list(&a11y).await?;
            let Some((bus, path)) = accessible_window(&running, open) else {
                // 터미널(foot)처럼 접근성을 지원하지 않는 앱은 창만 알린다. 창이 늦게 뜬 것이 아니다
                let since = *appeared.get_or_insert_with(Instant::now);
                if since.elapsed() >= LAUNCH_TREE_WAIT {
                    self.launching.remove(&app.id);
                    return Ok(Launched {
                        name: app.name,
                        id: app.id,
                        window: Some(open.title.clone()),
                        running: false,
                        tree: String::new(),
                    });
                }
                continue;
            };
            self.check_app(&a11y, &bus).await?;
            let tree = self.window_tree(&a11y, &session, &bus, &path).await?;
            self.launching.remove(&app.id);
            tracing::info!(
                ms = started.elapsed().as_millis() as u64,
                "launched {}",
                app.id
            );
            return Ok(Launched {
                name: app.name,
                id: app.id,
                window: Some(open.title.clone()),
                running: false,
                tree,
            });
        }
        Ok(Launched {
            name: app.name,
            id: app.id,
            window: None,
            running: false,
            tree: String::new(),
        })
    }

    /// 창 하나의 상호작용 요소 트리. 다음 스냅샷이 이것과 견준 diff를 주도록 세션에 남긴다
    async fn window_tree(
        &mut self,
        a11y: &A11y,
        session: &str,
        bus: &str,
        path: &str,
    ) -> Result<String, Failure> {
        let options = Options {
            interactive: true,
            hidden: false,
        };
        let roots = [path.to_string()];
        let rendered = self.render(a11y, session, bus, &roots, options).await?;
        let scope = format!("window {bus} {path}");
        self.sessions
            .get(session)
            .remember(scope_key(&scope, options), &rendered.text);
        Ok(rendered.text)
    }

    /// ref가 가리키는 요소와 그 창, 부모 (동작의 대상)
    async fn find_target(
        &mut self,
        a11y: &A11y,
        session: &str,
        reference: &str,
    ) -> Result<Target, Failure> {
        let (bus, path) = self
            .sessions
            .get(session)
            .target(reference)
            .map(|(bus, path)| (bus.to_string(), path.to_string()))
            .ok_or_else(|| {
                (
                    protocol::INVALID_PARAMS,
                    format!("unknown ref {reference} in session {session:?}; take a new snapshot"),
                )
            })?;
        let items = a11y.tree(&bus).await.map_err(|err| {
            self.a11y = None;
            agent_error(format!("cannot read {bus}: {err}"))
        })?;
        let tree = Tree::new(&items);
        let target = tree
            .get(&path)
            .ok_or_else(|| agent_error(format!("{reference} is gone; take a new snapshot")))?;
        // 웹 페이지에서 스크롤해 가려진 요소는 동작이 먼저 보이게 스크롤한다. 숨긴 요소(닫힌 팝오버, 다른
        // 탭 페이지)는 스크롤해도 보이지 않는다
        if !tree.visible(target) {
            return Err(agent_error(format!(
                "{reference} is hidden (in a closed popover, another tab page or the like)"
            )));
        }
        if target.states & (state::SENSITIVE | state::ENABLED) == 0 {
            return Err(agent_error(format!("{reference} is disabled")));
        }
        let window = window_of(&tree, &path)
            .ok_or_else(|| agent_error(format!("{reference} is not inside a window")))?;
        let window_title = tree
            .get(&window)
            .map_or(String::new(), |item| item.name.clone());
        if window_title == AUTH_WINDOW {
            return Err(auth_dialog());
        }
        Ok(Target {
            bus,
            window,
            window_title,
            parent: tree.get(&target.parent).cloned(),
            item: Some(target.clone()),
            off_screen: tree.off_screen(target),
            web: tree.in_web_page(target),
        })
    }

    /// `--app`이 접근성 버스의 앱에 맞지 않을 때: 컴포지터의 창에 맞으면 그 창이 트리를 내놓지 않는다고
    /// 알린다(사용자가 막은 앱이면 막았다고). 창에도 맞지 않거나 트리가 있는 앱이면 None
    async fn why_unmatched(
        &self,
        a11y: &A11y,
        apps: &[AppRef],
        placed: &[Placed],
        installed: &[DesktopApp],
        query: &str,
    ) -> Option<Failure> {
        let window = find_window(placed, apps, installed, query).ok()?;
        if apps.iter().any(|app| app.pid == Some(window.pid)) {
            return None;
        }
        Some(match self.blocked_pids(a11y).await {
            Ok(blocked) if blocked.contains(&window.pid) => blocked_app(),
            Ok(_) => agent_error(no_tree(
                &format!("the window {:?}", window.title),
                &window.app_id,
                window.pid,
            )),
            Err(failure) => failure,
        })
    }

    /// ref가 가리키는 요소에 동작을 하고, 화면이 멈출 때까지 기다려 그 창의 바뀐 줄을 돌려준다
    async fn act(&mut self, params: ActParams) -> Result<Acted, Failure> {
        let a11y = self.a11y().await?;
        if let Some([x, y]) = params.at {
            return self.click_at(&a11y, &params, x, y).await;
        }
        let session = params.session.clone().unwrap_or_else(|| "default".into());
        let found = if params.target.is_empty() {
            if !matches!(params.action.as_str(), "press" | "type" | "scroll") {
                let or = if params.action == "click" {
                    " or --at x,y"
                } else {
                    ""
                };
                return Err((
                    protocol::INVALID_PARAMS,
                    format!("{} needs a ref{or}", params.action),
                ));
            }
            if params.action == "scroll" && params.amount.is_none() {
                return Err((
                    protocol::INVALID_PARAMS,
                    "scroll needs a direction (up, down, left, right) or a ref".into(),
                ));
            }
            // 키와 휠은 `--app`의 창이나 지금 키보드 포커스를 가진 창으로 간다
            if a11y.auth_dialog_focused().await {
                return Err(auth_dialog());
            }
            match self.window_for(&a11y, params.app.as_deref()).await? {
                Chosen::Tree(target) => *target,
                Chosen::Blind(pid, title) => {
                    return self.act_blind(&a11y, &params, pid, &title).await;
                }
            }
        } else {
            let target = self.find_target(&a11y, &session, &params.target).await?;
            if let Some(query) = &params.app {
                self.check_ref_app(&a11y, &target.bus, query, &params.target)
                    .await?;
            }
            target
        };
        self.check_app(&a11y, &found.bus).await?;
        let Target {
            bus,
            window,
            window_title,
            item,
            parent,
            off_screen,
            web,
        } = found;

        let roots = [window.clone()];
        let before = self.render(&a11y, &session, &bus, &roots, FULL).await?;
        // 화면 밖 요소는 사람처럼 먼저 보이게 스크롤한다 (휠만 돌릴 scroll은 그 자리에서 돌린다)
        let wheel_only = params.action == "scroll" && params.amount.is_some();
        let scrolled = match item.as_ref().filter(|_| off_screen && !wheel_only) {
            Some(target) => Some(
                scroll_into_view(&a11y, &bus, &window_title, target)
                    .await
                    .map_err(|(code, message)| (code, format!("{}: {message}", params.target)))?,
            ),
            None => None,
        };
        let (done, expect) = match (params.action.as_str(), &item) {
            ("scroll", Some(_)) if !wheel_only => (
                scrolled
                    .clone()
                    .unwrap_or_else(|| "it is already on screen".into()),
                None,
            ),
            ("scroll", target) => {
                let pid = a11y
                    .pid(&bus)
                    .await
                    .ok_or_else(|| agent_error("cannot tell which process owns this app"))?;
                let over = target.as_ref().map(|target| (bus.as_str(), target));
                (
                    wheel(
                        &a11y,
                        pid,
                        &window_title,
                        over,
                        params.amount.unwrap_or_default(),
                    )
                    .await?,
                    None,
                )
            }
            ("press" | "type", _) => {
                press(&a11y, &bus, &window_title, item.as_ref(), &params).await?
            }
            ("click", Some(target)) if params.pointer => (
                pointer_click(&a11y, &bus, &window_title, target).await?,
                None,
            ),
            ("focus", Some(target)) => (
                focus_element(&a11y, &bus, &window_title, target).await?,
                None,
            ),
            (_, Some(target)) => {
                let done = match perform(&a11y, &bus, target, parent.as_ref(), web, &params).await?
                {
                    Some(done) => done,
                    // 접근성 동작이 없는 요소는 사람처럼 포인터로 누른다
                    None => format!(
                        "{}; it has no accessible action",
                        pointer_click(&a11y, &bus, &window_title, target).await?
                    ),
                };
                // 앱이 참이라고 답하고도 글을 버리는 일이 있어(Firefox 주소 막대) 멈춘 뒤 다시 읽어 본다
                let expect = (params.action == "fill")
                    .then(|| Expect::Text(params.value.clone().unwrap_or_default()));
                (done, expect)
            }
            (_, None) => unreachable!("only press, type and scroll work without a ref"),
        };
        let mut done = match scrolled.filter(|_| params.action != "scroll") {
            Some(scrolled) => format!("{scrolled}, then {done}"),
            None => done,
        };

        let settled = match self
            .settle(
                &a11y,
                &session,
                &bus,
                &roots,
                &before.text,
                settle_limit(params.settle_ms),
            )
            .await
        {
            Ok(settled) => settled,
            // 동작이 앱을 끝냈다 (닫기, 문서 버리기): 동작은 됐으므로 실패로 알리지 않는다. 실패로 보이면
            // AI가 같은 동작을 되풀이할 수 있다
            Err(_) if a11y.pid(&bus).await.is_none() => {
                return Ok(Acted {
                    done: format!("{done}; then the app quit (window {window_title:?} closed)"),
                    diff: String::new(),
                    settled_ms: 0,
                    settled: true,
                    observed: false,
                    new_windows: Vec::new(),
                });
            }
            Err(failure) => return Err(failure),
        };
        let acted = self.finish(&session, &bus, &window, &before, settled, done.clone());
        tracing::info!(
            action = %params.action,
            settled_ms = acted.settled_ms,
            settled = acted.settled,
            diff = acted.diff.lines().count(),
            "acted on {bus}"
        );
        if let (Some(expect), Some(target)) = (&expect, &item) {
            match a11y.text(&bus, &target.path).await {
                Some(now) if expect.holds(&now) => {}
                Some(now) => return Err(agent_error(expect.failure(&params.target, &now))),
                None => {
                    done.push_str("; could not read the field back to check it");
                    return Ok(Acted { done, ..acted });
                }
            }
        }
        Ok(acted)
    }

    /// 동작 뒤 그 창이 멈출 때까지 다시 읽는다: 바뀐 뒤 SETTLE_QUIET 동안 그대로면 멈춘 것이다. 바뀌지
    /// 않으면 SETTLE_FIRST까지 기다린다. `limit`이 지나면 멈추지 않았어도 그때의 트리를 돌려준다
    async fn settle(
        &mut self,
        a11y: &A11y,
        session: &str,
        bus: &str,
        roots: &[String],
        before: &str,
        limit: Duration,
    ) -> Result<Settled, Failure> {
        let started = Instant::now();
        let mut last = before.to_string();
        let mut changed: Option<Instant> = None;
        loop {
            async_io::Timer::after(SETTLE_STEP).await;
            let now = self.render(a11y, session, bus, roots, FULL).await?;
            // 불러오는 중인 문서(Firefox가 페이지를 받는 동안)는 바뀌고 있는 것으로 본다
            if now.text != last || now.busy {
                changed = Some(Instant::now());
            }
            let quiet = changed.map(|at| at.elapsed());
            if let Some(settled) = settle_state(quiet, started.elapsed(), limit) {
                return Ok(Settled {
                    after: now,
                    settled,
                    ms: started.elapsed().as_millis() as u64,
                });
            }
            last = now.text;
        }
    }

    /// 동작의 답: 동작 전과 멈춘 뒤의 diff, 새로 뜬 창. 다음 스냅샷의 diff는 이 뒤부터다
    fn finish(
        &mut self,
        session: &str,
        bus: &str,
        window: &str,
        before: &Rendered,
        settled: Settled,
        done: String,
    ) -> Acted {
        let after = settled.after;
        let new_windows = after
            .windows
            .iter()
            .filter(|(path, _)| !before.windows.iter().any(|(known, _)| known == path))
            .map(|(_, title)| title.clone())
            .collect();
        // 동작이 그 창을 닫았다 (앱은 남았다): 사라진 줄의 diff 대신 닫혔다고 알린다
        if !after.windows.iter().any(|(path, _)| path == window) {
            let title = before
                .windows
                .iter()
                .find(|(path, _)| path == window)
                .map_or("", |(_, title)| title.as_str());
            return Acted {
                done: format!("{done}; then its window {title:?} closed"),
                diff: String::new(),
                settled_ms: settled.ms,
                settled: settled.settled,
                observed: false,
                new_windows,
            };
        }
        let diff = diff::unified(&before.text, &after.text);
        let scope = format!("window {bus} {window}");
        self.sessions
            .get(session)
            .remember(scope_key(&scope, FULL), &after.text);
        Acted {
            done,
            diff,
            settled_ms: settled.ms,
            settled: settled.settled,
            observed: true,
            new_windows,
        }
    }

    /// ref 없는 동작(press, type, scroll)을 받을 창: `--app`의 창(트리가 있으면 그 앱의 활성 창이나 첫
    /// 창)이나 키보드 포커스를 가진 창. `--app`의 창은 키와 휠이 그리로 가게 앞으로 가져온다
    async fn window_for(&mut self, a11y: &A11y, app: Option<&str>) -> Result<Chosen, Failure> {
        let apps = self.list(a11y).await?;
        let Some(query) = app else {
            let focus = a11y.focus().await;
            if let Some((app, window)) = active_window(&apps, focus.as_ref()) {
                return Ok(Chosen::Tree(Box::new(Target {
                    bus: app.bus.clone(),
                    window: window.path.clone(),
                    window_title: window.title.clone(),
                    item: None,
                    parent: None,
                    off_screen: false,
                    web: false,
                })));
            }
            return match focus.filter(|focus| !focus.layer) {
                Some(focus) => Ok(Chosen::Blind(focus.pid, focus.title)),
                None => Err(agent_error(
                    "no window has the keyboard focus; pass --app or a ref",
                )),
            };
        };
        let placed = a11y.windows().await.unwrap_or_else(|err| {
            tracing::warn!("cannot read the window list: {err}");
            Vec::new()
        });
        let installed = desktop::installed(&desktop::language());
        if let Ok(app) = find_app(&apps, &placed, &installed, query)
            && let Some(window) = app
                .windows
                .iter()
                .find(|window| window.states & state::ACTIVE != 0)
                .or_else(|| app.windows.first())
        {
            if let Some(pid) = app.pid {
                activate(a11y, pid, &window.title).await?;
            }
            return Ok(Chosen::Tree(Box::new(Target {
                bus: app.bus.clone(),
                window: window.path.clone(),
                window_title: window.title.clone(),
                item: None,
                parent: None,
                off_screen: false,
                web: false,
            })));
        }
        // 트리가 없는 창 (접근성을 켜기 전에 뜬 Firefox, foot 등)
        let window = find_window(&placed, &apps, &installed, query)?;
        Ok(Chosen::Blind(window.pid, window.title.clone()))
    }

    /// ref가 `--app`이 가리키는 앱의 것인지. 아니면 어느 앱의 것인지 알린다
    async fn check_ref_app(
        &mut self,
        a11y: &A11y,
        bus: &str,
        query: &str,
        reference: &str,
    ) -> Result<(), Failure> {
        let apps = self.list(a11y).await?;
        let placed = a11y.windows().await.unwrap_or_default();
        let installed = desktop::installed(&desktop::language());
        let app = find_app(&apps, &placed, &installed, query)?;
        if app.bus == bus {
            return Ok(());
        }
        let owner = apps
            .iter()
            .find(|app| app.bus == bus)
            .map_or(bus, |app| app.name.as_str());
        Err((
            protocol::INVALID_PARAMS,
            format!(
                "{reference} is in {owner}, not in {}; drop --app (a ref already names its app)",
                app.name
            ),
        ))
    }

    /// 접근성 트리가 없는 창에 키·글·휠을 보낸다: 그 창을 앞으로 가져와 보내고, diff는 없다
    async fn act_blind(
        &mut self,
        a11y: &A11y,
        params: &ActParams,
        pid: u32,
        title: &str,
    ) -> Result<Acted, Failure> {
        if self.blocked_pids(a11y).await?.contains(&pid) {
            return Err(blocked_app());
        }
        activate(a11y, pid, title).await?;
        let done = if params.action == "scroll" {
            wheel(a11y, pid, title, None, params.amount.unwrap_or_default()).await?
        } else {
            press(a11y, "", title, None, params).await?.0
        };
        Ok(Acted {
            done: format!(
                "{done} in {title:?}, which has no accessibility tree (take a screenshot to see it)"
            ),
            diff: String::new(),
            settled_ms: 0,
            settled: true,
            observed: false,
            new_windows: Vec::new(),
        })
    }

    /// 창 안의 한 점을 포인터로 누른다 (ref가 없는 UI: 캔버스, 직접 그리는 화면). 점은 그 창 스크린샷의
    /// 픽셀이다. 창에 접근성 트리가 있으면 다른 동작처럼 멈출 때까지 기다려 diff를 준다
    async fn click_at(
        &mut self,
        a11y: &A11y,
        params: &ActParams,
        x: f64,
        y: f64,
    ) -> Result<Acted, Failure> {
        if params.action != "click" || !params.target.is_empty() {
            return Err((
                protocol::INVALID_PARAMS,
                "--at goes with click and no ref".into(),
            ));
        }
        if !(x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0) {
            return Err((
                protocol::INVALID_PARAMS,
                "--at needs two numbers of 0 or more".into(),
            ));
        }
        let session = params.session.clone().unwrap_or_else(|| "default".into());
        let window = self.target_window(a11y, params.app.as_deref()).await?;
        let running = self.list(a11y).await?;
        let tree = accessible_window(&running, &window);
        let before = match &tree {
            Some((bus, path)) => {
                self.check_app(a11y, bus).await?;
                let roots = [path.clone()];
                Some(self.render(a11y, &session, bus, &roots, FULL).await?)
            }
            None => None,
        };
        let (screen_x, screen_y) = a11y
            .click_window(window.pid, &window.title, x, y)
            .await
            .map_err(|err| agent_error(format!("the pointer click failed: {}", reason(&err))))?;
        let done = format!(
            "clicked with the pointer at ({x:.0}, {y:.0}) in {:?} (screen {screen_x:.0}, {screen_y:.0})",
            window.title
        );
        let (Some((bus, path)), Some(before)) = (tree, before) else {
            return Ok(Acted {
                done: format!("{done}; it has no accessibility tree (take a screenshot to see it)"),
                diff: String::new(),
                settled_ms: 0,
                settled: true,
                observed: false,
                new_windows: Vec::new(),
            });
        };
        let roots = [path.clone()];
        let settled = self
            .settle(
                a11y,
                &session,
                &bus,
                &roots,
                &before.text,
                settle_limit(params.settle_ms),
            )
            .await?;
        let acted = self.finish(&session, &bus, &path, &before, settled, done);
        tracing::info!(
            settled_ms = acted.settled_ms,
            diff = acted.diff.lines().count(),
            "clicked a point of {bus}"
        );
        Ok(acted)
    }

    /// 스크린샷과 click --at이 쓰는 컴포지터 창: `app`이 가리키는 앱의 맨 위 창, 없으면 키보드 포커스를
    /// 가진 창. 접근성 트리가 없는 창도 된다. 사용자가 막은 앱이면 오류
    async fn target_window(&mut self, a11y: &A11y, app: Option<&str>) -> Result<Placed, Failure> {
        let placed = a11y
            .windows()
            .await
            .map_err(|err| agent_error(format!("cannot read the window list: {err}")))?;
        let window = match app {
            Some(query) => {
                let apps = self.list(a11y).await?;
                let installed = desktop::installed(&desktop::language());
                find_window(&placed, &apps, &installed, query)?.clone()
            }
            None => {
                if a11y.auth_dialog_focused().await {
                    return Err(auth_dialog());
                }
                placed
                    .iter()
                    .rev()
                    .find(|window| window.focused)
                    .cloned()
                    .ok_or_else(|| {
                        agent_error(
                            "no window has the keyboard focus; pass --app (see `bunt agent apps`)",
                        )
                    })?
            }
        };
        if self.blocked_pids(a11y).await?.contains(&window.pid) {
            return Err(blocked_app());
        }
        Ok(window)
    }

    /// 창 하나의 스크린샷을 PNG 파일로 쓴다 (ref가 없는 UI를 볼 때). 컴포지터가 그 창만 따로 그리므로
    /// 가려진 부분도 보이고, 다른 앱(막은 앱 포함)과 셸의 인증 창은 들지 않는다
    async fn screenshot(&mut self, params: ScreenshotParams) -> Result<Screenshot, Failure> {
        let a11y = self.a11y().await?;
        let window = self.target_window(&a11y, params.app.as_deref()).await?;
        let path = match &params.out {
            Some(out) if Path::new(out).is_absolute() => PathBuf::from(out),
            Some(_) => {
                return Err((
                    protocol::INVALID_PARAMS,
                    "out must be an absolute path".into(),
                ));
            }
            None => new_screenshot_path()
                .map_err(|err| agent_error(format!("cannot make a screenshot file: {err}")))?,
        };
        let (width, height, scale, png) = a11y
            .capture_window(window.pid, &window.title)
            .await
            .map_err(|err| {
                agent_error(format!(
                    "cannot take a screenshot of {:?}: {}",
                    window.title,
                    reason(&err)
                ))
            })?;
        fs::write(&path, png)
            .map_err(|err| agent_error(format!("cannot write {}: {err}", path.display())))?;
        tracing::info!(width, height, "screenshot of {}", window.app_id);
        Ok(Screenshot {
            path: path.display().to_string(),
            width,
            height,
            scale,
            window: window.title,
            app_id: window.app_id,
            pid: window.pid,
        })
    }
}

/// 동작 뒤 기다린 결과
struct Settled {
    after: Rendered,
    /// 멈췄다 (거짓이면 상한이 지났다)
    settled: bool,
    ms: u64,
}

/// 동작 뒤 기다리기를 마칠지: Some(true)는 멈췄다(바뀐 뒤 SETTLE_QUIET 동안 그대로, 또는 SETTLE_FIRST까지
/// 바뀌지 않았다), Some(false)는 상한이 지나 그만둔다, None은 더 기다린다. `quiet`은 마지막으로 바뀐 뒤
/// 지난 시간이다 (바뀐 적이 없으면 None)
fn settle_state(quiet: Option<Duration>, waited: Duration, limit: Duration) -> Option<bool> {
    match quiet {
        Some(quiet) if quiet >= SETTLE_QUIET => Some(true),
        None if waited >= SETTLE_FIRST => Some(true),
        _ if waited >= limit => Some(false),
        _ => None,
    }
}

/// 동작마다 고른 기다림 상한 (없으면 SETTLE_MAX, 많아야 SETTLE_CAP)
fn settle_limit(ms: Option<u64>) -> Duration {
    ms.map_or(SETTLE_MAX, Duration::from_millis).min(SETTLE_CAP)
}

/// 동작 뒤 글 칸에 있어야 하는 글. 앱이 참이라고 답하고도 글을 버리는 일이 있어 다시 읽어 본다
/// (Firefox 주소 막대는 SetTextContents에 참을 주고 비어 있었다)
#[derive(Debug, PartialEq)]
enum Expect {
    /// fill: 글 칸의 글 전체가 이것이다
    Text(String),
    /// 접근성으로 커서 자리에 넣은 type: 글 칸에 이것이 들어 있다
    Contains(String),
}

impl Expect {
    /// 지금 읽은 글이 바란 대로인지. 줄바꿈은 \n으로 맞춰 견준다
    fn holds(&self, now: &str) -> bool {
        let now = now.replace("\r\n", "\n");
        match self {
            Expect::Text(text) => now == text.replace("\r\n", "\n"),
            Expect::Contains(text) => now.contains(&text.replace("\r\n", "\n")),
        }
    }

    /// 바란 대로가 아닐 때의 오류 글. 다른 길(type --ref, fill, --keys)을 알린다
    fn failure(&self, reference: &str, now: &str) -> String {
        let now = shorten(now, 80);
        match self {
            Expect::Text(text) => format!(
                "{reference} did not keep the text: it reads {now:?} after the app settled, not the \
                 {} characters sent. Type it instead: bunt agent type {text:?} --ref {reference}",
                text.chars().count()
            ),
            Expect::Contains(_) => format!(
                "{reference} did not take the text: the app accepted the accessible insert but the \
                 field reads {now:?}. Fill it, or pass --keys to press keys"
            ),
        }
    }
}

/// 글을 `limit`자까지 자르고 자른 글자 수를 붙인다
fn shorten(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}…[+{} chars]", count - limit)
}

/// D-Bus 오류의 설명 (오류 이름 없이)
fn reason(err: &zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(_, Some(message), _) => message.clone(),
        other => other.to_string(),
    }
}

/// 접근성 트리가 없는 창을 읽으려 할 때의 알림
fn no_tree(window: &str, app_id: &str, pid: u32) -> String {
    let why = if no_accessibility(app_id) {
        "this app does not support accessibility at all, so a restart does not help"
    } else {
        "Chromium, Electron, Firefox and Qt apps that started before accessibility was turned on \
         (before an AI was first allowed) expose it only after a restart"
    };
    format!(
        "{window} ({app_id}, pid {pid}) has no accessibility tree: {why}. Pass --app to read \
         another app (see `bunt agent apps`), or use `bunt agent screenshot` and `click --at` \
         on this one"
    )
}

/// 새 스크린샷 파일의 경로: $XDG_RUNTIME_DIR/buntos/screenshots/screenshot-<ms>.png. 메모리에 있는
/// 폴더라 오래된 것부터 지워 SCREENSHOTS_KEPT개만 남긴다
fn new_screenshot_path() -> io::Result<PathBuf> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?
        .join("buntos/screenshots");
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    let names = fs::read_dir(&dir)?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    for name in stale_screenshots(names, SCREENSHOTS_KEPT - 1) {
        let _ = fs::remove_file(dir.join(name));
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut path = dir.join(format!("screenshot-{millis}.png"));
    let mut number = 2;
    while path.exists() {
        path = dir.join(format!("screenshot-{millis}-{number}.png"));
        number += 1;
    }
    Ok(path)
}

/// 지울 스크린샷 파일 이름: 찍은 차례(이름 차례)로 세워 새것 `keep`개를 뺀 나머지
fn stale_screenshots(mut names: Vec<String>, keep: usize) -> Vec<String> {
    names.retain(|name| name.starts_with("screenshot-") && name.ends_with(".png"));
    names.sort();
    names.truncate(names.len().saturating_sub(keep));
    names
}

/// 세션이 지난 트리를 기억하는 열쇠
fn scope_key(scope: &str, options: Options) -> String {
    format!(
        "{scope} interactive={} hidden={}",
        options.interactive, options.hidden
    )
}

/// 요소가 든 창(앱 루트 바로 아래의 객체)
fn window_of(tree: &Tree, path: &str) -> Option<String> {
    let mut current = tree.get(path)?;
    loop {
        if current.parent == ROOT {
            return Some(current.path.clone());
        }
        current = tree.get(&current.parent)?;
    }
}

/// 동작 하나. 접근성 동작을 먼저 쓰고, 목록 항목은 목록에서 고른다
async fn perform(
    a11y: &A11y,
    bus: &str,
    target: &Item,
    parent: Option<&Item>,
    web: bool,
    params: &ActParams,
) -> Result<Option<String>, Failure> {
    let failed = |what: &str, err: zbus::Error| agent_error(format!("{what} failed: {err}"));
    let value = || {
        params.value.as_deref().ok_or_else(|| {
            (
                protocol::INVALID_PARAMS,
                format!("{} needs a value", params.action),
            )
        })
    };
    // 목록·표·트리 안의 항목이면 (부모, 부모 안의 번호)
    let selectable = parent
        .filter(|parent| parent.interfaces & iface::SELECTION != 0)
        .map(|parent| (parent.path.as_str(), target.index));
    match params.action.as_str() {
        "click" => {
            let actions = a11y.actions(bus, &target.path, web).await;
            if let Some((index, name)) = CLICK_ACTIONS.iter().find_map(|wanted| {
                actions
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(wanted))
                    .map(|index| (index, &actions[index]))
            }) {
                a11y.do_action(bus, &target.path, index as i32)
                    .await
                    .map_err(|err| failed("the accessible action", err))?;
                return Ok(Some(format!("clicked with the accessible action {name:?}")));
            }
            if let Some((list, index)) = selectable {
                a11y.select_child(bus, list, index)
                    .await
                    .map_err(|err| failed("selecting", err))?;
                return Ok(Some("selected it in its list".into()));
            }
            if let Some(name) = actions.first() {
                a11y.do_action(bus, &target.path, 0)
                    .await
                    .map_err(|err| failed("the accessible action", err))?;
                return Ok(Some(format!("ran its only accessible action {name:?}")));
            }
            Ok(None)
        }
        "fill" => {
            let text = value()?;
            if target.interfaces & iface::EDITABLE_TEXT == 0 || target.role == role::PASSWORD_TEXT {
                return Err(agent_error("this element is not an editable text field"));
            }
            let changed = a11y
                .set_text(bus, &target.path, text)
                .await
                .map_err(|err| failed("filling", err))?;
            if !changed {
                return Err(agent_error("the app did not accept the text"));
            }
            Ok(Some(format!(
                "filled with {} characters",
                text.chars().count()
            )))
        }
        "set" => {
            let number: f64 = value()?
                .parse()
                .map_err(|_| (protocol::INVALID_PARAMS, "set needs a number".to_string()))?;
            if target.interfaces & iface::VALUE == 0 {
                return Err(agent_error("this element has no value to set"));
            }
            a11y.set_value(bus, &target.path, number)
                .await
                .map_err(|err| failed("setting the value", err))?;
            Ok(Some(format!("set the value to {number}")))
        }
        "select" => {
            let (list, index) = selectable
                .ok_or_else(|| agent_error("this element is not in a selectable list"))?;
            a11y.select_child(bus, list, index)
                .await
                .map_err(|err| failed("selecting", err))?;
            Ok(Some("selected it in its list".into()))
        }
        other => Err((
            protocol::INVALID_PARAMS,
            format!("unknown action {other:?}; use click, fill, set, focus or select"),
        )),
    }
}

/// 활성 창: 컴포지터가 키보드 포커스를 준 앱의 활성 창(없으면 첫 창). 그 pid가 접근성 버스에 없으면
/// 제목이 포커스를 가진 창과 같은 활성 창이다(Flatpak 앱은 접근성 버스에 다른 프로세스로 붙는다).
/// 포커스를 가진 창이 트리를 내놓지 않으면 None이다: 다른 앱을 읽지 않는다. 컴포지터에 물을 수 없을
/// 때만 활성 상태인 첫 창이다. GTK는 셸의 레이어 창을 팝업이 닫힌 뒤에도 활성으로 알릴 때가 있다
fn active_window<'a>(
    apps: &'a [AppRef],
    focus: Option<&Focus>,
) -> Option<(&'a AppRef, &'a WindowRef)> {
    fn active(app: &AppRef) -> Option<&WindowRef> {
        app.windows
            .iter()
            .find(|window| window.states & state::ACTIVE != 0)
    }
    let Some(focus) = focus else {
        return apps
            .iter()
            .find_map(|app| active(app).map(|window| (app, window)));
    };
    if let Some(app) = apps.iter().find(|app| app.pid == Some(focus.pid))
        && let Some(window) = active(app).or_else(|| app.windows.first())
    {
        return Some((app, window));
    }
    if focus.layer || focus.title.is_empty() {
        return None;
    }
    apps.iter().find_map(|app| {
        active(app)
            .filter(|window| window.title == focus.title)
            .map(|window| (app, window))
    })
}

/// 컴포지터에는 창이 있는데 접근성 버스에 없는 앱의 창: pid가 같은 앱도, 제목이 같은 접근성 창도
/// 없다 (Flatpak 앱은 접근성 버스에 다른 프로세스로 붙어 pid가 다르다)
fn untracked(apps: &[AppRef], placed: &[Placed], installed: &[DesktopApp]) -> Vec<Untracked> {
    placed
        .iter()
        .filter(|window| {
            !apps.iter().any(|app| {
                app.pid == Some(window.pid)
                    || (!window.title.is_empty()
                        && app.windows.iter().any(|known| known.title == window.title))
            })
        })
        .map(|window| Untracked {
            name: installed
                .iter()
                .find(|app| app.owns(&window.app_id))
                .map_or_else(|| window.app_id.clone(), |app| app.name.clone()),
            app_id: window.app_id.clone(),
            pid: window.pid,
            title: window.title.clone(),
            unsupported: no_accessibility(&window.app_id),
        })
        .collect()
}

/// 접근성(AT-SPI)을 아예 지원하지 않는 앱 (앱 ID). 다시 켜도 트리가 없다
const NO_ACCESSIBILITY: [&str; 5] = [
    "foot",
    "footclient",
    "Alacritty",
    "kitty",
    "org.wezfurlong.wezterm",
];

fn no_accessibility(app_id: &str) -> bool {
    NO_ACCESSIBILITY.contains(&app_id)
}

/// `--app`이 가리키는 컴포지터 창 (접근성 트리가 없는 창도). 차례로: pid나 앱 ID가 같은 창, 설치된 앱
/// 가운데 이름이나 데스크톱 ID가 맞는 앱의 창, 접근성 이름이나 버스 이름이 같은 앱의 창, 제목이 같은
/// 창, 앱 ID나 제목에 그 글이 든 창. 한 앱의 창이 여럿이면 맨 위 창이다
fn find_window<'a>(
    placed: &'a [Placed],
    apps: &[AppRef],
    installed: &[DesktopApp],
    query: &str,
) -> Result<&'a Placed, Failure> {
    let lower = query.to_lowercase();
    let named = desktop::find(installed, query).ok();
    let accessible: HashSet<u32> = apps
        .iter()
        .filter(|app| app.name.to_lowercase() == lower || app.bus == query)
        .filter_map(|app| app.pid)
        .collect();
    let tiers: [&dyn Fn(&Placed) -> bool; 5] = [
        &|window| window.pid.to_string() == query || window.app_id.to_lowercase() == lower,
        &|window| named.is_some_and(|app| app.owns(&window.app_id)),
        &|window| accessible.contains(&window.pid),
        &|window| window.title.to_lowercase() == lower,
        &|window| {
            window.app_id.to_lowercase().contains(&lower)
                || window.title.to_lowercase().contains(&lower)
        },
    ];
    let matches = tiers
        .iter()
        .map(|tier| {
            placed
                .iter()
                .filter(|window| tier(window))
                .collect::<Vec<_>>()
        })
        .find(|matches| !matches.is_empty())
        .unwrap_or_default();
    let mut owners: Vec<&str> = Vec::new();
    for window in &matches {
        if !owners.contains(&window.app_id.as_str()) {
            owners.push(&window.app_id);
        }
    }
    let pids: HashSet<u32> = matches.iter().map(|window| window.pid).collect();
    match (matches.last(), pids.len()) {
        (None, _) => Err(agent_error(format!(
            "no window matches {query:?}; see `bunt agent apps`"
        ))),
        // 쌓인 순서의 끝이 맨 위다
        (Some(window), 1) => Ok(window),
        _ => Err(agent_error(format!(
            "{query:?} matches windows of several apps ({}); be more specific (a pid works)",
            owners.join(", ")
        ))),
    }
}

/// 범위 안에서 값, 내용, 속성을 따로 읽을 객체의 번호
fn in_scope(items: &[Item], roots: &[String], hidden: bool) -> Vec<usize> {
    let tree = Tree::new(items);
    let index: HashMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(at, item)| (item.path.as_str(), at))
        .collect();
    let mut wanted = Vec::new();
    let mut stack: Vec<&str> = roots.iter().map(String::as_str).collect();
    while let Some(path) = stack.pop() {
        let Some(item) = tree.get(path) else { continue };
        if !hidden && !tree.visible(item) {
            continue;
        }
        if item.wants_value() || item.wants_text() || item.wants_attributes() {
            wanted.push(index[path]);
        }
        stack.extend(tree.children(path).iter().map(|child| child.path.as_str()));
    }
    wanted
}

/// 컴포지터 창과 같은 접근성 창 (버스 이름, 경로): pid가 같은 앱에서 제목이 같은 창, 없으면 그 앱의
/// 첫 창
fn accessible_window(apps: &[AppRef], placed: &Placed) -> Option<(String, String)> {
    let app = apps.iter().find(|app| app.pid == Some(placed.pid))?;
    let window = app
        .windows
        .iter()
        .find(|window| window.title == placed.title)
        .or(app.windows.first())?;
    Some((app.bus.clone(), window.path.clone()))
}

/// `--app`이 가리키는 앱. 차례로: 접근성 이름(대소문자 무시)·버스 이름·pid가 같은 앱, 설치된 앱
/// 가운데 이름이나 데스크톱 ID가 맞는 하나(같은 것, 없으면 일부가 맞는 것. 컴포지터 창의 app_id로
/// 잇는다), 창 제목이 같은 앱, 이름이나 창 제목에 그 글이 든 앱
fn find_app<'a>(
    apps: &'a [AppRef],
    placed: &[Placed],
    installed: &[DesktopApp],
    query: &str,
) -> Result<&'a AppRef, Failure> {
    let lower = query.to_lowercase();
    let named: HashSet<u32> = desktop::find(installed, query)
        .map(|app| {
            placed
                .iter()
                .filter(|window| app.owns(&window.app_id))
                .map(|window| window.pid)
                .collect()
        })
        .unwrap_or_default();
    let tiers: [&dyn Fn(&AppRef) -> bool; 4] = [
        &|app| {
            app.name.to_lowercase() == lower
                || app.bus == query
                || app.pid.is_some_and(|pid| pid.to_string() == query)
        },
        &|app| app.pid.is_some_and(|pid| named.contains(&pid)),
        &|app| {
            app.windows
                .iter()
                .any(|window| window.title.to_lowercase() == lower)
        },
        &|app| {
            app.name.to_lowercase().contains(&lower)
                || app
                    .windows
                    .iter()
                    .any(|window| window.title.to_lowercase().contains(&lower))
        },
    ];
    let matches = tiers
        .iter()
        .map(|tier| apps.iter().filter(|app| tier(app)).collect::<Vec<_>>())
        .find(|matches| !matches.is_empty())
        .unwrap_or_default();
    match matches.as_slice() {
        [app] => Ok(app),
        [] => Err(agent_error(format!(
            "no app matches {query:?}; see `bunt agent apps`"
        ))),
        many => Err(agent_error(format!(
            "{query:?} matches {}; be more specific",
            many.iter()
                .map(|app| app.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// 키를 누르거나(press) 글을 친다(type). ref가 있으면 그 창을 앞으로 가져와 그 요소에 키보드 포커스를
/// 준 뒤 한다. 누를 때마다 다시 주므로 Tab은 늘 그 요소에서 나아간다. `keep_focus`면 창만 앞으로
/// 가져오고 창 안의 포커스는 그대로 둔다.
///
/// 글은 앱의 글 입력(text-input)에 확정한다. 글 입력이 켜지지 않으면 ref의 글 칸에는 접근성으로 커서
/// 자리에 넣고, 그것도 안 되면 `keys`일 때만 글자마다 키를 친다. 몰래 키로 바꿔 치면 포커스가 글 칸을
/// 벗어났을 때 키가 문서로 가 사이트 단축키가 돌고, 입력기가 한글 모드면 글이 바뀐다.
/// 접근성으로 넣었으면 멈춘 뒤 다시 읽어 볼 글을 함께 돌려준다
async fn press(
    a11y: &A11y,
    bus: &str,
    window_title: &str,
    target: Option<&Item>,
    params: &ActParams,
) -> Result<(String, Option<Expect>), Failure> {
    let typing = params.action == "type";
    let text = params.value.as_deref().unwrap_or_default();
    if typing && text.is_empty() {
        return Err((protocol::INVALID_PARAMS, "type needs text".into()));
    }
    let keys: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    if !typing && keys.is_empty() {
        return Err((
            protocol::INVALID_PARAMS,
            "press needs keys (e.g. \"ctrl+a BackSpace\")".into(),
        ));
    }
    if let Some(target) = target {
        let pid = a11y
            .pid(bus)
            .await
            .ok_or_else(|| agent_error("cannot tell which process owns this app"))?;
        let activated = a11y
            .activate_window(pid, window_title)
            .await
            .map_err(|err| agent_error(format!("activating the window failed: {err}")))?;
        if !activated {
            return Err(agent_error("the compositor has no window for this app"));
        }
        if !params.keep_focus {
            focus_element(a11y, bus, window_title, target)
                .await
                .map_err(|(code, message)| {
                    (
                        code,
                        format!(
                            "{message}; pass --keep-focus to {} where the focus is now",
                            params.action
                        ),
                    )
                })?;
        }
    }
    if !typing {
        a11y.press_keys(&keys)
            .await
            .map_err(|err| agent_error(format!("pressing keys failed: {err}")))?;
        return Ok((format!("pressed {}", keys.join(" ")), None));
    }
    let count = text.chars().count();
    if !wait_for_text_input(a11y).await {
        if let Some(target) = target
            .filter(|target| !params.keep_focus && target.interfaces & iface::EDITABLE_TEXT != 0)
            && a11y
                .insert_text(bus, &target.path, text)
                .await
                .unwrap_or(false)
        {
            return Ok((
                format!(
                    "inserted {count} characters at the caret (the app did not turn on text input)"
                ),
                Some(Expect::Contains(text.to_string())),
            ));
        }
        if !params.keys {
            return Err(nothing_typed(target.map(|_| params.target.as_str())));
        }
    }
    if params.keys {
        let how = a11y
            .type_text(text)
            .await
            .map_err(|err| agent_error(format!("typing failed: {}", reason(&err))))?;
        let how = if how == "text-input" {
            "through the app's text input"
        } else {
            "as keys"
        };
        return Ok((format!("typed {count} characters {how}"), None));
    }
    // 글 입력이 그새 꺼졌으면 컴포지터는 아무것도 치지 않고 거짓을 준다
    let committed = a11y
        .commit_text(text)
        .await
        .map_err(|err| agent_error(format!("typing failed: {}", reason(&err))))?;
    if !committed {
        return Err(nothing_typed(target.map(|_| params.target.as_str())));
    }
    Ok((
        format!("typed {count} characters through the app's text input"),
        None,
    ))
}

/// 글 입력이 켜지지 않아 아무것도 치지 않았을 때의 오류. 다른 길을 알린다
fn nothing_typed(reference: Option<&str>) -> Failure {
    let what = match reference {
        Some(reference) => format!(
            "{reference} did not take the text: the app did not turn on text input for it and \
             refused the accessible insert"
        ),
        None => "no text field has the keyboard focus (the focused app did not turn on text \
                 input)"
            .into(),
    };
    agent_error(format!(
        "{what}, so nothing was typed. Give a text field the focus (type --ref <ref>, focus <ref>, \
         click), or fill it; pass --keys to press keys instead, e.g. in a terminal or a game"
    ))
}

/// 요소에 키보드 포커스를 준다. GTK4는 접근성의 GrabFocus를 지원하지 않는다(NotSupported). 그래서 글
/// 칸은 사람처럼 포인터로 눌러 포커스를 주고 커서를 글 끝으로 옮긴다. 다른 요소는 누르면 동작이
/// 일어나므로 누르지 않는다
async fn focus_element(
    a11y: &A11y,
    bus: &str,
    window_title: &str,
    target: &Item,
) -> Result<String, Failure> {
    if target.states & state::FOCUSED != 0 {
        return Ok("it already has the keyboard focus".into());
    }
    if matches!(a11y.grab_focus(bus, &target.path).await, Ok(true)) {
        return Ok("moved the keyboard focus to it".into());
    }
    if target.interfaces & iface::EDITABLE_TEXT == 0 {
        return Err(agent_error(
            "the app cannot move the keyboard focus to this element; click it if that is safe",
        ));
    }
    pointer_click(a11y, bus, window_title, target).await?;
    let _ = a11y.caret_to_end(bus, &target.path).await;
    Ok("clicked the field to focus it and put the caret at the end".into())
}

/// 키보드 포커스를 가진 앱이 글 입력(text-input)을 켤 때까지 TEXT_INPUT_WAIT만큼 기다린다
async fn wait_for_text_input(a11y: &A11y) -> bool {
    let started = Instant::now();
    loop {
        match a11y.text_input_active().await {
            Ok(true) => return true,
            Ok(false) if started.elapsed() < TEXT_INPUT_WAIT => {
                async_io::Timer::after(TEXT_INPUT_STEP).await;
            }
            _ => return false,
        }
    }
}

/// 접근성 동작이 없는 요소의 가운데를 포인터로 누른다. 자리는 컴포지터가 놓은 창의 자리와 앱이
/// 알린 창 안 자리를 더한 것이다
async fn pointer_click(
    a11y: &A11y,
    bus: &str,
    window_title: &str,
    target: &Item,
) -> Result<String, Failure> {
    let pid = a11y
        .pid(bus)
        .await
        .ok_or_else(|| agent_error("cannot tell which process owns this app"))?;
    let window = compositor_window(a11y, pid, window_title).await?;
    let (x, y, width, height) = a11y
        .extents(bus, &target.path)
        .await
        .map_err(|err| agent_error(format!("cannot tell where the element is: {err}")))?;
    if width <= 0 || height <= 0 {
        return Err(agent_error("the element has no size on screen"));
    }
    // 다른 창에 가려 있으면 그 창을 누르게 되므로 먼저 앞으로 가져온다
    activate(a11y, pid, &window.title).await?;
    let at = (
        f64::from(window.x + x) + f64::from(width) / 2.0,
        f64::from(window.y + y) + f64::from(height) / 2.0,
    );
    a11y.click(at.0, at.1)
        .await
        .map_err(|err| agent_error(format!("the pointer click failed: {err}")))?;
    Ok(format!(
        "clicked with the pointer at ({:.0}, {:.0})",
        at.0, at.1
    ))
}

/// 컴포지터가 놓은 그 pid의 창: 제목이 같은 창, 없으면 맨 위 창
async fn compositor_window(a11y: &A11y, pid: u32, title: &str) -> Result<Placed, Failure> {
    let windows = a11y
        .windows()
        .await
        .map_err(|err| agent_error(format!("cannot read the window list: {err}")))?;
    windows
        .iter()
        .rev()
        .filter(|window| window.pid == pid)
        .find(|window| window.title == title)
        .or_else(|| windows.iter().rev().find(|window| window.pid == pid))
        .cloned()
        .ok_or_else(|| agent_error("the compositor has no window for this app"))
}

/// 창을 앞으로 가져온다
async fn activate(a11y: &A11y, pid: u32, title: &str) -> Result<(), Failure> {
    a11y.activate_window(pid, title)
        .await
        .map_err(|err| agent_error(format!("activating the window failed: {err}")))?;
    Ok(())
}

/// 휠을 돌린다: 요소가 있으면 그 위에서, 없으면 창 가운데에서. 눈금 수는 (가로, 세로)이고
/// 오른쪽·아래가 +다. 휠은 포인터 아래의 스크롤 영역을 움직이므로 요소는 스크롤할 목록이나 문서다
async fn wheel(
    a11y: &A11y,
    pid: u32,
    window_title: &str,
    over: Option<(&str, &Item)>,
    [dx, dy]: [f64; 2],
) -> Result<String, Failure> {
    let window = compositor_window(a11y, pid, window_title).await?;
    let (width, height) = (f64::from(window.width), f64::from(window.height));
    let (x, y) = match over {
        Some((bus, target)) => {
            let (x, y, w, h) = a11y
                .extents(bus, &target.path)
                .await
                .map_err(|err| agent_error(format!("cannot tell where the element is: {err}")))?;
            // 창보다 큰 문서는 창 안에 보이는 부분의 가운데
            let middle = |start: i32, length: i32, size: f64| {
                let from = f64::from(start).max(0.0);
                let to = f64::from(start.saturating_add(length)).min(size);
                if to > from {
                    (from + to) / 2.0
                } else {
                    size / 2.0
                }
            };
            (middle(x, w, width), middle(y, h, height))
        }
        None => (width / 2.0, height / 2.0),
    };
    activate(a11y, pid, &window.title).await?;
    let at = (f64::from(window.x) + x, f64::from(window.y) + y);
    a11y.scroll(at.0, at.1, dx, dy)
        .await
        .map_err(|err| agent_error(format!("scrolling failed: {}", reason(&err))))?;
    Ok(format!(
        "scrolled {} at ({:.0}, {:.0})",
        notches(dx, dy),
        at.0,
        at.1
    ))
}

/// "down 5 notches", "left 2 notches and up 1 notch"
fn notches(dx: f64, dy: f64) -> String {
    let one = |amount: f64, minus: &str, plus: &str| {
        let count = amount.abs();
        let unit = if count == 1.0 { "notch" } else { "notches" };
        let way = if amount < 0.0 { minus } else { plus };
        format!("{way} {count} {unit}")
    };
    match (dx != 0.0, dy != 0.0) {
        (true, true) => format!("{} and {}", one(dx, "left", "right"), one(dy, "up", "down")),
        (true, false) => one(dx, "left", "right"),
        _ => one(dy, "up", "down"),
    }
}

/// 화면 밖 요소를 보이게 스크롤한다. 앱이 스스로 하면(Component.ScrollTo, Firefox) 그것을 쓰고, 못
/// 하면(GTK4는 지원하지 않는다) 요소가 있는 쪽으로 창 가운데에서 휠을 돌리며 요소가 창 안에 들어올
/// 때까지 다시 본다
async fn scroll_into_view(
    a11y: &A11y,
    bus: &str,
    window_title: &str,
    target: &Item,
) -> Result<String, Failure> {
    let path = target.path.as_str();
    // Firefox는 웹 페이지 요소의 자리를 받아 둔 뒤에야 ScrollTo 뒤의 보임 상태(SHOWING)를 고친다
    let _ = a11y.extents(bus, path).await;
    if matches!(a11y.scroll_to(bus, path).await, Ok(true))
        && wait_until_shown(a11y, bus, path).await
    {
        return Ok("scrolled it into view".into());
    }
    let pid = a11y
        .pid(bus)
        .await
        .ok_or_else(|| agent_error("cannot tell which process owns this app"))?;
    let window = compositor_window(a11y, pid, window_title).await?;
    activate(a11y, pid, &window.title).await?;
    let center = (
        f64::from(window.x) + f64::from(window.width) / 2.0,
        f64::from(window.y) + f64::from(window.height) / 2.0,
    );
    let mut last = None;
    for turn in 1..=SCROLL_TURNS {
        let (x, y, width, height) = a11y
            .extents(bus, path)
            .await
            .map_err(|err| agent_error(format!("cannot tell where it is: {err}")))?;
        let (dx, dy) = (
            toward(x, width, window.width),
            toward(y, height, window.height),
        );
        if dx == 0.0 && dy == 0.0 {
            if wait_until_shown(a11y, bus, path).await {
                return Ok(format!(
                    "scrolled it into view with the wheel ({turn} turns)"
                ));
            }
            break;
        }
        // 더 움직이지 않으면 끝에 닿았거나 휠로 움직이는 영역이 아니다
        if last == Some((x, y)) {
            break;
        }
        last = Some((x, y));
        a11y.scroll(center.0, center.1, dx, dy)
            .await
            .map_err(|err| agent_error(format!("scrolling failed: {}", reason(&err))))?;
        async_io::Timer::after(SCROLL_PAUSE).await;
    }
    Err(agent_error(
        "it is off screen and scrolling did not bring it into view; scroll the list or page that \
         holds it (bunt agent scroll down --ref <list>) and take a new snapshot",
    ))
}

/// 요소(창 기준 start, 길이 length)가 창(크기 size) 밖에 있는 쪽으로 돌릴 눈금 수. 멀수록 많이
/// 돌리되(한 번에 SCROLL_NOTCHES까지) 창 안이면 0이다. 창보다 큰 요소는 머리를 창 위에 맞춘다
fn toward(start: i32, length: i32, size: i32) -> f64 {
    let distance = if start < 0 {
        start
    } else if start + length > size {
        (start + length - size).min(start)
    } else {
        return 0.0;
    };
    let notches = (f64::from(distance) / SCROLL_PIXELS).round();
    notches
        .clamp(-SCROLL_NOTCHES, SCROLL_NOTCHES)
        .abs()
        .max(1.0)
        * f64::from(distance.signum())
}

/// 요소가 보일 때까지(SHOWING) 잠깐 기다린다
async fn wait_until_shown(a11y: &A11y, bus: &str, path: &str) -> bool {
    let started = Instant::now();
    loop {
        if a11y.states(bus, path).await & state::SHOWING != 0 {
            return true;
        }
        if started.elapsed() >= SHOWN_WAIT {
            return false;
        }
        async_io::Timer::after(SETTLE_STEP).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_tell_the_ai_what_the_user_can_do() {
        // 처음에는 셸이 묻고 있다고 알려, AI가 사용자에게 요청 창에서 허용해 달라고 할 수 있게 한다
        let asking = Refusal::Asking.message("claude");
        assert!(asking.starts_with("claude is not allowed to control this computer."));
        assert!(asking.contains("\"Open System Settings\""));
        assert!(Refusal::Asked.message("claude").contains("already asked"));
        assert!(Refusal::TurnedOff.message("claude").contains("turned off"));
    }

    fn app(pid: u32, name: &str, windows: &[(&str, bool)]) -> AppRef {
        AppRef {
            bus: format!(":1.{pid}"),
            name: name.into(),
            pid: Some(pid),
            windows: windows
                .iter()
                .enumerate()
                .map(|(at, (title, active))| WindowRef {
                    path: format!("/w{pid}/{at}"),
                    title: (*title).into(),
                    states: if *active { state::ACTIVE } else { 0 },
                })
                .collect(),
        }
    }

    fn placed(pid: u32, app_id: &str, title: &str) -> Placed {
        Placed {
            pid,
            app_id: app_id.into(),
            title: title.into(),
            x: 0,
            y: 0,
            width: 800,
            height: 600,
            focused: false,
        }
    }

    fn focus(pid: u32, title: &str) -> Focus {
        Focus {
            pid,
            app_id: "org.example.App".into(),
            title: title.into(),
            layer: false,
        }
    }

    #[test]
    fn settling_stops_when_quiet_or_at_the_limit() {
        let ms = Duration::from_millis;
        let limit = SETTLE_MAX;
        // 바뀐 뒤 SETTLE_QUIET 동안 그대로면 멈췄다
        assert_eq!(settle_state(Some(SETTLE_QUIET), ms(400), limit), Some(true));
        assert_eq!(settle_state(Some(ms(100)), ms(400), limit), None);
        // 바뀌지 않으면 SETTLE_FIRST까지 기다린다
        assert_eq!(settle_state(None, ms(500), limit), None);
        assert_eq!(settle_state(None, SETTLE_FIRST, limit), Some(true));
        // 늘 바뀌는 화면은 상한에서 그만두고 멈추지 않았다고 알린다
        assert_eq!(settle_state(Some(ms(50)), limit, limit), Some(false));
        assert_eq!(settle_state(Some(ms(50)), ms(500), ms(500)), Some(false));
        // 상한이 SETTLE_FIRST보다 짧으면 바뀌지 않아도 멈췄다고 하지 않는다
        assert_eq!(settle_state(None, ms(300), ms(300)), Some(false));
        assert_eq!(settle_state(None, ms(50), Duration::ZERO), Some(false));
    }

    #[test]
    fn settle_limit_has_a_default_and_a_cap() {
        assert_eq!(settle_limit(None), SETTLE_MAX);
        assert_eq!(settle_limit(Some(500)), Duration::from_millis(500));
        assert_eq!(settle_limit(Some(0)), Duration::ZERO);
        assert_eq!(settle_limit(Some(600_000)), SETTLE_CAP);
    }

    #[test]
    fn fill_is_checked_against_what_the_field_reads() {
        let fill = Expect::Text("example.com".into());
        assert!(fill.holds("example.com"));
        assert!(!fill.holds(""));
        assert!(!fill.holds("example.com/"));
        assert!(Expect::Text("a\nb".into()).holds("a\r\nb"));
        // Firefox 주소 막대: 참을 답했지만 비어 있었다. 오류는 type --ref를 권한다
        let message = fill.failure("e82", "");
        assert!(
            message.starts_with("e82 did not keep the text"),
            "{message}"
        );
        assert!(
            message.contains("bunt agent type \"example.com\" --ref e82"),
            "{message}"
        );
        let insert = Expect::Contains("42".into());
        assert!(insert.holds("결과: 42 끝"));
        assert!(!insert.holds("결과: "));
        assert!(insert.failure("e5", "x").contains("--keys"));
    }

    #[test]
    fn long_text_is_shortened_in_messages() {
        assert_eq!(shorten("abc", 5), "abc");
        assert_eq!(shorten("가나다라마", 2), "가나…[+3 chars]");
    }

    #[test]
    fn old_screenshots_go_first() {
        let names = vec![
            "screenshot-1700000000300.png".to_string(),
            "notes.txt".to_string(),
            "screenshot-1700000000100.png".to_string(),
            "screenshot-1700000000200.png".to_string(),
        ];
        assert_eq!(
            stale_screenshots(names.clone(), 2),
            ["screenshot-1700000000100.png"]
        );
        assert!(stale_screenshots(names.clone(), 3).is_empty());
        assert_eq!(stale_screenshots(names, 0).len(), 3);
    }

    #[test]
    fn the_active_window_is_the_focused_app_and_never_another() {
        let apps = [
            app(10, "bunt-shell", &[("menubar", true)]),
            app(20, "Calculator", &[("Calculator", false)]),
        ];
        // pid가 같은 앱의 창
        let (found, window) = active_window(&apps, Some(&focus(20, "Calculator"))).unwrap();
        assert_eq!(
            (found.name.as_str(), window.title.as_str()),
            ("Calculator", "Calculator")
        );
        // 포커스를 가진 창(Electron 등)이 트리를 내놓지 않으면 활성으로 알린 다른 앱을 읽지 않는다
        assert!(active_window(&apps, Some(&focus(30, "Claude"))).is_none());
        // Flatpak 앱은 접근성 버스의 pid가 달라 제목으로 찾는다
        let flatpak = [app(40, "Firefox", &[("Mozilla Firefox", true)])];
        assert!(active_window(&flatpak, Some(&focus(41, "Mozilla Firefox"))).is_some());
        // 컴포지터에 물을 수 없으면 활성 상태인 첫 창
        let (found, _) = active_window(&apps, None).unwrap();
        assert_eq!(found.name, "bunt-shell");
    }

    #[test]
    fn windows_without_a_tree_are_listed() {
        let apps = [app(20, "Calculator", &[("Calculator", true)])];
        let windows = [
            placed(20, "org.gnome.Calculator", "Calculator"),
            placed(30, "com.anthropic.claude", "Claude"),
        ];
        let installed = [DesktopApp {
            id: "com.anthropic.claude".into(),
            name: "Claude".into(),
            untranslated: "Claude".into(),
            path: PathBuf::from("/usr/share/applications/com.anthropic.claude.desktop"),
            dbus: false,
            wm_class: None,
        }];
        let untracked = untracked(&apps, &windows, &installed);
        assert_eq!(untracked.len(), 1);
        assert_eq!(
            (untracked[0].name.as_str(), untracked[0].pid),
            ("Claude", 30)
        );
    }

    #[test]
    fn windows_are_found_by_pid_id_name_or_title() {
        let apps = [app(20, "Calculator", &[("Calculator", true)])];
        let windows = [
            placed(20, "org.gnome.Calculator", "Calculator"),
            placed(30, "firefox-esr", "lichess.org — Firefox"),
            placed(30, "firefox-esr", "Library"),
            placed(40, "org.gnome.TextEditor", "Untitled"),
        ];
        let find = |query: &str| find_window(&windows, &apps, &[], query).map(|w| w.title.clone());
        assert_eq!(find("40").unwrap(), "Untitled");
        assert_eq!(find("org.gnome.calculator").unwrap(), "Calculator");
        assert_eq!(find("calculator").unwrap(), "Calculator");
        // 한 앱의 창이 여럿이면 맨 위(쌓인 순서의 끝) 창
        assert_eq!(find("firefox").unwrap(), "Library");
        assert_eq!(find("lichess").unwrap(), "lichess.org — Firefox");
        assert!(find("nothing").is_err());
        // 여러 앱에 맞으면 고르지 않는다
        let message = find("org.gnome").unwrap_err().1;
        assert!(message.contains("several apps"), "{message}");
    }
}
