// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 선언형 설정 (ARCHITECTURE D7). 기본값은 data/shell.toml에 있다.

use serde::Deserialize;

const DEFAULT: &str = include_str!("../data/shell.toml");
const SYSTEM_PATH: &str = "/etc/buntos/shell.toml";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub dock: Dock,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Dock {
    pub icon_size: i32,
    pub margin_bottom: i32,
    pub items: Vec<DockItem>,
}

/// Dock 항목. 설치된 앱(.desktop 파일)이거나 셸에 들어 있는 항목이다
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum DockItem {
    App {
        /// .desktop 파일 ID (예: "org.gnome.TextEditor.desktop")
        app: String,
        /// 이름으로 쓸 Fluent 메시지 ID. 없으면 .desktop 파일의 이름을 쓴다
        label: Option<String>,
    },
    Builtin {
        builtin: Builtin,
        /// 이름으로 쓸 Fluent 메시지 ID
        label: String,
    },
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Builtin {
    /// 모든 앱 목록
    Apps,
}

impl Config {
    pub fn load() -> Self {
        match std::fs::read_to_string(SYSTEM_PATH) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => return config,
                Err(err) => eprintln!("bunt-shell: {SYSTEM_PATH}: {err}; using built-in defaults"),
            },
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                eprintln!("bunt-shell: {SYSTEM_PATH}: {err}; using built-in defaults")
            }
            Err(_) => {}
        }
        toml::from_str(DEFAULT).expect("built-in shell.toml must be valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_config_parses() {
        let config: Config = toml::from_str(DEFAULT).unwrap();
        assert!(!config.dock.items.is_empty());
        assert!(config.dock.items.iter().all(|item| match item {
            DockItem::App { app, .. } => app.ends_with(".desktop"),
            DockItem::Builtin { .. } => true,
        }));
        assert!(config.dock.items.iter().any(|item| matches!(
            item,
            DockItem::Builtin {
                builtin: Builtin::Apps,
                ..
            }
        )));
    }
}
