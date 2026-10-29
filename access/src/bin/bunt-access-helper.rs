// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-access-helper: 접근 정책(/etc/buntos/agent-access.toml)을 고치는 특권 도우미.
//!
//! 시스템 설정의 AI 에이전트 페이지가 `pkexec /usr/libexec/buntos/bunt-access-helper <명령> …`으로
//! 실행한다. polkit 동작은 `org.buntos.agent.access.change`이고 기본값이 auth_admin이라 바꿀 때마다
//! 관리자 암호를 묻는다(auth_admin_keep이면 허용받은 AI가 남은 인증으로 제 권한을 늘릴 수 있다).
//! 입력을 검사하고, 정책 디렉터리를 잠근 채 읽고 고쳐 원자적으로 쓴다.
//!
//! AI를 더할 때 실행 파일과 스크립트가 시스템 설정을 연 사용자(PKEXEC_UID)의 홈 아래에 있으면 판 번호·빌드
//! 해시 자리를 `*`로 바꾼 패턴도 적는다([`version_pattern`]). 스스로 업데이트하는 AI가 업데이트한 뒤에도
//! 허용이 풀리지 않는다.
//!
//! 시험할 때는 BUNT_ACCESS_POLICY_FILE로 다른 파일을 주고, 그때만 BUNT_ACCESS_HOME으로 패턴을 만들 홈도
//! 준다. pkexec는 환경 변수를 넘기지 않으므로 이 변수들은 root 권한으로 이어지지 않는다.

use std::{
    env,
    fs::{self, File},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
};

use bunt_access::{
    AllowedClient, POLICY_PATH, Policy, can_identify, normalize_app_id, valid_app_id, valid_name,
    valid_path, version_pattern,
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
    /// 패턴은 검사(check)가 채운다
    Add(AllowedClient),
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
    let test = env::var_os("BUNT_ACCESS_POLICY_FILE");
    let path = match &test {
        Some(path) => PathBuf::from(path),
        None => {
            if !running_as_root() {
                return Err(Failure::Invalid("must run as root (through pkexec)".into()));
            }
            PathBuf::from(POLICY_PATH)
        }
    };
    let command = check(command, test.is_some())?;
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
            Command::Add(AllowedClient {
                name: name.trim().to_owned(),
                exe,
                exe_pattern: None,
                script,
                script_pattern: None,
                enabled: true,
            })
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

/// 입력을 검사한다. 더하는 클라이언트는 있는 파일이어야 하고 링크를 푼 경로로 적는다. 홈 아래의
/// 경로에는 판 번호·빌드 해시 패턴을 붙인다 (test면 BUNT_ACCESS_HOME을 홈으로 쓴다)
fn check(command: Command, test: bool) -> Result<Command, Failure> {
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
        Command::Add(AllowedClient {
            name, exe, script, ..
        }) => {
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
            // 업데이트로 판 번호·빌드 해시 자리만 바뀐 실행 파일과 스크립트도 맞도록
            let home = caller_home(test);
            let pattern = |path: &Path| {
                home.as_ref()
                    .and_then(|(uid, home)| version_pattern(path, home, *uid))
            };
            Ok(Command::Add(AllowedClient {
                name,
                exe_pattern: pattern(&exe),
                exe,
                script_pattern: script.as_deref().and_then(pattern),
                script,
                enabled: true,
            }))
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
        Command::Add(client) => policy.add_client(client),
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

/// 유효 uid가 0인지
fn running_as_root() -> bool {
    status_uid(1) == Some(0)
}

/// /proc/self/status의 uid. 0번째는 실제, 1번째는 유효 uid다
fn status_uid(index: usize) -> Option<u32> {
    fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))?
        .split_whitespace()
        .nth(index)?
        .parse()
        .ok()
}

/// 패턴을 만들 사용자와 홈: pkexec가 준 PKEXEC_UID(없으면 실제 uid)와 /etc/passwd의 그 홈.
/// 시험(test)에서 BUNT_ACCESS_HOME이 있으면 그 디렉터리와 그 주인을 쓴다
fn caller_home(test: bool) -> Option<(u32, PathBuf)> {
    if let Some(home) = env::var_os("BUNT_ACCESS_HOME").filter(|_| test) {
        let home = PathBuf::from(home);
        return Some((fs::metadata(&home).ok()?.uid(), home));
    }
    let uid = env::var("PKEXEC_UID")
        .ok()
        .and_then(|uid| uid.parse().ok())
        .or_else(|| status_uid(0))?;
    let home = home_of(&fs::read_to_string("/etc/passwd").ok()?, uid)?;
    Some((uid, home))
}

/// passwd 형식(이름:암호:uid:gid:설명:홈:셸)에서 uid의 홈. 절대 경로가 아니면 None
fn home_of(passwd: &str, uid: u32) -> Option<PathBuf> {
    passwd
        .lines()
        .map(|line| line.split(':').collect::<Vec<_>>())
        .find(|fields| fields.len() == 7 && fields[2].parse() == Ok(uid))
        .map(|fields| PathBuf::from(fields[5]))
        .filter(|home| home.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_home_of_a_uid() {
        let passwd = "\
root:x:0:0:root:/root:/bin/bash
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
buntos:x:1000:1000:buntOS:/home/buntos:/bin/bash
broken:x:1001
relative:x:1002:1002::home/relative:/bin/sh
";
        assert_eq!(home_of(passwd, 1000), Some(PathBuf::from("/home/buntos")));
        assert_eq!(home_of(passwd, 0), Some(PathBuf::from("/root")));
        assert_eq!(home_of(passwd, 1001), None);
        assert_eq!(home_of(passwd, 1002), None);
        assert_eq!(home_of(passwd, 4242), None);
    }
}
