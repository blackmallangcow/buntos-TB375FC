// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! REPL 시험. 워커는 스레드로 띄우고(가두지 않는다, 가두기는 sandbox.rs와 tests/repl_worker.rs가 본다), 데몬은
//! 부른 것을 적고 정한 답을 주는 가짜 Desk다. 실제 데스크톱 없이 돈다.

use std::{sync::Mutex, thread};

use super::*;

type Answer = Box<dyn Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync>;

/// 부른 것을 적고 `answer`로 답하는 가짜 데몬
struct Fake {
    calls: Mutex<Vec<(String, Value)>>,
    answer: Answer,
}

impl Fake {
    fn new(
        answer: impl Fn(&str, &Value) -> Result<Value, Failure> + Send + Sync + 'static,
    ) -> Fake {
        Fake {
            calls: Mutex::new(Vec::new()),
            answer: Box::new(answer),
        }
    }

    fn calls(&self) -> Vec<(String, Value)> {
        lock(&self.calls).clone()
    }
}

impl Desk for Fake {
    fn call(&self, method: &str, params: Value) -> Result<Value, Failure> {
        let answer = (self.answer)(method, &params);
        lock(&self.calls).push((method.to_string(), params));
        answer
    }
}

fn repls() -> Repls {
    Repls::new(Box::new(Worker::in_thread))
}

fn run(repls: &Repls, desk: &dyn Desk, code: &str) -> Repl {
    run_with(repls, desk, code, |_| {})
}

fn run_with(
    repls: &Repls,
    desk: &dyn Desk,
    code: &str,
    edit: impl FnOnce(&mut ReplParams),
) -> Repl {
    let mut params = ReplParams {
        code: code.into(),
        ..ReplParams::default()
    };
    edit(&mut params);
    repls.run(desk, "claude", params).unwrap()
}

const CALCULATOR: &str = "- app: \"gnome-calculator\" [pid=812]\n- window \"계산기\" [ref=e1] [active]:\n  - button \"=\" [ref=e185]\n  - textbox [ref=e159] [focused] [value=\"6×7\"]\n";

fn desktop() -> Fake {
    Fake::new(|method, params| match method {
        "snapshot" => Ok(
            json!({ "tree": CALCULATOR, "diff": "", "first": !params["peek"].as_bool().unwrap_or(false) }),
        ),
        "act" => Ok(json!({
            "done": "clicked with the accessible action \"click\"",
            "diff": "@@ -4 +4 @@\n-  - textbox [ref=e159] [focused] [value=\"6×7\"]\n+  - textbox [ref=e159] [focused] [value=\"42\"]\n",
            "settled_ms": 533,
            "settled": true,
            "observed": true,
            "new_windows": [],
        })),
        _ => Err((protocol::AGENT_ERROR, format!("no {method} here"))),
    })
}

#[test]
fn variables_live_between_calls_and_can_be_declared_again() {
    let repls = repls();
    let desk = desktop();
    let first = run(&repls, &desk, "const answer = 6 * 7");
    assert!(first.fresh);
    // 값이 없는 문장(선언)은 아무것도 적지 않는다
    assert_eq!((first.output.as_str(), first.error), ("", None));
    let second = run(&repls, &desk, "answer");
    assert!(!second.fresh);
    assert_eq!(second.output, "42\n");
    assert_eq!(second.value, Some(json!(42)));
    // 브라우저 콘솔처럼 다시 선언할 수 있다
    let again = run(
        &repls,
        &desk,
        "const answer = 1; let more = answer + 1; more",
    );
    assert_eq!((again.output.as_str(), again.error), ("2\n", None));
    // 선언 없이 쓴 이름도 남는다
    run(&repls, &desk, "loose = 'kept'");
    assert_eq!(run(&repls, &desk, "loose").output, "kept\n");
    assert_eq!(repls.len(), 1);
}

#[test]
fn results_read_like_the_cli() {
    let repls = repls();
    let desk = desktop();
    let snapshot = run(&repls, &desk, "await desktop.app('calc').snapshot()");
    assert_eq!(snapshot.output, CALCULATOR);
    assert_eq!(snapshot.value.unwrap()["tree"], CALCULATOR);
    let clicked = run(&repls, &desk, "await desktop.locator('e185').click()");
    assert_eq!(
        clicked.output,
        "# e185: clicked with the accessible action \"click\" (settled in 533 ms)\n@@ -4 +4 @@\n-  - textbox [ref=e159] [focused] [value=\"6×7\"]\n+  - textbox [ref=e159] [focused] [value=\"42\"]\n"
    );
    let value = clicked.value.unwrap();
    assert_eq!(
        (value["ref"].as_str(), value["settledMs"].as_u64()),
        (Some("e185"), Some(533))
    );
    // console와 값이 차례로 나온다
    let logged = run(&repls, &desk, "console.log('a', 1, { b: [2] }); 'end'");
    assert_eq!(logged.output, "a 1 {\n  \"b\": [\n    2\n  ]\n}\nend\n");
    // 데몬에 넘긴 인자: 세션은 REPL의 것이고, 동작의 기다림은 남은 시간 안이다
    let calls = desk.calls();
    assert_eq!(calls[0].0, "snapshot");
    assert_eq!(calls[0].1["app"], "calc");
    assert_eq!(calls[0].1["session"], "default");
    assert_eq!(calls[1].0, "act");
    assert_eq!(
        (calls[1].1["ref"].as_str(), calls[1].1["action"].as_str()),
        (Some("e185"), Some("click"))
    );
    assert!(calls[1].1["settle_ms"].as_u64().unwrap() <= SETTLE_DEFAULT_MS);
}

#[test]
fn desktop_errors_are_typed_and_carry_their_code() {
    let repls = repls();
    let desk = Fake::new(|_, _| {
        Err((
            protocol::BLOCKED_APP,
            "the user has blocked AI agents from this app (System Settings > AI Agents)".into(),
        ))
    });
    let caught = run(
        &repls,
        &desk,
        "try { await desktop.app('bank').snapshot() } catch (e) { [e.name, e.code, e instanceof desktop.errors.DesktopError] }",
    );
    assert_eq!(caught.value, Some(json!(["BlockedAppError", -32002, true])));
    let thrown = run(&repls, &desk, "\n\nawait desktop.app('bank').snapshot()");
    let error = thrown.error.unwrap();
    assert!(
        error.starts_with("BlockedAppError: the user has blocked AI agents from this app"),
        "{error}"
    );
    assert!(
        error.contains("<repl>:3"),
        "the stack names the line: {error}"
    );
    // 잠긴 화면은 기다리는 중에도 바로 거절되고, 시간 초과도 이름이 있다
    let desk = Fake::new(|_, params| match params["app"].as_str() {
        Some("locked") => Err((protocol::SCREEN_LOCKED, "the screen is locked".into())),
        _ => Ok(json!({ "tree": "", "diff": "", "first": false })),
    });
    let names = run(
        &repls,
        &desk,
        "const names = []; for (const f of [() => desktop.app('locked').snapshot(), () => desktop.app('locked').wait({ text: 'x' }), () => desktop.wait({ text: 'x', timeout: 0 })]) { try { await f() } catch (e) { names.push(e.name) } } names",
    );
    assert_eq!(
        names.value,
        Some(json!([
            "ScreenLockedError",
            "ScreenLockedError",
            "TimeoutError"
        ]))
    );
}

#[test]
fn the_time_limit_stops_runaway_code_and_keeps_the_repl() {
    let repls = repls();
    let desk = desktop();
    let started = Instant::now();
    let stopped = run_with(&repls, &desk, "kept = 5; while (true) {}", |p| {
        p.timeout_ms = Some(300)
    });
    assert!(started.elapsed() < Duration::from_secs(3));
    let error = stopped.error.unwrap();
    assert!(
        error.starts_with("the code ran past its time limit (300 ms)"),
        "{error}"
    );
    // try/catch로 상한을 넘기지 못한다
    let caught = run_with(
        &repls,
        &desk,
        "try { for (;;) {} } catch (e) { 'escaped' }",
        |p| p.timeout_ms = Some(200),
    );
    assert!(caught.error.unwrap().contains("time limit"));
    // 시간 상한 전에 둔 변수는 남고 REPL은 계속 쓴다
    let after = run(&repls, &desk, "kept + 1");
    assert_eq!((after.output.as_str(), after.fresh), ("6\n", false));
    // 상한보다 긴 타이머도 상한에서 멈춘다
    let timer = run_with(
        &repls,
        &desk,
        "await new Promise((resolve) => setTimeout(resolve, 60000))",
        |p| p.timeout_ms = Some(200),
    );
    assert!(timer.error.unwrap().contains("time limit"));
}

#[test]
fn the_memory_limit_fails_the_allocation_not_the_repl() {
    let repls = repls();
    let desk = desktop();
    let full = run(
        &repls,
        &desk,
        "let big = []; for (;;) big.push('x'.repeat(1 << 16) + big.length)",
    );
    let error = full.error.unwrap();
    assert!(error.contains("out of memory"), "{error}");
    assert!(error.contains("32 MiB"), "{error}");
    let after = run(&repls, &desk, "big = null; 'fine'");
    assert_eq!(after.output, "fine\n");
}

#[test]
fn only_the_desktop_api_is_there() {
    let repls = repls();
    let desk = desktop();
    let globals = run(
        &repls,
        &desk,
        "['require', 'process', 'std', 'os', 'fetch', 'XMLHttpRequest', 'WebSocket', 'Deno', 'Bun', 'print', 'scriptArgs', '__bunt', 'Atomics', 'SharedArrayBuffer'].filter((name) => typeof globalThis[name] !== 'undefined')",
    );
    assert_eq!(globals.value, Some(json!([])));
    // desktop은 덮어쓸 수 없다
    let kept = run(
        &repls,
        &desk,
        "var desktop = 1; desktop = 2; typeof desktop.app",
    );
    assert_eq!(kept.output, "function\n");
}

#[test]
fn timers_promises_and_unsettled_code() {
    let repls = repls();
    let desk = desktop();
    let timed = run(
        &repls,
        &desk,
        "const order = []; setTimeout(() => order.push('late'), 30); const id = setTimeout(() => order.push('never'), 10); clearTimeout(id); await new Promise((resolve) => setTimeout(resolve, 60)); order",
    );
    assert_eq!(timed.value, Some(json!(["late"])));
    let stuck = run(&repls, &desk, "await new Promise(() => {})");
    assert!(stuck.error.unwrap().contains("never settles"));
    // 타이머 안의 오류는 알리기만 한다
    let noisy = run(
        &repls,
        &desk,
        "setTimeout(() => { throw new Error('boom') }, 0); await new Promise((r) => setTimeout(r, 20)); 'done'",
    );
    assert!(
        noisy.output.starts_with("Uncaught in a timer: Error: boom"),
        "{}",
        noisy.output
    );
    assert!(noisy.output.ends_with("done\n"));
    // 남은 타이머는 다음 호출로 넘어가지 않는다
    run(&repls, &desk, "leak = 0; setTimeout(() => leak++, 10)");
    let later = run(
        &repls,
        &desk,
        "await new Promise((r) => setTimeout(r, 30)); leak",
    );
    assert_eq!(later.output, "0\n");
}

#[test]
fn playwright_names_become_daemon_calls() {
    let repls = repls();
    let desk = desktop();
    run(
        &repls,
        &desk,
        "const app = desktop.app('계산기');\nawait app.keyboard.press('Control+A');\nawait app.keyboard.press(['ArrowLeft', 'Enter', 'Shift+Tab', 'KeyB', 'Digit7', ' ']);\nawait app.locator('aria-ref=e159').fill('6×7', { settle: 0 });\nawait app.locator('e159').type('안녕', { keys: true, keepFocus: true });\nawait app.mouse.click(10, 20);\nawait app.mouse.wheel(0, 250);\nawait app.scroll('up', 2)",
    );
    let acts: Vec<Value> = desk
        .calls()
        .into_iter()
        .filter(|(method, _)| method == "act")
        .map(|(_, params)| params)
        .collect();
    assert_eq!(acts.len(), 7);
    assert_eq!(acts[0]["value"], "ctrl+a");
    assert_eq!(acts[0]["app"], "계산기");
    assert_eq!(acts[1]["value"], "Left Return shift+Tab b 7 space");
    assert_eq!(
        (
            acts[2]["action"].as_str(),
            acts[2]["ref"].as_str(),
            acts[2]["settle_ms"].as_u64()
        ),
        (Some("fill"), Some("e159"), Some(0))
    );
    assert_eq!(
        (acts[3]["keys"].as_bool(), acts[3]["keep_focus"].as_bool()),
        (Some(true), Some(true))
    );
    assert_eq!(acts[4]["at"], json!([10, 20]));
    assert_eq!(acts[5]["amount"], json!([0, 3]));
    assert_eq!(acts[6]["amount"], json!([0, -2]));
    // 모르는 선택지는 받을 수 있는 것과 함께 알린다
    let wrong = run(
        &repls,
        &desk,
        "await desktop.locator('e1').click({ force: true })",
    );
    assert!(
        wrong
            .error
            .unwrap()
            .contains("click has no option \"force\"; it takes pointer, settle, timeout")
    );
    let bad_ref = run(&repls, &desk, "desktop.locator('#submit')");
    assert!(bad_ref.error.unwrap().contains("getByRole"));
}

#[test]
fn get_by_role_finds_one_ref_or_says_which() {
    let repls = repls();
    let tree = "- window \"w\" [ref=e1]:\n  - button \"OK\" [ref=e3]\n  - dialog \"d\" [ref=e4]:\n    - button \"OK\" [ref=e5]\n    - listitem \"합계 줄\" [ref=e6]:\n      - text: \"합계 42\"\n    - text: \"안내\"\n";
    let desk = Fake::new(move |method, _| match method {
        "snapshot" => Ok(
            json!({ "tree": format!("- app: \"a\" [pid=1]\n{tree}"), "diff": "", "first": false }),
        ),
        "act" => Ok(
            json!({ "done": "clicked", "diff": "", "settled_ms": 0, "settled": true, "observed": false, "new_windows": [] }),
        ),
        _ => Err((protocol::AGENT_ERROR, "no".into())),
    });
    let strict = run(
        &repls,
        &desk,
        "await desktop.app('a').getByRole('button', { name: 'OK' }).click()",
    );
    let error = strict.error.unwrap();
    assert!(
        error.contains("matches 2 elements; pick one with .first()"),
        "{error}"
    );
    assert!(error.contains("- button \"OK\" [ref=e5]"), "{error}");
    let last = run(
        &repls,
        &desk,
        "await desktop.app('a').getByRole('button', { name: 'OK' }).last().click()",
    );
    assert_eq!(last.output, "# e5: clicked\n");
    // ref 아래에서만 찾고, ref 없는 글 줄은 그것을 담은 요소(목록 항목)로 누른다
    let below = run(
        &repls,
        &desk,
        "await desktop.app('a').locator('e4').getByText('42').click()",
    );
    assert_eq!(below.output, "# e6: clicked\n");
    // 대화 상자에 바로 든 글: 그릇을 대신 누르지 않는다
    let loose = run(
        &repls,
        &desk,
        "await desktop.app('a').getByText('안내').click()",
    );
    let error = loose.error.unwrap();
    assert!(error.contains("nothing that holds it has a ref"), "{error}");
    let info = run(
        &repls,
        &desk,
        "const i = await desktop.app('a').getByText('42').info(); [i.ref, i.role, i.text]",
    );
    assert_eq!(info.value, Some(json!(["e6", "text", "합계 42"])));
    let count = run(
        &repls,
        &desk,
        "[await desktop.app('a').getByRole('button').count(), await desktop.app('a').getByRole('slider').count()]",
    );
    assert_eq!(count.value, Some(json!([2, 0])));
    // 찾을 때 읽은 스냅샷은 peek이라 다음 diff를 흐리지 않는다
    let peeks = desk
        .calls()
        .iter()
        .filter(|(method, params)| method == "snapshot" && params["peek"] == json!(true))
        .count();
    assert!(peeks >= 5, "{peeks}");
}

#[test]
fn the_bridge_lets_through_only_desktop_calls() {
    let desk = desktop();
    let mut bridge = Bridge {
        desk: &desk,
        session: "s",
        deadline: Instant::now() + Duration::from_secs(5),
        shots: Vec::new(),
    };
    for method in ["repl", "skills", "exec", ""] {
        let refused = bridge.call(method, json!({})).unwrap_err();
        assert_eq!(refused.0, protocol::METHOD_NOT_FOUND, "{method}");
    }
    let file = bridge
        .call("screenshot", json!({ "out": "/home/user/.bashrc" }))
        .unwrap_err();
    assert_eq!(file.0, protocol::INVALID_PARAMS);
    assert!(desk.calls().is_empty(), "nothing reached the daemon");
    // 세션은 REPL의 것으로 바꾼다
    bridge
        .call("snapshot", json!({ "session": "other", "app": "calc" }))
        .unwrap();
    assert_eq!(desk.calls()[0].1["session"], "s");
    // 남은 시간이 없으면 더 하지 않는다
    bridge.deadline = Instant::now();
    assert_eq!(
        bridge.call("act", json!({})).unwrap_err().0,
        protocol::TIMEOUT
    );
    assert_eq!(desk.calls().len(), 1);
}

#[test]
fn screenshots_come_back_as_images_when_asked() {
    let folder = std::env::temp_dir().join(format!("bunt-repl-test-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("shot.png");
    std::fs::write(&path, b"\x89PNG fake").unwrap();
    let shown = path.display().to_string();
    let desk = Fake::new(move |_, _| {
        Ok(
            json!({ "path": shown, "width": 4, "height": 3, "scale": 1.0, "window": "계산기", "app_id": "org.gnome.Calculator", "pid": 812 }),
        )
    });
    let repls = repls();
    let plain = run(&repls, &desk, "await desktop.screenshot()");
    assert_eq!(
        plain.output,
        format!(
            "# window \"계산기\" (org.gnome.Calculator, pid 812): 4x3 pixels at scale 1; desktop.app(\"org.gnome.Calculator\").mouse.click(x, y) takes these pixels\n{}\n",
            path.display()
        )
    );
    assert_eq!(plain.images.len(), 1);
    assert!(plain.images[0].data.is_none());
    let shots = run_with(
        &repls,
        &desk,
        "for (let i = 0; i < 6; i++) await desktop.screenshot()",
        |p| p.images = true,
    );
    assert_eq!(shots.images.len(), IMAGES_KEPT);
    assert_eq!(
        shots.images[0].data.as_deref(),
        Some(base64(b"\x89PNG fake").as_str())
    );
    assert_eq!(shots.images[0].mime, "image/png");
    std::fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_new_repl_says_why_variables_are_gone() {
    let repls = repls();
    let desk = desktop();
    run(&repls, &desk, "saved = 1");
    let reset = run_with(&repls, &desk, "typeof saved", |p| p.reset = true);
    assert_eq!((reset.output.as_str(), reset.fresh), ("undefined\n", true));
    // 같은 클라이언트의 다른 세션, 다른 클라이언트는 변수를 나누지 않는다
    run(&repls, &desk, "mine = 1");
    let other_session = run_with(&repls, &desk, "typeof mine", |p| {
        p.session = Some("s2".into())
    });
    assert_eq!(other_session.output, "undefined\n");
    let other_client = repls
        .run(
            &desk,
            "gemini",
            ReplParams {
                code: "typeof mine".into(),
                ..ReplParams::default()
            },
        )
        .unwrap();
    assert_eq!(other_client.output, "undefined\n");
    let missing = repls
        .run(
            &desk,
            "gemini",
            ReplParams {
                code: "nothingHere".into(),
                ..ReplParams::default()
            },
        )
        .unwrap();
    assert!(missing.error.unwrap().starts_with("ReferenceError"));
    let fresh = repls
        .run(
            &desk,
            "codex",
            ReplParams {
                code: "nothingHere".into(),
                ..ReplParams::default()
            },
        )
        .unwrap();
    assert!(fresh.error.unwrap().contains("this call ran in a new REPL"));
}

#[test]
fn a_dead_worker_is_replaced_before_it_runs_anything() {
    // 첫 워커는 코드를 받기 전에 끝나 있다 (오래 쉬어 스스로 끝난 워커)
    let started = Mutex::new(0);
    let repls = Repls::new(Box::new(move || {
        let mut count = lock(&started);
        *count += 1;
        if *count == 1 {
            let (reader, writer) = io::pipe()?;
            drop(writer);
            let (_, sink) = io::pipe()?;
            return Ok(Worker::new(Box::new(sink), Box::new(reader), None));
        }
        Worker::in_thread()
    }));
    let desk = desktop();
    let done = run(&repls, &desk, "1 + 1");
    assert_eq!((done.output.as_str(), done.fresh), ("2\n", true));
}

#[test]
fn a_worker_that_hangs_past_its_time_is_dropped() {
    // 코드를 받고도 답하지 않는 워커
    let repls = Repls::new(Box::new(|| {
        let (reader, writer) = io::pipe()?;
        let (from_daemon, sink) = io::pipe()?;
        thread::spawn(move || {
            let _keep = (writer, from_daemon);
            thread::sleep(Duration::from_secs(30));
        });
        Ok(Worker::new(Box::new(sink), Box::new(reader), None))
    }));
    let desk = desktop();
    let started = Instant::now();
    let failure = repls
        .run(
            &desk,
            "claude",
            ReplParams {
                code: "1".into(),
                timeout_ms: Some(10),
                ..ReplParams::default()
            },
        )
        .unwrap_err();
    assert_eq!(failure.0, protocol::AGENT_ERROR);
    assert!(failure.1.contains("was killed"), "{}", failure.1);
    let took = started.elapsed();
    assert!(
        took >= MIN_GRACE && took < MIN_GRACE + GRACE + Duration::from_secs(2),
        "{took:?}"
    );
}

#[test]
fn each_ai_gets_a_bounded_number_of_repls() {
    let repls = repls();
    let desk = desktop();
    for session in 0..PER_CLIENT + 2 {
        run_with(&repls, &desk, "1", |p| {
            p.session = Some(format!("s{session}"))
        });
    }
    assert_eq!(repls.len(), PER_CLIENT);
    for client in 0..TOTAL {
        repls
            .run(
                &desk,
                &format!("ai{client}"),
                ReplParams {
                    code: "1".into(),
                    ..ReplParams::default()
                },
            )
            .unwrap();
    }
    assert_eq!(repls.len(), TOTAL);
}

#[test]
fn oversized_input_and_output_are_bounded() {
    let repls = repls();
    let desk = desktop();
    let long = repls
        .run(
            &desk,
            "claude",
            ReplParams {
                code: "x".repeat(CODE_LIMIT + 1),
                ..ReplParams::default()
            },
        )
        .unwrap_err();
    assert_eq!(long.0, protocol::INVALID_PARAMS);
    let name = repls
        .run(
            &desk,
            "claude",
            ReplParams {
                code: "1".into(),
                session: Some("s".repeat(SESSION_LIMIT + 1)),
                ..ReplParams::default()
            },
        )
        .unwrap_err();
    assert_eq!(name.0, protocol::INVALID_PARAMS);
    let loud = run(
        &repls,
        &desk,
        "for (let i = 0; i < 30; i++) console.log('가'.repeat(10000))",
    );
    assert!(loud.output.chars().count() < worker::OUTPUT_LIMIT + 100);
    assert!(
        loud.output.ends_with("chars of output dropped]\n"),
        "{}",
        &loud.output[loud.output.len() - 80..]
    );
}

#[test]
fn base64_follows_rfc4648() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(plain.as_bytes()), encoded);
    }
    assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
}
