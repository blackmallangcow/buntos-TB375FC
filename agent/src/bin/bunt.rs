// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt: buntOS 명령줄 도구. 에이전트 데몬(buntd-agent)에 묻는 `bunt agent`와, 같은 데몬을 MCP로 여는
//! `bunt mcp`가 있다.

use std::{
    io::{self, BufRead, Read, Write},
    path::Path,
    process::ExitCode,
};

use bunt_agent::{
    mcp,
    protocol::{
        self, ActParams, Acted, App, Installed, LaunchParams, Launched, Repl, ReplParams,
        Screenshot, ScreenshotParams, Skill, Snapshot, SnapshotParams, Untracked, WaitParams,
        Waited,
    },
    repl::source,
    skills,
};
use serde_json::Value;

const USAGE: &str = "\
Usage: bunt agent <command> [options]
       bunt mcp [--session <name>]

Read and control the desktop through accessibility trees (buntd-agent). `bunt mcp` speaks MCP
on standard input and output for AI apps without a shell: its tools are repl and skills.

Commands:
  repl [<code>]            Run JavaScript that reads and controls the apps (Playwright-like:
                           desktop.app('Calculator').getByRole('button', { name: '7' }).click()).
                           Variables stay between calls. See `bunt agent repl --help`
  skills [<name>]          List the usage guides, or print one (repl, desktop)
  apps [--installed]       List running apps and their windows, or the installed apps
  launch <app>             Open an installed app (name or desktop ID) and print its window
  snapshot [options]       Print the active window as a compact tree with refs
  screenshot [--app <name>] [--out <file>]
                           Save a PNG of the active window, or of the app's window, and
                           print its path. For what has no refs, such as a canvas
  click <ref> [--pointer]  Press a button, switch, link or list item (--pointer: click it
                           with the pointer instead of its accessible action)
  click --at <x>,<y> [--app <name>]
                           Click with the pointer at a point of the active window (or the
                           app's window): pixels of its screenshot, from its top-left corner
  fill <ref> <text>        Replace the text of a text field. Fails when the field does not
                           read back the text once the app settles
  set <ref> <number>       Set the value of a slider or spin button
  focus <ref>              Move the keyboard focus to an element
  select <ref>             Select an item in its list
  press <keys> [--ref <ref> [--keep-focus]]
                           Press keys (e.g. ctrl+a BackSpace, Return) in the active window.
                           --ref moves the keyboard focus to that element before every press,
                           so Tab always starts from it; --keep-focus only brings its window
                           forward. To Tab through a form, press without --ref
  type <text> [--ref <ref> [--keep-focus]] [--keys]
                           Type text into the text field that has the keyboard focus (or the
                           ref) through its text input, in any language. Fails when no text
                           field has the focus; --keys then presses a key for each character
                           instead (layout characters only), e.g. in a terminal or a game
  scroll <ref>             Scroll until the element is on screen
  scroll <up|down|left|right> [<notches>] [--ref <ref>]
                           Turn the mouse wheel (default 5 notches) over the ref, or over the
                           middle of the active window. Use the ref of the list or page to move
  wait <condition> [--app <name> | --ref <ref>] [--gone] [--timeout <ms>]
                           Wait until something shows up, goes away or changes in the active
                           window (or the app's windows, or below the ref), then print it.
                           Default 10000 ms, at most 120000; 0 checks once. Conditions:
                             --text <text>                 a name, text or field value with it
                             --role <role> [--name <name>] an element (button, textbox, ...)
                             --window <title>, --new-window
                                                           a window of the app (or of any app)
                             --value <value>, --changed    the value of the --ref element
                             --ref <ref> alone             that element
                           --exact matches whole names; --nth <n> picks one of several matches

After an action the reply is what changed in that window once it stopped changing.
Elements of a web page that are scrolled out of view are marked [offscreen]; an action on
one scrolls it into view first. Take a new snapshot when refs may have moved.

Options for snapshot:
  --app <name>             Read this app's windows instead (app name, window title, pid)
  --ref <eN>               Read only below a ref from an earlier snapshot
  -i, --interactive        Only elements that can be clicked or typed into
  --hidden                 Include hidden elements
  --diff                   Print only what changed since the last snapshot of the same scope

Options for actions:
  --app <name>             Without a ref (click --at, press, type, scroll): act on this app's
                           window instead of the one with the keyboard focus. With a ref it
                           only checks that the ref is in that app
  --settle <ms>            Wait at most this long for the window to stop changing after an
                           action (default 3000; 0 reads it once right away). A page that is
                           still loading counts as changing

Options for all:
  --session <name>         Session for refs and diffs (default: default)
  --json                   Print the reply as JSON
";

/// scroll의 기본 눈금 수
const SCROLL_NOTCHES: f64 = 5.0;

fn main() -> ExitCode {
    // `| head`처럼 읽는 쪽이 먼저 닫으면 다른 명령줄 도구처럼 조용히 끝난다 (Rust는 SIGPIPE를 무시해
    // println!이 패닉한다)
    // 안전: 다른 스레드가 뜨기 전에 신호 처리만 기본값으로 되돌린다
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["agent", "apps", rest @ ..] => apps(rest),
        ["agent", "snapshot", rest @ ..] => snapshot(rest),
        ["agent", "screenshot", rest @ ..] => screenshot(rest),
        // click --at x,y에는 ref가 없다
        ["agent", "click", target, rest @ ..] if !target.starts_with('-') => {
            act("click", target, None, rest)
        }
        ["agent", "click", rest @ ..] => act("click", "", None, rest),
        ["agent", action @ ("focus" | "select"), target, rest @ ..] => {
            act(action, target, None, rest)
        }
        ["agent", action @ ("fill" | "set"), target, value, rest @ ..] => {
            act(action, target, Some(value), rest)
        }
        ["agent", action @ ("press" | "type"), keys, rest @ ..] => press(action, keys, rest),
        ["agent", "scroll", rest @ ..] => scroll(rest),
        ["agent", "wait", rest @ ..] => wait(rest),
        ["agent", "repl", rest @ ..] => repl(rest),
        ["agent", "skills", rest @ ..] => skills(rest),
        ["agent", "launch", app, rest @ ..] => launch(app, rest),
        ["mcp", rest @ ..] => serve_mcp(rest),
        []
        | ["help" | "-h" | "--help", ..]
        | ["agent"]
        | ["agent", "help" | "-h" | "--help", ..] => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        _ => Err(format!("unknown command: {}\n\n{USAGE}", args.join(" "))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("bunt: {message}");
            ExitCode::FAILURE
        }
    }
}

fn apps(args: &[&str]) -> Result<(), String> {
    let json = args.contains(&"--json");
    if let Some(other) = args
        .iter()
        .find(|arg| !matches!(**arg, "--json" | "--installed"))
    {
        return Err(format!("unknown option: {other}"));
    }
    if args.contains(&"--installed") {
        let reply = protocol::call("installed", Value::Null)?;
        if json {
            println!("{reply:#}");
            return Ok(());
        }
        let apps: Vec<Installed> = serde_json::from_value(reply["apps"].clone())
            .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
        for app in apps {
            println!("{}  ({})", app.name, app.id);
        }
        return Ok(());
    }
    let reply = protocol::call("apps", Value::Null)?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    let apps: Vec<App> = serde_json::from_value(reply["apps"].clone())
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    for app in apps {
        let pid = app.pid.map_or(String::new(), |pid| format!(" pid={pid}"));
        println!("{}{pid} bus={}", app.name, app.bus);
        for window in app.windows {
            let active = if window.active { " [active]" } else { "" };
            println!("  - window {:?}{active}", window.title);
        }
    }
    // 옛 데몬은 이 목록을 주지 않는다
    let untracked: Vec<Untracked> =
        serde_json::from_value(reply["untracked"].clone()).unwrap_or_default();
    if !untracked.is_empty() {
        println!("# no accessibility tree (screenshot, click --at, press and scroll still work):");
    }
    for window in untracked {
        let why = if window.unsupported {
            "the app does not support accessibility"
        } else {
            "restart the app to read it"
        };
        println!(
            "{} pid={} app={} ({why})\n  - window {:?}",
            window.name, window.pid, window.app_id, window.title
        );
    }
    Ok(())
}

/// `scroll <ref>`나 `scroll <방향> [눈금 수] [--ref <ref>]`
fn scroll(args: &[&str]) -> Result<(), String> {
    let (params, json) = scroll_params(args)?;
    send_act(&params, json)
}

fn scroll_params(args: &[&str]) -> Result<(ActParams, bool), String> {
    let Some((&first, rest)) = args.split_first() else {
        return Err("scroll needs a ref or a direction (up, down, left, right)".into());
    };
    let direction = match first {
        "up" => Some([0.0, -1.0]),
        "down" => Some([0.0, 1.0]),
        "left" => Some([-1.0, 0.0]),
        "right" => Some([1.0, 0.0]),
        _ => None,
    };
    let mut params = ActParams {
        action: "scroll".into(),
        ..ActParams::default()
    };
    let Some([x, y]) = direction else {
        // 방향이 아니면 보이게 할 ref다
        params.target = first.to_string();
        let json = act_options(&mut params, rest)?;
        return Ok((params, json));
    };
    let (count, mut rest) = match rest.split_first() {
        Some((count, after)) if !count.starts_with('-') => {
            let count: f64 = count
                .parse()
                .ok()
                .filter(|count: &f64| count.is_finite() && *count > 0.0)
                .ok_or(format!(
                    "scroll needs a positive number of notches, not {count:?}"
                ))?;
            (count, after.to_vec())
        }
        _ => (SCROLL_NOTCHES, rest.to_vec()),
    };
    if let Some(at) = rest.iter().position(|arg| *arg == "--ref") {
        params.target = rest.get(at + 1).ok_or("--ref needs a value")?.to_string();
        rest.drain(at..=at + 1);
    }
    params.amount = Some([x * count, y * count]);
    let json = act_options(&mut params, &rest)?;
    Ok((params, json))
}

const REPL_USAGE: &str = "\
Usage: bunt agent repl [options] [<code>...]
       bunt agent repl [options] < script.js

Run JavaScript in your REPL on buntd-agent. `desktop` reads and controls the apps of this
desktop like Playwright drives a browser; variables stay between calls. The code comes from
the arguments, from standard input (`-` or a pipe), or line by line when standard input is a
terminal (.help, .reset, .exit there).

Options:
  --session <name>         Your REPL and refs of this session (default: default)
  --timeout <ms>           Stop the code after this long (default 30000, at most 120000)
  --reset                  Start from a new REPL (forget the variables) before running
  --json                   Print the reply as JSON (the last value as data when it fits in
                           1 MiB, 100 levels and 50 000 values)
  --                       Everything after it is code

The exit status is 1 when the code throws or the daemon refuses.

";

/// `repl`의 선택지
#[derive(Debug, Default, PartialEq)]
struct ReplOptions {
    session: Option<String>,
    timeout_ms: Option<u64>,
    reset: bool,
    json: bool,
    help: bool,
    /// 코드를 표준 입력에서 읽는다 (`-`)
    stdin: bool,
    /// 인자로 받은 코드
    code: Vec<String>,
}

fn repl_options(args: &[&str]) -> Result<ReplOptions, String> {
    let mut options = ReplOptions::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .map(|value| value.to_string())
                .ok_or(format!("{name} needs a value"))
        };
        match *arg {
            "--session" => options.session = Some(value("--session")?),
            "--timeout" => {
                let ms = value("--timeout")?;
                options.timeout_ms = Some(
                    ms.parse()
                        .map_err(|_| format!("--timeout needs milliseconds, not {ms:?}"))?,
                );
            }
            "--reset" => options.reset = true,
            "--json" => options.json = true,
            "-h" | "--help" => options.help = true,
            "-" => options.stdin = true,
            "--" => {
                options
                    .code
                    .extend(rest.by_ref().map(|arg| arg.to_string()));
            }
            other if other.starts_with("--") => {
                return Err(format!(
                    "unknown option: {other} (put code that starts with -- after --)"
                ));
            }
            code => options.code.push(code.to_string()),
        }
    }
    if options.stdin && !options.code.is_empty() {
        return Err("give the code as arguments or on standard input (-), not both".into());
    }
    Ok(options)
}

/// `repl`: 인자나 표준 입력의 코드를 한 번 돌리거나, 터미널이면 한 줄씩 받는다
fn repl(args: &[&str]) -> Result<(), String> {
    let options = repl_options(args)?;
    if options.help {
        let guide = skills::find("repl").map_or("", |guide| guide.content());
        print!("{REPL_USAGE}{guide}");
        return Ok(());
    }
    if !options.code.is_empty() {
        return run_repl(&options, options.code.join(" "), options.reset);
    }
    // 안전: 표준 입력이 터미널인지만 묻는다
    if !options.stdin && unsafe { libc::isatty(0) } == 1 {
        return interactive(&options);
    }
    let mut code = String::new();
    io::stdin()
        .read_to_string(&mut code)
        .map_err(|err| format!("cannot read the code from standard input: {err}"))?;
    run_repl(&options, code, options.reset)
}

/// 코드를 데몬의 REPL에서 돌리고 출력을 적는다. 코드가 던진 오류는 실패다
fn run_repl(options: &ReplOptions, code: String, reset: bool) -> Result<(), String> {
    let params = ReplParams {
        code,
        session: options.session.clone(),
        timeout_ms: options.timeout_ms,
        reset,
        images: false,
    };
    let params = serde_json::to_value(&params).map_err(|err| err.to_string())?;
    let reply = protocol::call("repl", params)?;
    if options.json {
        println!("{reply:#}");
        return Ok(());
    }
    let repl: Repl = serde_json::from_value(reply)
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    print!("{}", repl.output);
    let _ = io::stdout().flush();
    match repl.error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// 터미널에서 한 줄씩: 괄호와 따옴표가 다 닫히면 그때까지의 줄을 한 번에 보낸다
fn interactive(options: &ReplOptions) -> Result<(), String> {
    let session = options.session.as_deref().unwrap_or("default");
    eprintln!(
        "# buntOS REPL (session {session}): desktop, console and setTimeout. .help, .reset, .exit"
    );
    let mut reset = options.reset;
    let mut code = String::new();
    let mut lines = io::stdin().lock();
    loop {
        eprint!("{}", if code.is_empty() { "bunt> " } else { "....> " });
        let mut line = String::new();
        match lines.read_line(&mut line) {
            Ok(0) => {
                eprintln!();
                return Ok(());
            }
            Ok(_) => {}
            Err(err) => return Err(format!("cannot read the terminal: {err}")),
        }
        if code.is_empty() {
            match line.trim() {
                "" => continue,
                ".exit" => return Ok(()),
                ".help" => {
                    eprintln!(
                        "# JavaScript with top-level await. Try: await desktop.apps()\n# The \
                         API: bunt agent skills repl. .reset forgets the variables, .exit (or \
                         Ctrl-D) leaves"
                    );
                    continue;
                }
                ".reset" => {
                    reset = true;
                    eprintln!("# the next code runs in a new REPL");
                    continue;
                }
                _ => {}
            }
        }
        code.push_str(&line);
        if !source::complete(&code) {
            continue;
        }
        let ran = run_repl(options, std::mem::take(&mut code), reset);
        reset = false;
        if let Err(message) = ran {
            eprintln!("bunt: {message}");
        }
    }
}

/// `skills`: 사용 안내의 목록이나 글 하나
fn skills(args: &[&str]) -> Result<(), String> {
    let (name, json) = match args {
        [] => (None, false),
        ["--json"] => (None, true),
        [name] => (Some(*name), false),
        [name, "--json"] | ["--json", name] => (Some(*name), true),
        _ => return Err("skills takes one name (bunt agent skills repl)".into()),
    };
    let reply = protocol::call("skills", serde_json::json!({ "name": name }))?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    if name.is_some() {
        let skill: Skill = serde_json::from_value(reply)
            .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
        print!("{}", skill.content.unwrap_or_default());
        return Ok(());
    }
    let list: Vec<Skill> = serde_json::from_value(reply["skills"].clone())
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    for skill in list {
        println!("{}  {}", skill.name, skill.description);
    }
    Ok(())
}

const MCP_USAGE: &str = "\
Usage: bunt mcp [--session <name>]

Serve MCP (JSON-RPC 2.0, one message per line) on standard input and output, for AI apps that
have no shell. It asks buntd-agent over the same socket as `bunt agent`, so the same user
settings apply: System Settings > AI Agents decides whether the app that started this may
control the computer. Tools:
  repl                     Run JavaScript with the desktop API (screenshots come back as images)
  skills                   List or read the usage guides

Options:
  --session <name>         REPL and refs of this session (default: default)

Add it to an MCP client as a stdio server with the command `bunt` and the arguments `mcp`.
";

/// `mcp`: 표준 입출력으로 MCP를 말한다. 표준 출력은 MCP만 쓴다
fn serve_mcp(args: &[&str]) -> Result<(), String> {
    let session = match args {
        [] => "default".to_string(),
        ["--session", name] => name.to_string(),
        ["-h" | "--help"] => {
            print!("{MCP_USAGE}");
            return Ok(());
        }
        _ => return Err(format!("unknown option\n\n{MCP_USAGE}")),
    };
    let server = mcp::Server::new(mcp::Socket, session);
    mcp::serve(io::stdin().lock(), io::stdout(), &server)
        .map_err(|err| format!("MCP stopped: {err}"))
}

/// `wait`: 조건이 맞을 때까지 기다려 본 것을 적는다
fn wait(args: &[&str]) -> Result<(), String> {
    let (params, json) = wait_params(args)?;
    let params = serde_json::to_value(&params).map_err(|err| err.to_string())?;
    let reply = protocol::call("wait", params)?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    let waited: Waited = serde_json::from_value(reply)
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    println!("# {} (after {} ms)", waited.done, waited.waited_ms);
    for found in &waited.matches {
        println!("{}", found.line);
        // 글 줄에는 ref가 없다: 누르려면 그것을 담은 요소의 ref를 쓴다
        if let Some(owner) = found
            .reference
            .as_deref()
            .filter(|owner| !found.line.contains(&format!("[ref={owner}]")))
        {
            println!("# (inside {owner})");
        }
    }
    if waited.count > waited.matches.len() {
        println!("# and {} more", waited.count - waited.matches.len());
    }
    Ok(())
}

fn wait_params(args: &[&str]) -> Result<(WaitParams, bool), String> {
    let mut params = WaitParams::default();
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .map(|value| value.to_string())
                .ok_or(format!("{name} needs a value"))
        };
        match *arg {
            "--app" => params.app = Some(value("--app")?),
            "--ref" => params.scope = Some(value("--ref")?),
            "--role" => params.role = Some(value("--role")?),
            "--name" => params.name = Some(value("--name")?),
            "--text" => params.text = Some(value("--text")?),
            "--window" => params.window = Some(value("--window")?),
            "--new-window" => params.window = Some(String::new()),
            "--value" => params.value = Some(value("--value")?),
            "--nth" => {
                let nth = value("--nth")?;
                params.nth = Some(
                    nth.parse()
                        .map_err(|_| format!("--nth needs a whole number, not {nth:?}"))?,
                );
            }
            "--timeout" => {
                let ms = value("--timeout")?;
                params.timeout_ms = Some(
                    ms.parse()
                        .map_err(|_| format!("--timeout needs milliseconds, not {ms:?}"))?,
                );
            }
            "--session" => params.session = Some(value("--session")?),
            "--changed" => params.changed = true,
            "--gone" => params.gone = true,
            "--exact" => params.exact = true,
            "--json" => json = true,
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok((params, json))
}

fn screenshot(args: &[&str]) -> Result<(), String> {
    let mut params = ScreenshotParams::default();
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .map(|value| value.to_string())
                .ok_or(format!("{name} needs a value"))
        };
        match *arg {
            "--app" => params.app = Some(value("--app")?),
            "--out" => params.out = Some(absolute(&value("--out")?)?),
            "--json" => json = true,
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let params = serde_json::to_value(&params).map_err(|err| err.to_string())?;
    let reply = protocol::call("screenshot", params)?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    let shot: Screenshot = serde_json::from_value(reply)
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    println!(
        "# window {:?} ({}, pid {}): {}x{} pixels at scale {}; click --at takes these pixels",
        shot.window, shot.app_id, shot.pid, shot.width, shot.height, shot.scale
    );
    println!("{}", shot.path);
    Ok(())
}

/// 데몬은 다른 디렉터리에서 돌므로 상대 경로는 지금 디렉터리를 붙여 넘긴다
fn absolute(path: &str) -> Result<String, String> {
    if Path::new(path).is_absolute() {
        return Ok(path.to_string());
    }
    let here = std::env::current_dir().map_err(|err| format!("cannot resolve {path}: {err}"))?;
    Ok(here.join(path).display().to_string())
}

/// "120,340" → [120, 340]
fn point(text: &str) -> Result<[f64; 2], String> {
    let bad = || format!("--at needs x,y in pixels (e.g. 120,340), not {text:?}");
    let (x, y) = text.split_once(',').ok_or_else(bad)?;
    let number = |part: &str| {
        part.trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(bad)
    };
    Ok([number(x)?, number(y)?])
}

fn snapshot(args: &[&str]) -> Result<(), String> {
    let mut params = SnapshotParams::default();
    let (mut diff, mut json) = (false, false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .map(|value| value.to_string())
                .ok_or(format!("{name} needs a value"))
        };
        match *arg {
            "--app" => params.app = Some(value("--app")?),
            "--ref" => params.scope = Some(value("--ref")?),
            "--session" => params.session = Some(value("--session")?),
            "-i" | "--interactive" => params.interactive = true,
            "--hidden" => params.hidden = true,
            "--diff" => diff = true,
            "--json" => json = true,
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let params = serde_json::to_value(&params).map_err(|err| err.to_string())?;
    let reply = protocol::call("snapshot", params)?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    let snapshot: Snapshot = serde_json::from_value(reply)
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    if !diff {
        print!("{}", snapshot.tree);
    } else if snapshot.first {
        println!("# note: no earlier snapshot of this scope; full tree follows");
        print!("{}", snapshot.tree);
    } else if snapshot.diff.is_empty() {
        println!("# no changes");
    } else {
        print!("{}", snapshot.diff);
    }
    Ok(())
}

fn act(action: &str, target: &str, value: Option<&str>, args: &[&str]) -> Result<(), String> {
    let mut params = ActParams {
        target: target.to_string(),
        action: action.to_string(),
        value: value.map(str::to_string),
        ..ActParams::default()
    };
    let json = act_options(&mut params, args)?;
    send_act(&params, json)
}

/// 동작을 보내고 답을 적는다: 한 일, 기다린 시간, 새 창, 그 창의 diff
fn send_act(params: &ActParams, json: bool) -> Result<(), String> {
    let value = serde_json::to_value(params).map_err(|err| err.to_string())?;
    let reply = protocol::call("act", value)?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    let acted: Acted = serde_json::from_value(reply)
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    let subject = if params.target.is_empty() {
        String::new()
    } else {
        format!("{}: ", params.target)
    };
    if !acted.observed {
        println!("# {subject}{}", acted.done);
        return Ok(());
    }
    let ms = acted.settled_ms;
    let waited = if acted.settled {
        format!("settled in {ms} ms")
    } else if acted.diff.is_empty() {
        format!("no change within {ms} ms; it may still change")
    } else {
        format!("still changing after {ms} ms; the diff is the window at that moment")
    };
    println!("# {subject}{} ({waited})", acted.done);
    for title in &acted.new_windows {
        println!("# new window: {title:?}");
    }
    if acted.diff.is_empty() {
        println!("# no change in this window");
    } else {
        print!("{}", acted.diff);
    }
    Ok(())
}

/// 동작의 옵션을 읽는다. 그 동작에 맞지 않는 옵션은 오류다. --json이면 참
fn act_options(params: &mut ActParams, args: &[&str]) -> Result<bool, String> {
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = |name: &str| {
            rest.next()
                .map(|value| value.to_string())
                .ok_or(format!("{name} needs a value"))
        };
        match *arg {
            "--session" => params.session = Some(value("--session")?),
            "--json" => json = true,
            "--pointer" => params.pointer = true,
            "--at" => params.at = Some(point(&value("--at")?)?),
            "--app" => params.app = Some(value("--app")?),
            "--keep-focus" => params.keep_focus = true,
            "--keys" => params.keys = true,
            "--settle" => {
                let ms = value("--settle")?;
                params.settle_ms = Some(
                    ms.parse()
                        .map_err(|_| format!("--settle needs milliseconds, not {ms:?}"))?,
                );
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let (action, has_ref) = (params.action.as_str(), !params.target.is_empty());
    // --app은 어느 동작에나 된다: ref가 없으면 그 앱의 창에 하고, 있으면 ref가 그 앱의 것인지 본다
    let misplaced = [
        (params.pointer, "--pointer", action == "click" && has_ref),
        (params.at.is_some(), "--at", action == "click" && !has_ref),
        (
            params.keep_focus,
            "--keep-focus",
            matches!(action, "press" | "type") && has_ref,
        ),
        (params.keys, "--keys", action == "type"),
    ];
    if let Some((_, name, _)) = misplaced.iter().find(|(used, _, fits)| *used && !fits) {
        return Err(format!(
            "{name} does not go with this {action}; see `bunt agent help`"
        ));
    }
    if action == "click" && !has_ref && params.at.is_none() {
        return Err("click needs a ref or --at x,y".into());
    }
    Ok(json)
}

fn press(action: &str, keys: &str, args: &[&str]) -> Result<(), String> {
    let mut target = "";
    let mut rest = args.to_vec();
    if let Some(at) = rest.iter().position(|arg| *arg == "--ref") {
        target = rest.get(at + 1).ok_or("--ref needs a value")?;
        rest.drain(at..=at + 1);
    }
    act(action, target, Some(keys), &rest)
}

fn launch(app: &str, args: &[&str]) -> Result<(), String> {
    let mut params = LaunchParams {
        app: app.to_string(),
        ..LaunchParams::default()
    };
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match *arg {
            "--session" => {
                params.session = Some(rest.next().ok_or("--session needs a value")?.to_string())
            }
            "--json" => json = true,
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let params = serde_json::to_value(&params).map_err(|err| err.to_string())?;
    let reply = protocol::call("launch", params)?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    let launched: Launched = serde_json::from_value(reply)
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    match launched.window {
        Some(window) => {
            let how = if launched.running {
                "was already open; brought forward"
            } else {
                "opened"
            };
            println!(
                "# {} ({}) {how}: window {window:?}",
                launched.name, launched.id
            );
            if launched.tree.is_empty() {
                println!(
                    "# this window has no accessibility tree; use screenshot and click --at on it"
                );
            }
            print!("{}", launched.tree);
        }
        None => println!(
            "# opened {} ({}); no new window appeared yet. Run `bunt agent apps`",
            launched.name, launched.id
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(action: &str, target: &str, args: &[&str]) -> Result<ActParams, String> {
        let mut params = ActParams {
            action: action.into(),
            target: target.into(),
            ..ActParams::default()
        };
        act_options(&mut params, args).map(|_| params)
    }

    #[test]
    fn points_are_two_pixel_numbers() {
        assert_eq!(point("120,340"), Ok([120.0, 340.0]));
        assert_eq!(point(" 1.5 , 2 "), Ok([1.5, 2.0]));
        assert!(point("120").is_err());
        assert!(point("-1,2").is_err());
        assert!(point("a,b").is_err());
    }

    #[test]
    fn options_fit_their_action() {
        let at = params("click", "", &["--at", "10,20", "--app", "firefox"]).unwrap();
        assert_eq!(at.at, Some([10.0, 20.0]));
        assert_eq!(at.app.as_deref(), Some("firefox"));
        assert!(params("click", "", &[]).is_err());
        assert!(params("click", "e3", &["--at", "1,2"]).is_err());
        assert!(params("click", "", &["--app", "firefox"]).is_err());
        assert!(params("click", "e3", &["--pointer"]).unwrap().pointer);
        let typed = params("type", "e4", &["--keep-focus", "--keys", "--settle", "500"]).unwrap();
        assert!(typed.keep_focus && typed.keys);
        assert_eq!(typed.settle_ms, Some(500));
        // --keep-focus는 ref가 있을 때만, --keys는 type에만
        assert!(params("press", "", &["--keep-focus"]).is_err());
        assert!(params("press", "e4", &["--keys"]).is_err());
        assert!(params("fill", "e4", &["--settle", "soon"]).is_err());
        // --app은 ref가 없으면 그 앱의 창에 하고, 있으면 ref가 그 앱의 것인지 본다
        assert!(params("click", "e3", &["--app", "계산기"]).is_ok());
        assert!(params("press", "", &["--app", "firefox"]).is_ok());
    }

    #[test]
    fn repl_takes_code_from_arguments_or_standard_input() {
        let options = repl_options(&[
            "--session",
            "s1",
            "await",
            "desktop.apps()",
            "--timeout",
            "500",
            "--reset",
        ])
        .unwrap();
        assert_eq!(options.code, ["await", "desktop.apps()"]);
        assert_eq!(
            (
                options.session.as_deref(),
                options.timeout_ms,
                options.reset
            ),
            (Some("s1"), Some(500), true)
        );
        // -- 뒤는 모두 코드다
        let dashed = repl_options(&["--json", "--", "--x", "-1"]).unwrap();
        assert!(dashed.json);
        assert_eq!(dashed.code, ["--x", "-1"]);
        assert!(repl_options(&["-"]).unwrap().stdin);
        assert!(repl_options(&["-", "1"]).is_err());
        assert!(repl_options(&["--timeout", "soon"]).is_err());
        assert!(repl_options(&["--frobnicate"]).is_err());
        assert!(repl_options(&["--help"]).unwrap().help);
        assert_eq!(repl_options(&[]).unwrap(), ReplOptions::default());
    }

    #[test]
    fn wait_reads_its_condition_and_scope() {
        let (params, json) = wait_params(&[
            "--app",
            "계산기",
            "--role",
            "button",
            "--name",
            "=",
            "--exact",
            "--nth",
            "-1",
            "--timeout",
            "0",
            "--json",
        ])
        .unwrap();
        assert!(json && params.exact);
        assert_eq!(params.app.as_deref(), Some("계산기"));
        assert_eq!(
            (params.role.as_deref(), params.name.as_deref()),
            (Some("button"), Some("="))
        );
        assert_eq!((params.nth, params.timeout_ms), (Some(-1), Some(0)));
        let (window, _) = wait_params(&["--new-window"]).unwrap();
        assert_eq!(window.window.as_deref(), Some(""));
        let (value, _) = wait_params(&["--ref", "e9", "--value", "42"]).unwrap();
        assert_eq!(
            (value.scope.as_deref(), value.value.as_deref()),
            (Some("e9"), Some("42"))
        );
        assert!(wait_params(&["--timeout", "soon"]).is_err());
        assert!(wait_params(&["--nth", "x"]).is_err());
        assert!(wait_params(&["--text"]).is_err());
        assert!(wait_params(&["--later"]).is_err());
        // 조건이 맞는지는 데몬이 본다 (wait::condition)
        assert!(wait_params(&[]).is_ok());
    }

    #[test]
    fn scroll_takes_a_ref_or_a_direction_with_notches() {
        let into_view = scroll_params(&["e12"]).unwrap().0;
        assert_eq!((into_view.target.as_str(), into_view.amount), ("e12", None));
        let down = scroll_params(&["down"]).unwrap().0;
        assert_eq!(down.amount, Some([0.0, SCROLL_NOTCHES]));
        let up = scroll_params(&["up", "3", "--ref", "e7", "--app", "firefox"])
            .unwrap()
            .0;
        assert_eq!(
            (up.target.as_str(), up.amount, up.app.as_deref()),
            ("e7", Some([0.0, -3.0]), Some("firefox"))
        );
        assert_eq!(
            scroll_params(&["left", "2"]).unwrap().0.amount,
            Some([-2.0, 0.0])
        );
        assert!(scroll_params(&[]).is_err());
        assert!(scroll_params(&["down", "0"]).is_err());
        assert!(scroll_params(&["down", "--ref"]).is_err());
    }
}
