// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 실제 데몬(`buntd-agent`)을 시험용 런타임 디렉터리에 띄우고 `bunt`로 붙어 본다. 데스크톱이 없는 곳에서 돈다:
//! 세션 버스가 없으면 데몬은 화면이 잠긴 것으로 보므로 데스크톱 호출은 ScreenLockedError다. JS와 사용 안내는
//! 데스크톱 없이도 된다.
//!
//! 데몬은 연결한 프로그램을 정책(/etc/buntos/agent-access.toml)으로 받는다. root로 돌리면 관리자라 늘 받고, 아니면
//! 이 시험 프로그램이 허용 목록에 없어 거절된다. 둘 다 본다. 거절 기록은 시험 디렉터리에 남는다(XDG_STATE_HOME).

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Daemon {
    child: Child,
    dir: PathBuf,
}

impl Daemon {
    fn start() -> Daemon {
        let dir = std::env::temp_dir().join(format!(
            "bunt-daemon-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("state")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_buntd-agent"));
        environment(&mut command, &dir);
        let child = command
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let socket = dir.join("buntos/agent.sock");
        let started = Instant::now();
        while !socket.exists() {
            assert!(started.elapsed() < Duration::from_secs(10), "no socket");
            std::thread::sleep(Duration::from_millis(20));
        }
        Daemon { child, dir }
    }

    fn bunt(&self, args: &[&str], input: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_bunt"));
        environment(&mut command, &self.dir);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        if let Some(input) = input {
            stdin.write_all(input.as_bytes()).unwrap();
        }
        drop(stdin);
        child.wait_with_output().unwrap()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 세션 버스와 사용자 디렉터리 없이, 시험 디렉터리만 보는 환경
fn environment(command: &mut Command, dir: &Path) {
    command
        .env_clear()
        .env("XDG_RUNTIME_DIR", dir)
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("HOME", dir);
}

fn admin() -> bool {
    // 안전: 인자 없는 시스템 호출이다
    unsafe { libc::geteuid() == 0 }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn the_cli_repl_keeps_variables_between_calls() {
    let daemon = Daemon::start();
    let first = daemon.bunt(&["agent", "repl", "answer = 6 * 7"], None);
    if !admin() {
        // 허용 목록에 없는 프로그램: JS를 돌리지 않고, 사용자에게 할 일을 알린다
        assert!(!first.status.success());
        let error = text(&first.stderr);
        assert!(
            error.contains("is not allowed to control this computer"),
            "{error}"
        );
        return;
    }
    assert!(first.status.success(), "{}", text(&first.stderr));
    assert_eq!(text(&first.stdout), "42\n");
    // 다른 프로세스(다음 CLI 호출)에서도 변수가 남는다
    let second = daemon.bunt(&["agent", "repl", "answer", "+", "1"], None);
    assert_eq!(text(&second.stdout), "43\n");
    // 표준 입력의 코드
    let piped = daemon.bunt(
        &["agent", "repl"],
        Some("const twice = answer * 2\ntwice\n"),
    );
    assert_eq!(text(&piped.stdout), "84\n");
    // 다른 세션은 따로다
    let other = daemon.bunt(&["agent", "repl", "--session", "s2", "typeof answer"], None);
    assert_eq!(text(&other.stdout), "undefined\n");
    let reset = daemon.bunt(&["agent", "repl", "--reset", "typeof answer"], None);
    assert_eq!(text(&reset.stdout), "undefined\n");
    let json = daemon.bunt(&["agent", "repl", "--json", "[1, 'a']"], None);
    let reply: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(reply["value"], serde_json::json!([1, "a"]));
    assert_eq!(reply["fresh"], false);
}

#[test]
fn the_cli_repl_reports_errors_and_refusals() {
    if !admin() {
        return;
    }
    let daemon = Daemon::start();
    // 세션 버스가 없으면 데몬은 화면이 잠긴 것으로 보고 거절한다: REPL 안의 호출도 정책을 거친다
    let locked = daemon.bunt(
        &[
            "agent",
            "repl",
            "console.log('before'); await desktop.apps()",
        ],
        None,
    );
    assert!(!locked.status.success());
    assert_eq!(text(&locked.stdout), "before\n");
    let error = text(&locked.stderr);
    assert!(
        error.starts_with("bunt: ScreenLockedError: the screen is locked"),
        "{error}"
    );
    let thrown = daemon.bunt(&["agent", "repl", "throw new Error('boom')"], None);
    assert!(!thrown.status.success());
    assert!(text(&thrown.stderr).starts_with("bunt: Error: boom"));
    let slow = daemon.bunt(&["agent", "repl", "--timeout", "200", "for (;;) {}"], None);
    assert!(text(&slow.stderr).contains("time limit (200 ms)"));
    // 기다리기도 데몬에서 거절된다
    let wait = daemon.bunt(&["agent", "wait", "--text", "42", "--timeout", "0"], None);
    assert!(
        text(&wait.stderr).contains("the screen is locked"),
        "{}",
        text(&wait.stderr)
    );
}

#[test]
fn skills_come_from_the_daemon_and_help_is_local() {
    let daemon = Daemon::start();
    let help = daemon.bunt(&["agent", "repl", "--help"], None);
    assert!(help.status.success());
    let help = text(&help.stdout);
    assert!(help.starts_with("Usage: bunt agent repl"), "{help}");
    assert!(help.contains("# buntOS desktop REPL"));
    assert!(!help.contains("SPDX"));
    if !admin() {
        return;
    }
    let list = daemon.bunt(&["agent", "skills"], None);
    let list = text(&list.stdout);
    assert!(list.starts_with("repl  JavaScript REPL API"), "{list}");
    assert!(list.contains("\ndesktop  How AI control works"));
    let repl = daemon.bunt(&["agent", "skills", "repl"], None);
    assert!(text(&repl.stdout).starts_with("# buntOS desktop REPL\n"));
    let missing = daemon.bunt(&["agent", "skills", "nope"], None);
    assert!(text(&missing.stderr).contains("no skill named \"nope\""));
}

/// `bunt mcp`에 MCP 메시지를 차례로 보내고, 입력을 닫은 뒤 받은 답을 준다
fn mcp(daemon: &Daemon, messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let input: String = messages
        .iter()
        .map(|message| format!("{message}\n"))
        .collect();
    let output = daemon.bunt(&["mcp"], Some(&input));
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn an_mcp_client_runs_the_repl_through_the_same_daemon() {
    use serde_json::json;
    let daemon = Daemon::start();
    let call = |id: u64, name: &str, arguments: serde_json::Value| json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
    let replies = mcp(
        &daemon,
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            call(
                3,
                "repl",
                json!({ "code": "const product = 6 * 7; product" }),
            ),
            call(4, "repl", json!({ "code": "product + 1" })),
            call(5, "repl", json!({ "code": "await desktop.apps()" })),
            call(6, "skills", json!({ "name": "repl" })),
        ],
    );
    // 도구 호출은 스레드에서 도므로 답의 차례는 id로 찾는다
    let reply = |id: u64| {
        replies
            .iter()
            .find(|reply| reply["id"] == id)
            .unwrap_or_else(|| panic!("no reply {id} in {replies:?}"))
            .clone()
    };
    assert_eq!(replies.len(), 6, "{replies:?}");
    assert_eq!(reply(1)["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(reply(2)["result"]["tools"][0]["name"], "repl");
    let first = &reply(3)["result"];
    if !admin() {
        // 허용 목록에 없는 MCP 클라이언트: 모델이 읽을 거절
        assert_eq!(first["isError"], true);
        assert!(
            first["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("is not allowed to control this computer")
        );
        return;
    }
    assert_eq!(first["content"][0]["text"], "42", "{first}");
    assert_eq!(first["isError"], false);
    // 도구 호출은 들어온 차례로 돌아 앞 호출의 변수가 남는다
    assert_eq!(reply(4)["result"]["content"][0]["text"], "43");
    // 데몬의 정책(세션 버스가 없어 잠긴 것으로 본다)이 MCP의 REPL에도 걸린다
    let locked = &reply(5)["result"];
    assert_eq!(locked["isError"], true);
    assert!(
        locked["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("ScreenLockedError: the screen is locked")
    );
    let guide = &reply(6)["result"];
    assert!(
        guide["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("# buntOS desktop REPL")
    );
}
