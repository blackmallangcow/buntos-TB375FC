// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Flatpak. flatpak 명령을 실행한다. 관리자(sudo 그룹)는 시스템 설치본에 넣는다: 앱 스토어와 같은 곳이고, flatpak의
//! polkit 규칙이 지금 쓰는 자리의 관리자에게는 암호를 묻지 않는다. 표준 사용자는 자기 설치본(--user)에 넣는다

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use gtk4::{gio, glib, prelude::*};

use crate::package::Kind;

/// 이미지가 넣는 Flathub 저장소 파일 (image/mkosi.postinst.chroot)
const FLATHUB_REPO: &str = "/usr/share/buntos/flathub.flatpakrepo";
const FLATHUB: &str = "flathub";

/// 어느 설치본에 넣는가
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    System,
    User,
}

impl Scope {
    fn flag(self) -> &'static str {
        match self {
            Self::System => "--system",
            Self::User => "--user",
        }
    }

    /// 앱의 .desktop 파일이 내보내지는 곳
    pub fn exports(self) -> PathBuf {
        match self {
            Self::System => PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            Self::User => glib::user_data_dir().join("flatpak/exports/share/applications"),
        }
    }
}

/// Debian에서 polkit의 관리자는 sudo 그룹이다 (ARCHITECTURE 6.5)
pub fn install_scope() -> Scope {
    let admin = Command::new("id")
        .arg("-Gn")
        .stdin(Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|group| group == "sudo")
        })
        .unwrap_or(false);
    if admin { Scope::System } else { Scope::User }
}

/// 진행 상황 글에서 백분율을 찾는다 ("Installing 1/2… 45%")
pub fn percent_in(line: &str) -> Option<u32> {
    let end = line.find('%')?;
    let start = line[..end]
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |index| index + 1);
    line[start..end]
        .parse()
        .ok()
        .filter(|percent| *percent <= 100)
}

/// flatpak을 실행하고 출력을 줄마다 넘긴다. 실패하면 마지막 오류 줄을 돌려준다
async fn run(arguments: Vec<OsString>, on_line: impl Fn(&str)) -> Result<(), String> {
    let mut argv = vec![OsString::from("flatpak")];
    argv.extend(arguments);
    let argv: Vec<&OsStr> = argv.iter().map(OsString::as_os_str).collect();
    let process = gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE,
    )
    .map_err(|err| err.to_string())?;
    let stdout = process.stdout_pipe().ok_or("no stdout")?;
    let lines = gio::DataInputStream::new(&stdout);
    lines.set_newline_type(gio::DataStreamNewlineType::Any);
    let mut last = String::new();
    while let Ok(Some(line)) = lines.read_line_utf8_future(glib::Priority::DEFAULT).await {
        let line = line.trim();
        if !line.is_empty() {
            on_line(line);
            last = line.to_owned();
        }
    }
    process.wait_check_future().await.map_err(|err| {
        if last.is_empty() {
            err.to_string()
        } else {
            error_text(&last).to_owned()
        }
    })
}

/// flatpak이 마지막에 찍은 오류 줄에서 머리("error: ")를 뺀다
fn error_text(line: &str) -> &str {
    line.strip_prefix("error: ").unwrap_or(line)
}

/// 관리자 인증 창을 닫았을 때의 오류인가. 시스템 도우미가 내는 글이라 사용자의 언어와 상관없이 이 글이다
pub fn is_not_authorized(message: &str) -> bool {
    message.contains("not allowed for user")
}

/// 사용자 설치본에 Flathub를 더한다. 시스템 설치본의 Flathub는 사용자 설치본에서 쓰지 못하는데, 번들의 런타임을
/// 받으려면 필요하다
async fn ensure_user_flathub() {
    let _ = run(
        vec![
            "remote-add".into(),
            "--user".into(),
            "--if-not-exists".into(),
            FLATHUB.into(),
            FLATHUB_REPO.into(),
        ],
        |_| {},
    )
    .await;
}

fn install_arguments(kind: Kind, path: &Path, scope: Scope, reinstall: bool) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = vec!["install".into(), scope.flag().into(), "-y".into()];
    // 이미 있는 앱은 --reinstall이 없으면 건너뛴다
    if reinstall {
        arguments.push("--reinstall".into());
    }
    arguments.push(
        if kind == Kind::FlatpakBundle {
            "--bundle"
        } else {
            "--from"
        }
        .into(),
    );
    arguments.push(path.into());
    arguments
}

/// 앱 파일(.flatpak, .flatpakref)을 설치하고, 넣은 설치본을 돌려준다. `-y`는 인증 창을 띄우지 못하는데, 번들과 새 저장소의
/// .flatpakref는 시스템 설치본에 원격을 더해야 해서(polkit `configure-remote`) 거절된다. 그때는 인증이 필요 없는 사용자
/// 설치본에 넣는다
pub async fn install_app(
    kind: Kind,
    path: &Path,
    scope: Scope,
    reinstall: bool,
    on_line: impl Fn(&str),
) -> Result<Scope, String> {
    if scope == Scope::User {
        ensure_user_flathub().await;
    }
    let first = run(install_arguments(kind, path, scope, reinstall), &on_line).await;
    match (first, scope) {
        (Ok(()), _) => Ok(scope),
        (Err(err), Scope::User) => Err(err),
        (Err(_), Scope::System) => {
            ensure_user_flathub().await;
            run(
                install_arguments(kind, path, Scope::User, reinstall),
                &on_line,
            )
            .await
            .map(|()| Scope::User)
        }
    }
}

/// 저장소 파일(.flatpakrepo)의 저장소를 더한다. 시스템 설치본이면 관리자 인증 창이 뜬다
pub async fn add_remote(path: &Path, name: &str, scope: Scope) -> Result<(), String> {
    run(
        vec![
            "remote-add".into(),
            scope.flag().into(),
            "--if-not-exists".into(),
            name.into(),
            path.into(),
        ],
        |_| {},
    )
    .await
}

/// 앱을 지우고, 그 앱만 쓰던 런타임도 지운다 (런타임은 앱보다 훨씬 크다). 앱의 자료(~/.var/app)는 macOS처럼 남긴다
pub async fn uninstall(id: &str, scope: Scope) -> Result<(), String> {
    run(
        vec![
            "uninstall".into(),
            scope.flag().into(),
            "-y".into(),
            id.into(),
        ],
        |_| {},
    )
    .await?;
    let _ = run(
        vec![
            "uninstall".into(),
            scope.flag().into(),
            "-y".into(),
            "--unused".into(),
        ],
        |_| {},
    )
    .await;
    Ok(())
}

/// 설치된 Flatpak 앱 하나
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub version: String,
    pub size: String,
    pub scope: Scope,
    pub origin: String,
}

/// flatpak list --columns=application,name,version,size,installation,origin 의 출력
pub fn parse_list(text: &str) -> Vec<App> {
    text.lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            let id = columns.next()?.trim();
            let name = columns.next().unwrap_or_default().trim();
            let version = columns.next().unwrap_or_default().trim();
            let size = columns.next().unwrap_or_default().trim();
            let installation = columns.next().unwrap_or_default().trim();
            let origin = columns.next().unwrap_or_default().trim();
            (!id.is_empty()).then(|| App {
                id: id.to_owned(),
                name: if name.is_empty() { id } else { name }.to_owned(),
                version: version.to_owned(),
                size: size.to_owned(),
                scope: if installation == "user" {
                    Scope::User
                } else {
                    Scope::System
                },
                origin: origin.to_owned(),
            })
        })
        .collect()
}

/// 설치된 앱 목록. flatpak이 없으면 비어 있다. 주 스레드에서 부르지 않는다
pub fn list_apps() -> Vec<App> {
    Command::new("flatpak")
        .args([
            "list",
            "--app",
            "--columns=application,name,version,size,installation,origin",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| parse_list(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

/// 저장소 이름이 이미 있는지 (.flatpakrepo의 "이미 설치됨")
pub fn has_remote(name: &str) -> bool {
    Command::new("flatpak")
        .args(["remotes", "--columns=name"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.trim() == name)
        })
        .unwrap_or(false)
}

/// 앱의 .desktop 파일. 내보낸 항목이 없으면(런타임, 창 없는 앱) None
pub fn desktop_file(id: &str, scope: Scope) -> Option<PathBuf> {
    let path = scope.exports().join(format!("{id}.desktop"));
    path.exists().then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_percent_from_progress_lines() {
        assert_eq!(percent_in("Installing 1/2… 45%  1.2 MB/s"), Some(45));
        assert_eq!(percent_in("100%"), Some(100));
        assert_eq!(percent_in("Installing app"), None);
        assert_eq!(percent_in("no digits %"), None);
        assert_eq!(percent_in("1200%"), None);
    }

    #[test]
    fn an_error_line_tells_a_refused_authorization() {
        let message =
            error_text("error: Flatpak system operation ConfigureRemote not allowed for user");
        assert_eq!(
            message,
            "Flatpak system operation ConfigureRemote not allowed for user"
        );
        assert!(is_not_authorized(message));
        assert!(!is_not_authorized(error_text(
            "error: Can't fetch summary from https://example.invalid/repo"
        )));
        assert_eq!(error_text("no prefix"), "no prefix");
    }

    #[test]
    fn reads_the_installed_list() {
        let apps = parse_list(
            "org.gnome.Calculator\tCalculator\t49.1\t12.3 MB\tsystem\tflathub\ncom.example.App\t\t\t1 GB\tuser\tother\n\n",
        );
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Calculator");
        assert_eq!(apps[0].scope, Scope::System);
        assert_eq!(apps[0].origin, "flathub");
        // 이름이 없는 앱은 ID를 이름으로 쓴다
        assert_eq!(apps[1].name, "com.example.App");
        assert_eq!(apps[1].scope, Scope::User);
    }
}
