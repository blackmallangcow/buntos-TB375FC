// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt: buntOS 명령줄 도구. 지금은 에이전트 데몬(buntd-agent)에 묻는 `bunt agent`만 있다.

use std::process::ExitCode;

use bunt_agent::protocol::{
    self, ActParams, Acted, App, Installed, LaunchParams, Launched, Snapshot, SnapshotParams,
};
use serde_json::Value;

const USAGE: &str = "\
Usage: bunt agent <command> [options]

Read the desktop as accessibility trees (buntd-agent).

Commands:
  apps [--installed]       List running apps and their windows, or the installed apps
  launch <app>             Open an installed app (name or desktop ID) and print its window
  snapshot [options]       Print the active window as a compact tree with refs
  click <ref> [--pointer]  Press a button, switch, link or list item (--pointer: click it
                           with the pointer instead of its accessible action)
  fill <ref> <text>        Replace the text of a text field
  set <ref> <number>       Set the value of a slider or spin button
  focus <ref>              Move the keyboard focus to an element
  select <ref>             Select an item in its list
  press <keys> [--ref <ref>]
                           Press keys (e.g. ctrl+a BackSpace, Return) in the window of the
                           ref, or in the active window
  type <text> [--ref <ref>] Type text as key presses (letters, digits and symbols on the
                           keyboard layout; use fill for Korean and other composed text)

After an action the reply is what changed in that window once it stopped changing.
Take a new snapshot when refs may have moved.

Options for snapshot and actions:
  --app <name>             Read this app's windows instead (app name, window title, pid)
  --ref <eN>               Read only below a ref from an earlier snapshot
  -i, --interactive        Only elements that can be clicked or typed into
  --hidden                 Include hidden elements
  --diff                   Print only what changed since the last snapshot of the same scope
  --session <name>         Session for refs and diffs (default: default)
  --json                   Print the reply as JSON
";

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
        [
            "agent",
            action @ ("click" | "focus" | "select"),
            target,
            rest @ ..,
        ] => act(action, target, None, rest),
        ["agent", action @ ("fill" | "set"), target, value, rest @ ..] => {
            act(action, target, Some(value), rest)
        }
        ["agent", action @ ("press" | "type"), keys, rest @ ..] => press(action, keys, rest),
        ["agent", "launch", app, rest @ ..] => launch(app, rest),
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
    Ok(())
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
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match *arg {
            "--session" => {
                params.session = Some(rest.next().ok_or("--session needs a value")?.to_string())
            }
            "--json" => json = true,
            "--pointer" => params.pointer = true,
            other => return Err(format!("unknown option: {other}")),
        }
    }
    let params = serde_json::to_value(&params).map_err(|err| err.to_string())?;
    let reply = protocol::call("act", params)?;
    if json {
        println!("{reply:#}");
        return Ok(());
    }
    let acted: Acted = serde_json::from_value(reply)
        .map_err(|err| format!("bad reply from buntd-agent: {err}"))?;
    let subject = if target.is_empty() {
        String::new()
    } else {
        format!("{target}: ")
    };
    println!(
        "# {subject}{} (settled in {} ms)",
        acted.done, acted.settled_ms
    );
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
                println!("# this window has no accessibility tree");
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
