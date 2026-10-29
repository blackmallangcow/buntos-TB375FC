// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt: buntOS 명령줄 도구. 지금은 에이전트 데몬(buntd-agent)에 묻는 `bunt agent`만 있다.

use std::{path::Path, process::ExitCode};

use bunt_agent::protocol::{
    self, ActParams, Acted, App, Installed, LaunchParams, Launched, Screenshot, ScreenshotParams,
    Snapshot, SnapshotParams, Untracked,
};
use serde_json::Value;

const USAGE: &str = "\
Usage: bunt agent <command> [options]

Read and control the desktop through accessibility trees (buntd-agent).

Commands:
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

After an action the reply is what changed in that window once it stopped changing.
Take a new snapshot when refs may have moved.

Options for snapshot and actions:
  --app <name>             Read this app's windows instead (app name, window title, pid)
  --ref <eN>               Read only below a ref from an earlier snapshot
  -i, --interactive        Only elements that can be clicked or typed into
  --hidden                 Include hidden elements
  --diff                   Print only what changed since the last snapshot of the same scope
  --settle <ms>            Wait at most this long for the window to stop changing after an
                           action (default 3000; 0 reads it once right away)
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
    // 옛 데몬은 이 목록을 주지 않는다
    let untracked: Vec<Untracked> =
        serde_json::from_value(reply["untracked"].clone()).unwrap_or_default();
    if !untracked.is_empty() {
        println!(
            "# no accessibility tree (restart the app to read it; screenshot and click --at work):"
        );
    }
    for window in untracked {
        println!(
            "{} pid={} app={}\n  - window {:?}",
            window.name, window.pid, window.app_id, window.title
        );
    }
    Ok(())
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
    let misplaced = [
        (params.pointer, "--pointer", action == "click" && has_ref),
        (params.at.is_some(), "--at", action == "click" && !has_ref),
        (params.app.is_some(), "--app", params.at.is_some()),
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
    }
}
