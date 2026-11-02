// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 사용자가 설치한 것의 목록과 지우기. 이미지에 든 패키지(/usr/share/buntos/base-packages)와 다른 패키지가 필요해서
//! 들어온 패키지(apt의 자동 설치 표시)는 보이지 않는다: 시스템의 일부를 지우지 않게 하고, 의존성은 패키지 관리자의 일로
//! 둔다.
//!
//! 이 파일의 목록 함수는 명령을 실행하고 파일을 읽으므로 주 스레드에서 부르지 않는다

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
};

use crate::{
    appimage, flatpak,
    package::DesktopEntry,
    packagekit::{self, FLAG_SIMULATE, Failure, Plan},
};

const BASE_PACKAGES: &str = "/usr/share/buntos/base-packages";

/// 어디서 왔는가. 지우는 방법이 다르다
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Deb { name: String },
    Flatpak { id: String, scope: flatpak::Scope },
    AppImage(appimage::Installed),
}

/// 목록의 항목 하나
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub source: Source,
    pub title: String,
    pub version: String,
    /// 보이는 크기 ("12.3 MB")
    pub size: Option<String>,
    pub summary: Option<String>,
    /// 앱이면 .desktop 파일 (이름, 아이콘, 실행)
    pub desktop: Option<PathBuf>,
}

// Debian 패키지

/// dpkg-query의 한 줄
#[derive(Debug, Clone, PartialEq)]
pub struct DpkgLine {
    pub name: String,
    pub version: String,
    /// KiB
    pub installed_size: Option<u64>,
    pub summary: String,
}

/// dpkg-query --show의 "이름\t판\t크기\t상태\t요약" 줄에서 설치된 것만 읽는다
pub fn parse_dpkg_list(text: &str) -> Vec<DpkgLine> {
    text.lines()
        .filter_map(|line| {
            let mut columns = line.splitn(5, '\t');
            let name = columns.next()?;
            let version = columns.next()?;
            let size = columns.next()?;
            let status = columns.next()?;
            let summary = columns.next().unwrap_or_default();
            (status == "installed").then(|| DpkgLine {
                name: name.to_owned(),
                version: version.to_owned(),
                installed_size: size.parse().ok(),
                summary: summary.to_owned(),
            })
        })
        .collect()
}

/// 사용자가 고른 패키지: 이미지에 없고 자동으로 들어오지 않은 것. 이름에 붙은 :arch는 떼고 비교한다
pub fn user_packages(
    all: Vec<DpkgLine>,
    base: &HashSet<String>,
    automatic: &HashSet<String>,
) -> Vec<DpkgLine> {
    all.into_iter()
        .filter(|line| {
            let name = plain_name(&line.name);
            !base.contains(name) && !automatic.contains(name)
        })
        .collect()
}

fn plain_name(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

fn lines_of(text: &str) -> HashSet<String> {
    text.lines()
        .map(|line| plain_name(line.trim()).to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// 이미지에 든 패키지 이름. 파일이 없으면(개발 중의 다른 시스템) None
pub fn base_packages() -> Option<HashSet<String>> {
    fs::read_to_string(BASE_PACKAGES)
        .ok()
        .map(|text| lines_of(&text))
}

/// apt가 다른 패키지 때문에 넣었다고 표시한 패키지 (/var/lib/apt/extended_states). apt-mark showauto와 같은 내용이고,
/// apt 캐시를 읽지 않아 훨씬 빠르다
pub fn parse_extended_states(text: &str) -> HashSet<String> {
    text.split("\n\n")
        .filter_map(|stanza| {
            let mut name = None;
            let mut automatic = false;
            for line in stanza.lines() {
                if let Some(value) = line.strip_prefix("Package:") {
                    name = Some(value.trim());
                } else if let Some(value) = line.strip_prefix("Auto-Installed:") {
                    automatic = value.trim() == "1";
                }
            }
            name.filter(|_| automatic)
                .map(|name| plain_name(name).to_owned())
        })
        .collect()
}

fn automatic_packages() -> HashSet<String> {
    fs::read_to_string("/var/lib/apt/extended_states")
        .map(|text| parse_extended_states(&text))
        .unwrap_or_default()
}

/// 패키지 이름을 늘어놓는다. 많으면 앞의 몇 개만 보인다 (수는 문구가 따로 알린다)
pub fn names_preview(names: &[String]) -> String {
    const SHOWN: usize = 6;
    if names.len() <= SHOWN {
        names.join(", ")
    } else {
        format!("{}, …", names[..SHOWN].join(", "))
    }
}

/// dpkg가 기록한 패키지의 파일 목록. 이름에 :arch가 붙는 패키지도 있다
fn package_files(name: &str) -> Vec<String> {
    let dir = Path::new("/var/lib/dpkg/info");
    let arch_list = dir.join(format!("{name}:{}.list", crate::package::HOST_ARCH));
    let plain_list = dir.join(format!("{name}.list"));
    fs::read_to_string(&plain_list)
        .or_else(|_| fs::read_to_string(&arch_list))
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// 패키지의 파일 가운데 메뉴에 보이는 앱의 .desktop을 찾는다
pub fn desktop_in_package(name: &str) -> Option<PathBuf> {
    package_files(name)
        .into_iter()
        .filter(|path| path.starts_with("/usr/share/applications/") && path.ends_with(".desktop"))
        .map(PathBuf::from)
        .find(|path| {
            fs::read(path)
                .ok()
                .and_then(|data| DesktopEntry::parse(&data))
                .is_some_and(|entry| !entry.hidden)
        })
}

fn list_debs() -> Vec<Item> {
    // 이미지 목록이 없으면 무엇이 시스템의 일부인지 모르므로 하나도 보이지 않는다
    let Some(base) = base_packages() else {
        return Vec::new();
    };
    let output = Command::new("dpkg-query")
        .args([
            "--show",
            "--showformat=${binary:Package}\t${Version}\t${Installed-Size}\t${db:Status-Status}\t${binary:Summary}\n",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    let all = parse_dpkg_list(&String::from_utf8_lossy(&output.stdout));
    user_packages(all, &base, &automatic_packages())
        .into_iter()
        .map(|line| {
            let name = plain_name(&line.name).to_owned();
            let desktop = desktop_in_package(&name);
            let entry = desktop
                .as_ref()
                .and_then(|path| fs::read(path).ok())
                .and_then(|data| DesktopEntry::parse(&data));
            Item {
                title: entry
                    .and_then(|entry| entry.name)
                    .unwrap_or_else(|| name.clone()),
                source: Source::Deb { name },
                version: line.version,
                size: line.installed_size.map(|kib| format_size(kib * 1024)),
                summary: (!line.summary.is_empty()).then_some(line.summary),
                desktop,
            }
        })
        .collect()
}

fn list_flatpaks() -> Vec<Item> {
    flatpak::list_apps()
        .into_iter()
        .map(|app| Item {
            desktop: flatpak::desktop_file(&app.id, app.scope),
            source: Source::Flatpak {
                id: app.id,
                scope: app.scope,
            },
            title: app.name,
            version: app.version,
            size: (!app.size.is_empty()).then_some(app.size),
            summary: None,
        })
        .collect()
}

fn list_appimages() -> Vec<Item> {
    appimage::list_installed()
        .into_iter()
        .map(|installed| {
            let entry = fs::read(&installed.desktop)
                .ok()
                .and_then(|data| DesktopEntry::parse(&data))
                .unwrap_or_default();
            let title = entry.name.clone().unwrap_or_else(|| {
                installed
                    .dir
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            Item {
                desktop: Some(installed.desktop.clone()),
                version: installed.version.clone().unwrap_or_default(),
                size: directory_size(&installed.dir).map(format_size),
                summary: entry.comment,
                title,
                source: Source::AppImage(installed),
            }
        })
        .collect()
}

/// 디렉터리의 파일 크기 합. 링크는 따라가지 않는다
fn directory_size(dir: &Path) -> Option<u64> {
    let mut total = 0;
    let mut pending = vec![dir.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).ok()?.filter_map(Result::ok) {
            let Ok(metadata) = entry.path().symlink_metadata() else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    Some(total)
}

pub fn format_size(bytes: u64) -> String {
    gtk4::glib::format_size(bytes).to_string()
}

/// 설치한 것을 이름 차례로 모은다
pub fn list() -> Vec<Item> {
    let mut items = list_debs();
    items.extend(list_flatpaks());
    items.extend(list_appimages());
    items.sort_by_cached_key(|item| item.title.to_lowercase());
    items
}

// 이미 설치되었는지

/// 받은 파일과 설치된 판의 관계
#[derive(Debug, Clone, PartialEq)]
pub enum Existing {
    None,
    Same,
    /// 설치된 판이 더 오래되었다
    Older(String),
    /// 설치된 판이 더 새롭다
    Newer(String),
}

/// dpkg가 판을 견준다 (Debian 판 번호 규칙)
fn dpkg_version_less(left: &str, right: &str) -> bool {
    Command::new("dpkg")
        .args(["--compare-versions", left, "lt", right])
        .stdin(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn deb_state(name: &str, version: Option<&str>) -> Existing {
    let output = Command::new("dpkg-query")
        .args([
            "--show",
            "--showformat=${db:Status-Status}\t${Version}",
            name,
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(output) = output else {
        return Existing::None;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let Some((status, installed)) = text.split_once('\t') else {
        return Existing::None;
    };
    if status != "installed" {
        return Existing::None;
    }
    match version {
        Some(version) if version == installed => Existing::Same,
        Some(version) if dpkg_version_less(installed, version) => {
            Existing::Older(installed.to_owned())
        }
        Some(_) => Existing::Newer(installed.to_owned()),
        None => Existing::Same,
    }
}

// 지우기

/// 지우기 전에 사람에게 보일 것
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RemovalPlan {
    /// 이 항목 말고도 함께 지워지는 패키지
    pub also: Vec<String>,
    /// 시스템의 일부가 함께 지워지므로 지우지 않는다
    pub blocked: Vec<String>,
    /// 자동으로 들어온 의존성도 지운다
    pub autoremove: bool,
}

/// 패키지를 지우면 무엇이 함께 지워지는지 본다. 이것 때문에 들어온 의존성을 함께 지우되, 이미지의 패키지가 끼면 빼고
/// 다시 본다. 그래도 끼면 지우지 않는다
pub async fn plan_deb_removal(name: &str) -> Result<RemovalPlan, Failure> {
    let ids = packagekit::resolve_installed(&[name.to_owned()]).await?;
    let base = base_packages().unwrap_or_default();
    let simulate = |autoremove: bool| {
        let ids = ids.clone();
        async move {
            let lines =
                packagekit::remove_packages(FLAG_SIMULATE, ids, true, autoremove, Rc::new(|_| {}))
                    .await?;
            Ok::<Plan, Failure>(packagekit::plan(&lines))
        }
    };
    let plan = simulate(true).await?;
    if !plan.remove.iter().any(|name| base.contains(name)) {
        return Ok(removal_plan(plan, name, &base, true));
    }
    let plan = simulate(false).await?;
    Ok(removal_plan(plan, name, &base, false))
}

fn removal_plan(plan: Plan, name: &str, base: &HashSet<String>, autoremove: bool) -> RemovalPlan {
    let blocked = plan
        .remove
        .iter()
        .filter(|other| base.contains(*other))
        .cloned()
        .collect();
    let also = plan
        .remove
        .into_iter()
        .filter(|other| other != name)
        .collect();
    RemovalPlan {
        also,
        blocked,
        autoremove,
    }
}

/// 지운다. 앱 스토어처럼 관리자 인증은 패키지만 묻는다
pub async fn remove(source: &Source, plan: &RemovalPlan) -> Result<(), RemoveError> {
    match source {
        Source::Deb { name } => {
            let ids = packagekit::resolve_installed(std::slice::from_ref(name))
                .await
                .map_err(RemoveError::PackageKit)?;
            packagekit::remove_packages(0, ids, true, plan.autoremove, Rc::new(|_| {}))
                .await
                .map(|_| ())
                .map_err(RemoveError::PackageKit)
        }
        Source::Flatpak { id, scope } => flatpak::uninstall(id, *scope)
            .await
            .map_err(RemoveError::Other),
        Source::AppImage(installed) => {
            appimage::uninstall(installed).map_err(|err| RemoveError::Other(err.to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoveError {
    PackageKit(Failure),
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn reads_installed_packages_only() {
        let lines = parse_dpkg_list(
            "code\t1.90\t400000\tinstalled\tEditor\nold\t1\t10\tconfig-files\tGone\nbad line\n",
        );
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].name, "code");
        assert_eq!(lines[0].installed_size, Some(400_000));
        assert_eq!(lines[0].summary, "Editor");
    }

    #[test]
    fn the_image_and_dependencies_are_not_listed() {
        let line = |name: &str| DpkgLine {
            name: name.to_owned(),
            version: "1".to_owned(),
            installed_size: None,
            summary: String::new(),
        };
        let kept = user_packages(
            vec![
                line("nautilus"),
                line("code"),
                line("libfoo1"),
                line("wine:i386"),
                line("libc6:arm64"),
            ],
            &set(&["nautilus", "libc6"]),
            &set(&["libfoo1"]),
        );
        let names: Vec<_> = kept.iter().map(|line| line.name.as_str()).collect();
        assert_eq!(names, ["code", "wine:i386"]);
    }

    #[test]
    fn reads_automatically_installed_packages() {
        let automatic = parse_extended_states(
            "Package: libfoo1\nArchitecture: arm64\nAuto-Installed: 1\n\nPackage: app\nArchitecture: arm64\nAuto-Installed: 0\n\nPackage: libbar:i386\nAuto-Installed: 1\n",
        );
        assert_eq!(automatic, set(&["libfoo1", "libbar"]));
        assert!(parse_extended_states("").is_empty());
    }

    #[test]
    fn long_name_lists_are_cut() {
        let names: Vec<String> = (1..=3).map(|n| format!("p{n}")).collect();
        assert_eq!(names_preview(&names), "p1, p2, p3");
        let names: Vec<String> = (1..=8).map(|n| format!("p{n}")).collect();
        assert_eq!(names_preview(&names), "p1, p2, p3, p4, p5, p6, …");
    }

    #[test]
    fn a_removal_that_takes_the_system_with_it_is_blocked() {
        let plan = Plan {
            install: Vec::new(),
            update: Vec::new(),
            remove: vec!["app".to_owned(), "libapp".to_owned(), "nautilus".to_owned()],
        };
        let result = removal_plan(plan, "app", &set(&["nautilus"]), false);
        assert_eq!(result.also, ["libapp", "nautilus"]);
        assert_eq!(result.blocked, ["nautilus"]);
    }

    #[test]
    fn counts_directory_sizes_without_following_links() {
        let top = std::env::temp_dir().join(format!("bunt-packages-size-{}", std::process::id()));
        fs::create_dir_all(top.join("a/b")).unwrap();
        fs::write(top.join("a/one"), [0u8; 10]).unwrap();
        fs::write(top.join("a/b/two"), [0u8; 20]).unwrap();
        std::os::unix::fs::symlink("/usr", top.join("a/link")).unwrap();
        let size = directory_size(&top).unwrap();
        // 링크 자체의 크기("/usr" 네 글자)만 더한다
        assert_eq!(size, 34);
        fs::remove_dir_all(&top).unwrap();
    }
}
