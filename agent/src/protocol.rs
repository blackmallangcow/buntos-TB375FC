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
}

/// `snapshot`의 답
#[derive(Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub tree: String,
    /// 같은 세션에서 같은 범위를 지난번에 읽은 트리와 견준 차이 (unified diff, 앞뒤 줄 없음)
    pub diff: String,
    /// 이 범위를 이 세션에서 처음 읽었다 (diff가 비어 있다)
    pub first: bool,
}

/// `act`의 인자: ref가 가리키는 요소에 동작 하나
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ActParams {
    pub session: Option<String>,
    /// 지난 스냅샷의 ref (예: "e12")
    #[serde(rename = "ref")]
    pub target: String,
    /// click, fill, set, focus, select, press
    pub action: String,
    /// fill의 글, set의 값, press의 키
    pub value: Option<String>,
    /// click을 접근성 동작 대신 포인터로 한다 (동작이 엉뚱하게 구는 앱에서)
    pub pointer: bool,
}

/// `act`의 답
#[derive(Debug, Serialize, Deserialize)]
pub struct Acted {
    /// 한 일 (예: "clicked with the accessible action \"click\"")
    pub done: String,
    /// 동작 전과 화면이 멈춘 뒤의 그 창 트리의 차이
    pub diff: String,
    /// 동작 뒤 화면이 멈출 때까지 걸린 시간
    pub settled_ms: u64,
    /// 동작 뒤 같은 앱에 새로 뜬 창의 제목
    pub new_windows: Vec<String>,
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

/// 데몬에 요청 하나를 보내고 답을 기다린다
pub fn call(method: &str, params: Value) -> Result<Value, String> {
    let path = socket_path().ok_or("XDG_RUNTIME_DIR is not set")?;
    let stream = UnixStream::connect(&path)
        .map_err(|err| format!("cannot reach buntd-agent at {}: {err}", path.display()))?;
    let request = Request {
        jsonrpc: "2.0".into(),
        id: Value::from(1),
        method: method.into(),
        params,
    };
    let mut line = serde_json::to_string(&request).map_err(|err| err.to_string())?;
    line.push('\n');
    (&stream)
        .write_all(line.as_bytes())
        .map_err(|err| format!("cannot send to buntd-agent: {err}"))?;
    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .map_err(|err| format!("no reply from buntd-agent: {err}"))?;
    let response: Response =
        serde_json::from_str(&reply).map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    match (response.result, response.error) {
        (_, Some(error)) => Err(error.message),
        (result, None) => Ok(result.unwrap_or(Value::Null)),
    }
}
