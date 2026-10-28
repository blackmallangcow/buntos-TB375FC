// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! buntd-agent: buntOS 에이전트 데몬 (ARCHITECTURE 6.3). 사용자 서비스로 돌며 접근성 버스를 읽어
//! 에이전트에게 앱의 스냅샷을 준다. 소켓(`$XDG_RUNTIME_DIR/buntos/agent.sock`)은 systemd가 열어
//! 넘기고(buntd-agent.socket), 직접 띄우면 스스로 연다. 요청은 한 번에 하나씩 처리한다.

use std::{
    collections::HashMap,
    fs,
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::PermissionsExt,
        io::FromRawFd,
        net::{UnixListener, UnixStream},
    },
    sync::{Arc, Mutex},
    time::Instant,
};

use bunt_agent::{
    atspi::{A11y, AppRef, ROOT, WindowRef},
    diff,
    protocol::{self, Request, Response, RpcError, Snapshot, SnapshotParams},
    session::Sessions,
    tree::{self, Item, Options, Tree, state},
};
use serde_json::{Value, json};

/// (JSON-RPC 오류 번호, 알림)
type Failure = (i64, String);

#[derive(Default)]
struct Agent {
    a11y: Option<A11y>,
    sessions: Sessions,
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
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let agent = agent.clone();
                std::thread::spawn(move || serve(stream, &agent));
            }
            Err(err) => tracing::warn!("accept failed: {err}"),
        }
    }
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

/// 연결 하나: 한 줄 요청마다 한 줄로 답한다
fn serve(stream: UnixStream, agent: &Mutex<Agent>) {
    let mut writer = &stream;
    for line in BufReader::new(&stream).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(mut reply) = serde_json::to_string(&handle(&line, agent)) else {
            return;
        };
        reply.push('\n');
        if writer.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

fn handle(line: &str, agent: &Mutex<Agent>) -> Response {
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
    let mut agent = agent
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = async_io::block_on(agent.dispatch(&request.method, request.params));
    tracing::info!(
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

    async fn list(&mut self, a11y: &A11y) -> Result<Vec<AppRef>, Failure> {
        a11y.apps().await.map_err(|err| {
            self.a11y = None;
            agent_error(format!("cannot list apps: {err}"))
        })
    }

    async fn apps(&mut self) -> Result<Value, Failure> {
        let a11y = self.a11y().await?;
        let apps = self.list(&a11y).await?;
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
                    let app = find_app(&apps, query)?;
                    let roots = app
                        .windows
                        .iter()
                        .map(|window| window.path.clone())
                        .collect();
                    (app.bus.clone(), roots, format!("app {}", app.bus))
                }
                None => {
                    let focused = a11y.focused_pid().await;
                    let (app, window) = active_window(&apps, focused).ok_or_else(|| {
                        agent_error("no active window; pass an app (see `bunt agent apps`)")
                    })?;
                    let scope = format!("window {} {}", app.bus, window.path);
                    (app.bus.clone(), vec![window.path.clone()], scope)
                }
            }
        };

        let mut items = a11y.tree(&bus).await.map_err(|err| {
            self.a11y = None;
            agent_error(format!("cannot read {bus}: {err}"))
        })?;
        let wanted = in_scope(&items, &roots, params.hidden);
        a11y.enrich(&bus, &mut items, wanted).await;
        let pid = a11y.pid(&bus).await;

        let tree = Tree::new(&items);
        let mut text = String::new();
        if params.interactive {
            text.push_str("# note: interactive elements only\n");
        }
        let app_name = tree.get(ROOT).map_or("", |app| app.name.as_str());
        text.push_str(&format!("- app: \"{app_name}\""));
        if let Some(pid) = pid {
            text.push_str(&format!(" [pid={pid}]"));
        }
        text.push('\n');
        let session = self.sessions.get(&session_name);
        let options = Options {
            interactive: params.interactive,
            hidden: params.hidden,
        };
        let roots: Vec<&str> = roots.iter().map(String::as_str).collect();
        let lines = tree::render(&tree, &roots, &options, &mut |path: &str| {
            session.ref_for(&bus, path)
        });
        for line in &lines {
            text.push_str(line);
            text.push('\n');
        }
        let key = format!(
            "{scope} interactive={} hidden={}",
            params.interactive, params.hidden
        );
        let previous = session.remember(key, &text);
        let diff = previous
            .as_deref()
            .map(|old| diff::unified(old, &text))
            .unwrap_or_default();
        tracing::info!(
            items = items.len(),
            lines = lines.len(),
            diff = diff.lines().count(),
            "snapshot of {bus}"
        );
        Ok(Snapshot {
            tree: text,
            diff,
            first: previous.is_none(),
        })
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

/// 앱 이름, 버스 이름, pid가 같은 앱. 없으면 이름이나 창 제목에 그 글이 든 앱
fn find_app<'a>(apps: &'a [AppRef], query: &str) -> Result<&'a AppRef, Failure> {
    let exact: Vec<&AppRef> = apps
        .iter()
        .filter(|app| {
            app.name == query
                || app.bus == query
                || app.pid.is_some_and(|pid| pid.to_string() == query)
        })
        .collect();
    let matches = if exact.is_empty() {
        let lower = query.to_lowercase();
        apps.iter()
            .filter(|app| {
                app.name.to_lowercase().contains(&lower)
                    || app
                        .windows
                        .iter()
                        .any(|window| window.title.to_lowercase().contains(&lower))
            })
            .collect()
    } else {
        exact
    };
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
