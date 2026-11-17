// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창의 앱 ID(Wayland app_id)와 설치된 앱(.desktop)을 짝짓는다.
//!
//! 대부분의 앱은 창의 앱 ID가 .desktop 파일의 이름이다(org.gnome.TextEditor). Electron 앱과 AppImage로 설치한 앱은
//! 다르다: Orca는 파일이 orca-ide.desktop이고 창은 orca라고 알리며, 그 이름을 파일의 StartupWMClass에 적는다.
//! 전에는 이름이 같은 파일만 찾아서 Dock과 앱 전환 창이 기본 아이콘을, 메뉴 막대가 앱 ID를 보였고, 모든 앱에서 다시
//! 누르면 떠 있는 창을 앞으로 가져오지 않고 하나 더 띄웠다(TB375FC, 2026-11-17). 에이전트(agent/src/desktop.rs)와
//! 같은 규칙으로 찾는다: 이름이 같은 파일이 먼저고, 없으면 StartupWMClass가 같은 앱이다

use std::{cell::RefCell, collections::HashMap};

use gtk4::{gio, prelude::*};

use crate::toplevels::Toplevels;

/// 설치된 앱 하나를 짝짓는 데 쓰는 것
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// .desktop을 뗀 파일 이름
    id: String,
    wm_class: Option<String>,
}

thread_local! {
    /// 이름이 같은 파일이 없던 앱 ID가 어느 앱(.desktop ID)인지. 설치된 앱이 바뀌면 비운다
    static BY_WM_CLASS: RefCell<HashMap<String, Option<String>>> = RefCell::default();
    static MONITOR: RefCell<Option<gio::AppInfoMonitor>> = const { RefCell::new(None) };
}

/// .desktop 파일 ID에서 .desktop을 뗀다. 대부분의 앱에서 창의 앱 ID와 같다
pub fn stem(desktop_id: &str) -> &str {
    desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id)
}

/// 그 앱 ID의 창을 가진 앱. 이름이 같은 파일이 먼저다
fn pick(app_id: &str, entries: &[Entry]) -> Option<usize> {
    let same = |a: &str| a == app_id;
    let alike = |a: &str| a.eq_ignore_ascii_case(app_id);
    let class =
        |entry: &Entry, test: &dyn Fn(&str) -> bool| entry.wm_class.as_deref().is_some_and(test);
    entries
        .iter()
        .position(|entry| same(&entry.id))
        .or_else(|| entries.iter().position(|entry| class(entry, &same)))
        .or_else(|| entries.iter().position(|entry| alike(&entry.id)))
        .or_else(|| entries.iter().position(|entry| class(entry, &alike)))
}

/// 창의 앱 ID가 그 이름들(앱의 [`window_ids`]) 가운데 하나인지. 대소문자만 다른 것도 같은 앱으로 본다
pub fn same_app(ids: &[String], app_id: &str) -> bool {
    !app_id.is_empty() && ids.iter().any(|id| id.eq_ignore_ascii_case(app_id))
}

/// 그 앱의 창이 알릴 수 있는 앱 ID: 파일 이름과 StartupWMClass
pub fn window_ids(info: &impl IsA<gio::AppInfo>) -> Vec<String> {
    let mut ids: Vec<String> = info
        .as_ref()
        .id()
        .map(|id| stem(&id).to_owned())
        .into_iter()
        .collect();
    let class = info
        .as_ref()
        .downcast_ref::<gio_unix::DesktopAppInfo>()
        .and_then(|info| info.startup_wm_class());
    if let Some(class) = class
        && !ids.iter().any(|id| *id == class.as_str())
    {
        ids.push(class.into());
    }
    ids
}

/// 창의 앱 ID로 설치된 앱을 찾는다. 없으면(설치하지 않고 띄운 프로그램) None
pub fn for_window(app_id: &str) -> Option<gio_unix::DesktopAppInfo> {
    if app_id.is_empty() {
        return None;
    }
    if let Some(info) = gio_unix::DesktopAppInfo::new(&format!("{app_id}.desktop")) {
        return Some(info);
    }
    watch_installed();
    let known = BY_WM_CLASS.with(|found| found.borrow().get(app_id).cloned());
    let desktop_id = known.unwrap_or_else(|| {
        let apps: Vec<gio_unix::DesktopAppInfo> = gio::AppInfo::all()
            .into_iter()
            .filter_map(|app| app.downcast().ok())
            .collect();
        let entries: Vec<Entry> = apps
            .iter()
            .map(|app| Entry {
                id: app.id().map(|id| stem(&id).to_owned()).unwrap_or_default(),
                wm_class: app.startup_wm_class().map(Into::into),
            })
            .collect();
        let found = pick(app_id, &entries)
            .and_then(|index| apps[index].id())
            .map(|id| id.to_string());
        BY_WM_CLASS.with(|cache| {
            cache.borrow_mut().insert(app_id.to_owned(), found.clone());
        });
        found
    });
    desktop_id.and_then(|id| gio_unix::DesktopAppInfo::new(&id))
}

/// 앱의 (지역화된) 이름. 설치된 앱이 아니면 앱 ID를 쓴다
pub fn name(app_id: &str) -> String {
    for_window(app_id)
        .map(|info| info.name().to_string())
        .unwrap_or_else(|| app_id.to_owned())
}

/// 그 앱의 창 가운데 가장 최근에 쓴 것을 앞으로 가져온다. 창이 없으면 false
pub fn activate(toplevels: &Toplevels, info: &impl IsA<gio::AppInfo>) -> bool {
    let ids = window_ids(info);
    let mut open: Vec<String> = Vec::new();
    for toplevel in toplevels.list() {
        if same_app(&ids, &toplevel.app_id) && !open.contains(&toplevel.app_id) {
            open.push(toplevel.app_id);
        }
    }
    open.iter().any(|app_id| toplevels.activate_app(app_id))
}

/// 설치된 앱이 바뀌면(설치, 삭제) 기억한 짝을 버린다
fn watch_installed() {
    MONITOR.with(|monitor| {
        if monitor.borrow().is_some() {
            return;
        }
        let watch = gio::AppInfoMonitor::get();
        watch.connect_changed(|_| {
            BY_WM_CLASS.with(|found| found.borrow_mut().clear());
        });
        monitor.replace(Some(watch));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, wm_class: Option<&str>) -> Entry {
        Entry {
            id: id.to_owned(),
            wm_class: wm_class.map(str::to_owned),
        }
    }

    #[test]
    fn a_window_finds_its_app_by_file_name_then_by_startup_wm_class() {
        let apps = [
            entry("org.gnome.TextEditor", None),
            entry("orca-ide", Some("orca")),
            entry("code", Some("Code")),
            entry("foot", None),
            entry("other-terminal", Some("foot")),
        ];
        // 대부분의 앱: 파일 이름이 앱 ID다
        assert_eq!(pick("org.gnome.TextEditor", &apps), Some(0));
        // Electron·AppImage 앱: 파일은 orca-ide.desktop, 창은 orca
        assert_eq!(pick("orca", &apps), Some(1));
        // 이름이 같은 파일이 StartupWMClass보다 먼저다
        assert_eq!(pick("foot", &apps), Some(3));
        // 대소문자만 다른 것은 마지막에 본다
        assert_eq!(pick("Orca", &apps), Some(1));
        assert_eq!(pick("Code", &apps), Some(2));
        assert_eq!(pick("CODE", &apps), Some(2));
        // 설치하지 않은 프로그램
        assert_eq!(pick("glmark2", &apps), None);
    }

    #[test]
    fn an_apps_windows_go_by_its_file_name_or_its_startup_wm_class() {
        let ids = ["orca-ide".to_owned(), "orca".to_owned()];
        assert!(same_app(&ids, "orca"));
        assert!(same_app(&ids, "orca-ide"));
        assert!(same_app(&ids, "Orca"));
        assert!(!same_app(&ids, "orc"));
        assert!(!same_app(&ids, ""));
        assert_eq!(stem("org.gnome.TextEditor.desktop"), "org.gnome.TextEditor");
        assert_eq!(stem("foot"), "foot");
    }
}
