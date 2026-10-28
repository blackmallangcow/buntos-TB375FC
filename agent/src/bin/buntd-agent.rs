// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! buntd-agent: buntOS 에이전트 데몬 (ARCHITECTURE 6.3). 사용자 서비스로 돌며 접근성 버스를 읽어
//! 에이전트에게 앱의 스냅샷을 준다. 소켓(`$XDG_RUNTIME_DIR/buntos/agent.sock`)은 systemd가 열어
//! 넘기고(buntd-agent.socket), 직접 띄우면 스스로 연다. 요청은 한 번에 하나씩 처리한다.
//!
//! 접근 정책(bunt-access, ARCHITECTURE 6.5): 연결마다 건너편 프로그램을 알아보고 시스템 설정에서
//! 허용한 AI의 요청만 받는다. 사용자가 막은 앱은 목록에서 빼고 읽거나 조작하지 않는다.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        io::{AsRawFd, FromRawFd},
        net::{UnixListener, UnixStream},
    },
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bunt_access::{Client, Policy};
use bunt_agent::{
    atspi::{A11y, AUTH_WINDOW, AppRef, Placed, ROOT, WindowRef},
    desktop::{self, DesktopApp},
    diff,
    protocol::{
        self, ActParams, Acted, Installed, LaunchParams, Launched, Request, Response, RpcError,
        Snapshot, SnapshotParams,
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
const SETTLE_MAX: Duration = Duration::from_secs(3);
/// 누르기에 쓰는 동작 이름 (GTK: 단추 click, 스위치 toggle, 글자 칸 activate)
const CLICK_ACTIONS: [&str; 6] = ["click", "press", "toggle", "activate", "jump", "open"];
/// 연 앱의 창이 뜨기를 기다리는 시간과 다시 보는 간격
const LAUNCH_WAIT: Duration = Duration::from_secs(20);
const LAUNCH_STEP: Duration = Duration::from_millis(250);
/// 연 앱이 이만큼 지나도록 창을 띄우지 못했으면 다시 열 때 새로 띄운다
const RELAUNCH_AFTER: Duration = Duration::from_secs(60);
/// 포커스를 받은 글 칸이 컴포지터에 글 입력(text-input)을 켜기를 기다리는 시간과 다시 보는 간격.
/// 켜지지 않으면 키로 친다. 포털을 기다리느라 바쁜 앱은 늦게 켠다
const TEXT_INPUT_WAIT: Duration = Duration::from_secs(1);
const TEXT_INPUT_STEP: Duration = Duration::from_millis(20);

/// 동작의 대상: 앱(접근성 버스 이름), 창, 요소와 그 부모. press는 요소 없이 창에만 보낼 수 있다
struct Target {
    bus: String,
    window: String,
    window_title: String,
    item: Option<Item>,
    parent: Option<Item>,
}

/// 그린 범위: 머리줄을 포함한 트리 글과 그 앱의 보이는 창 (경로, 제목)
struct Rendered {
    text: String,
    lines: usize,
    items: usize,
    windows: Vec<(String, String)>,
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
    bunt_access::record_request(client);
    tracing::warn!(
        client = %client.name,
        exe = %client.exe.display(),
        "refused a client the user has not allowed"
    );
    Err((
        protocol::NOT_ALLOWED,
        format!(
            "{} is not allowed to control this computer; the user can allow it in \
             System Settings > AI Agents",
            client.name
        ),
    ))
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
        let focused = a11y.focused_pid().await;
        let active = active_window(&apps, focused).map(|(_, window)| window.path.clone());
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
        Ok(json!({ "apps": apps }))
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
                    let app = find_app(&apps, &placed, &installed, query)?;
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
                    let focused = a11y.focused_pid().await;
                    let (app, window) = active_window(&apps, focused).ok_or_else(|| {
                        agent_error("no active window; pass an app (see `bunt agent apps`)")
                    })?;
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
        if !target.shown() {
            return Err(agent_error(format!("{reference} is not on screen")));
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
        })
    }

    /// ref가 가리키는 요소에 동작을 하고, 화면이 멈출 때까지 기다려 그 창의 바뀐 줄을 돌려준다
    async fn act(&mut self, params: ActParams) -> Result<Acted, Failure> {
        let a11y = self.a11y().await?;
        let session = params.session.clone().unwrap_or_else(|| "default".into());
        let found = if params.target.is_empty() {
            if params.action != "press" && params.action != "type" {
                return Err((
                    protocol::INVALID_PARAMS,
                    format!("{} needs a ref", params.action),
                ));
            }
            // 키는 지금 키보드 포커스를 가진 창으로 간다
            if a11y.auth_dialog_focused().await {
                return Err(auth_dialog());
            }
            let apps = self.list(&a11y).await?;
            let focused = a11y.focused_pid().await;
            let (app, window) = active_window(&apps, focused)
                .ok_or_else(|| agent_error("no active window; pass a ref"))?;
            Target {
                bus: app.bus.clone(),
                window: window.path.clone(),
                window_title: window.title.clone(),
                item: None,
                parent: None,
            }
        } else {
            self.find_target(&a11y, &session, &params.target).await?
        };
        self.check_app(&a11y, &found.bus).await?;
        let Target {
            bus,
            window,
            window_title,
            item,
            parent,
        } = found;

        let full = Options {
            interactive: false,
            hidden: false,
        };
        let roots = [window.clone()];
        let before = self.render(&a11y, &session, &bus, &roots, full).await?;
        let done = match (params.action.as_str(), &item) {
            ("press" | "type", _) => {
                press(&a11y, &bus, &window_title, item.as_ref(), &params).await?
            }
            ("click", Some(target)) if params.pointer => {
                pointer_click(&a11y, &bus, &window_title, target).await?
            }
            ("focus", Some(target)) => focus_element(&a11y, &bus, &window_title, target).await?,
            (_, Some(target)) => {
                match perform(&a11y, &bus, target, parent.as_ref(), &params).await? {
                    Some(done) => done,
                    // 접근성 동작이 없는 요소는 사람처럼 포인터로 누른다
                    None => format!(
                        "{}; it has no accessible action",
                        pointer_click(&a11y, &bus, &window_title, target).await?
                    ),
                }
            }
            (_, None) => unreachable!("only press works without a ref"),
        };

        // 화면이 멈출 때까지: 바뀐 뒤 SETTLE_QUIET 동안 그대로면 멈춘 것이다. 바뀌지 않으면
        // SETTLE_FIRST까지 기다린다
        let started = Instant::now();
        let mut last = before.text.clone();
        let mut changed: Option<Instant> = None;
        let after = loop {
            async_io::Timer::after(SETTLE_STEP).await;
            let now = self.render(&a11y, &session, &bus, &roots, full).await?;
            if now.text != last {
                changed = Some(Instant::now());
            }
            let quiet = changed.is_some_and(|at| at.elapsed() >= SETTLE_QUIET);
            let waited = started.elapsed();
            if quiet || (changed.is_none() && waited >= SETTLE_FIRST) || waited >= SETTLE_MAX {
                break now;
            }
            last = now.text;
        };
        let settled_ms = started.elapsed().as_millis() as u64;
        let diff = diff::unified(&before.text, &after.text);
        let new_windows = after
            .windows
            .iter()
            .filter(|(path, _)| !before.windows.iter().any(|(known, _)| known == path))
            .map(|(_, title)| title.clone())
            .collect();
        // 다음 스냅샷의 diff는 이 뒤부터다
        let scope = format!("window {bus} {window}");
        self.sessions
            .get(&session)
            .remember(scope_key(&scope, full), &after.text);
        tracing::info!(
            action = %params.action,
            settled_ms,
            diff = diff.lines().count(),
            "acted on {bus}"
        );
        Ok(Acted {
            done,
            diff,
            settled_ms,
            new_windows,
        })
    }
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
            let actions = a11y.actions(bus, &target.path).await;
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

/// 활성 창: 컴포지터가 키보드 포커스를 준 앱의 활성 창(없으면 첫 창). 그 앱을 모르면 활성 상태인
/// 첫 창이다. GTK는 셸의 레이어 창을 팝업이 닫힌 뒤에도 활성으로 알릴 때가 있다
fn active_window(apps: &[AppRef], focused: Option<u32>) -> Option<(&AppRef, &WindowRef)> {
    fn active(app: &AppRef) -> Option<&WindowRef> {
        app.windows
            .iter()
            .find(|window| window.states & state::ACTIVE != 0)
    }
    if let Some(app) = focused.and_then(|pid| apps.iter().find(|app| app.pid == Some(pid)))
        && let Some(window) = active(app).or_else(|| app.windows.first())
    {
        return Some((app, window));
    }
    apps.iter()
        .find_map(|app| active(app).map(|window| (app, window)))
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
        if !hidden && !item.shown() {
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

/// 키를 누르거나(press) 글자를 친다(type). ref가 있으면 그 창을 앞으로 가져와 그 요소에 포커스를
/// 준 뒤 한다
async fn press(
    a11y: &A11y,
    bus: &str,
    window_title: &str,
    target: Option<&Item>,
    params: &ActParams,
) -> Result<String, Failure> {
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
        let _ = focus_element(a11y, bus, window_title, target).await;
        // 글 칸이 글 입력을 켜지 않으면 접근성으로 커서 자리에 넣는다. 키로 치면 입력기가 한글 모드일 때
        // 글이 바뀐다
        if typing
            && target.interfaces & iface::EDITABLE_TEXT != 0
            && !wait_for_text_input(a11y).await
            && a11y
                .insert_text(bus, &target.path, text)
                .await
                .unwrap_or(false)
        {
            return Ok(format!(
                "inserted {} characters at the caret (the app did not turn on text input)",
                text.chars().count()
            ));
        }
    }
    if typing {
        let how = a11y
            .type_text(text)
            .await
            .map_err(|err| agent_error(format!("typing failed: {err}")))?;
        let how = if how == "text-input" {
            "through the app's text input"
        } else {
            "as keys"
        };
        return Ok(format!("typed {} characters {how}", text.chars().count()));
    }
    a11y.press_keys(&keys)
        .await
        .map_err(|err| agent_error(format!("pressing keys failed: {err}")))?;
    Ok(format!("pressed {}", keys.join(" ")))
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
    let windows = a11y
        .windows()
        .await
        .map_err(|err| agent_error(format!("cannot read the window list: {err}")))?;
    let window = windows
        .iter()
        .rev()
        .filter(|window| window.pid == pid)
        .find(|window| window.title == window_title)
        .or_else(|| windows.iter().rev().find(|window| window.pid == pid))
        .ok_or_else(|| agent_error("the compositor has no window for this app"))?;
    let (x, y, width, height) = a11y
        .extents(bus, &target.path)
        .await
        .map_err(|err| agent_error(format!("cannot tell where the element is: {err}")))?;
    if width <= 0 || height <= 0 {
        return Err(agent_error("the element has no size on screen"));
    }
    // 다른 창에 가려 있으면 그 창을 누르게 되므로 먼저 앞으로 가져온다
    a11y.activate_window(pid, &window.title)
        .await
        .map_err(|err| agent_error(format!("activating the window failed: {err}")))?;
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
