// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! buntd-setup: 처음 켤 때 계정을 만드는 시스템 서비스. 시스템 버스의 org.buntos.Setup1을 내고
//! 버스 활성화로 뜨며, 잠시 쓰이지 않으면 끝난다. 정규 계정이 하나도 없을 때만 계정을 만든다.
//! root와 로그인 화면 사용자(_greetd)만 부를 수 있다 (D-Bus 정책과 여기서 한 번 더 본다).
//!
//! `--credential`: 시험·개발용 계정을 systemd 자격 증명 buntos.account에서 같은 규칙으로 만든다
//! (buntos-account.service가 greetd보다 먼저 실행한다. 형식은 account::parse_credential)

use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bunt_greeter::{
    account::{self, Invalid, NewAccount},
    setup,
};
use zbus::{fdo, interface, message::Header, names::BusName};

/// 이만큼 아무도 부르지 않으면 끝난다. 다음 호출이 다시 띄운다
const IDLE_EXIT: Duration = Duration::from_secs(120);
/// 계정을 만든 뒤에는 답을 보낼 틈만 두고 끝난다 (더 만들 일이 없다)
const DONE_EXIT: Duration = Duration::from_secs(3);
/// 계정 만들기는 한 번에 하나다 (자격 증명과 D-Bus 요청이 겹칠 때)
const LOCK_PATH: &str = "/run/buntd-setup.lock";
/// 새 계정이 드는 그룹: 관리자(sudo. Debian polkit도 이 그룹을 관리자로 본다), 화면·GPU·입력 장치
const GROUPS: [&str; 4] = ["sudo", "video", "render", "input"];
const SHELL: &str = "/bin/bash";
const CREDENTIAL: &str = "buntos.account";

/// D-Bus 오류. 이름은 org.buntos.Setup1.Error.<변형 이름>이다 (setup::ERROR_PREFIX,
/// account::Invalid::error_name과 같다)
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.buntos.Setup1.Error")]
enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    InvalidFullName(String),
    InvalidAccountName(String),
    AccountNameTaken(String),
    PasswordTooShort(String),
    InvalidPassword(String),
    AccountExists(String),
    AccessDenied(String),
    Failed(String),
}

impl From<Invalid> for Error {
    fn from(invalid: Invalid) -> Self {
        let message = invalid.to_string();
        match invalid {
            Invalid::FullName => Self::InvalidFullName(message),
            Invalid::AccountName => Self::InvalidAccountName(message),
            Invalid::AccountNameTaken => Self::AccountNameTaken(message),
            Invalid::PasswordTooShort => Self::PasswordTooShort(message),
            Invalid::Password => Self::InvalidPassword(message),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Failed(err.to_string())
    }
}

impl From<fdo::Error> for Error {
    fn from(err: fdo::Error) -> Self {
        Self::ZBus(err.into())
    }
}

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        None => serve(),
        Some("--credential") => from_credential(),
        Some(other) => {
            eprintln!("buntd-setup: unknown argument {other}");
            ExitCode::from(2)
        }
    }
}

#[derive(Default)]
struct Activity {
    last: Option<Instant>,
    busy: u32,
    created: bool,
}

struct Setup {
    activity: Arc<Mutex<Activity>>,
}

#[interface(name = "org.buntos.Setup1")]
impl Setup {
    /// 정규 계정이 없을 때 관리자 계정을 만들고 uid를 돌려준다
    async fn create_account(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
        full_name: &str,
        account_name: &str,
        password: &str,
    ) -> Result<u32, Error> {
        self.update(|activity| activity.busy += 1);
        let result = async {
            let sender = header
                .sender()
                .ok_or_else(|| Error::AccessDenied("no sender".into()))?;
            let caller = fdo::DBusProxy::new(connection)
                .await?
                .get_connection_unix_user(BusName::from(sender.to_owned()))
                .await?;
            if !caller_allowed(caller) {
                eprintln!("buntd-setup: refused CreateAccount from uid {caller}");
                return Err(Error::AccessDenied(
                    "only root and the login window may create the first account".into(),
                ));
            }
            create(&NewAccount {
                account_name: account_name.to_owned(),
                full_name: full_name.to_owned(),
                password: password.to_owned(),
            })
        }
        .await;
        self.update(|activity| {
            activity.busy -= 1;
            activity.created |= result.is_ok();
        });
        result
    }
}

impl Setup {
    fn update(&self, f: impl FnOnce(&mut Activity)) {
        let mut activity = self.activity.lock().unwrap();
        f(&mut activity);
        activity.last = Some(Instant::now());
    }
}

/// root와 로그인 화면 사용자만 처음 계정을 만들 수 있다
fn caller_allowed(uid: u32) -> bool {
    if uid == 0 {
        return true;
    }
    let passwd = fs::read_to_string(account::PASSWD).unwrap_or_default();
    account::parse_passwd(&passwd)
        .iter()
        .any(|user| user.name == setup::GREETER_USER && user.uid == uid)
}

fn serve() -> ExitCode {
    let activity = Arc::new(Mutex::new(Activity {
        last: Some(Instant::now()),
        ..Activity::default()
    }));
    let service = Setup {
        activity: activity.clone(),
    };
    let connection = zbus::blocking::connection::Builder::system()
        .and_then(|builder| builder.name(setup::BUS_NAME))
        .and_then(|builder| builder.serve_at(setup::OBJECT_PATH, service))
        .and_then(|builder| builder.build());
    let _connection = match connection {
        Ok(connection) => connection,
        Err(err) => {
            eprintln!("buntd-setup: failed to provide {}: {err}", setup::BUS_NAME);
            return ExitCode::FAILURE;
        }
    };
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let activity = activity.lock().unwrap();
        let idle = activity.last.map_or(Duration::ZERO, |last| last.elapsed());
        if activity.busy == 0 && (idle >= IDLE_EXIT || (activity.created && idle >= DONE_EXIT)) {
            return ExitCode::SUCCESS;
        }
    }
}

fn from_credential() -> ExitCode {
    let Some(directory) = std::env::var_os("CREDENTIALS_DIRECTORY") else {
        eprintln!(
            "buntd-setup: CREDENTIALS_DIRECTORY is not set (run from buntos-account.service)"
        );
        return ExitCode::FAILURE;
    };
    let path = PathBuf::from(directory).join(CREDENTIAL);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("buntd-setup: {}: {err}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let new = match account::parse_credential(&text) {
        Ok(new) => new,
        Err(invalid) => {
            eprintln!("buntd-setup: {CREDENTIAL}: {invalid}");
            return ExitCode::FAILURE;
        }
    };
    match create(&new) {
        Ok(uid) => {
            eprintln!(
                "buntd-setup: created the account {} (uid {uid}) from the {CREDENTIAL} credential",
                new.account_name
            );
            ExitCode::SUCCESS
        }
        Err(Error::AccountExists(_)) => {
            eprintln!("buntd-setup: a regular account already exists; {CREDENTIAL} is ignored");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("buntd-setup: failed to create the account from {CREDENTIAL}: {err:?}");
            ExitCode::FAILURE
        }
    }
}

/// 계정을 만든다: useradd(홈 폴더, 같은 이름의 그룹, 관리자·장치 그룹, 별칭), chpasswd(암호).
/// AccountsService는 별칭(GECOS)과 계정을 /etc/passwd에서 스스로 읽는다
fn create(new: &NewAccount) -> Result<u32, Error> {
    let lock = fs::File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(LOCK_PATH)?;
    lock.lock()?;

    let passwd = fs::read_to_string(account::PASSWD)?;
    if !account::regular_users(&passwd).is_empty() {
        return Err(Error::AccountExists(
            "a regular account already exists".into(),
        ));
    }
    let full_name = account::check_full_name(&new.full_name)?;
    account::check_account_name(&new.account_name)?;
    account::check_password(&new.password)?;
    let group = fs::read_to_string(account::GROUP)?;
    if account::is_name_taken(&new.account_name, &passwd, &group) {
        return Err(Invalid::AccountNameTaken.into());
    }
    let groups: Vec<&str> = GROUPS
        .into_iter()
        .filter(|name| account::group_exists(name, &group))
        .collect();

    run(
        Command::new("useradd").args([
            "--create-home",
            "--user-group",
            "--shell",
            SHELL,
            "--comment",
            &full_name,
            "--groups",
            &groups.join(","),
            "--",
            &new.account_name,
        ]),
        None,
    )?;
    let input = format!("{}:{}\n", new.account_name, new.password);
    if let Err(err) = run(&mut Command::new("chpasswd"), Some(&input)) {
        // 암호 없는 계정을 남기지 않는다
        let _ = run(
            Command::new("userdel").args(["--remove", "--", &new.account_name]),
            None,
        );
        return Err(err);
    }

    let passwd = fs::read_to_string(account::PASSWD)?;
    let uid = account::parse_passwd(&passwd)
        .into_iter()
        .find(|user| user.name == new.account_name)
        .map(|user| user.uid)
        .ok_or_else(|| Error::Failed("the new account is missing from /etc/passwd".into()))?;
    eprintln!(
        "buntd-setup: created {} (uid {uid}, groups {})",
        new.account_name,
        groups.join(",")
    );
    Ok(uid)
}

/// 계정 도구를 깨끗한 환경에서 실행한다. 입력(암호)은 표준 입력으로만 넘기고 로그에 남기지 않는다
fn run(command: &mut Command, input: Option<&str>) -> Result<(), Error> {
    let program = command.get_program().to_string_lossy().into_owned();
    command
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|err| Error::Failed(format!("{program}: {err}")))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input.as_bytes())?;
    }
    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::Failed(format!(
            "{program}: {}: {}",
            output.status,
            stderr.trim()
        )))
    }
}
