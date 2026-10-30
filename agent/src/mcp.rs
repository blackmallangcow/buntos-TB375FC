// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MCP 어댑터 (`bunt mcp`, ARCHITECTURE 6.3·6.6절 3단계). 셸이 없는 AI 앱(채팅 앱)과 셸 권한을 주기 싫은 하네스가
//! 표준 입출력의 MCP로 붙는다. 같은 소켓으로 데몬에 묻는 얇은 어댑터라 정책(허용한 AI, 막은 앱, 사람만 답하는 창,
//! 잠금)은 데몬이 본다. 데몬은 소켓에 붙은 `bunt`을 건너뛰고 이것을 띄운 MCP 클라이언트를 AI로 알아본다.
//!
//! 도구는 Aside의 MCP처럼 작게 둘이다: repl(JS를 돌리고, 그 호출에 찍은 스크린샷은 그림으로 돌려준다)과
//! skills(사용 안내). 메시지는 한 줄에 JSON-RPC 2.0 하나다. 2024-11-05부터 2025-11-25까지의 initialize와
//! 2026-07-28의 server/discover(요청마다 `_meta`에 판을 싣는 상태 없는 판)를 다 받는다.
//!
//! 공식 SDK(rmcp, Apache-2.0)는 tokio와 매크로를 끌어오는데 메서드가 넷뿐이라 serde_json으로 직접 쓴다.

use std::{
    io::{self, BufRead, Write},
    sync::{Mutex, mpsc},
    thread,
};

use serde_json::{Value, json};

use crate::protocol::{self, Repl, RpcError};

/// 받는 판. 앞이 새것이다. initialize는 상태 없는 2026-07-28을 고르지 않는다
pub const VERSIONS: [&str; 5] = [
    "2026-07-28",
    "2025-11-25",
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];
/// initialize로 맺는 판 가운데 가장 새것
const LEGACY: &str = "2025-11-25";
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const SERVER_KEY: &str = "io.modelcontextprotocol/serverInfo";
/// 판이 맞지 않을 때의 오류 번호 (2026-07-28)
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;
const INVALID_REQUEST: i64 = -32600;
/// 도구 목록을 캐시해도 되는 시간 (2026-07-28의 CacheableResult). 꾸러미가 바뀌기 전에는 같다
const TTL_MS: u64 = 60 * 60 * 1000;

const INSTRUCTIONS: &str = "\
buntOS desktop. The repl tool runs JavaScript with a Playwright-like `desktop` object that reads \
and controls the apps on this computer through accessibility trees; the skills tool has the \
guides (read `repl` first). The user decides which AIs may control this computer and which apps \
are off limits; refused calls say what to ask the user. Prefer snapshots, refs and getByRole over \
screenshots, and wait for results instead of sleeping.";

const REPL_DESCRIPTION: &str = "\
Run JavaScript that reads and controls the apps on this buntOS desktop through accessibility \
trees. The API copies Playwright: `const calc = desktop.app('org.gnome.Calculator')`; \
`await calc.snapshot({ interactive: true })` gives a compact tree with refs \
(`- button \"=\" [ref=e185]`); `await calc.locator('e185').click()` or \
`await calc.getByRole('button', { name: '7', exact: true }).click()` acts and returns what changed \
in the window; also fill, type, press, keyboard, mouse, scroll, wait({ text }), \
waitForWindow(), screenshot() (returned as an image), desktop.apps(), desktop.launch(id). \
Top-level await works, the value of the last expression comes back, and variables stay between \
calls. There is no file, network or process access. The skills tool's `repl` guide has the whole \
API.";

/// 데몬에 묻는 길 (시험은 가짜를 쓴다)
pub trait Daemon: Sync {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError>;
}

/// 소켓(`$XDG_RUNTIME_DIR/buntos/agent.sock`)의 buntd-agent
pub struct Socket;

impl Daemon for Socket {
    fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        protocol::request(method, params)
    }
}

pub struct Server<D> {
    daemon: D,
    /// REPL과 ref의 세션
    session: String,
}

/// (오류 번호, 알림, 덧붙일 것)
type Failure = (i64, String, Option<Value>);

fn failure(code: i64, message: impl Into<String>) -> Failure {
    (code, message.into(), None)
}

impl<D: Daemon> Server<D> {
    pub fn new(daemon: D, session: String) -> Server<D> {
        Server { daemon, session }
    }

    /// 메시지 하나에 답한다. 알림(id가 없다)과 답(method가 없다)에는 답하지 않는다
    pub fn handle(&self, message: Value) -> Option<Value> {
        if let Value::Array(batch) = message {
            // 2025-03-26의 묶음
            let replies: Vec<Value> = batch
                .into_iter()
                .filter_map(|message| self.handle(message))
                .collect();
            return (!replies.is_empty()).then_some(Value::Array(replies));
        }
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        let (id, method) = match (id, method) {
            (Some(id), Some(method)) => (id, method),
            (None, Some(_)) | (Some(_), None) => return None,
            (None, None) => {
                return Some(error_reply(
                    Value::Null,
                    failure(INVALID_REQUEST, "not a JSON-RPC request"),
                ));
            }
        };
        let params = message.get("params").cloned().unwrap_or(json!({}));
        Some(match self.dispatch(method, &params) {
            Ok(mut result) => {
                // 2026-07-28: 답마다 서버를 알린다
                result["_meta"] = json!({ SERVER_KEY: server_info() });
                json!({ "jsonrpc": "2.0", "id": id, "result": result })
            }
            Err(failure) => error_reply(id, failure),
        })
    }

    fn dispatch(&self, method: &str, params: &Value) -> Result<Value, Failure> {
        // 상태 없는 판은 요청마다 판을 싣는다
        if let Some(requested) = params["_meta"][VERSION_KEY].as_str()
            && !VERSIONS.contains(&requested)
        {
            return Err((
                UNSUPPORTED_PROTOCOL_VERSION,
                format!("protocol version {requested} is not supported"),
                Some(json!({ "supported": VERSIONS, "requested": requested })),
            ));
        }
        match method {
            "initialize" => {
                let requested = params["protocolVersion"].as_str().unwrap_or(LEGACY);
                let version = if VERSIONS[1..].contains(&requested) {
                    requested
                } else {
                    LEGACY
                };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": server_info(),
                    "instructions": INSTRUCTIONS,
                }))
            }
            "server/discover" => Ok(json!({
                "supportedVersions": VERSIONS,
                "capabilities": { "tools": {} },
                "instructions": INSTRUCTIONS,
                "ttlMs": TTL_MS,
                "cacheScope": "public",
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": tools(),
                "ttlMs": TTL_MS,
                "cacheScope": "public",
            })),
            "tools/call" => self.call_tool(params),
            other => Err(failure(
                protocol::METHOD_NOT_FOUND,
                format!("unknown method {other}"),
            )),
        }
    }

    /// 도구 하나를 부른다. 도구의 실패(잘못된 인자, 데몬의 거절, JS 오류)는 모델이 읽고 고치도록 isError인 결과로
    /// 돌려준다. 없는 도구만 프로토콜 오류다
    fn call_tool(&self, params: &Value) -> Result<Value, Failure> {
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        match params["name"].as_str() {
            Some("repl") => Ok(self.repl(&arguments).unwrap_or_else(tool_error)),
            Some("skills") => Ok(self.skills(&arguments).unwrap_or_else(tool_error)),
            Some(other) => Err(failure(
                protocol::INVALID_PARAMS,
                format!("unknown tool {other:?}; there are repl and skills"),
            )),
            None => Err(failure(protocol::INVALID_PARAMS, "tools/call needs a name")),
        }
    }

    fn repl(&self, arguments: &Value) -> Result<Value, String> {
        let code = arguments["code"]
            .as_str()
            .ok_or("repl needs `code`: the JavaScript to run")?;
        let timeout = match &arguments["timeout"] {
            Value::Null => None,
            value => Some(
                value
                    .as_u64()
                    .ok_or("`timeout` is a whole number of milliseconds")?,
            ),
        };
        let reset = match &arguments["reset"] {
            Value::Null => false,
            value => value.as_bool().ok_or("`reset` is true or false")?,
        };
        let params = json!({
            "code": code,
            "session": self.session,
            "timeout_ms": timeout,
            "reset": reset,
            "images": true,
        });
        let reply = self.daemon.call("repl", params).map_err(refusal)?;
        let repl: Repl = serde_json::from_value(reply)
            .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
        let mut text = repl.output.trim_end_matches('\n').to_string();
        if let Some(error) = &repl.error {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(error);
        }
        if text.is_empty() {
            text = "undefined".into();
        }
        let mut content = vec![json!({ "type": "text", "text": text })];
        for image in &repl.images {
            if let Some(data) = &image.data {
                content.push(json!({ "type": "image", "data": data, "mimeType": image.mime }));
            }
        }
        Ok(json!({ "content": content, "isError": repl.error.is_some() }))
    }

    fn skills(&self, arguments: &Value) -> Result<Value, String> {
        let name = match &arguments["name"] {
            Value::Null => None,
            value => Some(value.as_str().ok_or("`name` is the name of a guide")?),
        };
        let reply = self
            .daemon
            .call("skills", json!({ "name": name }))
            .map_err(refusal)?;
        let text = match name {
            Some(_) => reply["content"].as_str().unwrap_or_default().to_string(),
            None => {
                let mut text = String::from("Guides (read one with the name):\n");
                for skill in reply["skills"].as_array().into_iter().flatten() {
                    text.push_str(&format!(
                        "- {}: {}\n",
                        skill["name"].as_str().unwrap_or_default(),
                        skill["description"].as_str().unwrap_or_default()
                    ));
                }
                text
            }
        };
        Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": false }))
    }
}

/// 데몬의 거절이나 닿지 못한 것. 데몬의 알림은 AI가 사용자에게 무엇을 부탁할지 적는다
fn refusal(error: RpcError) -> String {
    if error.message.starts_with("cannot reach buntd-agent") {
        return format!(
            "{}. buntOS starts it on demand (buntd-agent.socket); this works only inside the \
             user's desktop session",
            error.message
        );
    }
    error.message
}

fn tool_error(message: String) -> Value {
    json!({ "content": [{ "type": "text", "text": message }], "isError": true })
}

fn error_reply(id: Value, (code, message, data): Failure) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

fn server_info() -> Value {
    json!({
        "name": "buntos",
        "title": "buntOS desktop",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "repl",
            "title": "buntOS desktop REPL",
            "description": REPL_DESCRIPTION,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "JavaScript to run. Top-level await works; the value of the \
                                        last expression comes back. Variables stay between calls."
                    },
                    "timeout": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 120000,
                        "description": "Stop the code after this many milliseconds (default 30000)."
                    },
                    "reset": {
                        "type": "boolean",
                        "description": "Forget the variables of earlier calls before running."
                    }
                },
                "required": ["code"],
                "additionalProperties": false
            },
            "annotations": {
                "title": "buntOS desktop REPL",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": true
            }
        },
        {
            "name": "skills",
            "title": "buntOS usage guides",
            "description": "List the usage guides for the repl tool and the buntOS desktop, or \
                            read one by name. Read `repl` before your first repl call.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "The guide to read (repl, desktop). Leave it out to list them."
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "title": "buntOS usage guides",
                "readOnlyHint": true,
                "openWorldHint": false
            }
        }
    ])
}

/// 표준 입출력으로 MCP를 말한다. tools/call(과 그것을 담을 수 있는 묶음)은 따로 된 스레드에서 들어온 차례로 돌려,
/// 긴 REPL 호출 동안에도 다른 요청(ping, tools/list)에 답한다. 도구 호출은 모두 같은 REPL로 가고 데몬이 어차피 차례로 돌리므로 한꺼번에
/// 돌리지 않는다: 차례를 지켜야 앞 호출이 둔 변수를 뒤 호출이 쓴다. 입력이 끝나면 남은 호출을 마치고 돌아온다
pub fn serve<D: Daemon>(
    input: impl BufRead,
    output: impl Write + Send,
    server: &Server<D>,
) -> io::Result<()> {
    let output = Mutex::new(output);
    let write = |reply: Value| -> io::Result<()> {
        let mut line = serde_json::to_vec(&reply)?;
        line.push(b'\n');
        let mut output = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        output.write_all(&line)?;
        output.flush()
    };
    thread::scope(|scope| {
        let (calls, queue) = mpsc::channel::<Value>();
        let write = &write;
        scope.spawn(move || {
            for message in queue {
                if let Some(reply) = server.handle(message) {
                    // 읽는 쪽이 닫혔으면 버린다
                    let _ = write(reply);
                }
            }
        });
        for line in input.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let message: Value = match serde_json::from_str(&line) {
                Ok(message) => message,
                Err(err) => {
                    write(error_reply(
                        Value::Null,
                        failure(protocol::PARSE_ERROR, format!("not JSON: {err}")),
                    ))?;
                    continue;
                }
            };
            // 묶음(배열)도 도구 호출을 담을 수 있어 같은 줄에 세운다 (검토: 묶음의 도구 호출은 읽는 스레드에서 돌아
            // 앞의 도구 호출과 한꺼번에 돌았다)
            if message["method"] == "tools/call" || message.is_array() {
                // 받는 스레드는 이 함수가 끝날 때까지 산다
                let _ = calls.send(message);
            } else if let Some(reply) = server.handle(message) {
                write(reply)?;
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Instant},
    };

    use super::*;

    /// 부른 것을 적고 정한 답을 주는 가짜 데몬
    struct Fake {
        calls: Mutex<Vec<(String, Value)>>,
        answer: fn(&str, &Value) -> Result<Value, RpcError>,
    }

    impl Daemon for Fake {
        fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
            let answer = (self.answer)(method, &params);
            self.calls
                .lock()
                .unwrap()
                .push((method.to_string(), params));
            answer
        }
    }

    fn server(answer: fn(&str, &Value) -> Result<Value, RpcError>) -> Server<Fake> {
        Server::new(
            Fake {
                calls: Mutex::new(Vec::new()),
                answer,
            },
            "default".into(),
        )
    }

    fn desktop(method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "repl" if params["code"] == "throw" => Ok(json!({
                "output": "before\n",
                "error": "BlockedAppError: the user has blocked AI agents from this app",
                "fresh": false, "ms": 3, "images": [],
            })),
            "repl" => Ok(json!({
                "output": "# window \"계산기\": 2x2 pixels\n/run/user/1000/buntos/screenshots/s.png\n",
                "fresh": false,
                "ms": 12,
                "images": [{ "path": "/s.png", "mime": "image/png", "data": "iVBORw0KGgo=" }],
            })),
            "skills" if params["name"].is_null() => Ok(json!({
                "skills": [{ "name": "repl", "description": "JavaScript REPL API" }]
            })),
            "skills" => Ok(
                json!({ "name": "repl", "description": "d", "content": "# buntOS desktop REPL\n" }),
            ),
            _ => unreachable!(),
        }
    }

    fn request(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    fn result(server: &Server<Fake>, method: &str, params: Value) -> Value {
        let reply = server.handle(request(method, params)).unwrap();
        assert_eq!(reply["id"], 1);
        assert!(reply.get("error").is_none(), "{reply}");
        reply["result"].clone()
    }

    #[test]
    fn initialize_agrees_on_a_version_and_names_the_server() {
        let server = server(desktop);
        let known = result(
            &server,
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "c", "version": "1" } }),
        );
        assert_eq!(known["protocolVersion"], "2025-06-18");
        assert_eq!(known["serverInfo"]["name"], "buntos");
        assert!(known["capabilities"]["tools"].is_object());
        assert!(known["instructions"].as_str().unwrap().contains("repl"));
        // 모르는 판이면 아는 것 가운데 가장 새것을 내민다
        let newer = result(
            &server,
            "initialize",
            json!({ "protocolVersion": "2099-01-01" }),
        );
        assert_eq!(newer["protocolVersion"], LEGACY);
        // 상태 없는 판: 판을 묻고, 요청마다 판을 싣는다
        let discovered = result(&server, "server/discover", json!({}));
        assert_eq!(discovered["supportedVersions"][0], "2026-07-28");
        assert_eq!(discovered["cacheScope"], "public");
        let meta = json!({ "_meta": { VERSION_KEY: "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {} } });
        let listed = result(&server, "tools/list", meta);
        assert_eq!(listed["_meta"][SERVER_KEY]["name"], "buntos");
        let unknown = server
            .handle(request(
                "tools/list",
                json!({ "_meta": { VERSION_KEY: "1999-01-01" } }),
            ))
            .unwrap();
        assert_eq!(unknown["error"]["code"], UNSUPPORTED_PROTOCOL_VERSION);
        assert_eq!(unknown["error"]["data"]["requested"], "1999-01-01");
        assert_eq!(
            result(&server, "ping", json!({}))["_meta"][SERVER_KEY]["name"],
            "buntos"
        );
    }

    #[test]
    fn there_are_two_tools() {
        let server = server(desktop);
        let tools = result(&server, "tools/list", json!({}))["tools"].clone();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["repl", "skills"]);
        assert_eq!(tools[0]["inputSchema"]["required"], json!(["code"]));
        assert_eq!(tools[1]["annotations"]["readOnlyHint"], true);
    }

    #[test]
    fn repl_returns_text_and_screenshots_as_images() {
        let server = server(desktop);
        let called = result(
            &server,
            "tools/call",
            json!({ "name": "repl", "arguments": { "code": "await desktop.screenshot()", "timeout": 5000 } }),
        );
        assert_eq!(called["isError"], false);
        assert_eq!(
            called["content"][0],
            json!({ "type": "text", "text": "# window \"계산기\": 2x2 pixels\n/run/user/1000/buntos/screenshots/s.png" })
        );
        assert_eq!(
            called["content"][1],
            json!({ "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" })
        );
        let calls = server.daemon.calls.lock().unwrap().clone();
        assert_eq!(calls[0].0, "repl");
        assert_eq!(
            calls[0].1,
            json!({ "code": "await desktop.screenshot()", "session": "default", "timeout_ms": 5000, "reset": false, "images": true })
        );
        // 코드가 던진 오류: 그때까지의 출력과 함께 isError로
        let thrown = result(
            &server,
            "tools/call",
            json!({ "name": "repl", "arguments": { "code": "throw" } }),
        );
        assert_eq!(thrown["isError"], true);
        assert_eq!(
            thrown["content"][0]["text"],
            "before\nBlockedAppError: the user has blocked AI agents from this app"
        );
    }

    #[test]
    fn refusals_and_bad_arguments_are_tool_errors_the_model_can_read() {
        let refusing = server(|_, _| {
            Err(RpcError {
                code: protocol::NOT_ALLOWED,
                message:
                    "claude is not allowed to control this computer. buntOS is asking the user now"
                        .into(),
            })
        });
        let refused = result(
            &refusing,
            "tools/call",
            json!({ "name": "repl", "arguments": { "code": "1" } }),
        );
        assert_eq!(refused["isError"], true);
        assert!(
            refused["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("asking the user")
        );
        let skills = result(
            &refusing,
            "tools/call",
            json!({ "name": "skills", "arguments": {} }),
        );
        assert_eq!(skills["isError"], true);
        let unreachable = server(|_, _| {
            Err(RpcError {
                code: protocol::AGENT_ERROR,
                message:
                    "cannot reach buntd-agent at /run/user/1000/buntos/agent.sock: No such file"
                        .into(),
            })
        });
        let away = result(
            &unreachable,
            "tools/call",
            json!({ "name": "repl", "arguments": { "code": "1" } }),
        );
        assert!(
            away["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("desktop session")
        );
        // 잘못된 인자는 데몬에 가지 않는다
        let server = server(desktop);
        for arguments in [
            json!({}),
            json!({ "code": 1 }),
            json!({ "code": "1", "timeout": "soon" }),
            json!({ "code": "1", "reset": "yes" }),
        ] {
            let bad = result(
                &server,
                "tools/call",
                json!({ "name": "repl", "arguments": arguments }),
            );
            assert_eq!(bad["isError"], true, "{arguments}");
        }
        assert!(server.daemon.calls.lock().unwrap().is_empty());
        // 없는 도구는 프로토콜 오류다
        let missing = server
            .handle(request(
                "tools/call",
                json!({ "name": "exec", "arguments": {} }),
            ))
            .unwrap();
        assert_eq!(missing["error"]["code"], protocol::INVALID_PARAMS);
    }

    #[test]
    fn skills_list_and_read_guides() {
        let server = server(desktop);
        let list = result(&server, "tools/call", json!({ "name": "skills" }));
        assert_eq!(
            list["content"][0]["text"],
            "Guides (read one with the name):\n- repl: JavaScript REPL API\n"
        );
        let read = result(
            &server,
            "tools/call",
            json!({ "name": "skills", "arguments": { "name": "repl" } }),
        );
        assert_eq!(read["content"][0]["text"], "# buntOS desktop REPL\n");
    }

    #[test]
    fn notifications_responses_and_garbage() {
        let server = server(desktop);
        assert_eq!(
            server.handle(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })),
            None
        );
        assert_eq!(
            server.handle(json!({ "jsonrpc": "2.0", "id": 9, "result": {} })),
            None
        );
        let unknown = server.handle(request("resources/list", json!({}))).unwrap();
        assert_eq!(unknown["error"]["code"], protocol::METHOD_NOT_FOUND);
        let invalid = server.handle(json!({ "jsonrpc": "2.0" })).unwrap();
        assert_eq!(invalid["error"]["code"], INVALID_REQUEST);
        let batch = server
            .handle(json!([request("ping", json!({})), { "jsonrpc": "2.0", "method": "notifications/initialized" }]))
            .unwrap();
        assert_eq!(batch.as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_long_call_does_not_hold_up_other_requests() {
        let slow = server(|method, _| {
            if method == "repl" {
                std::thread::sleep(Duration::from_millis(300));
            }
            Ok(json!({ "output": "done\n", "fresh": false, "ms": 300, "images": [] }))
        });
        let input = [
            request(
                "tools/call",
                json!({ "name": "repl", "arguments": { "code": "slow" } }),
            ),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "repl", "arguments": { "code": "next" } } }),
        ]
        .iter()
        .map(|message| format!("{message}\nnot json\n"))
        .collect::<String>();
        let mut output = Vec::new();
        let started = Instant::now();
        serve(input.as_bytes(), &mut output, &slow).unwrap();
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "the call finished before returning"
        );
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 6);
        // ping과 JSON이 아닌 줄의 답이 느린 호출보다 먼저 왔고, 도구 호출은 들어온 차례로 끝났다
        assert_eq!(replies[0]["error"]["code"], protocol::PARSE_ERROR);
        assert_eq!(replies[1]["id"], 2);
        assert_eq!(replies[2]["error"]["code"], protocol::PARSE_ERROR);
        assert_eq!(replies[3]["error"]["code"], protocol::PARSE_ERROR);
        assert_eq!(
            (replies[4]["id"].as_u64(), replies[5]["id"].as_u64()),
            (Some(1), Some(3))
        );
        assert_eq!(replies[4]["result"]["content"][0]["text"], "done");
        assert!(started.elapsed() >= Duration::from_millis(600));
    }

    #[test]
    fn tool_calls_in_a_batch_wait_their_turn() {
        // 데몬의 repl이 한꺼번에 몇 번 불렸는지
        static RUNNING: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);
        let slow = server(|method, _| {
            if method == "repl" {
                let now = RUNNING.fetch_add(1, Ordering::SeqCst) + 1;
                MOST.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(200));
                RUNNING.fetch_sub(1, Ordering::SeqCst);
            }
            Ok(json!({ "output": "done\n", "fresh": false, "ms": 200, "images": [] }))
        });
        let call = |id: u64| json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": "repl", "arguments": { "code": "x" } } });
        let input = format!(
            "{}\n{}\n{}\n",
            call(1),
            json!([
                call(2),
                json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" })
            ]),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "ping" }),
        );
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output, &slow).unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(MOST.load(Ordering::SeqCst), 1, "tool calls ran at once");
        // 홀로 온 ping이 먼저, 도구 호출은 들어온 차례로 (묶음은 묶음으로 답한다)
        assert_eq!(replies[0]["id"], 3);
        assert_eq!(replies[1]["id"], 1);
        let batch = replies[2].as_array().unwrap();
        assert_eq!(
            (batch[0]["id"].as_u64(), batch[1]["id"].as_u64()),
            (Some(2), Some(4))
        );
    }
}
