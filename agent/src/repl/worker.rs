// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! REPL 워커: QuickJS로 AI의 JS를 돌리는 프로세스 (`buntd-agent --repl-worker`). 데몬이 AI 클라이언트와 세션마다
//! 하나씩 띄워 변수를 호출 사이에 지닌다(repl/mod.rs).
//!
//! 데몬과는 표준 입출력으로 한 줄에 JSON 하나씩 말한다:
//! - 데몬 → 워커: `{"eval": {"code", "timeout_ms"}}`, 데스크톱 호출의 답 `{"reply": {"ok": …}|{"err": …}}`
//! - 워커 → 데몬: 데스크톱 호출 `{"call": {"method", "params"}}`, 끝 `{"done": {"output", "error", "value"}}`
//!
//! JS에는 데스크톱 API(prelude.js), console, setTimeout만 있다. QuickJS의 std·os 모듈은 넣지 않아 파일·네트워크·
//! 프로세스 API가 없고, 엔진이 뚫려도 seccomp로 갇혀 있다(sandbox.rs). 호출마다 시간 상한(QuickJS의 중단 처리기는
//! try/catch로 잡히지 않는다)을 두고, REPL마다 메모리 상한(QuickJS 힙)을 둔다.

use std::{
    cell::{Cell, RefCell},
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    rc::Rc,
    time::{Duration, Instant},
};

use rquickjs::{
    CatchResultExt, CaughtError, Context, Ctx, Function, Object, Persistent, Promise, Runtime,
    Value, context::EvalOptions, promise::PromiseState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, value::RawValue};

use super::{pipe, sandbox, source};
use crate::{desk::Failure, protocol};

/// JS 힙의 상한. 넘기면 그 할당이 "out of memory"로 실패하고 REPL은 그대로 쓸 수 있다
pub const MEMORY_LIMIT: usize = 32 << 20;
/// JS 호출 깊이를 정하는 스택 상한 (넘기면 RangeError). 워커의 주 스레드 스택(8MiB)보다 작다
const STACK_LIMIT: usize = 1 << 20;
/// 호출 하나의 시간 상한의 최댓값 (데몬도 줄여 보낸다)
pub const MAX_TIMEOUT: Duration = Duration::from_secs(120);
/// 시간 상한 뒤에도 값과 오류를 글로 적는 데 주는 시간
const FORMAT_GRACE: Duration = Duration::from_secs(1);
/// 출력(console과 값)의 글자 수 상한. 넘친 글은 버리고 버린 글자 수를 적는다
pub const OUTPUT_LIMIT: usize = 100_000;
/// 오류 글의 글자 수 상한
pub const ERROR_LIMIT: usize = 10_000;
/// 워커와 데몬이 주고받는 JSON(데스크톱 호출의 인자, 마지막 식의 값)의 바이트 수, 중첩 깊이, 값 수의 상한 (`fits`)
pub const JSON_LIMIT: usize = 1 << 20;
pub const JSON_DEPTH: usize = 100;
pub const JSON_VALUES: usize = 50_000;
/// 데몬이 보내는 한 줄의 상한 (스냅샷 트리, 앱 목록)
const LINE_LIMIT: usize = 32 << 20;
/// 이만큼 아무 호출도 없으면 워커가 스스로 끝난다. 데몬은 그 전에(repl::IDLE) 끝낸다
const IDLE: Duration = Duration::from_secs(70 * 60);
const PRELUDE: &str = include_str!("prelude.js");

/// 데몬 → 워커
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToWorker {
    Eval { code: String, timeout_ms: u64 },
    Reply(Reply),
}

/// 데스크톱 호출의 답. JSON은 값 트리로 풀지 않고 글 그대로 나른다
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    Ok(Box<RawValue>),
    Err { code: i64, message: String },
}

/// 워커 → 데몬. 데몬은 이것을 믿지 않는다: 인자와 값은 글 그대로 받아 크기를 보고(`fits`) 메서드마다 정한 꼴로만
/// 읽는다(repl::Bridge)
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FromWorker {
    Call {
        method: String,
        params: Box<RawValue>,
    },
    Done(Done),
}

/// 코드 하나를 돌린 결과
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Done {
    /// console 출력과 마지막 식의 값을 적은 글
    pub output: String,
    /// 던진 오류 (ERROR_LIMIT 글자까지)
    pub error: Option<String>,
    /// 마지막 식의 값 (JSON으로 바꿀 수 있고 `fits`에 맞으면)
    pub value: Option<Box<RawValue>>,
}

/// JS가 데스크톱을 부르는 길 (데몬에 묻는다)
pub type Host = Box<dyn FnMut(&str, Box<RawValue>) -> Result<Box<RawValue>, Failure>>;

/// 워커와 데몬이 주고받는 JSON이 상한 안인지. 깊거나 값이 많은 JSON은 값 트리로 풀 때 바이트 수의 수십 배 메모리를
/// 쓰고, 너무 깊으면 받는 쪽(serde_json은 128단까지)이 읽지 못한다. 상한을 넘으면 그 까닭(“larger than …”)을 준다
pub fn fits(json: &str) -> Result<(), String> {
    if json.len() > JSON_LIMIT {
        return Err(format!("larger than {} KiB", JSON_LIMIT >> 10));
    }
    let (mut depth, mut deepest, mut values) = (0usize, 0usize, 1usize);
    let (mut quoted, mut escaped) = (false, false);
    for byte in json.bytes() {
        if quoted {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'[' | b'{' => {
                depth += 1;
                deepest = deepest.max(depth);
                values += 1;
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            // 배열의 값과 객체의 키·값 짝은 쉼표로 가른다
            b',' => values += 1,
            _ => {}
        }
    }
    if deepest > JSON_DEPTH {
        return Err(format!("nested more than {JSON_DEPTH} levels deep"));
    }
    if values > JSON_VALUES {
        return Err(format!("made of more than {JSON_VALUES} values"));
    }
    Ok(())
}

/// JS가 만든 JSON을 보낼 꼴로: 상한을 넘거나 JSON이 아니면 그 까닭
fn bounded(json: String) -> Result<Box<RawValue>, String> {
    fits(&json)?;
    RawValue::from_string(json).map_err(|err| err.to_string())
}

/// 글을 `limit` 글자로 자르고 자른 글자 수를 적는다
fn shorten(mut text: String, limit: usize) -> String {
    if let Some((cut, _)) = text.char_indices().nth(limit) {
        let dropped = text[cut..].chars().count();
        text.truncate(cut);
        text.push_str(&format!("…[+{dropped} chars]"));
    }
    text
}

/// 출력: console 줄과 값. OUTPUT_LIMIT 글자를 넘는 것은 버리고 센다
#[derive(Default)]
struct Output {
    text: String,
    chars: usize,
    dropped: usize,
}

impl Output {
    fn line(&mut self, line: &str) {
        let room = OUTPUT_LIMIT.saturating_sub(self.chars);
        let count = line.chars().count() + 1;
        if count <= room {
            self.text.push_str(line);
            self.text.push('\n');
            self.chars += count;
            return;
        }
        let kept: String = line.chars().take(room).collect();
        self.text.push_str(&kept);
        self.chars += room;
        self.dropped += count - room;
    }

    fn take(&mut self) -> String {
        let mut text = std::mem::take(&mut self.text);
        if self.dropped > 0 {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("…[+{} chars of output dropped]\n", self.dropped));
        }
        self.chars = 0;
        self.dropped = 0;
        text
    }
}

/// JS 함수와 Rust가 함께 쓰는 것
struct State {
    /// 지금 호출의 시간 상한. 중단 처리기가 본다
    deadline: Cell<Instant>,
    output: RefCell<Output>,
    /// setTimeout: (때, 번호)
    timers: RefCell<Vec<(Instant, u32)>>,
    host: RefCell<Host>,
}

impl State {
    /// native.call: 데몬에 묻고 답을 `{"ok": …}`나 `{"err": {"code", "message"}}` JSON으로 준다. 상한을 넘는
    /// 인자는 보내지 않는다(데몬은 그런 줄을 받으면 워커를 끝낸다)
    fn call(&self, method: &str, params: String) -> String {
        let reply = bounded(params)
            .map_err(|why| {
                (
                    protocol::INVALID_PARAMS,
                    format!("the arguments of {method} are {why}"),
                )
            })
            .and_then(|params| (self.host.borrow_mut())(method, params));
        match reply {
            Ok(value) => format!("{{\"ok\":{}}}", value.get()),
            Err((code, message)) => {
                json!({ "err": { "code": code, "message": message } }).to_string()
            }
        }
    }

    fn timer(&self, id: u32, ms: f64) {
        let seconds = if ms.is_finite() { ms / 1000.0 } else { 0.0 };
        let delay = Duration::from_secs_f64(seconds.clamp(0.0, MAX_TIMEOUT.as_secs_f64()));
        self.timers.borrow_mut().push((Instant::now() + delay, id));
    }

    fn untimer(&self, id: u32) {
        self.timers.borrow_mut().retain(|(_, known)| *known != id);
    }

    /// 가장 먼저 올 타이머
    fn next_timer(&self) -> Option<(Instant, u32)> {
        self.timers.borrow().iter().min().copied()
    }
}

/// QuickJS 런타임과 REPL 하나. 필드는 적은 차례로 버린다: JS 값(hooks)을 런타임보다 먼저 버려야 한다
pub struct Engine {
    /// prelude가 돌려준 함수: fire(타이머), clear, report(값 적기), describe(오류 적기)
    hooks: Persistent<Object<'static>>,
    context: Context,
    runtime: Runtime,
    state: Rc<State>,
}

/// 코드가 끝나지 못한 까닭
enum Stop<'js> {
    Thrown(Value<'js>),
    TimeLimit,
    NeverSettles,
}

impl Engine {
    pub fn new(host: Host) -> Result<Engine, String> {
        let runtime = Runtime::new().map_err(|err| err.to_string())?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(STACK_LIMIT);
        let state = Rc::new(State {
            deadline: Cell::new(Instant::now() + MAX_TIMEOUT),
            output: RefCell::new(Output::default()),
            timers: RefCell::new(Vec::new()),
            host: RefCell::new(host),
        });
        let watch = state.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || {
            Instant::now() >= watch.deadline.get()
        })));
        let context = Context::full(&runtime).map_err(|err| err.to_string())?;
        let hooks = context
            .with(|ctx| -> Result<_, String> {
                let failed = |err: rquickjs::Error| err.to_string();
                let native = Object::new(ctx.clone()).map_err(failed)?;
                let calls = state.clone();
                let call = Function::new(ctx.clone(), move |method: String, params: String| {
                    calls.call(&method, params)
                });
                native.set("call", call.map_err(failed)?).map_err(failed)?;
                let prints = state.clone();
                let print = Function::new(ctx.clone(), move |text: String| {
                    prints.output.borrow_mut().line(&text)
                });
                native
                    .set("print", print.map_err(failed)?)
                    .map_err(failed)?;
                let timers = state.clone();
                let timer =
                    Function::new(ctx.clone(), move |id: u32, ms: f64| timers.timer(id, ms));
                native
                    .set("timer", timer.map_err(failed)?)
                    .map_err(failed)?;
                let cancels = state.clone();
                let untimer = Function::new(ctx.clone(), move |id: u32| cancels.untimer(id));
                native
                    .set("untimer", untimer.map_err(failed)?)
                    .map_err(failed)?;
                ctx.globals().set("__bunt", native).map_err(failed)?;
                let mut options = EvalOptions::default();
                options.filename = Some("<prelude>".into());
                let hooks: Object = ctx
                    .eval_with_options(PRELUDE, options)
                    .catch(&ctx)
                    .map_err(|err| err.to_string())?;
                Ok(Persistent::save(&ctx, hooks))
            })
            .map_err(|err| format!("cannot start the REPL: {err}"))?;
        Ok(Engine {
            hooks,
            context,
            runtime,
            state,
        })
    }

    /// 코드를 돌려 출력, 오류, 값을 준다. 변수는 다음 호출에 남는다
    pub fn eval(&mut self, code: &str, timeout: Duration) -> Done {
        let timeout = timeout.min(MAX_TIMEOUT);
        self.state.deadline.set(Instant::now() + timeout);
        let source = source::replify(code);
        let state = self.state.clone();
        let hooks = self.hooks.clone();
        let mut done = self.context.with(|ctx| {
            let hooks = match hooks.restore(&ctx) {
                Ok(hooks) => hooks,
                Err(err) => {
                    return Done {
                        error: Some(format!("the REPL is broken: {err}")),
                        ..Done::default()
                    };
                }
            };
            let outcome = run(&ctx, &state, &hooks, source);
            // 시간 상한의 중단도 예외로 온다 (try/catch로 잡히지 않는다): 상한이 지났으면 그 까닭으로 알린다
            let late = Instant::now() >= state.deadline.get();
            // 값과 오류를 적는 JS는 시간 상한이 지났어도 잠깐 돈다
            state.deadline.set(Instant::now() + FORMAT_GRACE);
            let done = finish(&ctx, &state, &hooks, outcome, late, timeout);
            let _ = hooks
                .get::<_, Function>("clear")
                .and_then(|clear| clear.call::<_, ()>(()));
            done
        });
        self.state.timers.borrow_mut().clear();
        self.runtime.run_gc();
        done.output = self.state.output.borrow_mut().take() + &done.output;
        done
    }
}

/// 코드를 돌리고 약속이 끝날 때까지 일과 타이머를 돌린다
fn run<'js>(
    ctx: &Ctx<'js>,
    state: &State,
    hooks: &Object<'js>,
    source: String,
) -> Result<Value<'js>, Stop<'js>> {
    let mut options = EvalOptions::default();
    options.global = true;
    // 선언 없이 쓴 이름(x = 1)도 전역으로 남긴다 (브라우저 콘솔처럼)
    options.strict = false;
    // 최상위 await
    options.promise = true;
    options.filename = Some("<repl>".into());
    let promise: Promise = ctx
        .eval_with_options(source, options)
        .catch(ctx)
        .map_err(|caught| Stop::Thrown(thrown(ctx, caught)))?;
    loop {
        while ctx.execute_pending_job() {}
        match promise.state() {
            PromiseState::Resolved => {
                // 최상위 await를 쓴 전역 코드의 약속은 { value: 마지막 식의 값 }으로 끝난다. 값이 없는 문장(선언)은
                // value가 없다
                let value = promise
                    .result::<Object>()
                    .and_then(Result::ok)
                    .and_then(|wrapper| wrapper.get::<_, Value>("value").ok());
                return Ok(value.unwrap_or_else(|| Value::new_undefined(ctx.clone())));
            }
            PromiseState::Rejected => {
                let error = match promise.result::<Value>() {
                    Some(Err(_)) => ctx.catch(),
                    Some(Ok(value)) => value,
                    None => Value::new_undefined(ctx.clone()),
                };
                return Err(Stop::Thrown(error));
            }
            PromiseState::Pending => {}
        }
        let deadline = state.deadline.get();
        if Instant::now() >= deadline {
            return Err(Stop::TimeLimit);
        }
        let Some((due, id)) = state.next_timer() else {
            return Err(Stop::NeverSettles);
        };
        let wake = due.min(deadline);
        let now = Instant::now();
        if wake > now {
            std::thread::sleep(wake - now);
        }
        if Instant::now() < due {
            continue;
        }
        state.untimer(id);
        let fired = hooks
            .get::<_, Function>("fire")
            .and_then(|fire| fire.call::<_, ()>((id,)));
        if let Err(caught) = fired.catch(ctx) {
            // 타이머의 오류는 그 호출을 끝내지 않고 알리기만 한다
            let error = thrown(ctx, caught);
            let shown = describe(hooks, error);
            state
                .output
                .borrow_mut()
                .line(&format!("Uncaught in a timer: {shown}"));
        }
    }
}

/// 잡은 예외의 JS 값
fn thrown<'js>(ctx: &Ctx<'js>, caught: CaughtError<'js>) -> Value<'js> {
    match caught {
        CaughtError::Exception(exception) => exception.into_value(),
        CaughtError::Value(value) => value,
        CaughtError::Error(err) => rquickjs::String::from_str(ctx.clone(), &err.to_string())
            .map(|text| text.into_value())
            .unwrap_or_else(|_| Value::new_undefined(ctx.clone())),
    }
}

fn describe<'js>(hooks: &Object<'js>, error: Value<'js>) -> String {
    hooks
        .get::<_, Function>("describe")
        .and_then(|describe| describe.call::<_, String>((error,)))
        .unwrap_or_else(|_| "an error that cannot be shown".into())
}

/// 결과를 글로 적는다
fn finish<'js>(
    ctx: &Ctx<'js>,
    state: &State,
    hooks: &Object<'js>,
    outcome: Result<Value<'js>, Stop<'js>>,
    late: bool,
    timeout: Duration,
) -> Done {
    let value = match outcome {
        Ok(value) => value,
        Err(stop) => {
            let error = match stop {
                Stop::TimeLimit => time_limit(timeout),
                Stop::Thrown(_) if late => time_limit(timeout),
                // 오류 글도 줄이 넘치지 않게 자른다 (출력처럼)
                Stop::Thrown(error) => hint(shorten(describe(hooks, error), ERROR_LIMIT)),
                Stop::NeverSettles => "the code awaits a promise that never settles (no timers \
                                       are left to settle it)"
                    .into(),
            };
            return Done {
                error: Some(error),
                ..Done::default()
            };
        }
    };
    let report = hooks
        .get::<_, Function>("report")
        .and_then(|report| report.call::<_, Object>((value,)))
        .catch(ctx);
    let (text, json) = match report {
        Ok(report) => (
            report.get::<_, Option<String>>("text").ok().flatten(),
            report.get::<_, Option<String>>("json").ok().flatten(),
        ),
        Err(err) => (Some(format!("(the value cannot be shown: {err})")), None),
    };
    if let Some(text) = text {
        state.output.borrow_mut().line(&text);
    }
    Done {
        output: String::new(),
        error: None,
        // 상한을 넘는 값은 글(output)로만 보인다
        value: json.and_then(|json| bounded(json).ok()),
    }
}

fn time_limit(timeout: Duration) -> String {
    format!(
        "the code ran past its time limit ({} ms) and was stopped; what it set before that is kept. \
         Pass a longer timeout, or split the work into several calls",
        timeout.as_millis()
    )
}

/// 잘 나는 오류에 할 일을 붙인다
fn hint(error: String) -> String {
    if error.contains("out of memory") {
        return format!(
            "{error}\n(this REPL may use {} MiB of memory; drop big variables or reset it)",
            MEMORY_LIMIT >> 20
        );
    }
    if error.contains("redeclaration of") {
        return format!(
            "{error}\n(top-level let and const may be declared again in a later call, but classes \
             may not: assign them instead, like `Name = class {{ … }}`)"
        );
    }
    error
}

/// 워커가 데몬과 잇는 표준 입출력
struct Io<R> {
    lines: pipe::Lines<R>,
    out: Box<dyn Write>,
}

impl<R: Read + AsRawFd> Io<R> {
    /// 데몬에 데스크톱 호출을 보내고 답을 기다린다 (데몬이 정책을 보고 대신 한다)
    fn call(&mut self, method: &str, params: Box<RawValue>) -> Result<Box<RawValue>, Failure> {
        let gone = |err: io::Error| {
            (
                protocol::AGENT_ERROR,
                format!("lost the daemon of this REPL: {err}"),
            )
        };
        let call = FromWorker::Call {
            method: method.to_string(),
            params,
        };
        pipe::send(&mut self.out, &call).map_err(gone)?;
        let line = self
            .lines
            .next(None)
            .map_err(gone)?
            .ok_or_else(|| gone(io::ErrorKind::TimedOut.into()))?;
        match serde_json::from_str(&line) {
            Ok(ToWorker::Reply(Reply::Ok(value))) => Ok(value),
            Ok(ToWorker::Reply(Reply::Err { code, message })) => Err((code, message)),
            _ => Err(gone(io::ErrorKind::InvalidData.into())),
        }
    }
}

/// REPL 하나를 연 워커: 엔진과 데몬과의 입출력
pub struct Session<R> {
    engine: Engine,
    io: Rc<RefCell<Io<R>>>,
}

impl<R: Read + AsRawFd + 'static> Session<R> {
    /// 엔진을 만들고 prelude를 돌린다. 워커는 이것을 가두기(seccomp) 전에 한다
    pub fn start(input: R, output: impl Write + 'static) -> io::Result<Session<R>> {
        let io = Rc::new(RefCell::new(Io {
            lines: pipe::Lines::new(input, LINE_LIMIT),
            out: Box::new(output),
        }));
        let host_io = io.clone();
        let host: Host = Box::new(move |method, params| host_io.borrow_mut().call(method, params));
        let engine = Engine::new(host).map_err(io::Error::other)?;
        Ok(Session { engine, io })
    }

    /// 데몬이 닫거나 `idle` 동안 말이 없을 때까지 코드를 돌린다
    pub fn serve(mut self, idle: Option<Duration>) -> io::Result<()> {
        loop {
            let next = self
                .io
                .borrow_mut()
                .lines
                .next(idle.map(|idle| Instant::now() + idle));
            let line = match next {
                Ok(Some(line)) => line,
                // 오래 쓰지 않았거나 데몬이 닫았다
                Ok(None) => return Ok(()),
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(err) => return Err(err),
            };
            let Ok(ToWorker::Eval { code, timeout_ms }) = serde_json::from_str(&line) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "expected code to run",
                ));
            };
            let done = self.engine.eval(&code, Duration::from_millis(timeout_ms));
            pipe::send(&mut self.io.borrow_mut().out, &FromWorker::Done(done))?;
        }
    }
}

unsafe extern "C" {
    /// 시간대를 읽어 둔다 (POSIX, glibc)
    fn tzset();
}

/// `buntd-agent --repl-worker`: 상속한 것을 닫고, 엔진을 만들고, 가둔 뒤 데몬이 보내는 코드를 돌린다
pub fn main() -> ! {
    sandbox::close_inherited_fds();
    // 시간대 파일은 가두기 전에 읽어 둔다 (Date의 지역 시간)
    // 안전: 다른 스레드가 없다
    unsafe { tzset() };
    if let Err(err) = sandbox::limit_resources() {
        eprintln!("buntd-agent: cannot limit the REPL worker: {err}");
        std::process::exit(2);
    }
    // 안전: 표준 입출력은 이 프로세스만 쓴다. std의 Stdin은 버퍼를 두어 poll과 어긋나므로 파일로 바로 읽고 쓴다
    let (input, output) = unsafe { (File::from_raw_fd(0), File::from_raw_fd(1)) };
    let session = match Session::start(input, output) {
        Ok(session) => session,
        Err(err) => {
            eprintln!("buntd-agent: cannot start the REPL worker: {err}");
            std::process::exit(2);
        }
    };
    if let Err(err) = sandbox::confine() {
        // 가두지 못하면 JS를 돌리지 않는다
        eprintln!("buntd-agent: cannot sandbox the REPL worker: {err}");
        std::process::exit(2);
    }
    let code = match session.serve(Some(IDLE)) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("buntd-agent: REPL worker: {err}");
            1
        }
    };
    std::process::exit(code)
}
