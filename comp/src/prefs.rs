// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 사용자가 시스템 설정에서 고른 값(화면 모드와 배율, 키 반복, 마우스, 트랙패드, 키보드 단축키). 다음
//! 로그인에도 쓰도록 `$XDG_CONFIG_HOME/buntos/comp-state.toml`에 저장하며, 시스템 기본값(comp.toml,
//! 단축키 표)보다 앞선다.

use std::{collections::BTreeMap, io::ErrorKind, path::PathBuf};

use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "comp-state.toml";

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyboard: Option<KeyRepeat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<PointerPrefs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touchpad: Option<TouchpadPrefs>,
    #[serde(default, rename = "output", skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<OutputPrefs>,
    /// 기본값과 다르게 바꾼 단축키 (단축키 ID → 값)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shortcuts: BTreeMap<String, ShortcutPref>,
}

/// 바꾼 단축키 하나. 키가 비면 키를 지운 것이다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutPref {
    #[serde(default)]
    pub key: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

fn enabled() -> bool {
    true
}

/// 키를 누르고 있을 때 반복이 시작되기까지의 시간(ms)과 초당 반복 횟수. 횟수가 0이면 반복하지 않는다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyRepeat {
    pub delay: u32,
    pub rate: u32,
}

/// 마우스: 이동 속도(libinput 가속, -1~1), 자연스러운 스크롤, 보조 클릭을 왼쪽 단추로(왼손잡이),
/// 스크롤 속도(배수), 커서 크기(논리 픽셀)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointerPrefs {
    pub speed: f64,
    pub natural_scroll: bool,
    pub left_handed: bool,
    pub scroll_speed: f64,
    pub cursor_size: u32,
}

impl PointerPrefs {
    /// 고르기 전의 값. 커서 크기는 comp.toml의 [cursor] size다
    pub fn with_cursor_size(cursor_size: u32) -> Self {
        Self {
            speed: 0.0,
            natural_scroll: false,
            left_handed: false,
            scroll_speed: 1.0,
            cursor_size,
        }
    }
}

impl Default for PointerPrefs {
    fn default() -> Self {
        Self::with_cursor_size(24)
    }
}

/// 트랙패드 (macOS 26의 트랙패드 > 포인트 및 클릭): 탭하여 클릭하기, 보조 클릭 방식, 이동 속도(libinput
/// 가속, -1~1). 이동 속도는 macOS처럼 마우스와 따로 둔다. 자연스러운 스크롤은 macOS처럼 마우스와 같은
/// 값(PointerPrefs)이다. 빠진 칸은 기본값이다
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TouchpadPrefs {
    pub tap_to_click: bool,
    pub secondary_click: SecondaryClick,
    pub speed: f64,
}

impl Default for TouchpadPrefs {
    /// 탭하여 클릭하기는 macOS(기본 꺼짐)와 달리 켠다. 키보드 덮개의 트랙패드를 두고 사용자가
    /// 골랐다(2026-11-11)
    fn default() -> Self {
        Self {
            tap_to_click: true,
            secondary_click: SecondaryClick::TwoFingers,
            speed: 0.0,
        }
    }
}

/// 트랙패드의 보조 클릭. macOS의 "왼쪽 아래 모서리 클릭"과 "끔"은 libinput에 맞는 방식이 없어 두지
/// 않는다(왼손잡이 설정은 가운데 누름까지 오른쪽 클릭으로 바꾸고, 소프트웨어 단추를 끄는 방식은 없다)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecondaryClick {
    /// 두 손가락으로 누르거나 탭한다 (libinput clickfinger)
    TwoFingers,
    /// 오른쪽 아래 모서리를 누른다 (libinput 소프트웨어 단추 영역)
    BottomRight,
}

impl SecondaryClick {
    /// D-Bus와 저장 파일에서 쓰는 이름
    pub fn name(self) -> &'static str {
        match self {
            Self::TwoFingers => "two-fingers",
            Self::BottomRight => "bottom-right",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::TwoFingers, Self::BottomRight]
            .into_iter()
            .find(|choice| choice.name() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputPrefs {
    /// 커넥터 이름 (예: eDP-1)
    pub name: String,
    /// 고정한 모드. 없으면 화면이 권하는 모드를 따른다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ModePref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModePref {
    pub width: i32,
    pub height: i32,
    /// mHz
    pub refresh: i32,
}

impl Prefs {
    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("buntos").join(FILE_NAME))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|err| {
                tracing::warn!("{}: {err}; ignoring saved settings", path.display());
                Self::default()
            }),
            Err(err) if err.kind() == ErrorKind::NotFound => Self::default(),
            Err(err) => {
                tracing::warn!("{}: {err}", path.display());
                Self::default()
            }
        }
    }

    /// 파일에 쓴다. 쓰는 도중에 꺼져도 앞 내용이 남도록 임시 파일에 쓴 뒤 이름을 바꾼다
    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        let result = toml::to_string(self)
            .map_err(std::io::Error::other)
            .and_then(|text| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let temporary = path.with_extension("toml.tmp");
                std::fs::write(&temporary, text)?;
                std::fs::rename(&temporary, &path)
            });
        if let Err(err) = result {
            tracing::warn!("failed to save {}: {err}", path.display());
        }
    }

    pub fn output(&self, name: &str) -> Option<&OutputPrefs> {
        self.outputs.iter().find(|output| output.name == name)
    }

    pub fn output_mut(&mut self, name: &str) -> &mut OutputPrefs {
        let index = match self.outputs.iter().position(|output| output.name == name) {
            Some(index) => index,
            None => {
                self.outputs.push(OutputPrefs {
                    name: name.to_owned(),
                    mode: None,
                    scale: None,
                });
                self.outputs.len() - 1
            }
        };
        &mut self.outputs[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut prefs = Prefs {
            keyboard: Some(KeyRepeat {
                delay: 300,
                rate: 30,
            }),
            ..Prefs::default()
        };
        prefs.output_mut("Virtual-1").mode = Some(ModePref {
            width: 1280,
            height: 800,
            refresh: 60_000,
        });
        prefs.output_mut("Virtual-1").scale = Some(2.0);
        prefs.shortcuts.insert(
            "app.copy".into(),
            ShortcutPref {
                key: "<Alt>c".into(),
                enabled: false,
            },
        );
        let text = toml::to_string(&prefs).unwrap();
        assert_eq!(toml::from_str::<Prefs>(&text).unwrap(), prefs);
        assert_eq!(prefs.outputs.len(), 1);
    }

    #[test]
    fn empty_file_is_default() {
        assert_eq!(toml::from_str::<Prefs>("").unwrap(), Prefs::default());
    }

    #[test]
    fn touchpad_round_trip_and_defaults() {
        let prefs = Prefs {
            touchpad: Some(TouchpadPrefs {
                tap_to_click: false,
                secondary_click: SecondaryClick::BottomRight,
                speed: 0.5,
            }),
            ..Prefs::default()
        };
        let text = toml::to_string(&prefs).unwrap();
        assert!(text.contains("secondary_click = \"bottom-right\""));
        assert_eq!(toml::from_str::<Prefs>(&text).unwrap(), prefs);

        // 빠진 칸은 기본값이다. 탭하여 클릭하기는 켜져 있다
        let partial: Prefs = toml::from_str("[touchpad]\nspeed = -0.5\n").unwrap();
        assert_eq!(
            partial.touchpad,
            Some(TouchpadPrefs {
                speed: -0.5,
                ..TouchpadPrefs::default()
            })
        );
        assert!(TouchpadPrefs::default().tap_to_click);
    }

    #[test]
    fn secondary_click_names() {
        for choice in [SecondaryClick::TwoFingers, SecondaryClick::BottomRight] {
            assert_eq!(SecondaryClick::from_name(choice.name()), Some(choice));
        }
        assert_eq!(SecondaryClick::from_name("off"), None);
    }
}
