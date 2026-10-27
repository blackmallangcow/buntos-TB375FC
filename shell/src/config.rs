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

#[derive(Debug, Deserialize)]
pub struct DockItem {
    pub icon: String,
    /// Fluent 메시지 ID
    pub label: String,
    pub command: Vec<String>,
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
        assert!(
            config
                .dock
                .items
                .iter()
                .all(|item| !item.command.is_empty())
        );
    }
}
