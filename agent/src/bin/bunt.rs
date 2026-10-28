// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt: buntOS 명령줄 도구. 지금은 에이전트 데몬(buntd-agent)에 묻는 `bunt agent`만 있다.

use std::process::ExitCode;

use bunt_agent::protocol::{self, App, Snapshot, SnapshotParams};
use serde_json::Value;

const USAGE: &str = "\
Usage: bunt agent <command> [options]

Read the desktop as accessibility trees (buntd-agent).

Commands:
  apps                     List apps and their windows
  snapshot [options]       Print the active window as a compact tree with refs

Snapshot options:
  --app <name>             Read this app's windows instead (app name, window title, pid)
  --ref <eN>               Read only below a ref from an earlier snapshot
  -i, --interactive        Only elements that can be clicked or typed into
  --hidden                 Include hidden elements
  --diff                   Print only what changed since the last snapshot of the same scope
  --session <name>         Session for refs and diffs (default: default)
  --json                   Print the reply as JSON
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["agent", "apps", rest @ ..] => apps(rest),
        ["agent", "snapshot", rest @ ..] => snapshot(rest),
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
    let json = match args {
        [] => false,
        ["--json"] => true,
        _ => return Err(format!("unknown option: {}", args.join(" "))),
    };
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
