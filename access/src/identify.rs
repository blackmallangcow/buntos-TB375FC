// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 에이전트 데몬에 붙은 프로세스가 어떤 AI인지 알아본다.
//!
//! 소켓에 붙은 프로세스(대개 `bunt` CLI)에서 부모로 올라가며 처음 나오는 진짜 프로그램을 고른다.
//! - 셸(sh, bash, dash, zsh, fish 등)과 env·sudo·su·timeout·nohup·setsid 같은 감싼 프로그램, `bunt`
//!   자신은 건너뛴다. 실행 파일을 복사해 이름만 바꾼 프로그램이 건너뛰어지지 않게, /usr 아래 root가 가진
//!   파일만 건너뛴다. 소켓에 붙은 프로세스가 `bunt`이면 개발 중 빌드처럼 다른 곳에 있어도 건너뛴다.
//! - node·python 같은 인터프리터면 실행한 스크립트(옵션이 아닌 첫 인자)의 경로도 쓴다.
//! - 요청한 프로세스 자신이나 그 사이의 셸·감싼 프로그램이 root로 돌면 관리자다. root는 이미 무엇이든
//!   하기 때문이다 (직렬 콘솔과 e2e의 `sudo -u buntos bunt …`). login·sshd처럼 root인 프로그램이 사용자
//!   권한으로 띄운 셸 아래는 관리자가 아니다. 그 셸 안의 AI가 셸로 바꿔 실행(exec)해 root 조상을 빌릴 수
//!   있기 때문이다.
//! - 사용자로 도는 데몬은 root 프로세스의 실행 파일을 읽지 못한다(/proc/<pid>/exe의 ptrace 검사).
//!   그런 root 프로세스는 /proc/<pid>/status의 이름으로 셸·감싼 프로그램인지 본다. root 프로세스의
//!   이름은 사용자가 바꿀 수 없기 때문이다. 사용자 프로세스는 이름을 바꾸고 실행 파일을 숨길 수
//!   있으므로(PR_SET_DUMPABLE) 실행 파일을 읽지 못하면 알아보지 못한 클라이언트다.
//! - init(pid 1)과 systemd 서비스 관리자까지 올라가면(부모가 끝나 입양된 프로세스) 알아보지 못한
//!   클라이언트다.

use std::{
    fmt, fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use crate::{Client, desktop};

/// 부모를 얼마나 올라갈지
const MAX_DEPTH: usize = 64;
const MAX_NAME: usize = 64;

/// 건너뛰는 셸과 감싼 프로그램 (실행 파일 이름)
const WRAPPERS: &[&str] = &[
    "sh", "bash", "dash", "zsh", "fish", "ksh", "mksh", "env", "sudo", "su", "timeout", "nohup",
    "setsid", "nice", "ionice", "stdbuf", "xargs", "flock", "runuser", "bunt",
];

/// 셸: 셸 스크립트는 셸 프로세스로 돌아 건너뛰어지므로 허용할 수 없다
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "fish", "ksh", "mksh"];

/// 스크립트 경로를 따로 보는 인터프리터. python3.13처럼 판 번호가 붙은 이름도 맞는다
const INTERPRETERS: &[&str] = &[
    "node", "nodejs", "bun", "deno", "python", "pypy", "perl", "ruby", "php", "lua", "luajit",
];

/// 다음 인자를 값으로 받는 인터프리터 옵션
const VALUE_OPTIONS: &[&str] = &[
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
    "-C",
    "--conditions",
    "-W",
    "-X",
    "--check-hash-based-pycs",
];

/// 스크립트 파일 없이 코드나 모듈을 실행하는 옵션 (python -c, -m, node -e, -p)
const CODE_OPTIONS: &[&str] = &["-c", "-m", "-e", "-p", "--eval", "--print"];

/// 프로세스 하나에 대해 알아본 것
#[derive(Debug, Clone, Default)]
pub(crate) struct Process {
    pub ppid: u32,
    /// 실행 파일 (/proc/<pid>/exe). 읽지 못하면(root나 다른 사용자의 프로세스) 비어 있다
    pub exe: PathBuf,
    /// 프로세스 이름 (/proc/<pid>/status의 Name, 15자까지)
    pub name: String,
    /// 실행 파일이 /usr 아래에 있고 root가 가지며 그룹·남이 쓸 수 없다
    pub system: bool,
    /// 실제·유효·저장 uid 가운데 하나가 0이다
    pub root: bool,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

pub(crate) trait ProcessTable {
    fn process(&self, pid: u32) -> Option<Process>;
}

/// /proc에서 읽는다
struct Procfs;

impl ProcessTable for Procfs {
    fn process(&self, pid: u32) -> Option<Process> {
        let dir = PathBuf::from(format!("/proc/{pid}"));
        let status = fs::read_to_string(dir.join("status")).ok()?;
        let field = |name: &str| {
            status
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .map(str::trim)
        };
        let name = field("Name:")?.to_owned();
        let ppid = field("PPid:")?.parse().ok()?;
        let root = field("Uid:")?
            .split_whitespace()
            .take(3)
            .any(|uid| uid == "0");
        let exe = fs::read_link(dir.join("exe"))
            .map(strip_deleted)
            .unwrap_or_default();
        let system = exe.starts_with("/usr/")
            && fs::metadata(&exe).is_ok_and(|meta| meta.uid() == 0 && meta.mode() & 0o022 == 0);
        let args = fs::read(dir.join("cmdline"))
            .map(|bytes| {
                bytes
                    .split(|byte| *byte == 0)
                    .filter(|arg| !arg.is_empty())
                    .map(|arg| String::from_utf8_lossy(arg).into_owned())
                    .collect()
            })
            .unwrap_or_default();
        let cwd = fs::read_link(dir.join("cwd")).ok();
        Some(Process {
            ppid,
            exe,
            name,
            system,
            root,
            args,
            cwd,
        })
    }
}

/// 실행 중에 지워진(갱신된) 실행 파일은 링크 끝에 " (deleted)"가 붙는다
fn strip_deleted(path: PathBuf) -> PathBuf {
    match path
        .to_str()
        .and_then(|text| text.strip_suffix(" (deleted)"))
    {
        Some(text) => PathBuf::from(text),
        None => path,
    }
}

/// 소켓에 붙은 프로세스(pid)를 시킨 AI 클라이언트
pub fn identify(pid: u32) -> Client {
    identify_in(&Procfs, pid)
}

pub(crate) fn identify_in(table: &impl ProcessTable, pid: u32) -> Client {
    let mut admin = false;
    let mut pid = pid;
    for depth in 0..MAX_DEPTH {
        if pid <= 1 {
            break;
        }
        let Some(process) = table.process(pid) else {
            break;
        };
        let skipped = is_wrapper(&process, depth);
        if process.root && (depth == 0 || skipped) {
            admin = true;
        }
        if skipped {
            pid = process.ppid;
            continue;
        }
        if process.exe.as_os_str().is_empty() || is_service_manager(&process) {
            break;
        }
        return client_for(&process, admin);
    }
    Client {
        name: String::new(),
        exe: PathBuf::new(),
        script: None,
        admin,
    }
}

/// 건너뛸 셸·감싼 프로그램인지. depth는 소켓에 붙은 프로세스에서 몇 번 올라왔는지다
fn is_wrapper(process: &Process, depth: usize) -> bool {
    if process.exe.as_os_str().is_empty() {
        // 실행 파일을 읽지 못한 root 프로세스는 이름으로 본다 (사용자 프로세스는 건너뛰지 않는다)
        return process.root && WRAPPERS.contains(&process.name.as_str());
    }
    let name = file_name(&process.exe);
    WRAPPERS.contains(&name) && (process.system || (depth == 0 && name == "bunt"))
}

/// systemd(init, 사용자 서비스 관리자)는 부모가 끝난 프로세스를 입양하므로 클라이언트로 보지 않는다
fn is_service_manager(process: &Process) -> bool {
    process.system && process.exe == Path::new("/usr/lib/systemd/systemd")
}

fn client_for(process: &Process, admin: bool) -> Client {
    let script = if is_interpreter(&process.exe) {
        let options = process.args.get(1..).unwrap_or_default();
        let options: Vec<&str> = options.iter().map(String::as_str).collect();
        script_argument(&process.exe, &options, process.cwd.as_deref())
    } else {
        None
    };
    let desktop = desktop::desktop_app(
        &process.exe,
        script.as_ref().map(|(_, path)| path.as_path()),
    );
    let name = desktop
        .map(|app| app.name)
        .or_else(|| {
            script
                .as_ref()
                .map(|(given, _)| file_name(given).to_owned())
        })
        .or_else(|| {
            process
                .args
                .first()
                .map(|arg0| file_name(Path::new(arg0)).to_owned())
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| file_name(&process.exe).to_owned());
    Client {
        name: clean_name(&name),
        exe: process.exe.clone(),
        script: script.map(|(_, canonical)| canonical),
        admin,
    }
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
}

/// 제어 문자를 빼고 앞뒤 공백을 지우고 길이를 줄인다 (policy::valid_name을 지키는 이름)
fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).collect();
    name.trim()
        .chars()
        .take(MAX_NAME)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// 스크립트를 따로 보는 인터프리터인지 (실행 파일 이름)
pub(crate) fn is_interpreter(exe: &Path) -> bool {
    let name = file_name(exe);
    INTERPRETERS.iter().any(|interpreter| {
        name.strip_prefix(interpreter)
            .is_some_and(|version| version.chars().all(|c| c.is_ascii_digit() || c == '.'))
    })
}

/// 인터프리터 인자에서 스크립트를 찾는다: (준 대로의 경로, 링크를 푼 경로). 옵션이 아닌 첫 인자다.
/// bun과 deno의 `run`은 건너뛴다. 코드나 모듈을 실행하거나(-c, -m, -e) 파일이 없으면 None
pub(crate) fn script_argument(
    interpreter: &Path,
    args: &[&str],
    cwd: Option<&Path>,
) -> Option<(PathBuf, PathBuf)> {
    let subcommands = matches!(file_name(interpreter), "bun" | "deno");
    let mut args = args.iter().copied();
    let script = loop {
        let arg = args.next()?;
        if arg == "--" {
            break args.next()?;
        }
        if CODE_OPTIONS.contains(&arg) || arg == "-" {
            return None;
        }
        if VALUE_OPTIONS.contains(&arg) {
            args.next();
            continue;
        }
        if arg.starts_with('-') || (subcommands && arg == "run") {
            continue;
        }
        break arg;
    };
    let given = PathBuf::from(script);
    let path = if given.is_absolute() {
        given.clone()
    } else {
        cwd?.join(&given)
    };
    let canonical = fs::canonicalize(path).ok()?;
    canonical.is_file().then_some((given, canonical))
}

#[derive(Debug)]
pub enum ProgramError {
    Io(io::Error),
    /// 셸 스크립트, 셸이나 감싼 프로그램: 셸 프로세스는 건너뛰므로 허용해도 맞을 수 없다
    Shell,
    /// 모르는 인터프리터의 스크립트
    Interpreter(PathBuf),
}

impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Shell => write!(f, "shells, shell scripts and wrappers cannot be allowed"),
            Self::Interpreter(path) => {
                write!(f, "scripts for {} are not supported", path.display())
            }
        }
    }
}

impl std::error::Error for ProgramError {}

impl From<io::Error> for ProgramError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// 사용자가 고른 프로그램 파일을 [`identify`]가 알아볼 클라이언트로 바꾼다 (설정의 "다른 프로그램…").
/// 실행 파일이면 그 파일이, `#!` 스크립트면 인터프리터와 스크립트가 된다. 이름은 .desktop 앱 이름이나
/// 고른 파일의 이름(링크를 풀기 전)이다
pub fn program_client(path: &Path) -> Result<Client, ProgramError> {
    let canonical = fs::canonicalize(path)?;
    if !fs::metadata(&canonical)?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file").into());
    }
    let (exe, script) = match interpreter_line(&canonical)? {
        Some(interpreter) => {
            if SHELLS.contains(&file_name(&interpreter)) {
                return Err(ProgramError::Shell);
            }
            if !is_interpreter(&interpreter) {
                return Err(ProgramError::Interpreter(interpreter));
            }
            (interpreter, Some(canonical))
        }
        None => {
            if WRAPPERS.contains(&file_name(&canonical)) {
                return Err(ProgramError::Shell);
            }
            (canonical, None)
        }
    };
    let name = desktop::desktop_app(&exe, script.as_deref())
        .map(|app| app.name)
        .unwrap_or_else(|| file_name(path).to_owned());
    Ok(Client {
        name: clean_name(&name),
        exe,
        script,
        admin: false,
    })
}

/// [`identify`]가 이 (실행 파일, 스크립트)를 낼 수 있는지. 셸과 감싼 프로그램은 건너뛰고, 스크립트는
/// 아는 인터프리터만 따로 보며, `#!` 스크립트 파일 자체는 실행 파일로 보이지 않는다
pub fn can_identify(exe: &Path, script: Option<&Path>) -> bool {
    if WRAPPERS.contains(&file_name(exe)) {
        return false;
    }
    match script {
        Some(_) => is_interpreter(exe),
        None => !is_script(exe),
    }
}

/// `#!`로 시작하는 파일인지
fn is_script(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 2];
    fs::File::open(path).is_ok_and(|mut file| file.read_exact(&mut head).is_ok() && &head == b"#!")
}

/// `#!` 줄의 인터프리터 (링크를 푼 경로). `#!/usr/bin/env node`는 PATH에서 찾는다
fn interpreter_line(path: &Path) -> Result<Option<PathBuf>, ProgramError> {
    let mut head = [0u8; 256];
    let read = {
        use std::io::Read;
        let mut file = fs::File::open(path)?;
        file.read(&mut head)?
    };
    let Some(line) = head[..read].strip_prefix(b"#!") else {
        return Ok(None);
    };
    let line = line.split(|byte| *byte == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(line);
    let mut words = line.split_whitespace();
    let not_found = || io::Error::new(io::ErrorKind::NotFound, "interpreter not found");
    let first = words.next().ok_or_else(not_found)?;
    let program = if file_name(Path::new(first)) == "env" {
        // env -S node --flag, env VAR=1 node
        let name = words
            .find(|word| !word.starts_with('-') && !word.contains('='))
            .ok_or_else(not_found)?;
        desktop::find_program(name)
    } else {
        desktop::find_program(first)
    };
    program.map(Some).ok_or_else(|| not_found().into())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::policy::tests::temp_dir;

    #[derive(Default)]
    struct Table(HashMap<u32, Process>);

    impl Table {
        /// 시스템 프로그램(/usr 아래 root 소유)이 아닌 프로세스는 system을 끈다
        fn add(&mut self, pid: u32, ppid: u32, exe: &str, args: &[&str], root: bool) -> &mut Self {
            self.0.insert(
                pid,
                Process {
                    ppid,
                    exe: exe.into(),
                    name: file_name(Path::new(exe)).chars().take(15).collect(),
                    system: exe.starts_with("/usr/"),
                    root,
                    args: args.iter().map(|arg| arg.to_string()).collect(),
                    cwd: Some("/".into()),
                },
            );
            self
        }

        /// 실행 파일을 읽지 못하는 프로세스 (사용자 데몬이 본 root 프로세스, 숨긴 사용자 프로세스)
        fn add_hidden(&mut self, pid: u32, ppid: u32, name: &str, root: bool) -> &mut Self {
            self.0.insert(
                pid,
                Process {
                    ppid,
                    name: name.into(),
                    root,
                    ..Process::default()
                },
            );
            self
        }
    }

    impl ProcessTable for Table {
        fn process(&self, pid: u32) -> Option<Process> {
            self.0.get(&pid).cloned()
        }
    }

    #[test]
    fn skips_the_cli_shells_and_wrappers() {
        let mut table = Table::default();
        table
            .add(100, 1, "/usr/bin/foot", &["foot"], false)
            .add(110, 100, "/opt/ai/agent", &["agent", "--work"], false)
            .add(120, 110, "/usr/bin/bash", &["bash", "-c", "…"], false)
            .add(
                130,
                120,
                "/usr/bin/timeout",
                &["timeout", "5", "env"],
                false,
            )
            .add(140, 130, "/usr/bin/env", &["env", "A=1", "bunt"], false)
            .add(
                150,
                140,
                "/usr/bin/bunt",
                &["bunt", "agent", "snapshot"],
                false,
            );
        let client = identify_in(&table, 150);
        assert_eq!(client.exe, Path::new("/opt/ai/agent"));
        assert_eq!(client.name, "agent");
        assert_eq!(client.script, None);
        assert!(!client.admin);
    }

    #[test]
    fn a_renamed_copy_outside_usr_is_not_skipped() {
        let mut table = Table::default();
        table
            .add(100, 1, "/usr/bin/foot", &["foot"], false)
            // 사용자가 복사한 bash는 건너뛰지 않으므로 이것이 클라이언트다
            .add(110, 100, "/home/u/bin/bash", &["bash"], false)
            .add(120, 110, "/usr/bin/bunt", &["bunt"], false);
        assert_eq!(identify_in(&table, 120).exe, Path::new("/home/u/bin/bash"));
        // 소켓에 붙은 개발 중 bunt는 어디에 있어도 건너뛴다
        let mut table = Table::default();
        table.add(100, 1, "/usr/bin/foot", &["foot"], false).add(
            110,
            100,
            "/home/u/buntOS/target/release/bunt",
            &["bunt"],
            false,
        );
        assert_eq!(identify_in(&table, 110).exe, Path::new("/usr/bin/foot"));
    }

    #[test]
    fn root_wrappers_make_an_admin_client() {
        // 직렬 콘솔의 root가 sudo -u buntos bunt … 를 실행했다
        let mut table = Table::default();
        table
            .add(10, 1, "/usr/bin/login", &["login"], true)
            .add(20, 10, "/usr/bin/bash", &["-bash"], true)
            .add(
                30,
                20,
                "/usr/bin/sudo",
                &["sudo", "-u", "buntos", "bunt"],
                true,
            )
            .add(40, 30, "/usr/bin/bunt", &["bunt", "agent", "apps"], false);
        assert!(identify_in(&table, 40).admin);
        // root로 도는 bunt
        let mut table = Table::default();
        table.add(40, 1, "/usr/bin/bunt", &["bunt"], true);
        assert!(identify_in(&table, 40).admin);
    }

    #[test]
    fn root_wrappers_count_even_when_their_exe_is_unreadable() {
        // 사용자 데몬은 root 프로세스의 실행 파일을 읽지 못한다. 직렬 콘솔의 root가
        // sudo -u buntos bunt … 를 실행했다 (sudo는 감시 프로세스와 실행 프로세스 둘이다)
        let mut table = Table::default();
        table
            .add_hidden(448, 1, "login", true)
            .add_hidden(516, 448, "bash", true)
            .add_hidden(8244, 516, "sudo", true)
            .add_hidden(8246, 8244, "sudo", true)
            .add(
                8250,
                8246,
                "/usr/bin/bunt",
                &["bunt", "agent", "apps"],
                false,
            );
        assert!(identify_in(&table, 8250).admin);
        // root로 도는 bunt 자신
        let mut table = Table::default();
        table.add_hidden(40, 1, "bunt", true);
        assert!(identify_in(&table, 40).admin);
    }

    #[test]
    fn unreadable_programs_are_unknown() {
        // 콘솔 로그인: 실행 파일을 읽지 못하는 login(root) 위로는 관리자가 아니다
        let mut table = Table::default();
        table
            .add_hidden(10, 1, "login", true)
            .add(20, 10, "/usr/bin/bash", &["-bash"], false)
            .add(30, 20, "/usr/bin/bunt", &["bunt"], false);
        let client = identify_in(&table, 30);
        assert!(client.is_unknown());
        assert!(!client.admin);
        // 실행 파일을 숨기고 이름을 bash로 바꾼 사용자 프로세스는 건너뛰지 않는다
        let mut table = Table::default();
        table
            .add_hidden(10, 1, "sudo", true)
            .add_hidden(20, 10, "bash", false)
            .add(30, 20, "/usr/bin/bunt", &["bunt"], false);
        let client = identify_in(&table, 30);
        assert!(client.is_unknown());
        assert!(!client.admin);
    }

    #[test]
    fn a_root_program_above_a_user_shell_is_not_admin() {
        // 콘솔 로그인: login(root) → 사용자 셸 → bunt. 셸로 exec한 AI가 root 조상을 빌리지 못한다
        let mut table = Table::default();
        table
            .add(10, 1, "/usr/bin/login", &["login"], true)
            .add(20, 10, "/usr/bin/bash", &["-bash"], false)
            .add(30, 20, "/usr/bin/bunt", &["bunt"], false);
        let client = identify_in(&table, 30);
        assert!(!client.admin);
        assert_eq!(client.exe, Path::new("/usr/bin/login"));
    }

    #[test]
    fn orphans_adopted_by_init_are_unknown() {
        let mut table = Table::default();
        table.add(50, 1, "/usr/bin/bunt", &["bunt"], false);
        let client = identify_in(&table, 50);
        assert!(client.is_unknown());
        assert!(!client.admin);
        // 사용자 서비스 관리자에 입양된 경우
        let mut table = Table::default();
        table
            .add(
                5,
                1,
                "/usr/lib/systemd/systemd",
                &["/usr/lib/systemd/systemd", "--user"],
                false,
            )
            .add(50, 5, "/usr/bin/bunt", &["bunt"], false);
        assert!(identify_in(&table, 50).is_unknown());
        // 끝난 프로세스
        assert!(identify_in(&Table::default(), 77).is_unknown());
    }

    #[test]
    fn interpreters_are_identified_by_their_script() {
        let dir = temp_dir("interpreter");
        let script = dir.join("cli.js");
        fs::write(&script, "console.log(1)").unwrap();
        let link = dir.join("claude");
        std::os::unix::fs::symlink(&script, &link).unwrap();
        let canonical = fs::canonicalize(&script).unwrap();

        let mut table = Table::default();
        table
            .add(100, 1, "/usr/bin/foot", &["foot"], false)
            .add(
                110,
                100,
                "/usr/bin/node",
                &[
                    "node",
                    "--no-warnings",
                    "-r",
                    "dotenv/config",
                    link.to_str().unwrap(),
                ],
                false,
            )
            .add(120, 110, "/usr/bin/bunt", &["bunt"], false);
        let client = identify_in(&table, 120);
        assert_eq!(client.exe, Path::new("/usr/bin/node"));
        assert_eq!(client.script.as_deref(), Some(canonical.as_path()));
        // 이름은 실행한 대로의 스크립트 이름이다 (링크를 풀기 전)
        assert_eq!(client.name, "claude");

        // 상대 경로는 프로세스의 작업 디렉터리에서 찾는다
        let found = script_argument(
            Path::new("/usr/bin/python3.13"),
            &["-u", "cli.js"],
            Some(&dir),
        );
        assert_eq!(found.map(|(_, path)| path), Some(canonical.clone()));
        // 코드를 실행하면 스크립트가 없다
        assert_eq!(
            script_argument(Path::new("/usr/bin/python3"), &["-c", "cli.js"], Some(&dir)),
            None
        );
        assert_eq!(
            script_argument(Path::new("/usr/bin/node"), &["-e", "1"], Some(&dir)),
            None
        );
        let bun = script_argument(Path::new("/usr/bin/bun"), &["run", "cli.js"], Some(&dir));
        assert_eq!(bun.map(|(_, path)| path), Some(canonical));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn knows_interpreter_names() {
        assert!(is_interpreter(Path::new("/usr/bin/node")));
        assert!(is_interpreter(Path::new("/usr/bin/python3.13")));
        assert!(is_interpreter(Path::new("/usr/bin/python3")));
        assert!(is_interpreter(Path::new("/usr/bin/lua5.4")));
        assert!(!is_interpreter(Path::new("/usr/bin/nodemon")));
        assert!(!is_interpreter(Path::new("/usr/bin/python-config")));
        assert!(!is_interpreter(Path::new("/usr/bin/foot")));
    }

    #[test]
    fn programs_picked_in_settings_match_what_identify_reports() {
        let dir = temp_dir("program");
        let sh = fs::canonicalize("/bin/sh").unwrap();
        // ELF 실행 파일은 그 자체다 (시험 실행 파일)
        let me = std::env::current_exe().unwrap();
        let client = program_client(&me).unwrap();
        assert_eq!(client.exe, fs::canonicalize(&me).unwrap());
        assert_eq!(client.script, None);
        // 셸과 셸 스크립트는 허용할 수 없다
        assert!(matches!(program_client(&sh), Err(ProgramError::Shell)));
        let shell_script = dir.join("run.sh");
        fs::write(&shell_script, "#!/bin/sh\necho hi\n").unwrap();
        assert!(matches!(
            program_client(&shell_script),
            Err(ProgramError::Shell)
        ));
        // env로 찾는 인터프리터: PATH의 sh를 python처럼 쓸 수는 없으므로 모르는 인터프리터를 본다
        let unknown = dir.join("tool");
        fs::write(&unknown, "#!/usr/bin/env -S true --flag\n").unwrap();
        assert!(matches!(
            program_client(&unknown),
            Err(ProgramError::Interpreter(_))
        ));
        assert!(program_client(&dir.join("missing")).is_err());
        assert!(program_client(&dir).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn knows_which_clients_identify_can_report() {
        let dir = temp_dir("can-identify");
        let script = dir.join("cli.js");
        fs::write(&script, "#!/usr/bin/env node\n").unwrap();
        let me = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert!(can_identify(&me, None));
        assert!(can_identify(Path::new("/usr/bin/node"), Some(&script)));
        assert!(!can_identify(Path::new("/usr/bin/bash"), None));
        assert!(!can_identify(Path::new("/usr/bin/bash"), Some(&script)));
        // 모르는 인터프리터의 스크립트, 실행 파일 자리에 둔 스크립트
        assert!(!can_identify(&me, Some(&script)));
        assert!(!can_identify(&script, None));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn identifies_this_test_process() {
        // cargo가 띄운 시험 실행 파일 자신이 첫 진짜 프로그램이다
        let client = identify(std::process::id());
        assert_eq!(
            client.exe,
            fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
        );
        assert!(!client.name.is_empty());
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("  a\u{7}b  "), "ab");
        assert_eq!(clean_name(&"가".repeat(100)).chars().count(), 64);
    }
}
