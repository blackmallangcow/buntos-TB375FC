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
//!
//! 기다리기(`wait`)와 JS REPL(`repl`, bunt_agent::repl)은 호출 하나 안에서 데몬을 여러 번 부른다. 이것들은
//! 데몬을 붙잡지 않고, 그 안의 호출마다 정책을 다시 읽어 클라이언트를 받는다(Gate). REPL은 클라이언트와 세션마다
//! 띄운 워커 프로세스(`buntd-agent --repl-worker`, seccomp로 가둔다)에서 돈다.

use std::{
    cell::{Cell, RefCell},
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
    atspi::{
        self, A11y, AppRef, Declined, Focus, KEYBOARD_WINDOW, LOCK_WINDOW, Placed, ROOT, WindowRef,
    },
    desk::Desk,
    desktop::{self, DesktopApp},
    diff,
    protocol::{
        self, ActParams, Acted, Installed, LaunchParams, Launched, ReplParams, Request, Response,
        RpcError, Screenshot, ScreenshotParams, Snapshot, SnapshotParams, Untracked, WaitParams,
    },
    repl::{Repls, Worker, worker},
    session::{Aim, Sessions},
    skills,
    tree::{self, Item, Options, Tree, iface, role, state},
    wait,
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
/// 스크롤한 뒤 요소가 보이기(SHOWING)를 기다리는 시간. 보통 앱은 바로 고친다. 1초이던 때 바쁜 VM의 Firefox가
/// ScrollTo 뒤 그 안에 고치지 못해 e2e 5번이 실패했다(2026-10-31)
const SHOWN_WAIT: Duration = Duration::from_secs(5);
/// 휠을 돌린 뒤 요소의 자리가 이만큼 그대로면 끝에 닿았거나 휠로 움직이는 영역이 아니다. 전에는 한 번 다시 읽어
/// 그대로면 멈췄는데, 바쁜 앱은 자리를 늦게 고친다
const SCROLL_STUCK: Duration = Duration::from_secs(3);
/// 화면 밖 요소를 보이게 하는 데 쓰는 시간 전체의 상한. 요청은 하나씩 처리하므로 스크롤할 수 없는 요소나 아주
/// 느린 앱이 데몬을 오래 붙잡지 않게 한다. 지나면 그때 하던 한 바퀴(자리 읽기와 휠)만 마친다
const SCROLL_LIMIT: Duration = Duration::from_secs(15);
/// 스크린샷 폴더($XDG_RUNTIME_DIR, 메모리)에 남기는 파일 수
const SCREENSHOTS_KEPT: usize = 20;
/// 세션을 열 때 접근성을 켜며 버스의 답을 기다리는 시간. 로그인 중의 VM에서는 접근성 버스가 뜨는 데
/// 5초 넘게 걸렸다. 컴포지터는 기다리지 않는다
const PREPARE_WAIT: Duration = Duration::from_secs(30);

/// 동작의 대상: 앱(접근성 버스 이름), 창, 요소와 그 부모. press는 요소 없이 창에만 보낼 수 있다
struct Target {
    bus: String,
    /// 앱 이름 (답의 첫 줄에 받은 앱으로 적는다)
    app: String,
    window: String,
    window_title: String,
    item: Option<Item>,
    parent: Option<Item>,
    /// 요소가 웹 페이지에서 스크롤해 가려졌다 (동작 전에 보이게 스크롤한다)
    off_screen: bool,
    /// 요소가 웹 페이지 안에 있다 (Firefox는 그 동작과 자리를 늦게 알린다)
    web: bool,
    /// 요소가 드롭다운(콤보 상자)의 열린 목록 안에 있다 (누르기는 목록에서 고르지 않고 포인터로 누른다)
    drop_down: bool,
}

/// ref 없는 동작(press, type, scroll)을 받을 창
enum Chosen {
    /// 접근성 트리가 있는 창
    Tree(Box<Target>),
    /// 트리가 없는 창 (pid, 제목, 앱 이름). 키와 휠은 보내고 diff는 없다
    Blind(u32, String, String),
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
    /// 세션 버스. 요청마다 컴포지터에 화면이 잠겼는지 묻는다
    session: Option<zbus::Connection>,
    sessions: Sessions,
    /// 지금 요청에 쓰는 접근 정책. 설정에서 바꾼 것이 바로 듣도록 요청마다 다시 읽는다
    policy: Policy,
    /// 열었지만 아직 창이 뜨지 않은 앱 (데스크톱 ID, 연 시각)
    launching: HashMap<String, Instant>,
    /// 지금 요청을 보낸 AI 클라이언트의 이름. 동작을 시작할 때 컴포지터에 알린다(메뉴 막대의 AI 표시)
    client: String,
}

fn main() {
    // REPL 워커는 로그도 소켓도 쓰지 않는다. 무엇을 하기 전에 가른다
    if std::env::args().nth(1).as_deref() == Some("--repl-worker") {
        worker::main();
    }
    // 이 빌드의 크레이트 판과 커밋 (이슈 #1 P1-2). 커밋은 빌드할 때의 BUNTOS_COMMIT (이미지 빌드가 넣는다)
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const COMMIT: &str = match option_env!("BUNTOS_COMMIT") {
        Some(commit) if !commit.is_empty() => commit,
        _ => "unknown",
    };
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("buntd-agent {VERSION} (commit {COMMIT})");
        return;
    }
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
    // 기기의 보고와 소스 커밋을 맞춰 보게 판을 남긴다
    tracing::info!("buntd-agent {VERSION} (commit {COMMIT}) started");
    let agent = Arc::new(Mutex::new(Agent::default()));
    let repls = Arc::new(Repls::new(Box::new(Worker::spawn)));
    // 오래 쉰 REPL과 스스로 끝난 워커를 때때로 거둔다
    if let Err(err) = Repls::reap_in_background(&repls) {
        tracing::warn!("cannot start reaping REPL workers: {err}");
    }
    loop {
        // 받기 전에 본다: 끝나면 기다리는 연결은 소켓에 남고 systemd가 새 데몬을 띄워 넘긴다
        wait_for_connection(&listener);
        if replaced() {
            tracing::info!("buntd-agent was updated; exiting so the new one serves the socket");
            std::process::exit(0);
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let (agent, repls) = (agent.clone(), repls.clone());
                std::thread::spawn(move || serve(stream, &agent, &repls));
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
fn serve(stream: UnixStream, agent: &Mutex<Agent>, repls: &Repls) {
    let client = peer_pid(&stream).map(bunt_access::identify);
    let mut writer = &stream;
    for line in BufReader::new(&stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let response = handle(&line, agent, repls, client.as_ref());
        let Ok(mut reply) = serde_json::to_string(&response) else {
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
        "a window only the user can answer is open (authentication, network password, Bluetooth \
         pairing or clipboard history)"
            .into(),
    )
}

fn screen_locked() -> Failure {
    (
        protocol::SCREEN_LOCKED,
        "the screen is locked; only the user can unlock it with their password. Try again after \
         they unlock it"
            .into(),
    )
}

/// 셸의 화면 키보드는 사람만 누른다 (atspi::KEYBOARD_WINDOW)
fn on_screen_keyboard() -> Failure {
    (
        protocol::NOT_ALLOWED,
        "the on-screen keyboard is only for people; enter keys with press or type".into(),
    )
}

fn blocked_app() -> Failure {
    (
        protocol::BLOCKED_APP,
        "the user has blocked AI agents from this app (System Settings > AI Agents)".into(),
    )
}

/// 사람이 AI 제어를 멈췄다. 멈춤은 컴포지터가 갖고, 사람만 메뉴 막대나 단축키로 푼다
fn paused() -> Failure {
    (
        protocol::PAUSED,
        "the person paused AI control; only the person can resume it. Reading (snapshot, \
         screenshot, wait) still works"
            .into(),
    )
}

/// 사람이 쓰는 중이라 컴포지터가 받지 않았다. 다시 해 볼 때까지의 시간을 "try again in N ms"로 싣는다 (REPL의
/// UserActiveError가 이것을 읽는다)
fn user_active(what: &str, retry_after_ms: u64) -> Failure {
    (
        protocol::USER_ACTIVE,
        format!(
            "{what}: the person is using the computer, so nothing was sent; try again in \
             {retry_after_ms} ms"
        ),
    )
}

/// ref의 앱이 끝났거나 다시 떴다: ref를 준 때의 프로세스가 아니다
fn target_gone(reference: &str, pid: u32) -> Failure {
    (
        protocol::TARGET_GONE,
        format!(
            "{reference} is gone: {} or restarted (it was pid {pid}); take a new snapshot",
            protocol::APP_QUIT
        ),
    )
}

/// 컴포지터가 에이전트의 입력이나 활성화를 받지 않은 것을 AI에게 알릴 오류로 바꾼다. 사람을 먼저 둔 거절(사람이
/// 포커스를 옮겼다, 쓰는 중이다, 멈췄다)은 그 번호로 알린다. 데몬은 다시 활성화하거나 다시 눌러 되찾지 않는다
fn refused(err: &zbus::Error, what: &str) -> Failure {
    refusal(atspi::declined(err), &reason(err), what)
}

/// refused의 가르기: 컴포지터의 거절 까닭(`declined`)과 오류 알림(`why`)으로
fn refusal(declined: Option<Declined>, why: &str, what: &str) -> Failure {
    match declined {
        Some(Declined::FocusMoved) => (
            protocol::FOCUS_MOVED,
            format!(
                "{what}: the keyboard focus is not on that window any more (the person moved \
                 it), so nothing was sent. Do not take it back: read the window again and use a \
                 ref (fill and accessible actions need no focus) or --app"
            ),
        ),
        Some(Declined::UserActive { retry_after_ms }) => user_active(what, retry_after_ms),
        Some(Declined::Paused) => paused(),
        None => {
            if why.contains("only the user can answer") {
                auth_dialog()
            } else if why.contains("screen is locked") {
                screen_locked()
            } else if why.starts_with("that window is gone") {
                (protocol::TARGET_GONE, format!("{what} failed: {why}"))
            } else {
                agent_error(format!("{what} failed: {why}"))
            }
        }
    }
}

/// ref도 `--app`도 없는 키·글·휠을 받을 창이 아직 없다: 이 세션에서 AI가 다룬 창이 없다
fn no_aim(action: &str) -> Failure {
    (
        protocol::INVALID_PARAMS,
        format!(
            "{action} without a ref or --app goes only to the window you worked on last, and \
             this session has not worked on one yet; pass --app <app> (see `bunt agent apps`) \
             or a ref"
        ),
    )
}

/// 사람이 키보드 포커스를 옮겨 ref 없는 키·글·휠을 보내지 않을 때의 알림: 지금 포커스를 가진 곳과 다른 길
fn focus_moved(aim: &Aim, now: &str, action: &str) -> Failure {
    let (done, wanted) = match action {
        "type" => ("typed", "type into"),
        "press" => ("pressed", "press keys in"),
        _ => ("scrolled", "scroll"),
    };
    let app = if aim.app_id.is_empty() {
        aim.pid.to_string()
    } else {
        aim.app_id.clone()
    };
    (
        protocol::FOCUS_MOVED,
        format!(
            "{now}, not on {} {:?}, the window you worked on last; the person may be using it, \
             so nothing was {done}. Pass --app {app} or a ref to {wanted} your window (fill and \
             accessible actions need no focus)",
            aim.app, aim.title
        ),
    )
}

/// 지금 키보드 포커스를 가진 곳 (FOCUS_MOVED 알림의 앞머리)
fn focus_now(focus: Option<&Focus>, apps: &[AppRef], installed: &[DesktopApp]) -> String {
    match focus {
        None => "no window has the keyboard focus".into(),
        Some(focus) if focus.layer => {
            format!("the keyboard focus is on the shell's {}", focus.app_id)
        }
        Some(focus) => format!(
            "the keyboard focus is on {} {:?}",
            app_label(focus.pid, &focus.app_id, apps, installed),
            focus.title
        ),
    }
}

/// 사람에게 보이는 앱 이름: 접근성 버스의 이름, 설치된 앱의 이름, 앱 ID 차례
fn app_label(pid: u32, app_id: &str, apps: &[AppRef], installed: &[DesktopApp]) -> String {
    apps.iter()
        .find(|app| app.pid == Some(pid) && !app.name.is_empty())
        .map(|app| app.name.clone())
        .or_else(|| {
            installed
                .iter()
                .find(|app| app.owns(app_id))
                .map(|app| app.name.clone())
        })
        .unwrap_or_else(|| app_id.to_string())
}

fn handle(line: &str, agent: &Mutex<Agent>, repls: &Repls, client: Option<&Client>) -> Response {
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
    let started = Instant::now();
    let method = request.method.clone();
    let name = client.map_or("", |client| client.name.as_str());
    let dispatch =
        |policy: Policy, method: &str, params: Value| locked(agent, policy, name, method, params);
    let response = answer(request, client, &Policy::load, &dispatch, repls);
    tracing::info!(
        client = client.map_or("", |client| client.name.as_str()),
        method = %method,
        ms = started.elapsed().as_millis() as u64,
        ok = response.error.is_none(),
        "request"
    );
    response
}

/// 요청 하나에 답한다. 허용하지 않은 AI는 무엇을 하기 전에(기다리거나 JS를 돌리기 전에도) 거절한다.
/// 데몬을 여러 번 부르는 메서드(wait, repl)는 데몬을 붙잡지 않고, 부를 때마다 정책을 다시 본다(Gate)
fn answer(
    request: Request,
    client: Option<&Client>,
    policy: &dyn Fn() -> Policy,
    dispatch: &dyn Fn(Policy, &str, Value) -> Result<Value, Failure>,
    repls: &Repls,
) -> Response {
    let first = policy();
    if let Err(failure) = admit(client, &first) {
        return reply(request.id, Err(failure));
    }
    let gate = Gate {
        client,
        policy,
        dispatch,
    };
    fn to_value<T: serde::Serialize>(value: Result<T, Failure>) -> Result<Value, Failure> {
        value.and_then(|value| {
            serde_json::to_value(value).map_err(|err| agent_error(err.to_string()))
        })
    }
    let result = match request.method.as_str() {
        "wait" => to_value(
            serde_json::from_value::<WaitParams>(request.params)
                .map_err(|err| (protocol::INVALID_PARAMS, err.to_string()))
                .and_then(|params| wait::run(&gate, &params)),
        ),
        "repl" => to_value(
            serde_json::from_value::<ReplParams>(request.params)
                .map_err(|err| (protocol::INVALID_PARAMS, err.to_string()))
                .and_then(|params| repls.run(&gate, &repl_owner(client), params)),
        ),
        // 사용 안내는 데스크톱을 읽지 않으므로 데몬을 잡지 않는다
        "skills" => skills::answer(request.params),
        method => dispatch(first, method, request.params),
    };
    reply(request.id, result)
}

/// 스냅샷의 앱 머리줄 `- app: "이름" [pid=812]`. 이름은 앱이 정하므로 따옴표 안에 막아 적는다 (검토: 이름에 든
/// 따옴표와 줄바꿈으로 pid를 꾸미거나 요소 줄을 끼워 넣을 수 있었다)
fn app_header(name: &str, pid: Option<u32>) -> String {
    let name = one_line(name)
        .replace('\t', " ")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    match pid {
        Some(pid) => format!("- app: \"{name}\" [pid={pid}]"),
        None => format!("- app: \"{name}\""),
    }
}

/// 트리 줄 하나에 든 줄바꿈을 띄어쓰기로 바꾼다. tree.rs는 이름과 값을 따옴표에 막지만 앱이 준 속성 값(level)은 그대로
/// 써서, 줄바꿈이 든 값이 다른 요소의 줄(가짜 ref)을 끼워 넣을 수 있었다(검토). 모델과 줄 파서(outline.rs)가 보는 줄은
/// 요소 하나다
fn one_line(text: &str) -> String {
    text.replace(
        [
            '\n', '\r', '\u{0b}', '\u{0c}', '\u{85}', '\u{2028}', '\u{2029}',
        ],
        " ",
    )
}

/// REPL의 주인: 같은 AI(실행 파일과 스크립트)의 호출만 같은 REPL의 변수를 본다. 경로는 Debug 꼴(따옴표와
/// 이스케이프)로 적어 서로 다른 AI가 같은 글이 되지 않는다 (검토: 줄바꿈으로 이어 붙여, 경로에 줄바꿈이 든 다른 AI와
/// 같은 글이 될 수 있었다)
fn repl_owner(client: Option<&Client>) -> String {
    client.map_or_else(String::new, |client| {
        format!("{:?}", (&client.exe, &client.script, client.admin))
    })
}

/// 데몬을 잡고 메서드 하나를 처리한다. `client`는 요청한 AI의 이름이다
fn locked(
    agent: &Mutex<Agent>,
    policy: Policy,
    client: &str,
    method: &str,
    params: Value,
) -> Result<Value, Failure> {
    let mut agent = agent
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    agent.policy = policy;
    agent.client = client.to_string();
    async_io::block_on(agent.dispatch(method, params))
}

/// 정책을 거쳐 데몬의 메서드를 부르는 길 (bunt_agent::desk). 기다리기와 REPL은 호출 하나 안에서 이것을 여러 번
/// 부른다: 부를 때마다 정책을 다시 읽어 클라이언트를 받고(설정에서 끈 AI는 다음 호출부터 거절된다), 데몬을 잡아
/// 화면 잠금과 막은 앱을 본다(dispatch)
struct Gate<'a> {
    client: Option<&'a Client>,
    policy: &'a dyn Fn() -> Policy,
    dispatch: &'a dyn Fn(Policy, &str, Value) -> Result<Value, Failure>,
}

impl Desk for Gate<'_> {
    fn call(&self, method: &str, params: Value) -> Result<Value, Failure> {
        let policy = (self.policy)();
        admit(self.client, &policy)?;
        (self.dispatch)(policy, method, params)
    }
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
    /// 화면이 잠긴 동안에는 아무것도 읽거나 하지 않는다(ARCHITECTURE 6.5). 잠금을 푸는 것은 사람이 잠금 화면에 친
    /// 암호뿐이다. 처리하는 사이에 잠겼으면 읽은 것(트리, diff, 스크린샷)을 돌려주지 않는다
    async fn dispatch(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
        if self.screen_locked().await {
            return Err(screen_locked());
        }
        let result = self.run_method(method, params).await;
        if result.is_ok() && self.screen_locked().await {
            return Err(screen_locked());
        }
        result
    }

    /// 화면이 잠겼는지 컴포지터에 묻는다 (atspi::screen_locked). 세션 버스에 붙지 못하면 잠긴 것으로 본다:
    /// 앱의 트리는 접근성 버스로 읽으므로 세션 버스 없이도 읽힐 수 있다
    async fn screen_locked(&mut self) -> bool {
        if self.session.is_none() {
            self.session = zbus::Connection::session()
                .await
                .inspect_err(|err| tracing::warn!("no session bus: {err}"))
                .ok();
        }
        match &self.session {
            Some(session) => atspi::screen_locked(session).await,
            // 잠겼는지 모르면 거절한다
            None => true,
        }
    }

    async fn run_method(&mut self, method: &str, params: Value) -> Result<Value, Failure> {
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
                let acted = self.act_or_type(params).await?;
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

    /// 사람이 AI 제어를 멈췄으면 동작하지 않는다(읽기는 된다). 접근성 동작은 컴포지터를 거치지 않으므로 데몬이 먼저
    /// 본다. 멈춤 상태는 컴포지터가 갖는다. 옛 컴포지터에는 멈춤이 없다
    async fn check_paused(&self, a11y: &A11y) -> Result<(), Failure> {
        match a11y.agent_paused().await {
            Ok(false) => Ok(()),
            Ok(true) => Err(paused()),
            Err(err) if atspi::unknown_method(&err) => Ok(()),
            Err(err) => Err(agent_error(format!(
                "cannot tell whether the person paused AI control: {}",
                reason(&err)
            ))),
        }
    }

    /// 이 AI가 그 창을 읽거나 다루기 시작한다고 컴포지터에 알린다(메뉴 막대의 AI 표시). 알리지 못해도(옛
    /// 컴포지터) 동작은 한다
    async fn report(&self, a11y: &A11y, pid: u32, title: &str) {
        if let Err(err) = a11y.report_activity(&self.client, pid, title).await
            && !atspi::unknown_method(&err)
        {
            tracing::debug!("cannot report the agent activity: {err}");
        }
    }

    /// 이 세션에서 AI가 다룬 창으로 적는다. ref 없는 키·글·휠과 범위 없는 스냅샷이 이 창을 쓴다. 앱 ID와 레이어
    /// 표면인지는 컴포지터의 창 목록에서 읽는다
    async fn aim_at(
        &mut self,
        a11y: &A11y,
        session: &str,
        pid: u32,
        title: &str,
        app: &str,
        window: Option<(String, String)>,
    ) {
        let placed = a11y.windows().await.unwrap_or_default();
        let mine: Vec<&Placed> = placed.iter().filter(|window| window.pid == pid).collect();
        let found = mine
            .iter()
            .find(|window| window.title == title)
            .or(mine.last());
        let aim = Aim {
            pid,
            title: title.to_string(),
            app: app.to_string(),
            app_id: found.map_or_else(String::new, |window| window.app_id.clone()),
            window,
            // 그 pid의 창이 컴포지터에 없으면 셸의 레이어 표면이다 (창 목록을 읽지 못했으면 창으로 본다)
            layer: !placed.is_empty() && mine.is_empty(),
        };
        self.sessions.get(session).set_aim(aim);
    }

    /// ref도 `--app`도 없는 키·글·휠을 받을 창: 이 세션에서 AI가 마지막으로 다룬 창이 아직 키보드 포커스를 가졌을
    /// 때만 그 창이다. 사람이 포커스를 옮겼으면 FOCUS_MOVED로 거절한다(사람의 창에 치지 않는다, 설계 3장)
    async fn aimed_window(
        &mut self,
        a11y: &A11y,
        session: &str,
        apps: &[AppRef],
        placed: &[Placed],
        action: &str,
    ) -> Result<Chosen, Failure> {
        let Some(aim) = self.sessions.get(session).aim().cloned() else {
            return Err(no_aim(action));
        };
        let focus = a11y.focus().await;
        let Some(title) = aim_focused(&aim, focus.as_ref(), apps, placed) else {
            let installed = desktop::installed(&desktop::language());
            let now = focus_now(focus.as_ref(), apps, &installed);
            return Err(focus_moved(&aim, &now, action));
        };
        // 창 제목은 앱이 바꿀 수 있다(Firefox는 페이지 제목): 지금 제목으로 적어 둔다
        let aim = Aim { title, ..aim };
        self.sessions.get(session).set_aim(aim.clone());
        let tree = aim.window.as_ref().filter(|(bus, path)| {
            apps.iter()
                .any(|app| &app.bus == bus && app.windows.iter().any(|w| &w.path == path))
        });
        Ok(match tree {
            Some((bus, path)) => Chosen::Tree(Box::new(Target {
                bus: bus.clone(),
                app: aim.app,
                window: path.clone(),
                window_title: aim.title,
                item: None,
                parent: None,
                off_screen: false,
                web: false,
                drop_down: false,
            })),
            None => Chosen::Blind(aim.pid, aim.title, aim.app),
        })
    }

    /// 동작 하나. fill이 다시 읽기에서 글을 지키지 못했으면(Firefox 주소 막대는 SetTextContents에 참을 답하고
    /// 비어 있었다, 이슈 P3-1) 그 글 칸을 비우고 type의 길로 다시 넣는다. 둘 다 알린다
    async fn act_or_type(&mut self, params: ActParams) -> Result<Acted, Failure> {
        let retry = (params.action == "fill" && !params.target.is_empty()).then(|| {
            (
                params.session.clone(),
                params.target.clone(),
                params.value.clone().unwrap_or_default(),
                params.settle_ms,
            )
        });
        let failure = match self.act(params).await {
            Err(failure) if retry.is_some() && fill_not_kept(&failure) => failure,
            other => return other,
        };
        let Some((session, reference, text, settle_ms)) = retry else {
            return Err(failure);
        };
        let step = |action: &str, value: &str| ActParams {
            session: session.clone(),
            target: reference.clone(),
            action: action.into(),
            value: Some(value.into()),
            settle_ms,
            ..ActParams::default()
        };
        let typed = match self.act(step("press", "ctrl+a BackSpace")).await {
            Ok(_) => self.act(step("type", &text)).await,
            Err(failure) => Err(failure),
        };
        let acted = typed.map_err(|(code, message)| {
            (
                code,
                format!("{}; typing it instead failed too: {message}", failure.1),
            )
        })?;
        // type의 키보드 길은 다시 읽어 보지 않으므로 여기서 본다
        let name = session.unwrap_or_else(|| "default".into());
        let field = self
            .sessions
            .get(&name)
            .target(&reference)
            .map(|(bus, path)| (bus.to_string(), path.to_string()));
        if let Some((bus, path)) = field {
            let a11y = self.a11y().await?;
            let expect = Expect::Text(text.clone());
            if let Some(now) = a11y.text(&bus, &path).await
                && !expect.holds(&now)
            {
                return Err(agent_error(format!(
                    "{}; typing it instead did not keep it either: it reads {:?}",
                    failure.1,
                    shorten(&now, 80)
                )));
            }
        }
        Ok(Acted {
            done: format!(
                "{}; the app did not keep the text when filled, so it was cleared and typed \
                 instead",
                acted.done
            ),
            ..acted
        })
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

    /// 앱 목록. 사람만 답하는 창(관리자 인증, Wi-Fi 암호, Bluetooth 페어링), 잠금 화면, 셸의 화면 키보드는 빼서
    /// 스냅샷 범위에도 들지 않는다
    async fn list(&mut self, a11y: &A11y) -> Result<Vec<AppRef>, Failure> {
        let mut apps = a11y.apps().await.map_err(|err| {
            self.a11y = None;
            agent_error(format!("cannot list apps: {err}"))
        })?;
        apps.retain(|app| app.name != LOCK_WINDOW);
        for app in &mut apps {
            // 화면 키보드도 뺀다: 에이전트가 키보드의 키를 누르는 길을 열지 않는다
            app.windows.retain(|window| {
                !atspi::is_human_only(&window.title)
                    && window.title != LOCK_WINDOW
                    && window.title != KEYBOARD_WINDOW
            });
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
        // 범위가 AI가 다룬 창이 아니라 키보드 포커스의 창이면 그렇다고 트리 머리에 적는다
        let mut note: Option<String> = None;
        // 범위: ref, 앱, AI가 마지막으로 다룬 창, 활성 창 차례
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
                    // 읽기만 한 앱은 AI가 다룬 창(ref 없는 키·글·휠이 가는 곳)으로 적지 않는다: 사람이 쓰는 창을
                    // 읽은 뒤 ref 없이 친 글이 사람의 칸에 가지 않게 한다
                    (app.bus.clone(), roots, format!("app {}", app.bus))
                }
                None => {
                    if a11y.auth_dialog_focused().await {
                        return Err(auth_dialog());
                    }
                    // 기본 범위는 이 세션에서 AI가 마지막으로 다룬 창이다. 사람이 포커스를 옮겨도 AI가 읽는 창은 그대로다
                    let aim = self.sessions.get(&session_name).aim().cloned();
                    let aimed = aim.as_ref().and_then(|aim| {
                        let (bus, path) = aim.window.as_ref()?;
                        let app = apps.iter().find(|app| &app.bus == bus)?;
                        app.windows
                            .iter()
                            .any(|window| &window.path == path)
                            .then(|| (bus.clone(), path.clone()))
                    });
                    match aimed {
                        Some((bus, path)) => {
                            let scope = format!("window {bus} {path}");
                            (bus, vec![path], scope)
                        }
                        None => {
                            let focus = a11y.focus().await;
                            let Some((app, window)) = active_window(&apps, focus.as_ref()) else {
                                return Err(self.no_active_window(&a11y, focus.as_ref()).await);
                            };
                            note = Some(default_scope_note(aim.as_ref()));
                            let scope = format!("window {} {}", app.bus, window.path);
                            (app.bus.clone(), vec![window.path.clone()], scope)
                        }
                    }
                }
            }
        };

        // ref의 앱이 끝났으면 그렇다고 알린다: 기다리기(wait)는 이것을 사라진 것으로 읽는다. 검토: 접근성 버스의 오류로
        // 알려 ref로 사라지기를 기다리면 끝나지 않았다. 막은 앱 검사는 pid를 모르면 막은 앱으로 보므로 그보다 먼저 본다.
        // pid를 모르는 앱(샌드박스)일 수도 있어 트리도 읽어 본다
        let app_quit = |reference: &str| {
            agent_error(format!(
                "{reference} is gone: {}; take a new snapshot",
                protocol::APP_QUIT
            ))
        };
        if let Some(reference) = &params.scope
            && a11y.pid(&bus).await.is_none()
            && a11y.tree(&bus).await.is_err()
        {
            return Err(app_quit(reference));
        }
        self.check_app(&a11y, &bus).await?;

        let options = Options {
            interactive: params.interactive,
            hidden: params.hidden,
        };
        let rendered = match self
            .render(&a11y, &session_name, &bus, &roots, options)
            .await
        {
            Ok(rendered) => rendered,
            Err(failure) => match &params.scope {
                // 읽는 사이에 끝났다
                Some(reference) if a11y.pid(&bus).await.is_none() => {
                    return Err(app_quit(reference));
                }
                _ => return Err(failure),
            },
        };
        // 읽는 것도 알린다: 메뉴 막대의 AI 표시는 AI가 어느 앱을 읽고 다루는지 보인다(설계 6장)
        if let Some(pid) = a11y.pid(&bus).await {
            let title = rendered
                .windows
                .iter()
                .find(|(path, _)| roots.contains(path))
                .or(rendered.windows.first())
                .map(|(_, title)| title.clone())
                .unwrap_or_default();
            self.report(&a11y, pid, &title).await;
        }
        let key = scope_key(&scope, options);
        // 기다리기(wait)가 다시 읽는 것(peek)은 지난 트리를 바꾸지 않는다: 다음 스냅샷의 diff는 AI가 지난번에 본
        // 트리와 견준 것이어야 한다
        let previous = if params.peek {
            None
        } else {
            self.sessions
                .get(&session_name)
                .remember(key, &rendered.text)
        };
        let diff = previous
            .as_deref()
            .map(|old| {
                diff::limited(
                    old,
                    &rendered.text,
                    params.max_lines.unwrap_or(diff::MAX_LINES),
                )
            })
            .unwrap_or_default();
        tracing::info!(
            items = rendered.items,
            lines = rendered.lines,
            diff = diff.lines().count(),
            "snapshot of {bus}"
        );
        // 알림 줄은 diff에 들지 않게 견준 뒤에 붙인다
        let tree = match note {
            Some(note) => format!("{note}\n{}", rendered.text),
            None => rendered.text,
        };
        Ok(Snapshot {
            tree,
            diff,
            first: previous.is_none() && !params.peek,
            busy: rendered.busy,
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
        text.push_str(&app_header(app_name, pid));
        text.push('\n');
        let session = self.sessions.get(session);
        // 이 버스의 ref는 이 프로세스의 것이다: 앱이 끝나거나 다시 뜨면 옛 ref를 쓰지 않는다(TARGET_GONE)
        if let Some(pid) = pid {
            session.bind(bus, pid);
        }
        let root_paths: Vec<&str> = roots.iter().map(String::as_str).collect();
        let lines = tree::render(&tree, &root_paths, &options, &mut |path: &str| {
            session.ref_for(bus, path)
        });
        for line in &lines {
            text.push_str(&one_line(line));
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
        // 앱을 열거나 앞으로 가져오는 것도 동작이다: 사람이 멈췄으면 하지 않는다
        self.check_paused(&a11y).await?;
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
            // 사람이 다른 창을 쓰는 중이면 컴포지터가 활성화를 거절한다: 오류가 아니라 뒤에 둔 채로 알린다
            let background = match a11y.activate_window(open.pid, &open.title).await {
                Ok(_) => false,
                Err(err) if matches!(atspi::declined(&err), Some(Declined::UserActive { .. })) => {
                    true
                }
                Err(err) if atspi::declined(&err).is_some() => {
                    return Err(refused(&err, &format!("bringing {} forward", app.name)));
                }
                Err(err) => {
                    return Err(agent_error(format!(
                        "cannot bring {} forward: {err}",
                        app.name
                    )));
                }
            };
            self.report(&a11y, open.pid, &open.title).await;
            let running = self.list(&a11y).await?;
            let window = accessible_window(&running, open);
            // 다른 답처럼 접근성 버스의 앱 이름으로 적는다 (없으면 설치된 앱의 이름)
            let name = app_label(open.pid, &open.app_id, &running, std::slice::from_ref(&app));
            self.aim_at(
                &a11y,
                &session,
                open.pid,
                &open.title,
                &name,
                window.clone(),
            )
            .await;
            let tree = match window {
                Some((bus, path)) => self.window_tree(&a11y, &session, &bus, &path).await?,
                None => String::new(),
            };
            return Ok(Launched {
                name: app.name,
                id: app.id,
                window: Some(open.title.clone()),
                running: true,
                tree: in_background(background, tree),
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
            // 사람이 치는 중이면 컴포지터가 새 창에 키보드 포커스를 주지 않는다(협조적 활성화). 그러면 뒤에 연 것이다
            let background = a11y
                .focus()
                .await
                .is_some_and(|focus| focus.pid != open.pid);
            let Some((bus, path)) = accessible_window(&running, open) else {
                // 터미널(foot)처럼 접근성을 지원하지 않는 앱은 창만 알린다. 창이 늦게 뜬 것이 아니다
                let since = *appeared.get_or_insert_with(Instant::now);
                if since.elapsed() >= LAUNCH_TREE_WAIT {
                    self.launching.remove(&app.id);
                    let (pid, title) = (open.pid, open.title.clone());
                    self.report(&a11y, pid, &title).await;
                    self.aim_at(&a11y, &session, pid, &title, &app.name, None)
                        .await;
                    return Ok(Launched {
                        name: app.name,
                        id: app.id,
                        window: Some(title),
                        running: false,
                        tree: in_background(background, String::new()),
                    });
                }
                continue;
            };
            self.check_app(&a11y, &bus).await?;
            let (pid, title) = (open.pid, open.title.clone());
            self.report(&a11y, pid, &title).await;
            let name = app_label(pid, &open.app_id, &running, std::slice::from_ref(&app));
            let window = Some((bus.clone(), path.clone()));
            self.aim_at(&a11y, &session, pid, &title, &name, window)
                .await;
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
                window: Some(title),
                running: false,
                tree: in_background(background, tree),
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
        // ref는 그 앱의 프로세스에 묶여 있다. 앱이 끝났거나 다시 떴으면 버스 이름의 프로세스가 달라(없어)진다: 옛
        // ref로 다른 것을 누르지 않는다. 끝난 앱의 버스를 읽다가 접근성 연결을 버리지도 않는다
        if let Some(bound) = self.sessions.get(session).pid_of(&bus)
            && a11y.pid(&bus).await != Some(bound)
        {
            return Err(target_gone(reference, bound));
        }
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
        if atspi::is_human_only(&window_title) {
            return Err(auth_dialog());
        }
        if window_title == LOCK_WINDOW {
            return Err(screen_locked());
        }
        // 화면 키보드의 키는 사람만 누른다
        if window_title == KEYBOARD_WINDOW {
            return Err(on_screen_keyboard());
        }
        Ok(Target {
            bus,
            app: tree
                .get(ROOT)
                .map_or_else(String::new, |app| app.name.clone()),
            window,
            window_title,
            parent: tree.get(&target.parent).cloned(),
            item: Some(target.clone()),
            off_screen: tree.off_screen(target),
            web: tree.in_web_page(target),
            drop_down: tree.in_drop_down(target),
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
        // 사람이 AI 제어를 멈췄으면 어떤 동작도 하지 않는다. 접근성 동작은 컴포지터를 거치지 않으므로 여기서 본다
        self.check_paused(&a11y).await?;
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
            // 키와 휠은 `--app`의 창이나 이 세션에서 AI가 마지막으로 다룬 창(아직 키보드 포커스를 가졌을 때)으로 간다
            if a11y.auth_dialog_focused().await {
                return Err(auth_dialog());
            }
            match self.window_for(&a11y, &session, &params).await? {
                Chosen::Tree(target) => *target,
                Chosen::Blind(pid, title, app) => {
                    return self.act_blind(&a11y, &params, pid, &title, &app).await;
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
            app,
            window,
            window_title,
            item,
            parent,
            off_screen,
            web,
            drop_down,
        } = found;
        let pid = a11y.pid(&bus).await;
        if let Some(pid) = pid {
            // ref의 창은 이 세션에서 AI가 다룬 창이다 (ref 없는 키·글·휠과 범위 없는 스냅샷이 쓴다)
            if !params.target.is_empty() {
                let window = Some((bus.clone(), window.clone()));
                self.aim_at(&a11y, &session, pid, &window_title, &app, window)
                    .await;
            }
            self.report(&a11y, pid, &window_title).await;
            // 사람에게 AI가 다루는 요소를 보인다 (포인터로 누르면 AI 포인터가 보이므로 뺀다)
            if let Some(target) = item.as_ref().filter(|_| !params.pointer) {
                highlight(&a11y, pid, &window_title, &bus, &target.path).await;
            }
        }

        let roots = [window.clone()];
        let before = self.render(&a11y, &session, &bus, &roots, FULL).await?;
        // 키·글·포인터를 넣은 길 (답의 첫 줄에 적는다)
        let mut via = Via::default();
        // 화면 밖 요소는 사람처럼 먼저 보이게 스크롤한다 (휠만 돌릴 scroll은 그 자리에서 돌린다)
        let wheel_only = params.action == "scroll" && params.amount.is_some();
        let scrolled = match item.as_ref().filter(|_| off_screen && !wheel_only) {
            Some(target) => {
                let element = AppElement {
                    a11y: &a11y,
                    bus: &bus,
                    path: &target.path,
                    window_title: &window_title,
                    layer: RefCell::default(),
                    window: RefCell::default(),
                    via: Cell::default(),
                };
                let scrolled = scroll_into_view(&element)
                    .await
                    .map_err(|(code, message)| (code, format!("{}: {message}", params.target)))?;
                via.add(element.via.get());
                Some(scrolled)
            }
            None => None,
        };
        let process = || pid.ok_or_else(|| agent_error("cannot tell which process owns this app"));
        let (done, expect) = match (params.action.as_str(), &item) {
            ("scroll", Some(_)) if !wheel_only => (
                scrolled
                    .clone()
                    .unwrap_or_else(|| "it is already on screen".into()),
                None,
            ),
            ("scroll", target) => {
                let over = target.as_ref().map(|target| (bus.as_str(), target));
                (
                    wheel(
                        &a11y,
                        process()?,
                        &window_title,
                        over,
                        params.amount.unwrap_or_default(),
                        &mut via,
                    )
                    .await?,
                    None,
                )
            }
            ("press" | "type", _) => {
                let to = Recipient {
                    bus: &bus,
                    pid: process()?,
                    title: &window_title,
                };
                press(&a11y, &to, item.as_ref(), web, &params, &mut via).await?
            }
            ("click", Some(target)) if params.pointer => (
                pointer_click(&a11y, &bus, &window_title, target, &mut via).await?,
                None,
            ),
            ("focus", Some(target)) => (
                focus_element(&a11y, &bus, &window_title, target, &mut via).await?,
                None,
            ),
            (_, Some(target)) => {
                // 드롭다운의 열린 목록 항목은 사람처럼 포인터로 누른다 (perform)
                let picking = drop_down && params.action == "click";
                let done = match perform(
                    &a11y,
                    &bus,
                    target,
                    parent.as_ref(),
                    web,
                    drop_down,
                    &params,
                )
                .await?
                {
                    Some(done) => {
                        via.accessibility = true;
                        done
                    }
                    // 접근성 동작이 없는 요소는 사람처럼 포인터로 누른다
                    None => format!(
                        "{}; {}",
                        pointer_click(&a11y, &bus, &window_title, target, &mut via).await?,
                        if picking {
                            "drop-down list items are clicked with the pointer"
                        } else {
                            "it has no accessible action"
                        }
                    ),
                };
                // 앱이 참이라고 답하고도 글을 버리는 일이 있어(Firefox 주소 막대) 멈춘 뒤 다시 읽어 본다
                let expect = (params.action == "fill")
                    .then(|| Expect::Text(params.value.clone().unwrap_or_default()));
                (done, expect)
            }
            (_, None) => unreachable!("only press, type and scroll work without a ref"),
        };
        let done = match scrolled.filter(|_| params.action != "scroll") {
            Some(scrolled) => format!("{scrolled}, then {done}"),
            None => done,
        };
        // 답의 첫 줄: 받은 앱·창과 쓴 길(접근성 / 대상 확인 입력)
        let mut done = via.describe(&app, &window_title, &done);

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
            // 동작이 앱을 끝냈다 (닫기, 문서 버리기)
            Err(_) if app_gone(&a11y, &bus).await => {
                return Ok(quit_after(&bus, &done, &window_title, 0));
            }
            Err(failure) => return Err(failure),
        };
        // 앱은 마지막으로 읽은 뒤에 끝날 수 있다: 답을 돌려주기 전에 다시 본다(이슈 #1 P1-1. Firefox가 settle의
        // 상한 직전에 끝났는데 답은 끝나기 전의 diff를 주었다)
        if app_gone(&a11y, &bus).await {
            return Ok(quit_after(&bus, &done, &window_title, settled.ms));
        }
        let acted = self.finish(
            &session,
            &bus,
            &window,
            &before,
            settled,
            done.clone(),
            params.max_lines,
        );
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

    /// 동작의 답: 동작 전과 멈춘 뒤의 diff(`max_lines`보다 많이 바뀌었으면 요약), 새로 뜬 창. 다음 스냅샷의 diff는
    /// 이 뒤부터다
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        session: &str,
        bus: &str,
        window: &str,
        before: &Rendered,
        settled: Settled,
        done: String,
        max_lines: Option<usize>,
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
        let diff = diff::limited(
            &before.text,
            &after.text,
            max_lines.unwrap_or(diff::MAX_LINES),
        );
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

    /// ref 없는 동작(press, type, scroll)을 받을 창.
    /// - `--app`이 있으면 그 앱의 창(트리가 있으면 활성 창이나 첫 창). 키와 글은 그 창이 키보드 포커스를 가져야
    ///   들어가므로 앞으로 가져온다(사람이 쓰는 중이면 컴포지터가 거절한다). 휠은 컴포지터가 그 점에서 창을 보므로
    ///   가져오지 않는다. 셸(레이어 표면만 있는 앱)은 그 표면이 키보드 포커스를 가졌을 때 그 표면이다
    /// - 없으면 이 세션에서 AI가 마지막으로 다룬 창이 아직 키보드 포커스를 가졌을 때만 그 창이다(aimed_window)
    async fn window_for(
        &mut self,
        a11y: &A11y,
        session: &str,
        params: &ActParams,
    ) -> Result<Chosen, Failure> {
        let apps = self.list(a11y).await?;
        let placed = a11y.windows().await.unwrap_or_else(|err| {
            tracing::warn!("cannot read the window list: {err}");
            Vec::new()
        });
        let Some(query) = params.app.as_deref() else {
            return self
                .aimed_window(a11y, session, &apps, &placed, &params.action)
                .await;
        };
        let keyboard = params.action != "scroll";
        let installed = desktop::installed(&desktop::language());
        if let Ok(app) = find_app(&apps, &placed, &installed, query)
            && let Some(mut window) = app
                .windows
                .iter()
                .find(|window| window.states & state::ACTIVE != 0)
                .or_else(|| app.windows.first())
        {
            let pid = app
                .pid
                .ok_or_else(|| agent_error("cannot tell which process owns this app"))?;
            let mut title = window.title.clone();
            // 레이어 표면만 있는 앱(셸): 그 앱의 표면이 키보드 포커스를 가졌으면(열린 메뉴, 모든 앱 패널) 그 표면이다.
            // 아니면 press가 그 표면이 키보드를 받지 않는다고 알린다
            let layer = !placed.is_empty() && !placed.iter().any(|placed| placed.pid == pid);
            if layer {
                if let Some(focus) = a11y
                    .focus()
                    .await
                    .filter(|focus| focus.layer && focus.pid == pid)
                {
                    if let Some(focused) = app.windows.iter().find(|w| w.title == focus.app_id) {
                        window = focused;
                    }
                    title = focus.app_id;
                }
            } else if keyboard {
                activate(a11y, pid, &title).await?;
            }
            let target = Target {
                bus: app.bus.clone(),
                app: app.name.clone(),
                window: window.path.clone(),
                window_title: title,
                item: None,
                parent: None,
                off_screen: false,
                web: false,
                drop_down: false,
            };
            let accessible = Some((target.bus.clone(), target.window.clone()));
            self.aim_at(
                a11y,
                session,
                pid,
                &target.window_title,
                &target.app,
                accessible,
            )
            .await;
            return Ok(Chosen::Tree(Box::new(target)));
        }
        // 트리가 없는 창 (접근성을 켜기 전에 뜬 Firefox, foot 등)
        let window = find_window(&placed, &apps, &installed, query)?.clone();
        if keyboard {
            activate(a11y, window.pid, &window.title).await?;
        }
        let app = app_label(window.pid, &window.app_id, &apps, &installed);
        self.aim_at(a11y, session, window.pid, &window.title, &app, None)
            .await;
        Ok(Chosen::Blind(window.pid, window.title, app))
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

    /// 접근성 트리가 없는 창에 키·글·휠을 보낸다: 컴포지터가 그 창을 확인하고 넣는다(window_for가 `--app`의 창은
    /// 앞으로 가져왔고, 아니면 그 창이 키보드 포커스를 가진 것을 보았다). diff는 없다
    async fn act_blind(
        &mut self,
        a11y: &A11y,
        params: &ActParams,
        pid: u32,
        title: &str,
        app: &str,
    ) -> Result<Acted, Failure> {
        if self.blocked_pids(a11y).await?.contains(&pid) {
            return Err(blocked_app());
        }
        self.report(a11y, pid, title).await;
        let mut via = Via::default();
        let done = if params.action == "scroll" {
            let amount = params.amount.unwrap_or_default();
            wheel(a11y, pid, title, None, amount, &mut via).await?
        } else {
            let to = Recipient {
                bus: "",
                pid,
                title,
            };
            press(a11y, &to, None, false, params, &mut via).await?.0
        };
        let done = via.describe(app, title, &done);
        Ok(Acted {
            done: format!("{done}; it has no accessibility tree (take a screenshot to see it)"),
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
        let surface = self.target_surface(a11y, params.app.as_deref()).await?;
        let window = surface.placed();
        let running = self.list(a11y).await?;
        let tree = accessible_window(&running, &window);
        // 누르는 창은 이 세션에서 AI가 다룬 창이다
        let app = app_label(window.pid, &window.app_id, &running, &[]);
        self.report(a11y, window.pid, &window.title).await;
        self.aim_at(
            a11y,
            &session,
            window.pid,
            &window.title,
            &app,
            tree.clone(),
        )
        .await;
        let before = match &tree {
            Some((bus, path)) => {
                self.check_app(a11y, bus).await?;
                let roots = [path.clone()];
                Some(self.render(a11y, &session, bus, &roots, FULL).await?)
            }
            None => None,
        };
        let clicked = match &surface {
            Surface::Window(window) => a11y.click_window(window.pid, &window.title, x, y).await,
            // 셸 표면은 그 스크린샷(CaptureLayer)의 픽셀을 표면 기준 논리 좌표로 바꿔 누른다
            Surface::Layer(layer) => {
                let (left, top) = (x / layer.scale, y / layer.scale);
                if left >= f64::from(layer.width) || top >= f64::from(layer.height) {
                    return Err(agent_error(format!(
                        "the point is outside the surface ({:.0}x{:.0} pixels in its screenshot)",
                        f64::from(layer.width) * layer.scale,
                        f64::from(layer.height) * layer.scale
                    )));
                }
                a11y.click_layer(layer.pid, &layer.namespace, left, top)
                    .await
            }
        };
        // 사람이 포인터를 쓰는 중이거나 멈췄으면 컴포지터가 거절한다: 그 번호로 알리고 다시 누르지 않는다
        let (screen_x, screen_y) = clicked.map_err(|err| refused(&err, "the pointer click"))?;
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
        let settled = match self
            .settle(
                a11y,
                &session,
                &bus,
                &roots,
                &before.text,
                settle_limit(params.settle_ms),
            )
            .await
        {
            Ok(settled) => settled,
            // 누르기가 앱을 끝냈다 (act와 같다)
            Err(_) if app_gone(a11y, &bus).await => {
                return Ok(quit_after(&bus, &done, &window.title, 0));
            }
            Err(failure) => return Err(failure),
        };
        // 마지막으로 읽은 뒤에 끝났을 수도 있다 (act와 같다, 이슈 #1 P1-1)
        if app_gone(a11y, &bus).await {
            return Ok(quit_after(&bus, &done, &window.title, settled.ms));
        }
        let acted = self.finish(
            &session,
            &bus,
            &path,
            &before,
            settled,
            done,
            params.max_lines,
        );
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

    /// 스크린샷과 click --at이 쓰는 표면: target_window의 창, 없으면 레이어 이름이 `app`과 같은 셸의 레이어
    /// 표면(`bunt agent apps`의 bunt-shell 창 이름: bunt-dock, bunt-launcher 등). 셸 표면은 창 목록에 없어
    /// 찍거나 누르지 못했다
    async fn target_surface(&mut self, a11y: &A11y, app: Option<&str>) -> Result<Surface, Failure> {
        let not_found = match self.target_window(a11y, app).await {
            Ok(window) => return Ok(Surface::Window(window)),
            // 막은 앱이나 모호한 이름은 셸 표면으로 넘어가지 않고 그대로 알린다
            Err(err)
                if err.0 != protocol::AGENT_ERROR || err.1.contains(protocol::BE_MORE_SPECIFIC) =>
            {
                return Err(err);
            }
            Err(err) => err,
        };
        let Some(query) = app else {
            return Err(not_found);
        };
        let layers = a11y.layers().await.unwrap_or_default();
        layers
            .into_iter()
            .rev()
            // 사람만 답하는 창, 잠금 화면, 화면 키보드는 고르지 않는다 (choose_surface와 같다)
            .filter(|layer| {
                !atspi::is_human_only(&layer.namespace)
                    && layer.namespace != LOCK_WINDOW
                    && layer.namespace != KEYBOARD_WINDOW
            })
            .find(|layer| layer.namespace == query)
            .map(Surface::Layer)
            .ok_or(not_found)
    }

    /// 창 하나의 스크린샷을 PNG 파일로 쓴다 (ref가 없는 UI를 볼 때). 컴포지터가 그 창만 따로 그리므로
    /// 가려진 부분도 보이고, 다른 앱(막은 앱 포함)과 셸의 인증 창은 들지 않는다
    async fn screenshot(&mut self, params: ScreenshotParams) -> Result<Screenshot, Failure> {
        let a11y = self.a11y().await?;
        let surface = self.target_surface(&a11y, params.app.as_deref()).await?;
        let window = surface.placed();
        // 읽는 것도 알린다 (메뉴 막대의 AI 표시)
        self.report(&a11y, window.pid, &window.title).await;
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
        let captured = match &surface {
            Surface::Window(window) => a11y.capture_window(window.pid, &window.title).await,
            Surface::Layer(layer) => a11y.capture_layer(layer.pid, &layer.namespace).await,
        };
        let (width, height, scale, png) = captured.map_err(|err| {
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

/// 동작 뒤 앱이 끝났다는 답. 동작은 됐으므로 실패로 알리지 않는다: 실패로 보이면 AI가 같은 동작을 되풀이할 수
/// 있다. 그 앱에 보낸 최근 접근성 호출을 경고로 남긴다(이슈 #1 P1-3): 어느 호출에 앱이 끝났는지 로그로 알 수 있다
fn quit_after(bus: &str, done: &str, window_title: &str, settled_ms: u64) -> Acted {
    atspi::dump_calls(
        Some(bus),
        &format!("{bus} quit during an action on its window {window_title:?}"),
    );
    Acted {
        done: format!("{done}; then the app quit (window {window_title:?} closed)"),
        diff: String::new(),
        settled_ms,
        settled: true,
        observed: false,
        new_windows: Vec::new(),
    }
}

/// 앱이 끝났는지: 그 접근성 버스 이름의 주인이 없다. 고유 이름(:1.33)은 다시 쓰이지 않으므로 다시 뜬 앱도 끝난
/// 것으로 본다
async fn app_gone(a11y: &A11y, bus: &str) -> bool {
    a11y.pid(bus).await.is_none()
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

/// 연 앱의 창이 사람이 쓰는 창 뒤에 남았다는 알림 (launch의 트리 머리)
const OPENED_IN_BACKGROUND: &str =
    "# note: opened in the background; the person is using another window";

/// launch의 트리: 뒤에 열었으면 그렇다고 머리에 적는다. 앞으로 가져오지 않았으니 키·글은 --app으로 그 창을 고르거나
/// ref(접근성)로 다룬다
fn in_background(background: bool, tree: String) -> String {
    if background {
        format!("{OPENED_IN_BACKGROUND}\n{tree}")
    } else {
        tree
    }
}

/// 범위 없는 스냅샷이 AI가 다룬 창 대신 키보드 포커스의 창을 읽을 때 머리에 적는 줄
fn default_scope_note(aim: Option<&Aim>) -> String {
    match aim {
        None => "# note: this session has not worked on a window yet, so this is the window that \
                 has the keyboard focus"
            .into(),
        Some(aim) if aim.window.is_none() => format!(
            "# note: the window you worked on last ({} {:?}) has no accessibility tree, so this is \
             the window that has the keyboard focus",
            aim.app, aim.title
        ),
        Some(aim) => format!(
            "# note: the window you worked on last ({} {:?}) is gone, so this is the window that \
             has the keyboard focus",
            aim.app, aim.title
        ),
    }
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

/// 동작 하나. 접근성 동작을 먼저 쓰고, 목록 항목은 목록에서 고른다. 드롭다운의 열린 목록 항목(`drop_down`)은
/// 누르기 이름의 동작이 없으면 목록에서 고르거나 하나뿐인 동작을 돌리지 않고 None을 준다(포인터로 누른다)
async fn perform(
    a11y: &A11y,
    bus: &str,
    target: &Item,
    parent: Option<&Item>,
    web: bool,
    drop_down: bool,
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
            // GtkDropDown은 항목을 누를 때 값을 바꾼다. 목록에서 고르기(접근성 Selection)나 항목의 하나뿐인
            // 동작(listitem.scroll-to)으로는 값이 그대로여서, 시스템 설정의 드롭다운에서 고른 항목이 들지 않았다
            if drop_down {
                return Ok(None);
            }
            if let Some((list, index)) = selectable {
                a11y.select_child(bus, list, index)
                    .await
                    .map_err(|err| failed("selecting", err))?;
                return Ok(Some("selected it in its list".into()));
            }
            // 누르기 이름이 아닌 동작은 그것 하나뿐일 때만 쓴다. 여럿이면(동작 묶음을 단 GTK 위젯은 그 묶음의
            // 동작을 모두 내보낸다) 무엇이 일어날지 몰라 사람처럼 포인터로 누른다
            if let [name] = actions.as_slice() {
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

/// 키·글·휠을 받을 창: 접근성 버스 이름(트리가 없는 창이면 빈 글), 창을 가진 프로세스, 창 제목(셸의 레이어
/// 표면이면 레이어 이름)
struct Recipient<'a> {
    bus: &'a str,
    pid: u32,
    title: &'a str,
}

/// 키·글·포인터를 넣은 길. 답의 첫 줄에 적어 AI가 어느 길로 무엇이 갔는지 안다
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Via {
    /// 접근성(AT-SPI)의 동작·글·값: 키보드 포커스와 포인터를 쓰지 않는다
    accessibility: bool,
    /// 컴포지터가 대상 창을 확인하고 넣은 키·글·포인터(PressKeysOn 등, ClickWindow·ClickLayer)
    checked: bool,
    /// 대상을 확인하지 않는 옛 메서드로 넣었다 (셸의 레이어 표면, 옛 컴포지터)
    unchecked: bool,
    /// 컴포지터에 대상 확인 메서드가 없었다 (옛 컴포지터)
    old_compositor: bool,
}

/// 옛 컴포지터로 키·글·포인터를 넣었을 때 답에 붙이는 경고 (계약 C)
const UNCHECKED_WARNING: &str =
    "the compositor cannot check the target; the input may reach another window";

impl Via {
    fn add(&mut self, other: Via) {
        self.accessibility |= other.accessibility;
        self.checked |= other.checked;
        self.unchecked |= other.unchecked;
        self.old_compositor |= other.old_compositor;
    }

    /// 옛 메서드로 넣었다. `old_compositor`면 컴포지터가 새 메서드를 몰랐다
    fn fell_back(&mut self, old_compositor: bool) {
        self.unchecked = true;
        self.old_compositor |= old_compositor;
    }

    fn label(self) -> String {
        let mut ways = Vec::new();
        if self.accessibility {
            ways.push("accessibility");
        }
        if self.checked {
            ways.push("target-checked input");
        }
        if self.unchecked {
            ways.push("unchecked input");
        }
        if ways.is_empty() {
            ways.push("no input");
        }
        ways.join(" and ")
    }

    /// 답의 첫 줄: `텍스트 편집기 "새 문서" (accessibility): inserted 5 characters …`
    fn describe(self, app: &str, title: &str, done: &str) -> String {
        let warning = if self.old_compositor {
            format!("; warning: {UNCHECKED_WARNING}")
        } else {
            String::new()
        };
        let who = if app.is_empty() {
            format!("{title:?}")
        } else {
            format!("{app} {title:?}")
        };
        format!("{who} ({}): {done}{warning}", self.label())
    }
}

/// 브라우저 툴킷 (접근성 ToolkitName). 이 앱의 글 칸(주소 막대, 웹 칸)은 키보드 길로 넣는다
const BROWSER_TOOLKITS: [&str; 2] = ["Gecko", "Chromium"];

fn is_browser(toolkit: &str) -> bool {
    BROWSER_TOOLKITS
        .iter()
        .any(|browser| browser.eq_ignore_ascii_case(toolkit))
}

/// 포커스 없이 접근성으로 글을 넣고 지울 글 칸: 편집할 수 있고 웹 페이지와 브라우저 밖에 있다. 웹 칸과 브라우저
/// 주소 막대는 앱이 접근성 글을 버리거나(Firefox 주소 막대는 SetTextContents에 참을 답하고 비어 있었다) 페이지가
/// 키 이벤트를 기다리므로 키보드 길로 간다. 암호 칸도 키보드 길이다
fn accessible_field(item: &Item, web: bool, toolkit: &str) -> bool {
    item.interfaces & iface::EDITABLE_TEXT != 0
        && item.role != role::PASSWORD_TEXT
        && !web
        && !is_browser(toolkit)
}

/// "모두 고르고 지우기" 키 (ctrl+a BackSpace, ctrl+a Delete. ⌘A도): 글 칸의 글 지우기로 대신한다
fn clears_text(keys: &[String]) -> bool {
    let all = |key: &str| {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "ctrl+a" | "control+a" | "super+a" | "cmd+a" | "meta+a"
        )
    };
    let delete = |key: &str| {
        matches!(
            key.to_ascii_lowercase().as_str(),
            "backspace" | "delete" | "del"
        )
    };
    matches!(keys, [first, second] if all(first) && delete(second))
}

/// Return 하나: 요소의 activate 동작으로 대신한다
fn enter_key(keys: &[String]) -> bool {
    matches!(keys, [key] if matches!(key.to_ascii_lowercase().as_str(), "return" | "enter" | "kp_enter"))
}

/// 접근성으로 대신할 수 있는 키를 한다: 글 칸의 "모두 고르고 지우기"는 글 지우기(EditableText.DeleteText)로, Return은
/// 요소의 activate 동작으로. 키보드 포커스를 쓰지 않는다. 대신할 수 없으면(앱이 거절해도) None: 키보드 길로 간다
async fn accessible_keys(
    a11y: &A11y,
    bus: &str,
    target: &Item,
    keys: &[String],
    field: bool,
    browser: bool,
) -> Option<String> {
    if field && clears_text(keys) {
        match a11y.delete_all_text(bus, &target.path).await {
            Ok(count) => return Some(format!("deleted its {count} characters")),
            Err(err) => tracing::debug!("cannot delete the text accessibly: {err}"),
        }
    }
    // 브라우저의 activate 동작은 Return과 달리 칸에 포커스를 줄 뿐이다
    if !browser && enter_key(keys) {
        let actions = a11y.actions(bus, &target.path, false).await;
        if let Some(index) = actions
            .iter()
            .position(|name| name.eq_ignore_ascii_case("activate"))
            && matches!(
                a11y.do_action(bus, &target.path, index as i32).await,
                Ok(true)
            )
        {
            return Some("ran its accessible action \"activate\"".into());
        }
    }
    None
}

/// 키를 누르거나(press) 글을 친다(type).
///
/// ref의 글 칸(웹 페이지와 브라우저 밖)은 포커스 없이 접근성으로 다룬다(설계 3장 1층): 글은 끝에 넣고
/// (EditableText.InsertText), "모두 고르고 지우기"는 글 지우기로, Return은 activate 동작으로 한다. 사람이 다른 창에
/// 치는 동안에도 섞이지 않고 포커스를 옮기지 않는다. 앞에는 포인터로 눌러 포커스를 주어 사람의 커서를 옮기고 글
/// 순서를 엉키게 했다(시험 10b).
///
/// 나머지는 키보드 길이다: ref가 있으면 그 창을 앞으로 가져와(사람이 쓰는 중이면 컴포지터가 거절한다) 그 요소에
/// 키보드 포커스를 준 뒤, 컴포지터가 그 창이 아직 키보드 포커스를 가졌는지 보고 넣는다(대상 확인 입력). 사람이
/// 포커스를 옮겼으면 FOCUS_MOVED로 알리고 되빼앗지 않는다. `keep_focus`면 창만 앞으로 가져온다.
///
/// 글은 앱의 글 입력(text-input)에 확정한다. 글 입력이 켜지지 않으면 ref의 글 칸에는 접근성으로 커서
/// 자리에 넣고, 그것도 안 되면 `keys`일 때만 글자마다 키를 친다. 몰래 키로 바꿔 치면 포커스가 글 칸을
/// 벗어났을 때 키가 문서로 가 사이트 단축키가 돌고, 입력기가 한글 모드면 글이 바뀐다.
/// 접근성으로 넣었으면 멈춘 뒤 다시 읽어 볼 글을 함께 돌려준다
async fn press(
    a11y: &A11y,
    to: &Recipient<'_>,
    target: Option<&Item>,
    web: bool,
    params: &ActParams,
    via: &mut Via,
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
    let count = text.chars().count();
    // 앱이 접근성으로 넣은 글을 받지 않아 키보드 길로 친다
    let mut refused_insert = false;
    if let Some(target) = target.filter(|_| !params.keep_focus) {
        let toolkit = a11y.toolkit(to.bus).await.unwrap_or_default();
        let field = accessible_field(target, web, &toolkit);
        if typing && field {
            match a11y.insert_at_end(to.bus, &target.path, text).await {
                Ok(true) => {
                    via.accessibility = true;
                    return Ok((
                        format!("inserted {count} characters at the end of the text"),
                        Some(Expect::Contains(text.to_string())),
                    ));
                }
                Ok(false) => refused_insert = true,
                Err(err) => {
                    tracing::debug!("the accessible insert failed: {err}");
                    refused_insert = true;
                }
            }
        }
        if !typing {
            let browser = web || is_browser(&toolkit);
            if let Some(done) = accessible_keys(a11y, to.bus, target, &keys, field, browser).await {
                via.accessibility = true;
                return Ok((format!("{done} (for {})", keys.join(" ")), None));
            }
        }
    }
    // 셸의 레이어 표면(메뉴 막대, 모든 앱 패널 등)은 앞으로 가져올 창이 없다. 레이어 표면이면 가져오지 않고, 그
    // 표면이 키보드를 가졌는지 본다
    let surface = compositor_surface(a11y, to.pid, to.title).await?;
    if let Some(target) = target {
        if let Surface::Window(window) = &surface {
            let activated = a11y
                .activate_window(to.pid, &window.title)
                .await
                .map_err(|err| refused(&err, "activating the window"))?;
            if !activated {
                return Err(agent_error("the compositor has no window for this app"));
            }
        }
        if !params.keep_focus {
            focus_element(a11y, to.bus, to.title, target, via)
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
    // 메뉴 막대와 Dock은 키보드를 받지 않는다. 그대로 치면 키가 포커스를 가진 다른 앱으로 간다
    if let Surface::Layer(layer) = &surface
        && !layer_has_keyboard(a11y.focus().await.as_ref(), layer)
    {
        return Err(agent_error(format!(
            "{} of the shell does not have the keyboard focus, so nothing was {}; click the \
             element instead, or open its menu first",
            layer.namespace,
            if typing { "typed" } else { "pressed" }
        )));
    }
    if !typing {
        send_keys(a11y, to.pid, &surface, &keys, via).await?;
        return Ok((format!("pressed {}", keys.join(" ")), None));
    }
    let refused_note = if refused_insert {
        "the app refused the accessible insert, so "
    } else {
        ""
    };
    if !wait_for_text_input(a11y).await {
        if let Some(target) = target
            .filter(|target| !params.keep_focus && target.interfaces & iface::EDITABLE_TEXT != 0)
            && !refused_insert
            && a11y
                .insert_text(to.bus, &target.path, text)
                .await
                .unwrap_or(false)
        {
            via.accessibility = true;
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
        let how = send_typed(a11y, to.pid, &surface, text, via).await?;
        let how = if how == "text-input" {
            "through the app's text input"
        } else {
            "as keys"
        };
        return Ok((
            format!("{refused_note}typed {count} characters {how}"),
            None,
        ));
    }
    // 글 입력이 그새 꺼졌으면 컴포지터는 아무것도 치지 않고 거짓을 준다
    if !send_committed(a11y, to.pid, &surface, text, via).await? {
        return Err(nothing_typed(target.map(|_| params.target.as_str())));
    }
    Ok((
        format!("{refused_note}typed {count} characters through the app's text input"),
        None,
    ))
}

/// 키를 그 창에 누른다. 컴포지터가 그 창이 키보드 포커스를 가졌는지 같은 디스패치에서 보고 넣는다(PressKeysOn).
/// 셸의 레이어 표면은 대상을 받는 메서드가 없어 키보드 포커스에 누른다(press가 먼저 그 표면이 포커스를 가졌는지
/// 본다). 옛 컴포지터도 그렇게 넣고 경고를 붙인다
async fn send_keys(
    a11y: &A11y,
    pid: u32,
    surface: &Surface,
    keys: &[String],
    via: &mut Via,
) -> Result<(), Failure> {
    const WHAT: &str = "pressing keys";
    let old = match surface {
        Surface::Window(window) => match a11y.press_keys_on(pid, &window.title, keys).await {
            Ok(()) => {
                via.checked = true;
                return Ok(());
            }
            Err(err) if atspi::unknown_method(&err) => true,
            Err(err) => return Err(refused(&err, WHAT)),
        },
        Surface::Layer(_) => false,
    };
    a11y.press_keys(keys)
        .await
        .map_err(|err| refused(&err, WHAT))?;
    via.fell_back(old);
    Ok(())
}

/// 그 창의 켜진 글 입력에 글을 확정한다 (CommitTextOn. 레이어 표면과 옛 컴포지터는 send_keys처럼)
async fn send_committed(
    a11y: &A11y,
    pid: u32,
    surface: &Surface,
    text: &str,
    via: &mut Via,
) -> Result<bool, Failure> {
    const WHAT: &str = "typing";
    let old = match surface {
        Surface::Window(window) => match a11y.commit_text_on(pid, &window.title, text).await {
            Ok(committed) => {
                via.checked = true;
                return Ok(committed);
            }
            Err(err) if atspi::unknown_method(&err) => true,
            Err(err) => return Err(refused(&err, WHAT)),
        },
        Surface::Layer(_) => false,
    };
    let committed = a11y
        .commit_text(text)
        .await
        .map_err(|err| refused(&err, WHAT))?;
    via.fell_back(old);
    Ok(committed)
}

/// 그 창에 글을 친다: 글 입력이 켜졌으면 확정하고 아니면 글자마다 키를 친다 (TypeTextOn. `--keys`)
async fn send_typed(
    a11y: &A11y,
    pid: u32,
    surface: &Surface,
    text: &str,
    via: &mut Via,
) -> Result<String, Failure> {
    const WHAT: &str = "typing";
    let old = match surface {
        Surface::Window(window) => match a11y.type_text_on(pid, &window.title, text).await {
            Ok(how) => {
                via.checked = true;
                return Ok(how);
            }
            Err(err) if atspi::unknown_method(&err) => true,
            Err(err) => return Err(refused(&err, WHAT)),
        },
        Surface::Layer(_) => false,
    };
    let how = a11y
        .type_text(text)
        .await
        .map_err(|err| refused(&err, WHAT))?;
    via.fell_back(old);
    Ok(how)
}

/// 접근성 동작의 대상 요소를 사람에게 AI 색 테두리로 잠깐 보인다 (ShowAgentTarget). 자리를 모르거나 옛 컴포지터면
/// 건너뛴다
async fn highlight(a11y: &A11y, pid: u32, title: &str, bus: &str, path: &str) {
    let Some(bounds) = a11y.bounds(bus, path).await else {
        return;
    };
    if let Err(err) = a11y.show_target(pid, title, bounds).await
        && !atspi::unknown_method(&err)
    {
        tracing::debug!("cannot show the agent target: {err}");
    }
}

/// ref 없는 키·글·휠을 받을 수 있는지: AI가 마지막으로 다룬 창(`aim`)이 지금 키보드 포커스를 가졌다. 맞으면 그
/// 창의 지금 제목(컴포지터 창 제목, 셸의 레이어 표면이면 그 이름)을 준다. 컴포지터가 넣을 때 다시 본다(R1)
fn aim_focused(
    aim: &Aim,
    focus: Option<&Focus>,
    apps: &[AppRef],
    placed: &[Placed],
) -> Option<String> {
    let focus = focus?;
    if aim.layer || focus.layer {
        return (aim.layer && focus.layer && focus.pid == aim.pid && focus.app_id == aim.title)
            .then(|| aim.title.clone());
    }
    if focus.pid != aim.pid {
        return None;
    }
    // 접근성 창으로 본다: 그 앱에서 활성인 창이 AI의 창이어야 한다. 제목이 바뀌어도(Firefox) 경로는 그대로다
    if let Some((bus, path)) = &aim.window
        && let Some(app) = apps.iter().find(|app| &app.bus == bus)
        && let Some(active) = app
            .windows
            .iter()
            .find(|window| window.states & state::ACTIVE != 0)
    {
        return (&active.path == path).then(|| focus.title.clone());
    }
    // 트리가 없거나 활성 창을 모르면 제목으로 본다. 그 앱의 창이 하나뿐이면 제목이 바뀐 것이다
    let windows = placed.iter().filter(|window| window.pid == aim.pid).count();
    (focus.title == aim.title || windows <= 1).then(|| focus.title.clone())
}

/// fill이 다시 읽기에서 글을 지키지 못한 오류인지 (Expect::failure의 글)
const FILL_NOT_KEPT: &str = "did not keep the text";

fn fill_not_kept(failure: &Failure) -> bool {
    failure.0 == protocol::AGENT_ERROR && failure.1.contains(FILL_NOT_KEPT)
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

/// 요소에 키보드 포커스를 준다(`focus`, 키보드 길의 press·type). GTK4는 접근성의 GrabFocus를 지원하지
/// 않는다(NotSupported). 그래서 글 칸은 사람처럼 포인터로 눌러 포커스를 주고 커서를 글 끝으로 옮긴다: 컴포지터가 그
/// 점의 맨 위가 그 창인지 보고, 사람이 쓰는 중이면 거절한다(ClickOn). 다른 요소는 누르면 동작이 일어나므로 누르지
/// 않는다. 글 칸의 type --ref는 이 길을 쓰지 않고 접근성으로 넣는다
async fn focus_element(
    a11y: &A11y,
    bus: &str,
    window_title: &str,
    target: &Item,
    via: &mut Via,
) -> Result<String, Failure> {
    if target.states & state::FOCUSED != 0 {
        return Ok("it already has the keyboard focus".into());
    }
    if matches!(a11y.grab_focus(bus, &target.path).await, Ok(true)) {
        via.accessibility = true;
        return Ok("moved the keyboard focus to it".into());
    }
    if target.interfaces & iface::EDITABLE_TEXT == 0 {
        return Err(agent_error(
            "the app cannot move the keyboard focus to this element; click it if that is safe",
        ));
    }
    pointer_click(a11y, bus, window_title, target, via).await?;
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

/// 접근성 동작이 없는 요소의 가운데를 포인터로 누른다. 자리는 앱이 알린 창 안 자리(창 기준 논리 좌표)이고,
/// 컴포지터가 그 점의 맨 위가 그 창인지 같은 디스패치에서 보고 누른다(ClickOn: 가려 있으면 누르지 않고, 앞으로
/// 가져와야 하면 사람이 쓰는 중인지 본다). 옛 컴포지터에서는 창을 앞으로 가져와 화면 좌표를 누르고 경고를 붙인다.
/// 셸의 레이어 표면(메뉴 막대, Dock, 모든 앱 패널과 그 열린 메뉴)은 컴포지터가 표면 기준 자리를 받아 누른다
async fn pointer_click(
    a11y: &A11y,
    bus: &str,
    window_title: &str,
    target: &Item,
    via: &mut Via,
) -> Result<String, Failure> {
    let pid = a11y
        .pid(bus)
        .await
        .ok_or_else(|| agent_error("cannot tell which process owns this app"))?;
    // 창만 찾던 compositor_window 대신 셸의 레이어 표면도 찾는다. 셸 단추(로고 메뉴, 시계, Dock 아이콘)가
    // "the compositor has no window for this app"으로 실패했다
    let surface = compositor_surface(a11y, pid, window_title).await?;
    let (x, y, width, height) = a11y
        .extents(bus, &target.path)
        .await
        .map_err(|err| agent_error(format!("cannot tell where the element is: {err}")))?;
    if width <= 0 || height <= 0 {
        return Err(agent_error("the element has no size on screen"));
    }
    // 요소 가운데의 창(표면) 기준 좌표
    let center = (
        f64::from(x) + f64::from(width) / 2.0,
        f64::from(y) + f64::from(height) / 2.0,
    );
    const WHAT: &str = "the pointer click";
    let at = match surface {
        Surface::Window(window) => {
            match a11y.click_on(pid, &window.title, center.0, center.1).await {
                Ok(at) => {
                    via.checked = true;
                    at
                }
                Err(err) if atspi::unknown_method(&err) => {
                    // 옛 컴포지터: 다른 창에 가려 있으면 그 창을 누르게 되므로 먼저 앞으로 가져온다
                    activate(a11y, pid, &window.title).await?;
                    let at = (
                        f64::from(window.x) + center.0,
                        f64::from(window.y) + center.1,
                    );
                    a11y.click(at.0, at.1)
                        .await
                        .map_err(|err| refused(&err, WHAT))?;
                    via.fell_back(true);
                    at
                }
                Err(err) => return Err(refused(&err, WHAT)),
            }
        }
        // 레이어 표면은 늘 제 층에 있어 앞으로 가져올 것이 없다. 컴포지터가 그 점에서 포인터가 실제로 이
        // 표면이나 그 열린 메뉴에 가는지 보고 누른다: 자동으로 숨은 막대 자리를 누르면 아래 창이 눌린다
        Surface::Layer(layer) => {
            let at = a11y
                .click_layer(pid, &layer.namespace, center.0, center.1)
                .await
                .map_err(|err| refused(&err, WHAT))?;
            via.checked = true;
            at
        }
    };
    Ok(format!(
        "clicked with the pointer at ({:.0}, {:.0})",
        at.0, at.1
    ))
}

/// 접근성 창이 컴포지터에서 놓인 것: 앱의 창이거나 셸의 레이어 표면
enum Surface {
    Window(Placed),
    Layer(atspi::Layer),
}

impl Surface {
    /// 창처럼 다룰 때의 자리(전역 논리 좌표). 셸은 레이어 창의 접근성 이름을 레이어 이름으로 두므로, 레이어
    /// 표면의 제목은 레이어 이름이다
    fn placed(&self) -> Placed {
        match self {
            Surface::Window(window) => window.clone(),
            Surface::Layer(layer) => Placed {
                pid: layer.pid,
                app_id: layer.namespace.clone(),
                title: layer.namespace.clone(),
                x: layer.x,
                y: layer.y,
                width: layer.width,
                height: layer.height,
                focused: false,
            },
        }
    }
}

/// 컴포지터가 놓은 그 pid의 표면 (choose_surface). 사람만 답하는 창과 잠금 화면은 거절한다
async fn compositor_surface(a11y: &A11y, pid: u32, title: &str) -> Result<Surface, Failure> {
    if atspi::is_human_only(title) {
        return Err(auth_dialog());
    }
    if title == LOCK_WINDOW {
        return Err(screen_locked());
    }
    // 화면 키보드의 키는 사람만 누른다
    if title == KEYBOARD_WINDOW {
        return Err(on_screen_keyboard());
    }
    let windows = a11y
        .windows()
        .await
        .map_err(|err| agent_error(format!("cannot read the window list: {err}")))?;
    // GetLayers가 없는 옛 컴포지터에서는 창만 찾는다
    let layers = a11y.layers().await.unwrap_or_else(|err| {
        tracing::warn!("cannot read the shell surfaces: {err}");
        Vec::new()
    });
    choose_surface(&windows, &layers, pid, title)
        .ok_or_else(|| agent_error("the compositor has no window for this app"))
}

/// 그 pid의 표면: 제목이 같은 창, 이름이 같은 레이어 표면, 없으면 맨 위 창. 셸은 레이어 창의 접근성
/// 이름을 레이어 이름으로 두므로, 셸에 창(다시 시작 확인 창 등)이 떠 있어도 메뉴 막대는 메뉴 막대로
/// 찾는다. 사람만 답하는 창과 잠금 화면의 레이어 표면은 고르지 않는다(컴포지터도 알리지 않는다)
fn choose_surface(
    windows: &[Placed],
    layers: &[atspi::Layer],
    pid: u32,
    title: &str,
) -> Option<Surface> {
    let of_pid = || windows.iter().rev().filter(|window| window.pid == pid);
    of_pid()
        .find(|window| window.title == title)
        .map(|window| Surface::Window(window.clone()))
        .or_else(|| {
            layers
                .iter()
                .rev()
                // 화면 키보드도 고르지 않는다 (컴포지터도 알리지 않는다)
                .filter(|layer| {
                    !atspi::is_human_only(&layer.namespace)
                        && layer.namespace != LOCK_WINDOW
                        && layer.namespace != KEYBOARD_WINDOW
                })
                .find(|layer| layer.pid == pid && layer.namespace == title)
                .map(|layer| Surface::Layer(layer.clone()))
        })
        .or_else(|| {
            of_pid()
                .next()
                .map(|window| Surface::Window(window.clone()))
        })
}

/// 키보드 포커스가 그 레이어 표면(이나 그 열린 메뉴)에 있는지. 컴포지터는 팝업의 포커스를 그 뿌리
/// 표면의 레이어 이름으로 알린다
fn layer_has_keyboard(focus: Option<&Focus>, layer: &atspi::Layer) -> bool {
    focus.is_some_and(|focus| {
        focus.layer && focus.pid == layer.pid && focus.app_id == layer.namespace
    })
}

/// 창을 앞으로 가져와 키보드 포커스를 준다. 사람이 쓰는 중이면 컴포지터가 거절하고(USER_ACTIVE), 데몬은 다시 해
/// 보지 않고 알린다. 이미 그 창에 포커스가 있으면 컴포지터는 아무것도 바꾸지 않는다
async fn activate(a11y: &A11y, pid: u32, title: &str) -> Result<(), Failure> {
    a11y.activate_window(pid, title)
        .await
        .map_err(|err| refused(&err, "activating the window"))?;
    Ok(())
}

/// 휠을 돌린다: 요소가 있으면 그 위에서, 없으면 창 가운데에서. 눈금 수는 (가로, 세로)이고
/// 오른쪽·아래가 +다. 휠은 포인터 아래의 스크롤 영역을 움직이므로 요소는 스크롤할 목록이나 문서다.
/// 컴포지터가 그 점의 맨 위가 그 창인지 보고 돌린다(ScrollOn). 옛 컴포지터에서는 창을 앞으로 가져와 돌린다
async fn wheel(
    a11y: &A11y,
    pid: u32,
    window_title: &str,
    over: Option<(&str, &Item)>,
    [dx, dy]: [f64; 2],
    via: &mut Via,
) -> Result<String, Failure> {
    // 셸의 레이어 표면(모든 앱 패널 등)은 창 목록에 없어, 창만 찾던 이 자리에서 "the compositor has no window
    // for this app"으로 실패했다. 레이어 표면이면 컴포지터가 표면 기준 자리에서 돌린다(ScrollLayer)
    let surface = compositor_surface(a11y, pid, window_title).await?;
    let placed = surface.placed();
    let (width, height) = (f64::from(placed.width), f64::from(placed.height));
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
    const WHAT: &str = "scrolling";
    match &surface {
        Surface::Window(window) => match a11y.scroll_on(pid, &window.title, x, y, dx, dy).await {
            Ok(()) => via.checked = true,
            Err(err) if atspi::unknown_method(&err) => {
                activate(a11y, pid, &window.title).await?;
                a11y.scroll(f64::from(window.x) + x, f64::from(window.y) + y, dx, dy)
                    .await
                    .map_err(|err| refused(&err, WHAT))?;
                via.fell_back(true);
            }
            Err(err) => return Err(refused(&err, WHAT)),
        },
        Surface::Layer(layer) => {
            a11y.scroll_layer(pid, &layer.namespace, x, y, dx, dy)
                .await
                .map_err(|err| refused(&err, WHAT))?;
            via.checked = true;
        }
    }
    let at = (f64::from(placed.x) + x, f64::from(placed.y) + y);
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

/// 화면 밖 요소를 보이게 스크롤할 때 쓰는 것: 요소의 자리와 보임 상태를 읽고, 앱에 스크롤을 부탁하거나 그
/// 창에서 휠을 돌린다. 시계도 여기서 읽어, 시험은 스크롤을 늦게 고치는 가짜 앱과 가짜 시계로 바꾼다
trait Scrollable {
    /// 지금 시각
    fn now(&self) -> Instant;
    /// `duration`만큼 쉰다
    async fn pause(&self, duration: Duration);
    /// 요소의 자리와 크기 (창 기준 논리 좌표: x, y, 너비, 높이)
    async fn extents(&self) -> zbus::Result<(i32, i32, i32, i32)>;
    /// 요소가 화면에 보이는가 (SHOWING)
    async fn shown(&self) -> bool;
    /// 앱이 요소가 보이게 스스로 스크롤한다 (Component.ScrollTo)
    async fn scroll_to(&self) -> zbus::Result<bool>;
    /// 요소가 있는 창이 컴포지터에서 놓인 자리를 준다. 휠은 포인터 아래의 창으로 가므로 컴포지터가 그 점의 맨 위가
    /// 그 창인지 보고 돌린다(옛 컴포지터에서는 돌리기 전에 앞으로 가져온다)
    async fn raise_window(&self) -> Result<Placed, Failure>;
    /// 화면의 논리 좌표 (x, y)에서 휠을 돌린다. 눈금 수 dx, dy는 오른쪽·아래가 +다
    async fn wheel(&self, x: f64, y: f64, dx: f64, dy: f64) -> zbus::Result<()>;
}

/// 접근성 버스에 있는 앱의 요소와 그 창
struct AppElement<'a> {
    a11y: &'a A11y,
    bus: &'a str,
    path: &'a str,
    window_title: &'a str,
    /// raise_window가 찾은 셸의 레이어 표면. 휠은 그 표면 기준 자리에서 돌린다(ScrollLayer)
    layer: RefCell<Option<atspi::Layer>>,
    /// raise_window가 찾은 창. 휠은 그 창 기준 자리에서 돌린다(ScrollOn)
    window: RefCell<Option<Placed>>,
    /// 스크롤에 쓴 길 (답의 첫 줄)
    via: Cell<Via>,
}

impl AppElement<'_> {
    fn used(&self, how: impl FnOnce(&mut Via)) {
        let mut via = self.via.get();
        how(&mut via);
        self.via.set(via);
    }
}

impl Scrollable for AppElement<'_> {
    fn now(&self) -> Instant {
        Instant::now()
    }

    async fn pause(&self, duration: Duration) {
        async_io::Timer::after(duration).await;
    }

    async fn extents(&self) -> zbus::Result<(i32, i32, i32, i32)> {
        self.a11y.extents(self.bus, self.path).await
    }

    async fn shown(&self) -> bool {
        self.a11y.states(self.bus, self.path).await & state::SHOWING != 0
    }

    async fn scroll_to(&self) -> zbus::Result<bool> {
        let asked = self.a11y.scroll_to(self.bus, self.path).await;
        if matches!(asked, Ok(true)) {
            self.used(|via| via.accessibility = true);
        }
        asked
    }

    async fn raise_window(&self) -> Result<Placed, Failure> {
        let pid = self
            .a11y
            .pid(self.bus)
            .await
            .ok_or_else(|| agent_error("cannot tell which process owns this app"))?;
        // 셸의 레이어 표면(모든 앱 패널 등)도 찾는다. 창만 찾던 때는 "the compositor has no window for this
        // app"으로 실패했다. 창을 앞으로 가져오지 않는다: 컴포지터가 휠을 돌릴 때 그 점에서 창을 확인한다
        let surface = compositor_surface(self.a11y, pid, self.window_title).await?;
        match &surface {
            Surface::Window(window) => {
                self.window.replace(Some(window.clone()));
            }
            Surface::Layer(layer) => {
                self.layer.replace(Some(layer.clone()));
            }
        }
        Ok(surface.placed())
    }

    async fn wheel(&self, x: f64, y: f64, dx: f64, dy: f64) -> zbus::Result<()> {
        let layer = self.layer.borrow().clone();
        let window = self.window.borrow().clone();
        match (layer, window) {
            // 화면 좌표를 표면 기준 자리로 바꾼다
            (Some(layer), _) => {
                let (x, y) = (x - f64::from(layer.x), y - f64::from(layer.y));
                self.a11y
                    .scroll_layer(layer.pid, &layer.namespace, x, y, dx, dy)
                    .await?;
                self.used(|via| via.checked = true);
                Ok(())
            }
            // 화면 좌표를 창 기준 자리로 바꾼다
            (None, Some(window)) => {
                let (left, top) = (x - f64::from(window.x), y - f64::from(window.y));
                match self
                    .a11y
                    .scroll_on(window.pid, &window.title, left, top, dx, dy)
                    .await
                {
                    Ok(()) => {
                        self.used(|via| via.checked = true);
                        Ok(())
                    }
                    // 옛 컴포지터: 앞으로 가져와 화면 좌표에서 돌린다
                    Err(err) if atspi::unknown_method(&err) => {
                        self.a11y.activate_window(window.pid, &window.title).await?;
                        self.a11y.scroll(x, y, dx, dy).await?;
                        self.used(|via| via.fell_back(true));
                        Ok(())
                    }
                    Err(err) => Err(err),
                }
            }
            (None, None) => self.a11y.scroll(x, y, dx, dy).await,
        }
    }
}

/// 화면 밖 요소를 보이게 스크롤한다. 앱이 스스로 하면(Component.ScrollTo, Firefox) 그것을 쓰고, 못
/// 하거나(GTK4는 지원하지 않는다) SHOWN_WAIT까지 보이지 않으면 휠을 돌린다. 바쁜 앱은 자리와 보임 상태를
/// 늦게 고치므로 기다리되, 모두 SCROLL_LIMIT 안에 마친다
async fn scroll_into_view(element: &impl Scrollable) -> Result<String, Failure> {
    let deadline = element.now() + SCROLL_LIMIT;
    // Firefox는 웹 페이지 요소의 자리를 받아 둔 뒤에야 ScrollTo 뒤의 보임 상태(SHOWING)를 고친다
    let _ = element.extents().await;
    let asked = matches!(element.scroll_to().await, Ok(true));
    if asked && wait_until_shown(element, (element.now() + SHOWN_WAIT).min(deadline)).await {
        return Ok("scrolled it into view".into());
    }
    match wheel_into_view(element, deadline).await? {
        // 앱이 ScrollTo를 SHOWN_WAIT보다 늦게 고쳤고 휠은 돌리지 않았다
        Some(0) if asked => Ok("scrolled it into view".into()),
        Some(0) => Ok("it came into view".into()),
        // 전에는 돌린 휠 수가 아니라 자리를 읽은 수를 "(1 turns)"처럼 알렸다
        Some(turns) => {
            let unit = if turns == 1 { "turn" } else { "turns" };
            Ok(format!(
                "scrolled it into view with the wheel ({turns} {unit})"
            ))
        }
        None => Err(agent_error(
            "it is off screen and scrolling did not bring it into view; scroll the list or page \
             that holds it (bunt agent scroll down --ref <list>) and take a new snapshot",
        )),
    }
}

/// 요소가 있는 쪽으로 창 가운데에서 휠을 돌리며 요소가 창 안에 들어와 보일 때까지 다시 본다. 보이면 돌린
/// 휠 수를 준다. 요소가 SCROLL_STUCK 동안 움직이지 않거나, 창 안에서 SHOWN_WAIT까지 보이지 않거나,
/// SCROLL_TURNS를 다 돌렸거나 `deadline`이 지나면 None이다
async fn wheel_into_view(
    element: &impl Scrollable,
    deadline: Instant,
) -> Result<Option<usize>, Failure> {
    if element.now() >= deadline {
        return Ok(None);
    }
    let window = element.raise_window().await?;
    let center = (
        f64::from(window.x) + f64::from(window.width) / 2.0,
        f64::from(window.y) + f64::from(window.height) / 2.0,
    );
    let mut turns = 0;
    // 마지막으로 휠을 돌린 때와 그때 요소의 자리
    let mut turned: Option<(Instant, (i32, i32))> = None;
    // 요소가 자리로는 창 안에 든 때. 그 뒤로 보임 상태를 기다린다
    let mut inside: Option<Instant> = None;
    while element.now() < deadline {
        let (x, y, width, height) = element
            .extents()
            .await
            .map_err(|err| agent_error(format!("cannot tell where it is: {err}")))?;
        let (dx, dy) = (
            toward(x, width, window.width),
            toward(y, height, window.height),
        );
        if dx == 0.0 && dy == 0.0 {
            if element.shown().await {
                return Ok(Some(turns));
            }
            // 1초만 기다리던 것을 늘렸다: 바쁜 앱은 자리를 고친 뒤에도 보임 상태를 늦게 고친다. SHOWN_WAIT가
            // 지나도 보이지 않으면 창 안의 다른 영역에 가려 있다
            let since = *inside.get_or_insert_with(|| element.now());
            if element.now().saturating_duration_since(since) >= SHOWN_WAIT {
                return Ok(None);
            }
            element.pause(SETTLE_STEP).await;
            continue;
        }
        inside = None;
        // 자리를 한 번 다시 읽어 그대로면 끝에 닿았다고 보던 것을 고쳤다: 바쁜 앱은 앞의 휠을 늦게 고친다.
        // 그동안 휠을 더 돌리면 앱이 한꺼번에 고쳐 지나치므로, 돌리지 않고 SCROLL_STUCK까지 다시 본다
        if let Some((at, _)) = turned.filter(|(_, last)| *last == (x, y)) {
            if element.now().saturating_duration_since(at) >= SCROLL_STUCK {
                // 더 움직이지 않는다: 끝에 닿았거나 휠로 움직이는 영역이 아니다
                return Ok(None);
            }
            element.pause(SCROLL_PAUSE).await;
            continue;
        }
        if turns == SCROLL_TURNS {
            return Ok(None);
        }
        element
            .wheel(center.0, center.1, dx, dy)
            .await
            .map_err(|err| refused(&err, "scrolling"))?;
        turns += 1;
        turned = Some((element.now(), (x, y)));
        element.pause(SCROLL_PAUSE).await;
    }
    Ok(None)
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

/// 요소가 보일 때까지(SHOWING) `until`까지 기다린다
async fn wait_until_shown(element: &impl Scrollable, until: Instant) -> bool {
    loop {
        if element.shown().await {
            return true;
        }
        if element.now() >= until {
            return false;
        }
        element.pause(SETTLE_STEP).await;
        // 보임 상태만 다시 읽던 것에 자리 읽기를 더했다: Firefox는 자리를 받아 둔 뒤에야 보임 상태를 고치고,
        // 바쁘면 ScrollTo 뒤의 자리도 늦게 고친다
        let _ = element.extents().await;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use bunt_access::AllowedClient;

    use super::*;

    fn claude() -> Client {
        Client {
            name: "claude".into(),
            exe: PathBuf::from("/usr/bin/claude"),
            script: None,
            admin: false,
        }
    }

    /// claude가 목록에 있는 정책. 끈 AI는 거절하며 요청 기록(사용자 파일)에 남기지 않는다
    fn policy_with(enabled: bool) -> Policy {
        Policy {
            clients: vec![AllowedClient {
                name: "claude".into(),
                exe: PathBuf::from("/usr/bin/claude"),
                exe_pattern: None,
                script: None,
                script_pattern: None,
                enabled,
            }],
            blocked_apps: Vec::new(),
        }
    }

    #[test]
    fn every_call_through_the_gate_is_admitted_again() {
        let client = claude();
        // 사용자가 세 번째 읽기부터 이 AI를 끈다
        let loads = Cell::new(0);
        let policy = || {
            loads.set(loads.get() + 1);
            policy_with(loads.get() < 3)
        };
        let dispatched = RefCell::new(Vec::new());
        let dispatch = |_policy: Policy, method: &str, _params: Value| {
            dispatched.borrow_mut().push(method.to_string());
            Err((protocol::SCREEN_LOCKED, "locked".to_string()))
        };
        let gate = Gate {
            client: Some(&client),
            policy: &policy,
            dispatch: &dispatch,
        };
        // 데몬(dispatch)이 잠금을 보고 거절한다
        assert_eq!(
            gate.call("snapshot", Value::Null).unwrap_err().0,
            protocol::SCREEN_LOCKED
        );
        assert_eq!(
            gate.call("apps", Value::Null).unwrap_err().0,
            protocol::SCREEN_LOCKED
        );
        // 끈 뒤의 호출은 데몬에 닿지 않는다
        let refused = gate.call("apps", Value::Null).unwrap_err();
        assert_eq!(refused.0, protocol::NOT_ALLOWED);
        assert!(refused.1.contains("turned off"), "{}", refused.1);
        assert_eq!(*dispatched.borrow(), ["snapshot", "apps"]);
        // 알아보지 못한 클라이언트는 처음부터 거절한다
        let unknown = Gate {
            client: None,
            policy: &policy,
            dispatch: &dispatch,
        };
        assert_eq!(
            unknown.call("apps", Value::Null).unwrap_err().0,
            protocol::NOT_ALLOWED
        );
        assert_eq!(dispatched.borrow().len(), 2);
    }

    #[test]
    fn waiting_stops_when_the_user_turns_the_ai_off() {
        let client = claude();
        let loads = Cell::new(0);
        let policy = || {
            loads.set(loads.get() + 1);
            policy_with(loads.get() < 3)
        };
        let dispatch = |_policy: Policy, method: &str, params: Value| {
            assert_eq!((method, params["peek"].as_bool()), ("snapshot", Some(true)));
            Ok(json!({ "tree": "- window \"w\" [ref=e1]\n", "diff": "", "first": false }))
        };
        let gate = Gate {
            client: Some(&client),
            policy: &policy,
            dispatch: &dispatch,
        };
        let params = WaitParams {
            text: Some("never".into()),
            timeout_ms: Some(5_000),
            ..WaitParams::default()
        };
        let started = Instant::now();
        let refused = wait::run(&gate, &params).unwrap_err();
        assert_eq!(refused.0, protocol::NOT_ALLOWED);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    fn repl_request(code: &str) -> Request {
        Request {
            jsonrpc: "2.0".into(),
            id: json!(7),
            method: "repl".into(),
            params: json!({ "code": code, "timeout_ms": 5_000 }),
        }
    }

    #[test]
    fn the_repl_runs_only_for_allowed_ais() {
        let repls = Repls::new(Box::new(Worker::in_thread));
        let client = claude();
        let dispatch = |_: Policy, method: &str, _: Value| -> Result<Value, Failure> {
            panic!("{method} reached the daemon")
        };
        // 꺼 둔 AI와 알아보지 못한 클라이언트는 JS를 돌리지 않는다 (REPL을 띄우지도 않는다)
        let off = || policy_with(false);
        for client in [Some(&client), None] {
            let refused = answer(
                repl_request("while (true) {}"),
                client,
                &off,
                &dispatch,
                &repls,
            );
            assert_eq!(refused.id, json!(7));
            assert_eq!(refused.error.unwrap().code, protocol::NOT_ALLOWED);
        }
        assert!(repls.is_empty());
        // 기다리기도 같다
        let wait = Request {
            method: "wait".into(),
            params: json!({ "text": "x" }),
            ..repl_request("")
        };
        let refused = answer(wait, Some(&client), &off, &dispatch, &repls);
        assert_eq!(refused.error.unwrap().code, protocol::NOT_ALLOWED);
    }

    #[test]
    fn every_desktop_call_of_a_repl_goes_through_the_policy() {
        let repls = Repls::new(Box::new(Worker::in_thread));
        let client = claude();
        // 1: 요청을 받을 때, 2: 첫 데스크톱 호출 (허용). 3부터: 사용자가 이 AI를 껐다
        let loads = Cell::new(0);
        let policy = || {
            loads.set(loads.get() + 1);
            policy_with(loads.get() <= 2)
        };
        let reached = RefCell::new(Vec::new());
        // 데몬은 막은 앱과 잠긴 화면을 거절한다
        let dispatch = |_: Policy, method: &str, params: Value| -> Result<Value, Failure> {
            reached.borrow_mut().push(method.to_string());
            if params["app"] == json!("bank") {
                return Err(blocked_app());
            }
            Err(screen_locked())
        };
        let code = "const seen = [];\n\
                    for (const f of [() => desktop.app('bank').snapshot(), () => desktop.apps()]) {\n\
                      try { await f() } catch (e) { seen.push(e.name) }\n\
                    }\n\
                    seen";
        let ran = answer(
            repl_request(code),
            Some(&client),
            &policy,
            &dispatch,
            &repls,
        );
        let repl: protocol::Repl = serde_json::from_value(ran.result.unwrap()).unwrap();
        assert_eq!(repl.error, None);
        assert_eq!(
            repl.value,
            Some(json!(["BlockedAppError", "NotAllowedError"]))
        );
        assert_eq!(*reached.borrow(), ["snapshot"]);
        // 다시 켜면 같은 REPL이 이어진다 (변수가 남았다)
        let on = || policy_with(true);
        let again = answer(
            repl_request("seen.length"),
            Some(&client),
            &on,
            &dispatch,
            &repls,
        );
        let repl: protocol::Repl = serde_json::from_value(again.result.unwrap()).unwrap();
        assert_eq!((repl.output.as_str(), repl.fresh), ("2\n", false));
        // 잠긴 화면은 REPL 안에서도 거절된다
        let locked = answer(
            repl_request("await desktop.snapshot()"),
            Some(&client),
            &on,
            &dispatch,
            &repls,
        );
        let repl: protocol::Repl = serde_json::from_value(locked.result.unwrap()).unwrap();
        assert!(
            repl.error
                .as_deref()
                .unwrap()
                .starts_with("ScreenLockedError: the screen is locked"),
            "{:?}",
            repl.error
        );
    }

    #[test]
    fn repls_belong_to_one_ai() {
        let mut other = claude();
        other.script = Some(PathBuf::from("/opt/other/agent.js"));
        assert_ne!(repl_owner(Some(&claude())), repl_owner(Some(&other)));
        let mut admin = claude();
        admin.admin = true;
        assert_ne!(repl_owner(Some(&claude())), repl_owner(Some(&admin)));
        // 경로의 줄바꿈으로 실행 파일과 스크립트의 경계를 꾸미지 못한다
        let mut joined = claude();
        joined.exe = PathBuf::from("/a\n/b");
        joined.script = None;
        let mut split = claude();
        split.exe = PathBuf::from("/a");
        split.script = Some(PathBuf::from("/b\n"));
        assert_ne!(repl_owner(Some(&joined)), repl_owner(Some(&split)));
    }

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

    fn layer(pid: u32, namespace: &str) -> atspi::Layer {
        atspi::Layer {
            pid,
            namespace: namespace.into(),
            x: 0,
            y: 0,
            width: 1280,
            height: 24,
            scale: 1.0,
        }
    }

    /// 고른 표면: 창이면 ("window", 제목), 레이어 표면이면 ("layer", 이름)
    fn chosen(surface: Option<Surface>) -> Option<(&'static str, String)> {
        surface.map(|surface| match surface {
            Surface::Window(window) => ("window", window.title),
            Surface::Layer(layer) => ("layer", layer.namespace),
        })
    }

    #[test]
    fn shell_surfaces_are_found_by_their_layer_name() {
        // 셸(pid 10)에 다시 시작 확인 창이 떠 있고, 계산기(pid 20)가 있다
        let windows = [
            placed(10, "org.buntos.Shell", "Restart"),
            placed(20, "org.gnome.Calculator", "Calculator"),
        ];
        let layers = [
            layer(10, "bunt-menubar"),
            layer(10, "bunt-dock"),
            // 컴포지터는 알리지 않지만, 알려도 고르지 않는다
            layer(10, "bunt-polkit-agent"),
            layer(10, LOCK_WINDOW),
        ];
        let choose = |pid, title| chosen(choose_surface(&windows, &layers, pid, title));
        // 메뉴 막대와 Dock의 요소는 셸의 창이 아니라 그 레이어 표면을 누른다
        assert_eq!(
            choose(10, "bunt-menubar"),
            Some(("layer", "bunt-menubar".into()))
        );
        assert_eq!(choose(10, "bunt-dock"), Some(("layer", "bunt-dock".into())));
        // 제목이 같은 창이 먼저다. 접근성 이름과 창 제목이 다르면 맨 위 창이다(Firefox 등)
        assert_eq!(choose(10, "Restart"), Some(("window", "Restart".into())));
        assert_eq!(
            choose(20, "Calculator"),
            Some(("window", "Calculator".into()))
        );
        assert_eq!(
            choose(20, "Calculator — 1"),
            Some(("window", "Calculator".into()))
        );
        // 다른 앱(pid 20)의 이름으로는 셸 표면을 고르지 않는다
        assert_eq!(
            chosen(choose_surface(&windows[1..], &layers, 20, "bunt-dock")),
            Some(("window", "Calculator".into()))
        );
        assert_eq!(chosen(choose_surface(&[], &layers, 20, "bunt-dock")), None);
        // 사람만 답하는 창과 잠금 화면의 레이어 표면은 고르지 않는다
        assert_eq!(
            chosen(choose_surface(&[], &layers, 10, "bunt-polkit-agent")),
            None
        );
        assert_eq!(chosen(choose_surface(&[], &layers, 10, LOCK_WINDOW)), None);
        // 창도 레이어 표면도 없으면 없다
        assert_eq!(chosen(choose_surface(&[], &[], 10, "bunt-menubar")), None);
    }

    /// 셸의 레이어 표면은 창처럼 다룰 때(휠, 스크린샷, click --at) 레이어 이름을 제목으로 쓰고 컴포지터가 놓은
    /// 자리를 그대로 쓴다. 휠과 누르기는 그 자리를 빼 표면 기준 좌표로 보낸다
    #[test]
    fn a_shell_surface_is_placed_like_a_window() {
        let dock = atspi::Layer {
            x: 300,
            y: 740,
            width: 680,
            height: 60,
            scale: 1.25,
            ..layer(10, "bunt-dock")
        };
        let placed = Surface::Layer(dock).placed();
        assert_eq!(
            (placed.pid, placed.title.as_str(), placed.app_id.as_str()),
            (10, "bunt-dock", "bunt-dock")
        );
        assert_eq!(
            (placed.x, placed.y, placed.width, placed.height),
            (300, 740, 680, 60)
        );
        assert!(!placed.focused);
    }

    /// 셸의 화면 키보드는 사람만 누른다: 레이어 표면이 알려져도 고르지 않는다 (atspi::KEYBOARD_WINDOW)
    #[test]
    fn the_on_screen_keyboard_is_never_a_surface_to_press() {
        let layers = [layer(10, "bunt-dock"), layer(10, KEYBOARD_WINDOW)];
        assert_eq!(
            chosen(choose_surface(&[], &layers, 10, KEYBOARD_WINDOW)),
            None
        );
        assert_eq!(
            chosen(choose_surface(&[], &layers, 10, "bunt-dock")),
            Some(("layer", "bunt-dock".into()))
        );
        // 사람만 답하는 창이 아니므로 떠 있어도 에이전트의 다른 동작을 막지 않는다
        assert!(!atspi::is_human_only(KEYBOARD_WINDOW));
    }

    #[test]
    fn keys_go_to_a_shell_surface_only_while_it_has_the_keyboard() {
        let launcher = layer(10, "bunt-launcher");
        let layer_focus = |pid, namespace: &str| Focus {
            pid,
            app_id: namespace.into(),
            title: String::new(),
            layer: true,
        };
        // 모든 앱 패널(키보드 독점)이나 그 열린 메뉴가 포커스를 가졌다
        assert!(layer_has_keyboard(
            Some(&layer_focus(10, "bunt-launcher")),
            &launcher
        ));
        // 메뉴 막대·Dock처럼 키보드를 받지 않는 표면이면 포커스는 다른 곳에 있다
        assert!(!layer_has_keyboard(
            Some(&layer_focus(10, "bunt-menubar")),
            &launcher
        ));
        assert!(!layer_has_keyboard(
            Some(&focus(20, "Calculator")),
            &launcher
        ));
        // 다른 클라이언트의 같은 이름, 창 포커스, 포커스 없음
        assert!(!layer_has_keyboard(
            Some(&layer_focus(30, "bunt-launcher")),
            &launcher
        ));
        let window = Focus {
            layer: false,
            ..layer_focus(10, "bunt-launcher")
        };
        assert!(!layer_has_keyboard(Some(&window), &launcher));
        assert!(!layer_has_keyboard(None, &launcher));
    }

    #[test]
    fn app_names_and_attributes_cannot_add_lines() {
        // 앱이 정한 이름에 따옴표, pid, 줄바꿈과 요소 줄을 넣어도 머리줄 하나이고 pid는 데몬이 붙인 것이다
        let header = app_header(
            "Notes\" [pid=4242]\n- button \"Continue\" [ref=e7]",
            Some(812),
        );
        assert!(!header.contains('\n'), "{header}");
        assert_eq!(bunt_agent::outline::app_pid(&header), Some(812));
        assert!(bunt_agent::outline::parse(&header).is_empty());
        // 속성 값의 줄바꿈은 그 줄 안에 남는다: 가짜 요소 줄과 ref가 생기지 않는다
        let line = one_line("- heading \"x\" [level=1]\n- button \"Continue\" [ref=e7]");
        let parsed = bunt_agent::outline::parse(&line);
        assert_eq!(parsed.len(), 1, "{line}");
        assert_eq!(parsed[0].reference, None);
        assert_eq!(one_line("a\u{2028}b\rc"), "a b c");
    }

    #[test]
    fn app_errors_say_what_wait_reads() {
        // 기다리기(wait.rs)는 이 알림으로 앱이 없는 것과 이름이 여럿에 맞는 것을 가른다
        let apps = [
            app(20, "Text Editor", &[("a.txt", true)]),
            app(21, "Text Editor", &[("b.txt", false)]),
        ];
        // AppRef는 Debug가 없어 답을 버리고 오류만 본다
        let missing = find_app(&apps, &[], &[], "calculator")
            .map(drop)
            .unwrap_err()
            .1;
        assert!(missing.starts_with(protocol::NO_APP_MATCHES), "{missing}");
        let many = find_app(&apps, &[], &[], "text editor")
            .map(drop)
            .unwrap_err()
            .1;
        assert!(many.contains(protocol::BE_MORE_SPECIFIC), "{many}");
        let windows = [
            placed(30, "firefox-esr", "a"),
            placed(31, "org.gnome.TextEditor", "a"),
        ];
        let several = find_window(&windows, &[], &[], "a").unwrap_err().1;
        assert!(several.contains(protocol::BE_MORE_SPECIFIC), "{several}");
    }

    /// AI가 마지막으로 다룬 창: 텍스트 편집기(pid 40)의 첫 창
    fn editor_aim() -> Aim {
        Aim {
            pid: 40,
            title: "Untitled".into(),
            app: "Text Editor".into(),
            app_id: "org.gnome.TextEditor".into(),
            window: Some((":1.40".into(), "/w40/0".into())),
            layer: false,
        }
    }

    #[test]
    fn ref_less_input_goes_only_to_the_window_the_ai_worked_on() {
        let aim = editor_aim();
        let editor = [app(40, "Text Editor", &[("Untitled", true)])];
        let placed_editor = [placed(40, "org.gnome.TextEditor", "Untitled")];
        let check =
            |focus: Option<&Focus>, apps: &[AppRef]| aim_focused(&aim, focus, apps, &placed_editor);
        assert_eq!(
            check(Some(&focus(40, "Untitled")), &editor).as_deref(),
            Some("Untitled")
        );
        // 사람이 설정(pid 50)을 눌렀다. 포커스가 없을 때도 보내지 않는다
        assert_eq!(check(Some(&focus(50, "Settings")), &editor), None);
        assert_eq!(check(None, &editor), None);
        // 앱이 제목을 바꿨다(Firefox의 페이지 제목): 접근성 창의 경로가 같으면 그 창이고, 지금 제목을 준다
        assert_eq!(
            check(Some(&focus(40, "*Untitled")), &editor).as_deref(),
            Some("*Untitled")
        );
        // 같은 앱의 다른 창(사람이 연 둘째 창)이 활성이다
        let two = [app(
            40,
            "Text Editor",
            &[("Untitled", false), ("Other", true)],
        )];
        assert_eq!(check(Some(&focus(40, "Other")), &two), None);
        // 셸의 레이어 표면(모든 앱 패널)에 포커스가 있다
        let launcher = Focus {
            pid: 10,
            app_id: "bunt-launcher".into(),
            title: String::new(),
            layer: true,
        };
        assert_eq!(check(Some(&launcher), &editor), None);

        // 트리가 없는 창(foot)은 제목으로 본다. 창이 하나뿐이면 제목이 바뀐 것이다
        let blind = Aim {
            pid: 60,
            title: "~".into(),
            window: None,
            ..editor_aim()
        };
        let one = [placed(60, "foot", "~/src")];
        assert_eq!(
            aim_focused(&blind, Some(&focus(60, "~/src")), &[], &one).as_deref(),
            Some("~/src")
        );
        let many = [placed(60, "foot", "~"), placed(60, "foot", "~/src")];
        assert_eq!(
            aim_focused(&blind, Some(&focus(60, "~/src")), &[], &many),
            None
        );
        assert_eq!(
            aim_focused(&blind, Some(&focus(60, "~")), &[], &many).as_deref(),
            Some("~")
        );

        // 셸의 레이어 표면은 그 표면이 키보드 포커스를 가졌을 때만이다
        let shell = Aim {
            pid: 10,
            title: "bunt-launcher".into(),
            window: None,
            layer: true,
            ..editor_aim()
        };
        assert_eq!(
            aim_focused(&shell, Some(&launcher), &[], &[]).as_deref(),
            Some("bunt-launcher")
        );
        let menubar = Focus {
            app_id: "bunt-menubar".into(),
            ..launcher.clone()
        };
        assert_eq!(aim_focused(&shell, Some(&menubar), &[], &[]), None);
        assert_eq!(aim_focused(&shell, Some(&focus(10, "x")), &[], &[]), None);
    }

    #[test]
    fn refusals_without_a_target_say_where_the_focus_is() {
        let apps = [app(50, "System Settings", &[("Settings", true)])];
        let now = focus_now(Some(&focus(50, "Settings")), &apps, &[]);
        assert_eq!(now, "the keyboard focus is on System Settings \"Settings\"");
        let (code, message) = focus_moved(&editor_aim(), &now, "type");
        assert_eq!(code, protocol::FOCUS_MOVED);
        for part in [
            "the keyboard focus is on System Settings \"Settings\"",
            "not on Text Editor \"Untitled\"",
            "nothing was typed",
            "--app org.gnome.TextEditor",
        ] {
            assert!(message.contains(part), "{part}: {message}");
        }
        assert!(
            focus_moved(&editor_aim(), &now, "scroll")
                .1
                .contains("nothing was scrolled")
        );
        // 모르는 앱은 앱 ID, 레이어 표면은 그 이름, 포커스가 없으면 그렇다고
        assert!(
            focus_now(Some(&focus(70, "Claude")), &[], &[]).contains("org.example.App \"Claude\"")
        );
        let layer = Focus {
            pid: 10,
            app_id: "bunt-launcher".into(),
            title: String::new(),
            layer: true,
        };
        assert_eq!(
            focus_now(Some(&layer), &apps, &[]),
            "the keyboard focus is on the shell's bunt-launcher"
        );
        assert_eq!(
            focus_now(None, &apps, &[]),
            "no window has the keyboard focus"
        );
        // 다룬 창이 없는 세션은 대상을 고르라고 한다
        let (code, message) = no_aim("press");
        assert_eq!(code, protocol::INVALID_PARAMS);
        assert!(
            message.contains("--app") && message.contains("a ref"),
            "{message}"
        );
    }

    #[test]
    fn compositor_refusals_become_their_own_errors() {
        let moved = refusal(Some(Declined::FocusMoved), "", "pressing keys");
        assert_eq!(moved.0, protocol::FOCUS_MOVED);
        assert!(
            moved
                .1
                .starts_with("pressing keys: the keyboard focus is not on that window")
        );
        // REPL의 UserActiveError가 "in N ms"를 읽는다
        let busy = refusal(
            Some(Declined::UserActive {
                retry_after_ms: 1200,
            }),
            "",
            "typing",
        );
        assert_eq!(busy.0, protocol::USER_ACTIVE);
        assert!(busy.1.ends_with("try again in 1200 ms"), "{}", busy.1);
        assert_eq!(refusal(Some(Declined::Paused), "", "x").0, protocol::PAUSED);
        // 컴포지터의 다른 거절도 그 뜻의 번호로
        let auth = "a window only the user can answer is open (authentication, …)";
        assert_eq!(refusal(None, auth, "x").0, protocol::NOT_ALLOWED);
        let locked = "the screen is locked; only the user can unlock it with their password";
        assert_eq!(refusal(None, locked, "x").0, protocol::SCREEN_LOCKED);
        let gone = "that window is gone; list the windows again";
        assert_eq!(refusal(None, gone, "x").0, protocol::TARGET_GONE);
        let other = refusal(None, "something else", "scrolling");
        assert_eq!(
            other,
            (
                protocol::AGENT_ERROR,
                "scrolling failed: something else".into()
            )
        );
        // ref의 앱이 끝났다: 기다리기와 AI가 사라진 것으로 읽는 글
        let (code, message) = target_gone("e5", 812);
        assert_eq!(code, protocol::TARGET_GONE);
        assert!(message.contains(protocol::APP_QUIT) && message.contains("pid 812"));
    }

    #[test]
    fn answers_name_the_window_and_the_way() {
        let accessible = Via {
            accessibility: true,
            ..Via::default()
        };
        assert_eq!(
            accessible.describe("Text Editor", "Untitled", "inserted 3 characters"),
            "Text Editor \"Untitled\" (accessibility): inserted 3 characters"
        );
        let checked = Via {
            checked: true,
            ..Via::default()
        };
        assert_eq!(
            checked.describe("", "e2e canvas", "pressed Return"),
            "\"e2e canvas\" (target-checked input): pressed Return"
        );
        let mut both = accessible;
        both.add(checked);
        assert!(
            both.describe("A", "w", "x")
                .contains("(accessibility and target-checked input)")
        );
        // 옛 컴포지터로 넣었으면 경고를 붙인다(계약 C의 글)
        let mut old = Via::default();
        old.fell_back(true);
        let answer = old.describe("Text Editor", "Untitled", "pressed ctrl+a");
        assert!(answer.contains("(unchecked input)"), "{answer}");
        assert!(
            answer.ends_with(
                "; warning: the compositor cannot check the target; the input may reach another \
                 window"
            ),
            "{answer}"
        );
        // 셸의 레이어 표면에 옛 메서드로 넣은 것은 경고하지 않는다
        let mut layer = Via::default();
        layer.fell_back(false);
        assert!(
            !layer
                .describe("bunt-shell", "bunt-menubar", "pressed Escape")
                .contains("warning")
        );
        assert!(
            Via::default()
                .describe("A", "w", "x")
                .contains("(no input)")
        );
    }

    #[test]
    fn keys_that_accessibility_does_instead() {
        let keys =
            |text: &str| -> Vec<String> { text.split_whitespace().map(String::from).collect() };
        for clear in [
            "ctrl+a BackSpace",
            "ctrl+a Delete",
            "super+a backspace",
            "control+A del",
        ] {
            assert!(clears_text(&keys(clear)), "{clear}");
        }
        for not in [
            "ctrl+a",
            "BackSpace",
            "ctrl+a BackSpace x",
            "ctrl+c BackSpace",
            "Delete ctrl+a",
        ] {
            assert!(!clears_text(&keys(not)), "{not}");
        }
        assert!(
            enter_key(&keys("Return")) && enter_key(&keys("KP_Enter")) && enter_key(&keys("enter"))
        );
        assert!(!enter_key(&keys("Return Return")) && !enter_key(&keys("ctrl+Return")));
    }

    #[test]
    fn text_fields_outside_browsers_take_text_without_the_focus() {
        let field = Item {
            interfaces: iface::EDITABLE_TEXT,
            role: role::TEXT,
            ..Item::default()
        };
        assert!(accessible_field(&field, false, "GTK"));
        assert!(accessible_field(&field, false, ""));
        // 웹 칸과 브라우저(Firefox 주소 막대, Chromium·Electron)는 키보드 길이다
        assert!(!accessible_field(&field, true, "GTK"));
        assert!(!accessible_field(&field, false, "Gecko"));
        assert!(!accessible_field(&field, false, "chromium"));
        let password = Item {
            role: role::PASSWORD_TEXT,
            ..field.clone()
        };
        assert!(!accessible_field(&password, false, "GTK"));
        let label = Item {
            interfaces: 0,
            ..field
        };
        assert!(!accessible_field(&label, false, "GTK"));
    }

    #[test]
    fn a_fill_the_app_dropped_is_typed_again() {
        // act_or_type는 fill의 다시 읽기 실패를 이 글로 알아본다 (Expect::failure와 같이 바뀌어야 한다)
        let dropped = agent_error(Expect::Text("example.com".into()).failure("e82", ""));
        assert!(fill_not_kept(&dropped));
        assert!(!fill_not_kept(&agent_error(
            Expect::Contains("x".into()).failure("e5", "")
        )));
        assert!(!fill_not_kept(&(
            protocol::USER_ACTIVE,
            FILL_NOT_KEPT.into()
        )));
    }

    #[test]
    fn notes_say_why_the_scope_or_the_window_is_not_the_usual_one() {
        assert!(default_scope_note(None).contains("has not worked on a window yet"));
        let blind = Aim {
            window: None,
            ..editor_aim()
        };
        assert!(default_scope_note(Some(&blind)).contains("has no accessibility tree"));
        let gone = default_scope_note(Some(&editor_aim()));
        assert!(gone.starts_with(
            "# note: the window you worked on last (Text Editor \"Untitled\") is gone"
        ));
        assert_eq!(in_background(false, "tree".into()), "tree");
        let note = in_background(true, "- app: \"Calculator\"\n".into());
        assert!(note.starts_with(
            "# note: opened in the background; the person is using another window\n- app:"
        ));
        assert_eq!(paused().0, protocol::PAUSED);
    }

    /// 가짜 페이지의 창 높이 (placed()가 놓는 창)
    const PAGE_HEIGHT: i32 = 600;

    /// 가짜 웹 페이지. 문서를 스크롤한 거리로 요소의 자리와 보임 상태를 알리되, 바쁜 VM의 Firefox처럼 받은
    /// 스크롤을 `lag`가 지나서야 자리에 알리고 보임 상태는 그보다 `shown_lag` 더 늦게 고친다. 접근성 호출
    /// 하나는 `call`만큼 걸린다. 시계가 가짜라 몇 초를 기다리는 시험도 바로 끝난다
    struct Page {
        started: Instant,
        clock: Cell<Duration>,
        /// 요소가 문서에서 놓인 세로 자리와 높이
        top: i32,
        height: i32,
        /// 문서를 스크롤할 수 있는 거리 (0부터 이것까지)
        end: i32,
        /// 휠 눈금 하나에 움직이는 거리
        notch: i32,
        /// ScrollTo를 하는 앱이다 (Firefox. GTK4는 NotSupported)
        scroll_to: bool,
        /// 창 안의 다른 영역에 가려 늘 보이지 않는다
        covered: bool,
        lag: Duration,
        shown_lag: Duration,
        call: Duration,
        /// 앱이 받은 스크롤: (자리에 알리기 시작하는 때, 스크롤한 거리)
        scrolls: RefCell<Vec<(Duration, i32)>>,
        wheels: Cell<usize>,
    }

    impl Page {
        /// e2e 5번의 긴 페이지: 맨 위를 보고 있고, 단추는 4000px 아래 문서 끝에 있다
        fn new() -> Page {
            Page {
                started: Instant::now(),
                clock: Cell::new(Duration::ZERO),
                top: 4060,
                height: 30,
                end: 3500,
                notch: 50,
                scroll_to: true,
                covered: false,
                lag: Duration::ZERO,
                shown_lag: Duration::ZERO,
                call: Duration::from_millis(1),
                scrolls: RefCell::new(Vec::new()),
                wheels: Cell::new(0),
            }
        }

        fn elapsed(&self) -> Duration {
            self.clock.get()
        }

        /// 접근성 호출 하나에 드는 시간
        fn answer(&self) {
            self.clock.set(self.clock.get() + self.call);
        }

        /// `at`에 앱이 자리로 알리는 스크롤 거리
        fn scrolled(&self, at: Duration) -> i32 {
            self.scrolls
                .borrow()
                .iter()
                .rev()
                .find(|(from, _)| *from <= at)
                .map_or(0, |(_, to)| *to)
        }

        /// 앱이 마지막으로 받은 스크롤. 아직 알리지 않았어도 앱은 그리로 간다
        fn heading(&self) -> i32 {
            self.scrolls.borrow().last().map_or(0, |(_, to)| *to)
        }

        /// 앱이 스크롤을 받는다. 자리에는 `lag` 뒤에 알린다
        fn take_scroll(&self, to: i32) {
            let to = to.clamp(0, self.end);
            self.scrolls
                .borrow_mut()
                .push((self.clock.get() + self.lag, to));
        }

        /// `at`에 앱이 알리는 요소의 창 기준 세로 자리
        fn y(&self, at: Duration) -> i32 {
            self.top - self.scrolled(at)
        }
    }

    impl Scrollable for Page {
        fn now(&self) -> Instant {
            self.started + self.clock.get()
        }

        async fn pause(&self, duration: Duration) {
            self.clock.set(self.clock.get() + duration);
        }

        async fn extents(&self) -> zbus::Result<(i32, i32, i32, i32)> {
            self.answer();
            Ok((100, self.y(self.elapsed()), 200, self.height))
        }

        async fn shown(&self) -> bool {
            self.answer();
            // Firefox처럼 창에 조금이라도 들면 보인다
            let y = self.y(self.elapsed().saturating_sub(self.shown_lag));
            !self.covered && y < PAGE_HEIGHT && y + self.height > 0
        }

        async fn scroll_to(&self) -> zbus::Result<bool> {
            self.answer();
            if !self.scroll_to {
                return Err(zbus::Error::Failure("NotSupported".into()));
            }
            // ATSPI_SCROLL_ANYWHERE: 요소가 다 보이는 가장 가까운 곳으로 간다
            let lowest = self.top + self.height - PAGE_HEIGHT;
            self.take_scroll(self.heading().clamp(lowest, self.top));
            Ok(true)
        }

        async fn raise_window(&self) -> Result<Placed, Failure> {
            self.answer();
            Ok(placed(7, "firefox-esr", "top"))
        }

        async fn wheel(&self, _x: f64, _y: f64, _dx: f64, dy: f64) -> zbus::Result<()> {
            self.answer();
            self.wheels.set(self.wheels.get() + 1);
            self.take_scroll(self.heading() + dy as i32 * self.notch);
            Ok(())
        }
    }

    /// 가짜 페이지에서 화면 밖 요소를 보이게 스크롤한다: (답, 걸린 시간, 돌린 휠 수)
    fn scroll_page(page: &Page) -> (Result<String, Failure>, Duration, usize) {
        let answer = async_io::block_on(scroll_into_view(page));
        (answer, page.elapsed(), page.wheels.get())
    }

    #[test]
    fn an_app_that_scrolls_at_once_is_not_kept_waiting() {
        let page = Page::new();
        let (answer, took, wheels) = scroll_page(&page);
        assert_eq!(answer.unwrap(), "scrolled it into view");
        assert_eq!(wheels, 0);
        // 전과 같다: 자리 읽기, ScrollTo, 보임 상태 읽기를 한 번씩 하고 쉬지 않는다
        assert_eq!(took, 3 * page.call);
    }

    #[test]
    fn a_busy_app_that_shows_it_late_is_waited_for() {
        // 바쁜 VM의 Firefox: ScrollTo를 3초 뒤에야 자리에, 1초 더 뒤에 보임 상태에 알린다. 1초만 기다리던
        // 때는 휠 길로 넘어가 다시 읽은 자리가 그대로인 것을 끝으로 보고 실패했다
        let page = Page {
            lag: Duration::from_secs(3),
            shown_lag: Duration::from_secs(1),
            call: Duration::from_millis(40),
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        assert_eq!(answer.unwrap(), "scrolled it into view");
        assert_eq!(wheels, 0);
        assert!(
            took >= Duration::from_secs(4) && took < SHOWN_WAIT,
            "{took:?}"
        );
    }

    #[test]
    fn a_scroll_the_app_makes_after_the_wait_is_still_seen() {
        // ScrollTo를 SHOWN_WAIT보다 1초 늦게 고치는 앱: 휠 길로 넘어가 휠을 한 번 돌린 뒤 자리가 그대로여도
        // 기다리다가, 앱이 ScrollTo를 고치면 보인 것을 안다
        let page = Page {
            lag: SHOWN_WAIT + Duration::from_secs(1),
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        assert_eq!(
            answer.unwrap(),
            "scrolled it into view with the wheel (1 turn)"
        );
        assert_eq!(wheels, 1);
        assert!(took < SHOWN_WAIT + SCROLL_STUCK, "{took:?}");
    }

    #[test]
    fn without_wheel_turns_the_answer_says_what_scrolled() {
        // ScrollTo를 4.9초 뒤 자리에, 0.5초 더 뒤 보임 상태에 알리는 앱: SHOWN_WAIT 뒤 휠 길에서 자리는 이미
        // 창 안이라 휠을 돌리지 않고 보임 상태를 기다린다. 휠 수 대신 ScrollTo가 했다고 알린다
        let page = Page {
            lag: Duration::from_millis(4_900),
            shown_lag: Duration::from_millis(500),
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        assert_eq!(answer.unwrap(), "scrolled it into view");
        assert_eq!(wheels, 0);
        assert!(took > SHOWN_WAIT, "{took:?}");
        // ScrollTo를 못 하는 앱에서 요소가 이미 창 안에 있었다: 스크롤하지 않았다고 알린다
        let page = Page {
            scroll_to: false,
            top: 300,
            ..Page::new()
        };
        let (answer, _, wheels) = scroll_page(&page);
        assert_eq!(answer.unwrap(), "it came into view");
        assert_eq!(wheels, 0);
    }

    #[test]
    fn the_wheel_waits_for_an_app_that_moves_late() {
        // ScrollTo를 못 하는 앱이 휠을 1초 늦게 고친다. 전에는 다시 읽은 자리가 그대로라 끝에 닿았다고 보고
        // 실패했다. 기다리는 동안 휠을 더 돌리지 않으므로 바로 고치는 앱과 같은 수만 돌린다
        let page = |lag| Page {
            scroll_to: false,
            top: 1200,
            lag,
            ..Page::new()
        };
        let quick = page(Duration::ZERO);
        let (quick_answer, quick_took, quick_wheels) = scroll_page(&quick);
        assert_eq!(
            quick_answer.as_deref().unwrap(),
            "scrolled it into view with the wheel (4 turns)"
        );
        assert!(quick_took < Duration::from_secs(1), "{quick_took:?}");
        let late = page(Duration::from_secs(1));
        let (late_answer, late_took, late_wheels) = scroll_page(&late);
        assert_eq!(late_answer, quick_answer);
        assert_eq!(late_wheels, quick_wheels);
        assert!(late_took < SCROLL_LIMIT, "{late_took:?}");
        // 지나치지 않았다: 요소가 창 안에 다 든다
        let y = late.y(late.elapsed());
        assert!(y >= 0 && y + late.height <= PAGE_HEIGHT, "{y}");
    }

    #[test]
    fn the_wheel_waits_for_showing_that_comes_late() {
        // 휠을 고친 자리는 바로 알리지만 보임 상태는 2초 늦게 고치는 앱 (1초만 기다리던 때는 실패했다)
        let page = Page {
            scroll_to: false,
            top: 1200,
            shown_lag: Duration::from_secs(2),
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        assert_eq!(
            answer.unwrap(),
            "scrolled it into view with the wheel (4 turns)"
        );
        assert_eq!(wheels, 4);
        assert!(
            took >= Duration::from_secs(2) && took < SHOWN_WAIT,
            "{took:?}"
        );
    }

    #[test]
    fn an_element_that_cannot_be_scrolled_to_fails_soon() {
        // 문서가 스크롤되지 않는다: ScrollTo는 참이라고 답하지만 아무것도 움직이지 않는다
        let page = Page {
            end: 0,
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        let (code, message) = answer.unwrap_err();
        assert_eq!(code, protocol::AGENT_ERROR);
        assert!(
            message.starts_with("it is off screen and scrolling did not bring it into view"),
            "{message}"
        );
        // ScrollTo 뒤 SHOWN_WAIT를 기다리고, 휠을 한 번 돌려 SCROLL_STUCK 동안 움직이지 않는 것을 본다
        assert_eq!(wheels, 1);
        let expected = SHOWN_WAIT + SCROLL_STUCK;
        assert!(
            took >= expected && took < expected + Duration::from_millis(500),
            "{took:?}"
        );
    }

    #[test]
    fn an_element_covered_inside_the_window_fails_after_the_wait() {
        // 자리로는 창 안인데 다른 영역에 가려 보이지 않는다: 휠을 돌리지 않고 SHOWN_WAIT까지 본다
        let page = Page {
            scroll_to: false,
            top: 300,
            covered: true,
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        assert!(answer.is_err());
        assert_eq!(wheels, 0);
        assert!(
            took >= SHOWN_WAIT && took < SHOWN_WAIT + Duration::from_millis(500),
            "{took:?}"
        );
    }

    #[test]
    fn the_wheel_stops_after_its_turns() {
        // 휠 눈금 하나에 1px만 움직이는 앱: 요소가 다가오는 동안에도 SCROLL_TURNS까지만 돌린다
        let page = Page {
            scroll_to: false,
            notch: 1,
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        assert!(answer.is_err());
        assert_eq!(wheels, SCROLL_TURNS);
        assert!(took < SCROLL_LIMIT, "{took:?}");
    }

    #[test]
    fn a_slow_desktop_stops_at_the_limit() {
        // 접근성 호출 하나가 0.7초 걸리고 휠 눈금 하나에 1px만 움직인다: 요소가 다가오는 동안 상한이 지난다.
        // 상한이 지난 뒤에는 그때 하던 한 바퀴(자리 읽기, 휠, 쉼)만 마친다
        let call = Duration::from_millis(700);
        let page = Page {
            scroll_to: false,
            notch: 1,
            call,
            ..Page::new()
        };
        let (answer, took, wheels) = scroll_page(&page);
        assert!(answer.is_err());
        assert!(wheels < SCROLL_TURNS, "{wheels}");
        assert!(
            took >= SCROLL_LIMIT && took <= SCROLL_LIMIT + 2 * call + SCROLL_PAUSE,
            "{took:?}"
        );
    }
}
