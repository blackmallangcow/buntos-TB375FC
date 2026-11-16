// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 데몬(buntd-agent)과 클라이언트(bunt CLI, 앞으로 MCP 어댑터) 사이의 JSON-RPC 2.0.
//! 유닉스 소켓에 한 줄에 요청 하나, 한 줄에 답 하나를 쓴다.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `$XDG_RUNTIME_DIR` 아래의 소켓 (data/buntd-agent.socket과 같다)
pub const SOCKET: &str = "buntos/agent.sock";

pub fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").map(|dir| PathBuf::from(dir).join(SOCKET))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

pub const PARSE_ERROR: i64 = -32700;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
/// 접근성 버스나 앱에서 난 오류
pub const AGENT_ERROR: i64 = -32000;
/// 사용자가 이 클라이언트(AI)에게 이 컴퓨터의 제어를 허용하지 않았다 (시스템 설정 > AI 에이전트)
pub const NOT_ALLOWED: i64 = -32001;
/// 사용자가 AI에게서 막은 앱
pub const BLOCKED_APP: i64 = -32002;
/// 화면이 잠겼다. 잠긴 동안 에이전트는 아무것도 읽거나 하지 못하고, 사람이 잠금 화면에 암호를 쳐야만 풀린다
pub const SCREEN_LOCKED: i64 = -32003;
/// 기다리던 것이 시간 상한 안에 일어나지 않았다 (`wait`)
pub const TIMEOUT: i64 = -32004;
/// 사람이 키보드 포커스를 다른 창으로 옮겨, 그 창에 보내려던 키·글·휠을 보내지 않았다. 되빼앗지 않는다: ref나
/// `--app`으로 대상을 다시 고른다
pub const FOCUS_MOVED: i64 = -32005;
/// 사람이 지금 컴퓨터를 쓰고 있어(키를 치거나 포인터를 움직이는 중) 아무것도 보내지 않았다. 알림 글의
/// "try again in N ms"가 다시 해 볼 때까지의 시간이다
pub const USER_ACTIVE: i64 = -32006;
/// ref, 동작, 기다리기의 대상 앱(창)이 끝났거나 다시 떴다(pid가 바뀌었다). 받은 ref와 기다리던 것은 그 앱의
/// 것이었으므로 새로 스냅샷을 읽는다
pub const TARGET_GONE: i64 = -32007;
/// 사람이 AI 제어를 멈췄다. 읽기는 되고, 다시 하기는 사람만 누른다
pub const PAUSED: i64 = -32008;

/// 데몬 알림(AGENT_ERROR)의 앞머리: `app`에 맞는 앱이 없다. 기다리기는 이것을 그 앱이 없는 것으로 읽는다
pub const NO_APP_MATCHES: &str = "no app matches";
/// 데몬 알림의 일부: `app`이 여러 앱에 맞는다. 기다리기는 기다려도 하나로 정해지지 않으므로 바로 알린다
pub const BE_MORE_SPECIFIC: &str = "be more specific";
/// 데몬 알림의 일부: ref가 가리키던 요소의 앱이 끝났다. 기다리기는 이것도 그 앱이 없는 것으로 읽는다
pub const APP_QUIT: &str = "its app quit";

/// `snapshot`의 인자
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapshotParams {
    /// 세션 이름 (기본 "default"). 같은 세션의 스냅샷은 ref를 같이 쓰고 diff를 만든다
    pub session: Option<String>,
    /// 앱 이름, 창 제목 일부, pid, 접근성 버스 이름. 없으면 활성 창
    pub app: Option<String>,
    /// 이 ref 아래만 (예: "e12")
    #[serde(rename = "ref")]
    pub scope: Option<String>,
    /// 상호작용할 수 있는 요소만
    pub interactive: bool,
    /// 숨은 요소도
    pub hidden: bool,
    /// 읽기만 하고 다음 diff가 견줄 지난 트리는 그대로 둔다. 기다리기(`wait`)가 다시 읽을 때 쓴다: 다음
    /// 스냅샷의 diff는 기다리는 동안 읽은 것이 아니라 AI가 지난번에 본 트리와 견준 것이어야 한다
    pub peek: bool,
    /// diff가 이보다 많은 줄(지운 줄과 더한 줄)이면 줄 대신 요약 한 줄을 준다. 없으면 400 (diff::MAX_LINES)
    pub max_lines: Option<usize>,
}

/// `snapshot`의 답
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub tree: String,
    /// 같은 세션에서 같은 범위를 지난번에 읽은 트리와 견준 차이 (unified diff, 앞뒤 줄 없음)
    pub diff: String,
    /// 이 범위를 이 세션에서 처음 읽었다 (diff가 비어 있다)
    pub first: bool,
    /// 범위 안의 보이는 문서가 불러오는 중이다 (BUSY). 기다리기(`wait --loaded`, 창 기다리기)가 본다
    #[serde(default)]
    pub busy: bool,
}

/// `act`의 인자: ref가 가리키는 요소에 동작 하나
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ActParams {
    pub session: Option<String>,
    /// 지난 스냅샷의 ref (예: "e12")
    #[serde(rename = "ref")]
    pub target: String,
    /// click, fill, set, focus, select, press, type, scroll
    pub action: String,
    /// fill의 글, set의 값, press의 키, type의 글
    pub value: Option<String>,
    /// click을 접근성 동작 대신 포인터로 한다 (동작이 엉뚱하게 구는 앱에서)
    pub pointer: bool,
    /// click을 ref 대신 창 안의 이 점에 포인터로 한다. 창 스크린샷(`screenshot`)의 픽셀이고 창의 왼쪽
    /// 위가 (0, 0)이다
    pub at: Option<[f64; 2]>,
    /// 동작할 창의 앱 (`snapshot`의 app과 같다). ref가 없는 동작(click --at, press, type, scroll)은 그
    /// 앱의 창에 하고, 없으면 키보드 포커스를 가진 창에 한다. ref가 있으면 ref가 그 앱의 것인지 본다
    pub app: Option<String>,
    /// scroll에서 돌릴 휠 눈금 수 (가로, 세로. 오른쪽·아래가 +). 없으면 ref가 보이게 스크롤한다
    pub amount: Option<[f64; 2]>,
    /// press·type에 ref가 있을 때 그 창만 앞으로 가져오고 키보드 포커스는 창 안에서 옮기지 않는다
    pub keep_focus: bool,
    /// type에서 키보드 포커스를 가진 앱이 글 입력을 켜지 않았으면 글자마다 키를 친다. 없으면 치지 않고
    /// 오류다
    pub keys: bool,
    /// 동작 뒤 화면이 멈추기를 기다리는 시간의 상한(ms). 없으면 3000
    pub settle_ms: Option<u64>,
    /// diff가 이보다 많은 줄(지운 줄과 더한 줄)이면 줄 대신 요약 한 줄을 준다. 없으면 400 (diff::MAX_LINES)
    pub max_lines: Option<usize>,
}

/// `act`의 답
#[derive(Debug, Serialize, Deserialize)]
pub struct Acted {
    /// 한 일 (예: "clicked with the accessible action \"click\"")
    pub done: String,
    /// 동작 전과 화면이 멈춘 뒤의 그 창 트리의 차이
    pub diff: String,
    /// 동작 뒤 화면이 멈출 때까지 걸린 시간 (멈추지 않았으면 기다린 시간)
    pub settled_ms: u64,
    /// 기다리는 동안 화면이 멈췄다. 거짓이면 상한까지 바뀌고 있었거나 바뀐 것을 아직 보지 못했다
    #[serde(default = "yes")]
    pub settled: bool,
    /// 그 창의 접근성 트리를 읽어 diff를 만들었다. 거짓이면 트리가 없는 창이라 diff가 없다
    #[serde(default = "yes")]
    pub observed: bool,
    /// 동작 뒤 같은 앱에 새로 뜬 창의 제목
    pub new_windows: Vec<String>,
}

fn yes() -> bool {
    true
}

/// `screenshot`의 인자
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenshotParams {
    /// 찍을 창의 앱 (`snapshot`의 app과 같다). 없으면 키보드 포커스를 가진 창
    pub app: Option<String>,
    /// PNG를 쓸 절대 경로. 없으면 `$XDG_RUNTIME_DIR/buntos/screenshots` 아래에 새 파일
    pub out: Option<String>,
}

/// `screenshot`의 답: 창 하나만 그린 PNG. 창 영역의 왼쪽 위가 (0, 0)이고 픽셀은 화면의 물리 픽셀이다
#[derive(Debug, Serialize, Deserialize)]
pub struct Screenshot {
    pub path: String,
    pub width: u32,
    pub height: u32,
    /// 그 창이 있는 화면의 배율 (픽셀 = 논리 좌표 × 배율)
    pub scale: f64,
    /// 창 제목
    pub window: String,
    /// Wayland 앱 ID (= 데스크톱 ID)
    pub app_id: String,
    pub pid: u32,
}

/// `launch`의 인자
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchParams {
    pub session: Option<String>,
    /// 데스크톱 ID나 앱 이름 (일부만 써도 하나로 정해지면 된다)
    pub app: String,
}

/// `launch`의 답
#[derive(Debug, Serialize, Deserialize)]
pub struct Launched {
    pub name: String,
    pub id: String,
    /// 새로 뜬 창의 제목. 기다리는 동안 창이 뜨지 않았으면 None
    pub window: Option<String>,
    /// 이미 창이 있던 앱이라 새로 띄우지 않고 그 창을 앞으로 가져왔다
    pub running: bool,
    /// 그 창의 상호작용 요소 트리 (접근성 트리를 내놓지 않는 창이면 빈 글)
    pub tree: String,
}

/// 설치된 앱 하나 (`apps`의 installed)
#[derive(Debug, Serialize, Deserialize)]
pub struct Installed {
    pub id: String,
    pub name: String,
}

/// `apps`의 답 한 줄
#[derive(Debug, Serialize, Deserialize)]
pub struct App {
    pub name: String,
    /// 접근성 버스의 고유 이름
    pub bus: String,
    pub pid: Option<u32>,
    pub windows: Vec<Window>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Window {
    pub title: String,
    pub active: bool,
}

/// `apps`의 답에서 접근성 트리를 내놓지 않는 창 (컴포지터에는 있다). 접근성을 켜기 전에 뜬 Chromium,
/// Electron, Firefox, Qt 앱이 그렇다. screenshot과 click --at은 이 창에도 쓴다
#[derive(Debug, Serialize, Deserialize)]
pub struct Untracked {
    /// 설치된 앱의 이름. 모르면 앱 ID
    pub name: String,
    pub app_id: String,
    pub pid: u32,
    pub title: String,
    /// 접근성을 아예 지원하지 않는 앱이다(foot 같은 터미널). 거짓이면 다시 켜면 트리를 내놓는다
    #[serde(default)]
    pub unsupported: bool,
}

/// `wait`의 인자: 범위(ref 아래, 앱의 창, 없으면 활성 창)를 다시 읽으며 조건이 맞을 때까지 기다린다.
/// 조건은 하나다: 요소(role·name·text), 창(window), ref 요소의 값(value, changed), 또는 ref 요소 자체
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WaitParams {
    pub session: Option<String>,
    /// 범위: 이 앱의 창 (`snapshot`의 app과 같다)
    pub app: Option<String>,
    /// 범위: 이 ref 아래. 조건이 없으면 이 요소가 보이기(gone이면 사라지기)를 기다린다
    #[serde(rename = "ref")]
    pub scope: Option<String>,
    /// 역할이 이것인 요소 (ARIA 이름: button, textbox, listitem …)
    pub role: Option<String>,
    /// 이름에 이 글이 든 요소
    pub name: Option<String>,
    /// 이름, 글 줄, 값에 이 글이 든 줄
    pub text: Option<String>,
    /// name·text·window가 통째로 같아야 한다 (아니면 대소문자를 가리지 않고 일부만 맞아도 된다)
    pub exact: bool,
    /// 맞는 요소가 여럿일 때 고를 차례 (0부터, 음수는 끝에서)
    pub nth: Option<i64>,
    /// 제목에 이 글이 든 창. 빈 글이면 기다리기 시작한 뒤 새로 뜬 창
    pub window: Option<String>,
    /// ref 요소의 값(값이 없으면 이름)이 이것이 될 때까지
    pub value: Option<String>,
    /// ref 요소의 값(값이 없으면 이름)이 처음 본 것과 달라질 때까지
    pub changed: bool,
    /// 나타나기 대신 사라지기(창은 닫히기)를 기다린다
    pub gone: bool,
    /// 범위의 보이는 문서가 다 불러올 때까지(BUSY가 풀릴 때까지). window와 함께 주면 창이 뜬 뒤 다 불러오지
    /// 못하고 시간이 다 되었을 때 성공 대신 TIMEOUT이다
    pub loaded: bool,
    /// 기다리는 시간의 상한(ms). 0이면 한 번만 본다. 없으면 10초
    pub timeout_ms: Option<u64>,
}

/// `wait`가 찾은 요소 한 줄
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Found {
    /// 동작에 쓸 ref. 줄에 ref가 없으면(글 줄) 가장 가까운 조상의 ref다
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    /// 트리 글의 그 줄 (들여쓰기 뺀 것)
    pub line: String,
    pub role: String,
    pub name: String,
    /// 글 칸·막대의 값 (`value=`)
    pub value: Option<String>,
    /// 줄 끝의 상태 (focused, checked, disabled …)
    pub states: Vec<String>,
    /// 그 요소와 아래 줄의 이름·글을 이은 것
    pub text: String,
}

/// `wait`의 답
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Waited {
    /// 본 것 (예: `button "OK" appeared`)
    pub done: String,
    pub waited_ms: u64,
    /// 맞는 요소 (nth로 고른 것. 사라지기를 기다렸으면 없다)
    pub found: Option<Found>,
    /// 맞는 요소 수와 앞의 몇 개
    pub count: usize,
    pub matches: Vec<Found>,
    /// 창을 기다렸으면 그 창의 제목
    pub window: Option<String>,
}

/// `repl`의 인자: JavaScript를 이 클라이언트(AI)의 REPL에서 돌린다. 변수는 호출 사이에 남는다
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplParams {
    pub code: String,
    /// REPL과 ref·diff를 나누는 세션 (기본 "default"). 세션마다 변수가 따로다
    pub session: Option<String>,
    /// 실행 시간의 상한(ms). 없으면 30초, 많아야 120초
    pub timeout_ms: Option<u64>,
    /// 먼저 새 REPL로 바꾼다 (지난 변수를 버린다)
    pub reset: bool,
    /// 이번 호출에 찍은 스크린샷을 답에 base64 PNG로 넣는다 (MCP 어댑터가 그림으로 넘긴다)
    pub images: bool,
}

/// `repl`의 답
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Repl {
    /// console 출력과 마지막 식의 값을 `bunt agent`처럼 적은 글
    pub output: String,
    /// 코드가 던진 오류 (없으면 성공)
    pub error: Option<String>,
    /// 마지막 식의 값을 JSON으로 (바꿀 수 없거나 너무 크면 없다)
    pub value: Option<Value>,
    /// 새 REPL에서 돌았다 (지난 변수가 없다)
    pub fresh: bool,
    pub ms: u64,
    /// 이번 호출에 찍은 스크린샷 (`images`일 때만 data가 있다)
    pub images: Vec<ReplImage>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ReplImage {
    pub path: String,
    pub mime: String,
    /// base64
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// `skills`의 인자: 이름이 없으면 목록, 있으면 그 안내 글
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsParams {
    pub name: Option<String>,
}

/// 사용 안내 하나 (Markdown)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// 데몬에 요청 하나를 보내고 답을 기다린다
pub fn call(method: &str, params: Value) -> Result<Value, String> {
    request(method, params).map_err(|error| error.message)
}

/// `call`과 같고, 실패하면 오류 번호도 준다. 데몬에 닿지 못하면 AGENT_ERROR다
pub fn request(method: &str, params: Value) -> Result<Value, RpcError> {
    let unreachable = |message: String| RpcError {
        code: AGENT_ERROR,
        message,
    };
    let path = socket_path().ok_or_else(|| unreachable("XDG_RUNTIME_DIR is not set".into()))?;
    let stream = UnixStream::connect(&path).map_err(|err| {
        unreachable(format!(
            "cannot reach buntd-agent at {}: {err}",
            path.display()
        ))
    })?;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: Value::from(1),
        method: method.into(),
        params,
    };
    let mut line = serde_json::to_string(&request).map_err(|err| unreachable(err.to_string()))?;
    line.push('\n');
    (&stream)
        .write_all(line.as_bytes())
        .map_err(|err| unreachable(format!("cannot send to buntd-agent: {err}")))?;
    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .map_err(|err| unreachable(format!("no reply from buntd-agent: {err}")))?;
    let response: Response = serde_json::from_str(&reply)
        .map_err(|err| unreachable(format!("bad reply from buntd-agent: {err}")))?;
    match (response.result, response.error) {
        (_, Some(error)) => Err(error),
        (result, None) => Ok(result.unwrap_or(Value::Null)),
    }
}
