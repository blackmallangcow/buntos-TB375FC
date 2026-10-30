// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 실제 REPL 워커 프로세스(`buntd-agent --repl-worker`)를 띄워 본다. 데스크톱 없이 돈다: 시험이 데몬 자리에서
//! 워커가 청하는 데스크톱 호출에 답한다. 가두기(seccomp, 자원 상한, 닫은 파일 기술자)가 프로세스에 걸렸는지 본다.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

struct Worker {
    child: Child,
    /// 닫으면 데몬이 끝난 것과 같다
    to: Option<ChildStdin>,
    from: BufReader<ChildStdout>,
}

impl Worker {
    fn spawn() -> Worker {
        let mut child = Command::new(env!("CARGO_BIN_EXE_buntd-agent"))
            .arg("--repl-worker")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let to = child.stdin.take();
        let from = BufReader::new(child.stdout.take().unwrap());
        Worker { child, to, from }
    }

    fn send(&mut self, message: Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.to
            .as_mut()
            .unwrap()
            .write_all(line.as_bytes())
            .unwrap();
    }

    fn read(&mut self) -> Value {
        let mut line = String::new();
        self.from.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|err| panic!("{err}: {line:?}"))
    }

    /// 코드를 돌리고, 데스크톱 호출에는 `answer`로 답하고, 끝을 준다
    fn eval(&mut self, code: &str, timeout_ms: u64, answer: impl Fn(&Value) -> Value) -> Value {
        self.send(json!({ "eval": { "code": code, "timeout_ms": timeout_ms } }));
        loop {
            let message = self.read();
            if let Some(done) = message.get("done") {
                return done.clone();
            }
            let reply = answer(&message["call"]);
            self.send(json!({ "reply": reply }));
        }
    }

    fn proc(&self, file: &str) -> String {
        std::fs::read_to_string(format!("/proc/{}/{file}", self.child.id())).unwrap()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn the_worker_runs_code_and_asks_the_daemon_for_desktop_calls() {
    let mut worker = Worker::spawn();
    let done = worker.eval(
        "const s = await desktop.app('calc').snapshot();\nconsole.log('read', s.tree.length);\ns",
        5_000,
        |call| {
            assert_eq!(call["method"], "snapshot");
            assert_eq!(call["params"]["app"], "calc");
            json!({ "ok": { "tree": "- app: \"calc\"\n", "diff": "", "first": true } })
        },
    );
    assert_eq!(done["error"], Value::Null);
    assert_eq!(done["output"], "read 14\n- app: \"calc\"\n");
    // 변수가 다음 호출에 남는다
    let next = worker.eval("s.first", 5_000, |_| unreachable!());
    assert_eq!(next["value"], json!(true));
    // 데몬의 거절은 이름 있는 오류다
    let refused = worker.eval("await desktop.apps()", 5_000, |_| {
        json!({ "err": { "code": -32001, "message": "claude is not allowed to control this computer" } })
    });
    let error = refused["error"].as_str().unwrap();
    assert!(
        error.starts_with(
            "NotAllowedError: claude is not allowed to control this computer\n    at <eval> (<repl>:1:"
        ),
        "{error}"
    );
}

#[test]
fn the_worker_is_sandboxed() {
    let mut worker = Worker::spawn();
    // 가두기는 엔진을 만든 뒤에 하므로 한 번 돌려 본 다음에 본다
    let done = worker.eval(
        "new Date().getFullYear() > 2000 && Math.random() < 1",
        5_000,
        |_| unreachable!(),
    );
    assert_eq!(done["value"], json!(true));
    let status = worker.proc("status");
    assert!(status.contains("\nSeccomp:\t2\n"), "{status}");
    assert!(status.contains("\nNoNewPrivs:\t1\n"), "{status}");
    // 상속한 파일 기술자는 닫혔다 (표준 입출력만)
    let fds: Vec<String> = std::fs::read_dir(format!("/proc/{}/fd", worker.child.id()))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    let mut fds: Vec<u32> = fds.iter().map(|fd| fd.parse().unwrap()).collect();
    fds.sort();
    assert_eq!(fds, [0, 1, 2]);
    let limits = worker.proc("limits");
    for (name, value) in [
        ("Max core file size", "0"),
        ("Max file size", "0"),
        ("Max processes", "0"),
        ("Max address space", "1073741824"),
    ] {
        let line = limits
            .lines()
            .find(|line| line.starts_with(name))
            .unwrap_or_else(|| panic!("{name} in {limits}"));
        assert_eq!(
            line.split_whitespace().nth(name.split(' ').count()),
            Some(value),
            "{line}"
        );
    }
}

#[test]
fn the_worker_stops_runaway_code_and_keeps_going() {
    let mut worker = Worker::spawn();
    let started = Instant::now();
    let stopped = worker.eval("kept = 1; for (;;) {}", 300, |_| unreachable!());
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(
        stopped["error"]
            .as_str()
            .unwrap()
            .starts_with("the code ran past its time limit (300 ms)")
    );
    let after = worker.eval("kept + 1", 5_000, |_| unreachable!());
    assert_eq!(after["output"], "2\n");
}

#[test]
fn the_worker_ends_when_the_daemon_goes_away() {
    let mut worker = Worker::spawn();
    let done = worker.eval("1", 5_000, |_| unreachable!());
    assert_eq!(done["output"], "1\n");
    // 데몬이 끝나면 파이프가 닫힌다
    worker.to.take();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = worker.child.try_wait().unwrap() {
            break status;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the worker did not end"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "{status:?}");
}
