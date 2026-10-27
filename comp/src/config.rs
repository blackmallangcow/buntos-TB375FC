// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 선언형 설정 (ARCHITECTURE D7). 기본값은 data/comp.toml에 있다.

use serde::Deserialize;

const DEFAULT: &str = include_str!("../data/comp.toml");
const SYSTEM_PATH: &str = "/etc/buntos/comp.toml";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub wallpaper: Wallpaper,
    pub keyboard: Keyboard,
    pub cursor: Cursor,
    pub session: Session,
}

#[derive(Debug, Deserialize)]
pub struct Wallpaper {
    pub top: String,
    pub bottom: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Keyboard {
    pub layout: String,
    pub repeat_delay: i32,
    pub repeat_rate: i32,
}

#[derive(Debug, Deserialize)]
pub struct Cursor {
    pub theme: String,
    pub size: u32,
}

#[derive(Debug, Deserialize)]
pub struct Session {
    pub startup: Vec<Vec<String>>,
    pub terminal: Vec<String>,
}

impl Config {
    pub fn load() -> Self {
        match std::fs::read_to_string(SYSTEM_PATH) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => return config,
                Err(err) => tracing::warn!("{SYSTEM_PATH}: {err}; using built-in defaults"),
            },
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!("{SYSTEM_PATH}: {err}; using built-in defaults")
            }
            Err(_) => {}
        }
        toml::from_str(DEFAULT).expect("built-in comp.toml must be valid")
    }
}

/// "#rrggbb" 형식의 색을 [r, g, b]로 바꾼다.
pub fn parse_hex_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_config_parses() {
        let config: Config = toml::from_str(DEFAULT).unwrap();
        assert!(parse_hex_color(&config.wallpaper.top).is_some());
        assert!(parse_hex_color(&config.wallpaper.bottom).is_some());
        assert!(!config.session.terminal.is_empty());
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex_color("#5b7cfa"), Some([0x5b, 0x7c, 0xfa]));
        assert_eq!(parse_hex_color("5b7cfa"), None);
        assert_eq!(parse_hex_color("#5b7"), None);
    }
}
