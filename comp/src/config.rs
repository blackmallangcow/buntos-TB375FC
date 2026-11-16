// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 선언형 설정 (ARCHITECTURE D7). 기본값은 data/comp.toml에 있다.

use std::path::PathBuf;

use serde::Deserialize;

use crate::input_mode::ModeSetting;

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
    /// 입력 모드 (input_mode.rs). 없으면 장치로 정한다
    #[serde(default)]
    pub input: Input,
    /// AI 에이전트 입력의 사람 우선 규칙 (agent_guard.rs). 없으면 기본값이다
    #[serde(default)]
    pub agent: Agent,
}

/// AI 에이전트 (agent_guard.rs)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Agent {
    /// 사람의 키·포인터·터치 입력 뒤 에이전트의 입력과 포커스 바꾸기를 미루는 시간 (ms)
    #[serde(default = "default_yield_ms")]
    pub yield_ms: u64,
}

impl Default for Agent {
    fn default() -> Self {
        Self {
            yield_ms: default_yield_ms(),
        }
    }
}

fn default_yield_ms() -> u64 {
    crate::agent_guard::DEFAULT_YIELD_MS
}

/// 입력 모드 (설계안 5.1절)
#[derive(Debug, Default, Deserialize)]
pub struct Input {
    /// "auto"(기본)는 붙은 입력 장치로 정하고, "pointer"·"touch"는 장치와 상관없이 그 모드로 둔다(개발·시험용).
    /// 모르는 값은 알리고 auto로 읽는다
    #[serde(default)]
    pub mode: ModeSetting,
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
    /// 잠금 화면(ext-session-lock 클라이언트). 잠금 프로토콜을 쓸 수 있는 클라이언트로 띄운다.
    /// 비면 화면을 잠그지 않는다 (로그인 화면)
    #[serde(default)]
    pub lock: Vec<String>,
    /// 셸이 끝나면 컴포지터도 끝낸다. 로그인 화면은 bunt-greeter가 끝나야 greetd가 사용자 세션을 연다
    #[serde(default, rename = "exit-with-shell")]
    pub exit_with_shell: bool,
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
        // 사용자 세션은 화면을 잠그고, 셸이 끝나도 남는다
        assert!(!config.session.lock.is_empty());
        assert!(!config.session.exit_with_shell);
        // 입력 모드는 장치로 정한다
        assert_eq!(config.input.mode, ModeSetting::Auto);
        // 사람 입력 뒤 AI는 1.5초 기다린다
        assert_eq!(config.agent.yield_ms, 1500);
    }

    /// [agent]에 yield-ms가 빠져도 기본값으로 읽고, 적은 값은 그대로 쓴다. [agent]가 아예 없는 설정은 로그인 화면의
    /// 시험이 본다
    #[test]
    fn agent_yield_defaults_and_overrides() {
        let empty = DEFAULT.replace("yield-ms = 1500\n", "");
        assert_ne!(empty, DEFAULT, "comp.toml sets yield-ms");
        let config: Config = toml::from_str(&empty).unwrap();
        assert_eq!(config.agent.yield_ms, crate::agent_guard::DEFAULT_YIELD_MS);
        let config: Config =
            toml::from_str(&DEFAULT.replace("yield-ms = 1500", "yield-ms = 400")).unwrap();
        assert_eq!(config.agent.yield_ms, 400);
    }

    /// 로그인 화면의 설정(greeter/data/root/etc/buntos/greeter.toml): 잠그지 않고, bunt-greeter가 끝나면
    /// 컴포지터도 끝나야 greetd가 사용자 세션을 연다
    #[test]
    fn greeter_config_exits_with_its_shell_and_never_locks() {
        let config: Config = toml::from_str(include_str!(
            "../../greeter/data/root/etc/buntos/greeter.toml"
        ))
        .unwrap();
        assert!(config.session.exit_with_shell);
        assert!(config.session.lock.is_empty());
        assert_eq!(config.session.shell, ["bunt-greeter"]);
        // [input]이 없는 설정도 읽고, 입력 모드는 장치로 정한다
        assert_eq!(config.input.mode, ModeSetting::Auto);
        // [agent]가 없으면 기본값이다
        assert_eq!(config.agent.yield_ms, crate::agent_guard::DEFAULT_YIELD_MS);
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex_color("#5b7cfa"), Some([0x5b, 0x7c, 0xfa]));
        assert_eq!(parse_hex_color("5b7cfa"), None);
        assert_eq!(parse_hex_color("#5b7"), None);
    }
}
