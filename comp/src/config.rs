// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 선언형 설정 (ARCHITECTURE D7). 기본값은 data/comp.toml에 있다.

use std::path::PathBuf;

use serde::Deserialize;

const DEFAULT: &str = include_str!("../data/comp.toml");
const SYSTEM_PATH: &str = "/etc/buntos/comp.toml";
/// 설정 파일을 바꾸는 환경 변수. 로그인 화면은 사용자 세션과 다른 셸을 띄운다
const PATH_VARIABLE: &str = "BUNT_COMP_CONFIG";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub wallpaper: Wallpaper,
    pub keyboard: Keyboard,
    pub cursor: Cursor,
    pub session: Session,
    #[serde(default)]
    pub effects: Effects,
}

/// 화면 효과
#[derive(Debug, Deserialize)]
pub struct Effects {
    /// 셸 유리(메뉴, 제어 센터, Dock, 모든 앱 패널) 뒤를 흐리게 그린다
    #[serde(default = "enabled")]
    pub blur: bool,
}

impl Default for Effects {
    fn default() -> Self {
        Self { blur: enabled() }
    }
}

fn enabled() -> bool {
    true
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
    /// startup보다 먼저, 하나씩 끝날 때까지 기다리며 실행하는 명령
    #[serde(default)]
    pub prepare: Vec<Vec<String>>,
    /// 셸. 창 목록·창 조작 프로토콜을 쓸 수 있는 특권 클라이언트로 띄운다
    #[serde(default)]
    pub shell: Vec<String>,
    /// 입력기(한글 입력). 입력기 프로토콜을 쓸 수 있는 클라이언트로 띄우고, 끝나면 다시 띄운다
    #[serde(default, rename = "input-method")]
    pub input_method: Vec<String>,
    #[serde(default)]
    pub startup: Vec<Vec<String>>,
    pub terminal: Vec<String>,
}

impl Config {
    pub fn load() -> Self {
        let path = std::env::var_os(PATH_VARIABLE)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(SYSTEM_PATH));
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => return config,
                Err(err) => {
                    tracing::warn!("{}: {err}; using built-in defaults", path.display())
                }
            },
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!("{}: {err}; using built-in defaults", path.display())
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
        assert!(!config.session.input_method.is_empty());
        assert!(config.effects.blur);
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex_color("#5b7cfa"), Some([0x5b, 0x7c, 0xfa]));
        assert_eq!(parse_hex_color("5b7cfa"), None);
        assert_eq!(parse_hex_color("#5b7"), None);
    }
}
