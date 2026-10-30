// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! JS REPL (ARCHITECTURE 6.6절 3단계): AI가 Playwright 문법의 JS로 데스크톱을 읽고 다루고, 동작과 확인을 호출 하나에
//! 묶는다. 변수는 호출 사이에 남는다. `bunt agent repl`과 MCP 어댑터(`bunt mcp`)의 repl 도구가 데몬의 `repl`
//! 메서드로 여기에 온다.
//!
//! 실행 환경은 데몬이 AI 클라이언트(실행 파일·스크립트)와 세션마다 띄운 워커 프로세스다(worker.rs).
//! - 데몬 안: CLI는 호출마다 새 프로세스라 변수를 지닐 수 없고, 데몬은 이미 오래 돌며 연결마다 클라이언트를
//!   알아본다. 클라이언트마다 따로 두어 한 AI가 다른 AI의 변수(읽은 화면 내용)를 보지 못한다
//! - 따로 된 프로세스: JS 엔진(QuickJS, C)의 버그나 abort가 데몬을 죽이지 않고, 워커는 seccomp로 갇혀(sandbox.rs)
//!   엔진이 뚫려도 파이프 말고는 아무것도 하지 못한다. 시간 상한을 넘겨 멈춘 워커는 죽인다
//! - 워커가 청하는 데스크톱 호출은 여기서 허용 목록으로 거르고 Desk로 데몬에 넘긴다. 데몬은 부를 때마다 정책을 다시
//!   읽어 클라이언트를 받고, 화면 잠금, 막은 앱, 사람만 답하는 창을 본다. 파일 쓰기(screenshot의 out)는 받지 않는다
//! - 워커가 보낸 것은 믿지 않는다: 줄 길이와 JSON의 크기·깊이·값 수에 상한을 두고(worker::fits), 인자는 메서드마다
//!   정한 꼴로만 읽는다

pub mod pipe;
pub mod sandbox;
pub mod source;
pub mod worker;

use std::{
    collections::HashMap,
    io::{self, Write},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, value::RawValue};

use self::{
    pipe::{Lines, ReadFd, Source},
    worker::{Done, FromWorker, Reply, ToWorker},
};
use crate::{
    desk::{Desk, Failure},
    protocol::{
        self, ActParams, LaunchParams, Repl, ReplImage, ReplParams, ScreenshotParams,
        SnapshotParams, WaitParams,
    },
    wait,
};

/// 호출 하나의 시간 상한: 기본값과 최댓값
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TIMEOUT_MS: u64 = 120_000;
/// 코드의 바이트 수 상한
const CODE_LIMIT: usize = 1 << 20;
/// 세션 이름의 글자 수 상한
const SESSION_LIMIT: usize = 128;
/// 시간 상한이 지난 뒤 워커가 답하기를 기다리는 시간. 그 뒤로도 답이 없으면 죽인다. 데스크톱 호출(앱 열기는 20초)이
/// 상한을 넘겨 끝난 직후라도 워커가 중단을 알릴 시간은 준다
const GRACE: Duration = Duration::from_secs(5);
const MIN_GRACE: Duration = Duration::from_secs(2);
/// 이만큼 쓰지 않은 REPL은 끝낸다
const IDLE: Duration = Duration::from_secs(60 * 60);
/// 데몬 전체와 클라이언트 하나가 띄울 수 있는 REPL 수
const TOTAL: usize = 8;
const PER_CLIENT: usize = 4;
/// 워커가 보내는 한 줄의 상한. 맞게 도는 워커의 줄은 2MiB를 넘지 않는다: 출력(OUTPUT_LIMIT 글자)과 오류(ERROR_LIMIT
/// 글자)는 JSON에서 글자마다 6바이트까지, 값과 인자는 JSON_LIMIT까지다
const LINE_LIMIT: usize = 4 << 20;
/// 답에 붙이는 스크린샷 수와 파일 하나의 크기 상한
const IMAGES_KEPT: usize = 4;
const IMAGE_LIMIT: u64 = 16 << 20;
/// REPL이 부를 수 있는 데몬 메서드. repl(되부르기)은 없다
const METHODS: [&str; 7] = [
    "apps",
    "installed",
    "snapshot",
    "launch",
    "act",
    "screenshot",
    "wait",
];
/// 데몬의 동작 기본 기다림 (buntd-agent의 SETTLE_MAX)
const SETTLE_DEFAULT_MS: u64 = 3_000;

/// REPL 하나를 가리키는 열쇠: AI 클라이언트와 세션
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    /// 클라이언트를 가리는 글 (실행 파일, 스크립트, 관리자)
    pub client: String,
    pub session: String,
}

/// 띄운 워커: 보내는 파이프, 받는 파이프, 프로세스
pub struct Worker {
    to: Box<dyn Write + Send>,
    from: Lines<Source>,
    child: Option<Child>,
}

impl Worker {
    pub fn new(to: Box<dyn Write + Send>, from: Box<dyn ReadFd>, child: Option<Child>) -> Worker {
        Worker {
            to,
            from: Lines::new(Source(from), LINE_LIMIT),
            child,
        }
    }

    /// `buntd-agent --repl-worker`를 띄운다. 환경 변수 없이 루트 디렉터리에서 돈다
    pub fn spawn() -> io::Result<Worker> {
        // 데몬 파일이 갱신돼 지워졌어도 /proc/self/exe는 도는 실행 파일 그대로다
        let mut child = Command::new("/proc/self/exe")
            .arg("--repl-worker")
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let to = child.stdin.take().ok_or(io::ErrorKind::BrokenPipe)?;
        let from = child.stdout.take().ok_or(io::ErrorKind::BrokenPipe)?;
        Ok(Worker::new(Box::new(to), Box::new(from), Some(child)))
    }
}

impl Worker {
    /// 스레드로 도는 워커. 가두지 않으므로 시험에만 쓴다 (데몬은 늘 spawn으로 프로세스를 띄운다)
    #[doc(hidden)]
    pub fn in_thread() -> io::Result<Worker> {
        let (from_daemon, to_worker) = io::pipe()?;
        let (from_worker, to_daemon) = io::pipe()?;
        std::thread::Builder::new()
            .name("repl-worker".into())
            .stack_size(8 << 20)
            .spawn(move || {
                if let Ok(session) = worker::Session::start(from_daemon, to_daemon) {
                    let _ = session.serve(None);
                }
            })?;
        Ok(Worker::new(
            Box::new(to_worker),
            Box::new(from_worker),
            None,
        ))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// REPL 하나: 워커(아직 없거나 끝났으면 None)와 마지막으로 쓴 때
struct Context {
    worker: Mutex<Option<Worker>>,
    used: Mutex<Instant>,
}

impl Context {
    fn new() -> Context {
        Context {
            worker: Mutex::new(None),
            used: Mutex::new(Instant::now()),
        }
    }

    fn idle(&self) -> Duration {
        lock(&self.used).elapsed()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 워커를 띄우는 길 (시험은 스레드로 띄운다)
pub type Spawn = Box<dyn Fn() -> io::Result<Worker> + Send + Sync>;

/// 데몬이 지닌 REPL들
pub struct Repls {
    contexts: Mutex<HashMap<Key, Arc<Context>>>,
    spawn: Spawn,
}

impl Repls {
    pub fn new(spawn: Spawn) -> Repls {
        Repls {
            contexts: Mutex::new(HashMap::new()),
            spawn,
        }
    }

    /// 지금 띄운 REPL 수 (워커가 없는 자리 포함)
    pub fn len(&self) -> usize {
        lock(&self.contexts).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 코드를 이 클라이언트의 REPL에서 돌린다. 데스크톱 호출은 `desk`(데몬의 정책)로 간다
    pub fn run(&self, desk: &dyn Desk, client: &str, params: ReplParams) -> Result<Repl, Failure> {
        let started = Instant::now();
        let session = params.session.clone().unwrap_or_else(|| "default".into());
        if session.is_empty() || session.chars().count() > SESSION_LIMIT {
            return Err(invalid(format!(
                "a session name has 1 to {SESSION_LIMIT} characters"
            )));
        }
        if params.code.len() > CODE_LIMIT {
            return Err(invalid(format!(
                "the code is longer than {} KiB",
                CODE_LIMIT >> 10
            )));
        }
        let timeout = Duration::from_millis(
            params
                .timeout_ms
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(1, MAX_TIMEOUT_MS),
        );
        let key = Key {
            client: client.to_string(),
            session,
        };
        let context = self.context(&key)?;
        // 같은 REPL의 호출은 차례로 돈다 (MCP 클라이언트가 한꺼번에 보내도)
        let mut slot = lock(&context.worker);
        if params.reset {
            *slot = None;
        }
        let mut fresh = slot.is_none();
        let mut bridge = Bridge {
            desk,
            session: &key.session,
            deadline: started + timeout,
            shots: Vec::new(),
        };
        let mut retried = false;
        let done = loop {
            if slot.is_none() {
                *slot = Some(
                    (self.spawn)()
                        .map_err(|err| agent_error(format!("cannot start the REPL: {err}")))?,
                );
                fresh = true;
            }
            let worker = slot.as_mut().expect("a worker was just started");
            match drive(worker, &params.code, timeout, &mut bridge) {
                Ok(done) => break done,
                // 코드를 받기 전에 끝난 워커(오래 쉬어 스스로 끝났다)는 새로 띄워 한 번 더 한다. 데스크톱 호출을
                // 하나도 하지 않았으므로 되풀이해도 같은 동작을 두 번 하지 않는다
                Err(Stopped::Early(_)) if !retried => {
                    retried = true;
                    *slot = None;
                }
                Err(Stopped::Early(message) | Stopped::Late(message)) => {
                    *slot = None;
                    return Err(agent_error(message));
                }
            }
        };
        drop(slot);
        *lock(&context.used) = Instant::now();
        let mut error = done.error;
        // 앞 호출의 변수를 쓰려다 없으면 왜 없는지 알린다
        if let Some(message) = error
            .as_mut()
            .filter(|message| fresh && !params.reset && message.starts_with("ReferenceError"))
        {
            message.push_str(
                "\n(this call ran in a new REPL: variables from earlier calls are gone. The \
                 daemon restarted, the REPL was idle for an hour, or it was stopped)",
            );
        }
        Ok(Repl {
            output: done.output,
            error,
            // 크기를 본 값이라(drive) 트리로 풀어도 메모리가 크게 늘지 않는다
            value: done
                .value
                .and_then(|value| serde_json::from_str(value.get()).ok()),
            fresh,
            ms: started.elapsed().as_millis() as u64,
            images: images(&bridge.shots, params.images),
        })
    }

    /// 열쇠의 REPL. 없으면 자리를 만든다: 오래 쓰지 않은 REPL은 끝내고, 자리가 모자라면 쉬고 있는 REPL 가운데
    /// 가장 오래 쓰지 않은 것(클라이언트가 너무 많이 띄웠으면 그 클라이언트의 것)을 끝낸다
    fn context(&self, key: &Key) -> Result<Arc<Context>, Failure> {
        let mut contexts = lock(&self.contexts);
        contexts.retain(|_, context| !(context.idle() > IDLE && end(context)));
        if let Some(context) = contexts.get(key) {
            return Ok(context.clone());
        }
        let mine = contexts
            .keys()
            .filter(|known| known.client == key.client)
            .count();
        if mine >= PER_CLIENT {
            evict(&mut contexts, |known| known.client == key.client).ok_or_else(|| {
                agent_error(format!(
                    "this AI already runs {PER_CLIENT} REPL sessions and all are busy; wait for one"
                ))
            })?;
        } else if contexts.len() >= TOTAL {
            evict(&mut contexts, |_| true).ok_or_else(|| {
                agent_error(format!(
                    "{TOTAL} REPL sessions are busy; try again when one finishes"
                ))
            })?;
        }
        let context = Arc::new(Context::new());
        contexts.insert(key.clone(), context.clone());
        Ok(context)
    }
}

/// 쉬고 있는(돌고 있지 않은) REPL의 워커를 끝낸다. 돌고 있으면 거짓
fn end(context: &Context) -> bool {
    match context.worker.try_lock() {
        Ok(mut worker) => {
            *worker = None;
            true
        }
        Err(_) => false,
    }
}

/// 고른 것 가운데 가장 오래 쓰지 않은, 쉬고 있는 REPL을 끝내고 뺀다
fn evict(contexts: &mut HashMap<Key, Arc<Context>>, pick: impl Fn(&Key) -> bool) -> Option<Key> {
    let mut candidates: Vec<(Duration, Key)> = contexts
        .iter()
        .filter(|(key, _)| pick(key))
        .map(|(key, context)| (context.idle(), key.clone()))
        .collect();
    candidates.sort_by_key(|(idle, _)| std::cmp::Reverse(*idle));
    let key = candidates
        .into_iter()
        .map(|(_, key)| key)
        .find(|key| end(&contexts[key]))?;
    contexts.remove(&key);
    Some(key)
}

fn invalid(message: String) -> Failure {
    (protocol::INVALID_PARAMS, message)
}

fn agent_error(message: impl Into<String>) -> Failure {
    (protocol::AGENT_ERROR, message.into())
}

/// 워커가 끝을 알리지 못한 까닭
enum Stopped {
    /// 코드를 받기 전에 끝났다 (아무 호출도 하지 않았다)
    Early(String),
    /// 도는 중에 끝났거나 답이 없어 죽였다
    Late(String),
}

/// 워커에 코드를 보내고, 워커가 청하는 데스크톱 호출을 대신 하고, 끝을 받는다
fn drive(
    worker: &mut Worker,
    code: &str,
    timeout: Duration,
    bridge: &mut Bridge,
) -> Result<Done, Stopped> {
    let eval = ToWorker::Eval {
        code: code.to_string(),
        timeout_ms: timeout.as_millis() as u64,
    };
    let mut heard = false;
    let lost = |heard: bool, why: String| {
        if heard {
            Stopped::Late(format!(
                "the REPL stopped while running the code ({why}); its variables are gone"
            ))
        } else {
            Stopped::Early(format!("the REPL stopped ({why}); its variables are gone"))
        }
    };
    if let Err(err) = pipe::send(&mut worker.to, &eval) {
        return Err(lost(false, err.to_string()));
    }
    loop {
        // 상한이 지난 뒤로도 답이 없는 워커는 멈춘 것이다 (데스크톱 호출이 상한을 넘겨 끝났어도 MIN_GRACE는 준다)
        let wait_until = (bridge.deadline + GRACE).max(Instant::now() + MIN_GRACE);
        let line = match worker.from.next(Some(wait_until)) {
            Ok(Some(line)) => line,
            Ok(None) => {
                return Err(Stopped::Late(format!(
                    "the REPL did not stop at its time limit ({} ms) and was killed; its \
                     variables are gone",
                    timeout.as_millis()
                )));
            }
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => {
                return Err(lost(heard, "it crashed".into()));
            }
            Err(err) => return Err(lost(true, err.to_string())),
        };
        heard = true;
        // 인자와 값은 글 그대로 받는다 (serde_json은 RawValue를 되부르지 않고 훑는다)
        match serde_json::from_str::<FromWorker>(&line) {
            Ok(FromWorker::Done(done)) => {
                if let Some(Err(why)) = done.value.as_ref().map(|value| worker::fits(value.get())) {
                    return Err(lost(true, format!("it sent a value {why}")));
                }
                return Ok(done);
            }
            Ok(FromWorker::Call { method, params }) => {
                let reply = match bridge
                    .call(&method, &params)
                    .and_then(|value| to_raw(&value))
                {
                    Ok(value) => Reply::Ok(value),
                    Err((code, message)) => Reply::Err { code, message },
                };
                if let Err(err) = pipe::send(&mut worker.to, &ToWorker::Reply(reply)) {
                    return Err(lost(true, err.to_string()));
                }
            }
            Err(_) => return Err(lost(true, "it sent something unexpected".into())),
        }
    }
}

/// 워커의 데스크톱 호출을 데몬에 넘긴다
struct Bridge<'a> {
    desk: &'a dyn Desk,
    session: &'a str,
    deadline: Instant,
    /// 이번 호출에 찍은 스크린샷
    shots: Vec<String>,
}

impl Bridge<'_> {
    /// 워커가 청한 데스크톱 호출. 인자는 메서드마다 정한 꼴로만 읽고 모르는 키는 버린다: 값 트리를 만들지 않아 워커가
    /// 데몬의 메모리를 부풀리지 못하고, 받지 않는 인자(파일 경로 같은)를 숨겨 보내지 못한다
    fn call(&mut self, method: &str, params: &RawValue) -> Result<Value, Failure> {
        if !METHODS.contains(&method) {
            return Err((
                protocol::METHOD_NOT_FOUND,
                format!("the REPL cannot call {method:?}"),
            ));
        }
        worker::fits(params.get())
            .map_err(|why| invalid(format!("the arguments of {method} are {why}")))?;
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err((
                protocol::TIMEOUT,
                "this REPL call ran out of time; nothing more was done".into(),
            ));
        }
        let left_ms = left.as_millis() as u64;
        // ref와 diff는 REPL의 세션 것이다
        let session = Some(self.session.to_string());
        let params = match method {
            "snapshot" => to_value(SnapshotParams {
                session,
                ..parse(params)?
            })?,
            "launch" => to_value(LaunchParams {
                session,
                ..parse(params)?
            })?,
            "act" => {
                let act: ActParams = parse(params)?;
                // 동작의 기다림은 남은 시간 안에서
                let settle = act.settle_ms.unwrap_or(SETTLE_DEFAULT_MS).min(left_ms);
                to_value(ActParams {
                    session,
                    settle_ms: Some(settle),
                    ..act
                })?
            }
            "screenshot" => {
                let shot: ScreenshotParams = parse(params)?;
                // REPL은 파일을 쓰지 못한다. 스크린샷은 데몬의 스크린샷 폴더에 남고 경로가 답에 있다
                if shot.out.is_some() {
                    return Err(invalid(
                        "the REPL cannot choose where files go: screenshot() saves into the \
                         screenshot folder and returns the path"
                            .into(),
                    ));
                }
                let shot = self.desk.call(method, to_value(shot)?)?;
                if let Some(path) = shot["path"].as_str() {
                    self.shots.push(path.to_string());
                }
                return Ok(shot);
            }
            "wait" => {
                let mut wait: WaitParams = parse(params)?;
                wait.session = session;
                let timeout = wait.timeout_ms.unwrap_or(wait::DEFAULT_TIMEOUT_MS);
                wait.timeout_ms = Some(timeout.min(left_ms));
                return to_value(wait::run(self.desk, &wait)?);
            }
            // apps, installed: 인자가 없다
            _ => Value::Null,
        };
        self.desk.call(method, params)
    }
}

/// 워커가 보낸 인자를 메서드의 인자 꼴로 읽는다 (null은 기본값)
fn parse<T: DeserializeOwned + Default>(params: &RawValue) -> Result<T, Failure> {
    if params.get() == "null" {
        return Ok(T::default());
    }
    serde_json::from_str(params.get()).map_err(|err| invalid(err.to_string()))
}

fn to_value(value: impl Serialize) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|err| agent_error(err.to_string()))
}

fn to_raw(value: &Value) -> Result<Box<RawValue>, Failure> {
    serde_json::value::to_raw_value(value).map_err(|err| agent_error(err.to_string()))
}

/// 스크린샷 파일을 답에 붙인다. `data`면 base64로 넣는다 (MCP 어댑터가 그림으로 넘긴다)
fn images(paths: &[String], data: bool) -> Vec<ReplImage> {
    paths
        .iter()
        .take(IMAGES_KEPT)
        .map(|path| ReplImage {
            path: path.clone(),
            mime: "image/png".into(),
            data: data
                .then(|| read_image(path))
                .flatten()
                .map(|bytes| base64(&bytes)),
        })
        .collect()
}

fn read_image(path: &str) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > IMAGE_LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut io::Read::take(file, IMAGE_LIMIT), &mut bytes).ok()?;
    Some(bytes)
}

/// 표준 base64 (RFC 4648, 채움 있음)
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |acc, (at, &byte)| {
            acc | u32::from(byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(ALPHABET[(triple >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
