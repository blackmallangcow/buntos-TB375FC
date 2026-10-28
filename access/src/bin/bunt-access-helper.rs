// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-access-helper: 접근 정책(/etc/buntos/agent-access.toml)을 고치는 특권 도우미.
//!
//! 시스템 설정의 AI 에이전트 페이지가 `pkexec /usr/libexec/buntos/bunt-access-helper <명령> …`으로
//! 실행한다. polkit 동작은 `org.buntos.agent.access.change`이고 기본값이 auth_admin이라 바꿀 때마다
//! 관리자 암호를 묻는다(auth_admin_keep이면 허용받은 AI가 남은 인증으로 제 권한을 늘릴 수 있다).
//! 입력을 검사하고, 정책 디렉터리를 잠근 채 읽고 고쳐 원자적으로 쓴다.
//!
//! 시험할 때는 BUNT_ACCESS_POLICY_FILE로 다른 파일을 준다. pkexec는 환경 변수를 넘기지 않으므로
//! 이 변수는 root 권한으로 이어지지 않는다.

use std::{
    env,
    fs::{self, File},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::ExitCode,
};

use bunt_access::{
    POLICY_PATH, Policy, can_identify, normalize_app_id, valid_app_id, valid_name, valid_path,
};

const USAGE: &str = "\
usage: bunt-access-helper <command> [arguments]
  add-client NAME EXE [SCRIPT]   allow an AI client (and turn it on)
  remove-client EXE [SCRIPT]
  enable-client EXE [SCRIPT]
  disable-client EXE [SCRIPT]
  block-app APP_ID...            keep AI agents away from these apps (desktop IDs)
  unblock-app APP_ID...";

enum Command {
    Add {
        name: String,
        exe: PathBuf,
        script: Option<PathBuf>,
    },
    Remove {
        exe: PathBuf,
        script: Option<PathBuf>,
    },
    Enable {
        exe: PathBuf,
        script: Option<PathBuf>,
        enabled: bool,
    },
    Block(Vec<String>),
    Unblock(Vec<String>),
}

enum Failure {
    Usage(String),
    Invalid(String),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(message)) => {
            eprintln!("bunt-access-helper: {message}\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Invalid(message)) => {
            eprintln!("bunt-access-helper: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Failure> {
    let args = env::args_os()
        .skip(1)
        .map(|arg| {
            arg.into_string()
                .map_err(|_| Failure::Invalid("arguments must be UTF-8".into()))
        })
        .collect::<Result<Vec<String>, Failure>>()?;
    let command = parse(&args)?;
    let path = match env::var_os("BUNT_ACCESS_POLICY_FILE") {
        Some(path) => PathBuf::from(path),
        None => {
            if !running_as_root() {
                return Err(Failure::Invalid("must run as root (through pkexec)".into()));
            }
            PathBuf::from(POLICY_PATH)
        }
    };
    let command = check(command)?;
    update(&path, command).map_err(|err| Failure::Invalid(format!("{}: {err}", path.display())))
}

fn parse(args: &[String]) -> Result<Command, Failure> {
    let Some((command, rest)) = args.split_first() else {
        return Err(Failure::Usage("missing command".into()));
    };
    let client = |rest: &[String]| match rest {
        [exe] => Ok((PathBuf::from(exe), None)),
        [exe, script] => Ok((PathBuf::from(exe), Some(PathBuf::from(script)))),
        _ => Err(Failure::Usage(format!("{command}: expected EXE [SCRIPT]"))),
    };
    let apps = |rest: &[String]| {
        if rest.is_empty() {
            Err(Failure::Usage(format!("{command}: expected APP_ID...")))
        } else {
            Ok(rest.to_vec())
        }
    };
    Ok(match command.as_str() {
        "add-client" => {
            let Some((name, rest)) = rest.split_first() else {
                return Err(Failure::Usage(
                    "add-client: expected NAME EXE [SCRIPT]".into(),
                ));
            };
            let (exe, script) = client(rest)?;
            Command::Add {
                name: name.trim().to_owned(),
                exe,
                script,
            }
        }
        "remove-client" => {
            let (exe, script) = client(rest)?;
            Command::Remove { exe, script }
        }
        "enable-client" | "disable-client" => {
            let (exe, script) = client(rest)?;
            Command::Enable {
                exe,
                script,
                enabled: command == "enable-client",
            }
        }
        "block-app" => Command::Block(apps(rest)?),
        "unblock-app" => Command::Unblock(apps(rest)?),
        _ => return Err(Failure::Usage(format!("unknown command {command}"))),
    })
}

/// 입력을 검사한다. 더하는 클라이언트는 있는 파일이어야 하고 링크를 푼 경로로 적는다
fn check(command: Command) -> Result<Command, Failure> {
    let path = |path: &Path| {
        if valid_path(path) {
            Ok(())
        } else {
            Err(Failure::Invalid(format!(
                "{}: expected a clean absolute path",
                path.display()
            )))
        }
    };
    let apps = |apps: Vec<String>| {
        apps.into_iter()
            .map(|app| {
                let id = normalize_app_id(&app);
                if valid_app_id(id) {
                    Ok(id.to_owned())
                } else {
                    Err(Failure::Invalid(format!("{app}: not a desktop ID")))
                }
            })
            .collect::<Result<Vec<_>, _>>()
    };
    match command {
        Command::Add { name, exe, script } => {
            if !valid_name(&name) {
                return Err(Failure::Invalid(
                    "the name must be 1 to 64 characters without control characters".into(),
                ));
            }
            path(&exe)?;
            if let Some(script) = &script {
                path(script)?;
            }
            let regular = |path: &Path| {
                fs::canonicalize(path)
                    .ok()
                    .filter(|canonical| canonical.is_file())
                    .ok_or_else(|| {
                        Failure::Invalid(format!("{}: not an existing file", path.display()))
                    })
            };
            let exe = regular(&exe)?;
            let script = match &script {
                Some(script) => Some(regular(script)?),
                None => None,
            };
            // 셸·감싼 프로그램처럼 identify가 낼 수 없는 클라이언트는 허용해도 맞을 수 없다
            if !can_identify(&exe, script.as_deref()) {
                return Err(Failure::Invalid(format!(
                    "{}: agents are not identified as this program (shells, wrappers and \
                     scripts of unknown interpreters cannot be allowed)",
                    exe.display()
                )));
            }
            Ok(Command::Add { name, exe, script })
        }
        Command::Remove { exe, script } => {
            path(&exe)?;
            if let Some(script) = &script {
                path(script)?;
            }
            Ok(Command::Remove { exe, script })
        }
        Command::Enable {
            exe,
            script,
            enabled,
        } => {
            path(&exe)?;
            if let Some(script) = &script {
                path(script)?;
            }
            Ok(Command::Enable {
                exe,
                script,
                enabled,
            })
        }
        Command::Block(list) => Ok(Command::Block(apps(list)?)),
        Command::Unblock(list) => Ok(Command::Unblock(apps(list)?)),
    }
}

/// 정책 디렉터리를 잠그고 읽어 고친 뒤 쓴다. 바뀐 것이 없으면 쓰지 않는다
fn update(path: &Path, command: Command) -> Result<(), String> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or("the policy file has no directory")?;
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755))
            .map_err(|err| err.to_string())?;
    }
    // 두 도우미가 함께 돌아도 한 번에 하나씩 고친다 (파일은 이름을 바꿔 덮으므로 디렉터리를 잠근다)
    let lock = File::open(dir).map_err(|err| err.to_string())?;
    lock.lock().map_err(|err| err.to_string())?;
    let text = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err.to_string()),
    };
    let mut policy = match text.as_deref().map(Policy::parse) {
        None => Policy::default(),
        Some(Ok(policy)) => policy,
        // 틀린 파일은 빈 정책(모두 거절)과 같으므로 새로 시작해도 권한이 늘지 않는다
        Some(Err(err)) => {
            eprintln!("bunt-access-helper: replacing the broken policy file: {err}");
            Policy::default()
        }
    };
    let before = policy.clone();
    match command {
        Command::Add { name, exe, script } => policy.add_client(&name, &exe, script.as_deref()),
        Command::Remove { exe, script } => {
            policy.remove_client(&exe, script.as_deref());
        }
        Command::Enable {
            exe,
            script,
            enabled,
        } => {
            if !policy.set_enabled(&exe, script.as_deref(), enabled) {
                return Err(format!("{}: no such client", exe.display()));
            }
        }
        Command::Block(apps) => {
            for app in apps {
                policy.block_app(&app);
            }
        }
        Command::Unblock(apps) => {
            for app in apps {
                policy.unblock_app(&app);
            }
        }
    }
    if policy == before && text.is_some() {
        return Ok(());
    }
    policy.save_to(path).map_err(|err| err.to_string())
}

/// 유효 uid가 0인지 (/proc/self/status)
fn running_as_root() -> bool {
    fs::read_to_string("/proc/self/status").is_ok_and(|status| {
        status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .and_then(|uids| uids.split_whitespace().nth(1))
            == Some("0")
    })
}
