// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설치된 앱(.desktop 파일)을 찾는다. `bunt agent launch`가 이름이나 데스크톱 ID로 앱을 연다

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct DesktopApp {
    /// 데스크톱 ID (.desktop을 뺀 파일 이름, 예: org.gnome.TextEditor)
    pub id: String,
    /// 지금 언어의 이름
    pub name: String,
    /// 번역하지 않은 이름 (Name=, 대개 영어). 에이전트는 영어 이름으로 찾기도 한다
    pub untranslated: String,
    pub path: PathBuf,
    /// DBusActivatable=true: 세션 버스가 앱을 띄우고, 창은 org.freedesktop.Application.Activate로
    /// 연다
    pub dbus: bool,
    /// StartupWMClass: 창의 Wayland app_id가 데스크톱 ID와 다른 앱이 적는다
    pub wm_class: Option<String>,
}

impl DesktopApp {
    /// 이 앱의 창인지: 창의 Wayland app_id가 데스크톱 ID나 StartupWMClass와 같다
    pub fn owns(&self, app_id: &str) -> bool {
        app_id == self.id || self.wm_class.as_deref() == Some(app_id)
    }

    /// 데스크톱 ID나 이름(번역한 것이나 하지 않은 것)이 그 글과 같다 (대소문자 무시)
    pub fn is_called(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        [&self.id, &self.name, &self.untranslated]
            .iter()
            .any(|name| name.to_lowercase() == query)
    }
}

/// 앱 메뉴에 보이는 설치된 앱 ($XDG_DATA_HOME, $XDG_DATA_DIRS의 applications). 앞 디렉터리가
/// 같은 ID를 가린다
pub fn installed(language: &str) -> Vec<DesktopApp> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let dirs = home
        .into_iter()
        .chain(system.split(':').map(PathBuf::from))
        .map(|dir| dir.join("applications"));
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let Some(id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".desktop"))
                .map(str::to_string)
            else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(entry) = shown_entry(&text, language) {
                apps.push(DesktopApp {
                    id,
                    name: entry.name,
                    untranslated: entry.untranslated,
                    path,
                    dbus: entry.dbus,
                    wm_class: entry.wm_class,
                });
            }
        }
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}

/// .desktop 파일의 [Desktop Entry]에서 읽은 것
struct Entry {
    name: String,
    untranslated: String,
    dbus: bool,
    wm_class: Option<String>,
}

/// 앱 메뉴에 보이는 앱이면 그 항목. [Desktop Entry]의 Type=Application이고 NoDisplay·Hidden이
/// 아니며, OnlyShowIn·NotShowIn이 이 데스크톱($XDG_CURRENT_DESKTOP)을 빼지 않는다
fn shown_entry(text: &str, language: &str) -> Option<Entry> {
    let desktops: Vec<String> = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|desktop| !desktop.is_empty())
        .map(str::to_string)
        .collect();
    let listed = |value: &str| {
        value
            .split(';')
            .any(|desktop| desktops.iter().any(|current| current == desktop))
    };
    let mut in_entry = false;
    let (mut name, mut localized, mut application, mut hidden) = (None, None, false, false);
    let (mut dbus, mut wm_class) = (false, None);
    let localized_key = format!("Name[{language}]");
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Type" => application = value == "Application",
            "NoDisplay" | "Hidden" => hidden |= value == "true",
            "OnlyShowIn" => hidden |= !listed(value),
            "NotShowIn" => hidden |= listed(value),
            "DBusActivatable" => dbus = value == "true",
            "StartupWMClass" => wm_class = Some(value.to_string()),
            "Name" => name = Some(value.to_string()),
            _ if key == localized_key => localized = Some(value.to_string()),
            _ => {}
        }
    }
    let untranslated = name?;
    (application && !hidden).then(|| Entry {
        name: localized.unwrap_or_else(|| untranslated.clone()),
        untranslated,
        dbus,
        wm_class,
    })
}

/// 데스크톱 ID나 이름(번역한 것이나 하지 않은 것)이 같은 앱. 없으면 ID나 이름에 그 글이 든 앱 하나
pub fn find<'a>(apps: &'a [DesktopApp], query: &str) -> Result<&'a DesktopApp, String> {
    let exact: Vec<&DesktopApp> = apps.iter().filter(|app| app.is_called(query)).collect();
    let matches = if exact.is_empty() {
        let lower = query.to_lowercase();
        apps.iter()
            .filter(|app| {
                [&app.id, &app.name, &app.untranslated]
                    .iter()
                    .any(|name| name.to_lowercase().contains(&lower))
            })
            .collect()
    } else {
        exact
    };
    match matches.as_slice() {
        [app] => Ok(app),
        [] => Err(format!(
            "no installed app matches {query:?}; see `bunt agent apps --installed`"
        )),
        many => Err(format!(
            "{query:?} matches {}; be more specific",
            many.iter()
                .map(|app| format!("{} ({})", app.name, app.id))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// 지금 언어의 짧은 이름 (ko_KR.UTF-8 → ko)
pub fn language() -> String {
    std::env::var("LANG")
        .ok()
        .and_then(|lang| lang.split(['_', '.']).next().map(str::to_string))
        .filter(|lang| !lang.is_empty())
        .unwrap_or_else(|| "en".into())
}

/// 앱을 연다. 앱은 따로 만든 systemd scope에서 돈다. 그러지 않으면 데몬 서비스의 cgroup에 남아
/// 데몬이 멈출 때 함께 끝난다. DBusActivatable 앱은 이것으로 열지 않는다 (atspi::A11y::activate_app)
pub fn launch(path: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("systemd-run")
        .args([
            "--user",
            "--scope",
            "--collect",
            "--quiet",
            "--",
            "gio",
            "launch",
        ])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "gio launch failed ({status})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown_name(text: &str, language: &str) -> Option<String> {
        shown_entry(text, language).map(|entry| entry.name)
    }

    #[test]
    fn shown_names_follow_the_language_and_skip_hidden_entries() {
        let entry = "[Desktop Entry]\nType=Application\nName=Text Editor\nName[ko]=텍스트 편집기\n";
        assert_eq!(shown_name(entry, "ko").as_deref(), Some("텍스트 편집기"));
        assert_eq!(
            shown_entry(entry, "ko")
                .map(|entry| entry.untranslated)
                .as_deref(),
            Some("Text Editor")
        );
        // XDG_CURRENT_DESKTOP이 없으면 OnlyShowIn이 있는 항목은 숨는다
        if std::env::var_os("XDG_CURRENT_DESKTOP").is_none() {
            assert_eq!(shown_name(&format!("{entry}OnlyShowIn=KDE;\n"), "ko"), None);
            assert!(shown_name(&format!("{entry}NotShowIn=KDE;\n"), "ko").is_some());
        }
        assert_eq!(shown_name(entry, "en").as_deref(), Some("Text Editor"));
        assert_eq!(shown_name(&format!("{entry}NoDisplay=true\n"), "ko"), None);
        assert_eq!(
            shown_name("[Desktop Entry]\nType=Link\nName=x\n", "ko"),
            None
        );
        // 다른 묶음의 Name은 보지 않는다
        let action =
            "[Desktop Entry]\nType=Application\nName=Files\n[Desktop Action new]\nName=New\n";
        assert_eq!(shown_name(action, "en").as_deref(), Some("Files"));
    }

    #[test]
    fn windows_belong_to_the_desktop_id_or_the_wm_class() {
        let text = "[Desktop Entry]\nType=Application\nName=Firefox\nStartupWMClass=firefox\n";
        let entry = shown_entry(text, "en").unwrap();
        let app = DesktopApp {
            id: "firefox-esr".into(),
            name: entry.name,
            untranslated: entry.untranslated,
            path: PathBuf::new(),
            dbus: entry.dbus,
            wm_class: entry.wm_class,
        };
        assert!(app.owns("firefox-esr"));
        assert!(app.owns("firefox"));
        assert!(!app.owns("org.gnome.Calculator"));
    }

    #[test]
    fn dbus_activation_is_read_from_the_entry_group() {
        let entry = "[Desktop Entry]\nType=Application\nName=Text Editor\n";
        let dbus = |text: &str| shown_entry(text, "en").is_some_and(|entry| entry.dbus);
        assert!(dbus(&format!("{entry}DBusActivatable=true\n")));
        assert!(!dbus(entry));
        assert!(!dbus(&format!(
            "{entry}[Desktop Action new]\nDBusActivatable=true\n"
        )));
    }

    #[test]
    fn find_prefers_exact_matches() {
        let app = |id: &str, name: &str, untranslated: &str| DesktopApp {
            id: id.into(),
            name: name.into(),
            untranslated: untranslated.into(),
            path: PathBuf::new(),
            dbus: false,
            wm_class: None,
        };
        let apps = [
            app("org.gnome.TextEditor", "텍스트 편집기", "Text Editor"),
            app("org.gnome.Calculator", "계산기", "Calculator"),
            app("org.buntos.Settings", "시스템 설정", "System Settings"),
        ];
        assert_eq!(find(&apps, "계산기").unwrap().id, "org.gnome.Calculator");
        assert_eq!(find(&apps, "text editor").unwrap().name, "텍스트 편집기");
        assert_eq!(
            find(&apps, "org.buntos.settings").unwrap().name,
            "시스템 설정"
        );
        assert_eq!(find(&apps, "text").unwrap().id, "org.gnome.TextEditor");
        assert!(find(&apps, "org.gnome").is_err());
        assert!(find(&apps, "없는 앱").is_err());
    }
}
