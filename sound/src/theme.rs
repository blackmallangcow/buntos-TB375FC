// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! XDG 사운드 테마(freedesktop.org Sound Theme Specification)에서 소리 이름의 파일을 찾는다.
//! - 찾는 곳: $XDG_DATA_HOME/sounds, $XDG_DATA_DIRS의 sounds. 테마 안의 stereo 디렉터리
//! - 테마: 고른 테마(org.gnome.desktop.sound theme-name), 없으면 freedesktop
//! - 이름이 없으면 끝의 "-부분"을 떼어 다시 찾는다(message-new-instant → message-new → message)
//!
//! 테마의 index.theme(상속, 디렉터리 목록)는 읽지 않는다. 이미지의 sound-theme-freedesktop은 stereo 하나뿐이다.

use std::path::{Path, PathBuf};

/// 테마가 없거나 이름을 찾지 못했을 때 찾는 테마
pub const FALLBACK_THEME: &str = "freedesktop";
const EXTENSIONS: [&str; 3] = ["oga", "ogg", "wav"];

/// 시스템 설정의 사운드 페이지에서 고를 수 있는 경고음. freedesktop 사운드 테마의 이름이다. 이미지에 없는
/// 파일은 시스템 설정이 목록에서 뺀다
pub const ALERT_SOUNDS: [&str; 7] = [
    "message-new-instant",
    "message",
    "bell",
    "complete",
    "dialog-information",
    "dialog-warning",
    "window-attention",
];

/// 경고음의 기본값 (셸 스키마 alert-sound의 기본값과 같다)
pub const DEFAULT_ALERT_SOUND: &str = "message-new-instant";

/// 사운드 테마를 찾는 디렉터리들 (앞이 우선)
pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    home.into_iter()
        .chain(
            system
                .split(':')
                .filter(|dir| !dir.is_empty())
                .map(PathBuf::from),
        )
        .map(|dir| dir.join("sounds"))
        .collect()
}

/// 이름 다음에 찾을 이름: 끝의 "-부분"을 뗀다
fn fallback(name: &str) -> Option<&str> {
    name.rsplit_once('-').map(|(rest, _)| rest)
}

/// 소리 이름의 파일. 이름이 경로처럼 보이면(/, .) 받지 않는다
pub fn find(name: &str, theme: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() || name.contains(['/', '.']) {
        return None;
    }
    let mut themes = vec![theme];
    if theme != FALLBACK_THEME {
        themes.push(FALLBACK_THEME);
    }
    let mut candidate = Some(name);
    while let Some(name) = candidate {
        for theme in &themes {
            if theme.is_empty() || theme.contains('/') {
                continue;
            }
            for dir in dirs {
                for extension in EXTENSIONS {
                    let path = dir
                        .join(theme)
                        .join("stereo")
                        .join(format!("{name}.{extension}"));
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        candidate = fallback(name);
    }
    None
}

/// 알림 힌트 sound-file의 값(경로나 file:// URI)에서 파일. 상대 경로와 다른 URI는 받지 않는다
pub fn file_from_hint(value: &str) -> Option<PathBuf> {
    let path = match value.strip_prefix("file://") {
        Some(rest) => percent_decode(rest)?,
        None => value.to_owned(),
    };
    let path = PathBuf::from(path);
    (path.is_absolute() && path.is_file()).then_some(path)
}

/// URI 경로의 %XX를 푼다. 잘못된 꼴이면 None
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(byte);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// pw-play 인자: 알림 역할(media.role Notification)과 스트림 음량(선형)으로 파일을 재생한다
pub fn play_args(file: &Path, volume: f64) -> Vec<String> {
    vec![
        "pw-play".to_owned(),
        "--media-role".to_owned(),
        "Notification".to_owned(),
        "--volume".to_owned(),
        format!("{:.3}", volume.clamp(0.0, 1.0)),
        file.to_string_lossy().into_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험마다 따로 쓰는 임시 디렉터리
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bunt-sound-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"OggS").unwrap();
    }

    #[test]
    fn names_fall_back_by_dropping_parts_and_themes() {
        let root = temp_dir("theme");
        let (user, system) = (root.join("home/sounds"), root.join("usr/sounds"));
        touch(&system.join("freedesktop/stereo/message.oga"));
        touch(&system.join("freedesktop/stereo/bell.oga"));
        touch(&user.join("custom/stereo/bell.wav"));
        let dirs = [user.clone(), system.clone()];
        // 고른 테마가 먼저다
        assert_eq!(
            find("bell", "custom", &dirs),
            Some(user.join("custom/stereo/bell.wav"))
        );
        // 고른 테마에 없으면 freedesktop
        assert_eq!(
            find("message", "custom", &dirs),
            Some(system.join("freedesktop/stereo/message.oga"))
        );
        // 끝 부분을 떼어 찾는다
        assert_eq!(
            find("message-new-instant", "freedesktop", &dirs),
            Some(system.join("freedesktop/stereo/message.oga"))
        );
        assert_eq!(find("no-such-sound", "freedesktop", &dirs), None);
        // 경로를 이름으로 쓰지 못한다
        assert_eq!(find("../bell", "freedesktop", &dirs), None);
        assert_eq!(
            find("bell", "../custom", &dirs),
            Some(system.join("freedesktop/stereo/bell.oga"))
        );
        assert_eq!(find("", "freedesktop", &dirs), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn hint_files_must_be_absolute_and_exist() {
        let root = temp_dir("hint");
        let file = root.join("새 소리.oga");
        touch(&file);
        assert_eq!(file_from_hint(file.to_str().unwrap()), Some(file.clone()));
        let uri = format!(
            "file://{}",
            file.to_str()
                .unwrap()
                .replace(' ', "%20")
                .replace("새", "%EC%83%88")
        );
        assert_eq!(file_from_hint(&uri), Some(file.clone()));
        assert_eq!(file_from_hint("relative.oga"), None);
        assert_eq!(file_from_hint("https://example.com/a.oga"), None);
        assert_eq!(file_from_hint("file:///bad%zz"), None);
        assert_eq!(
            file_from_hint(root.join("missing.oga").to_str().unwrap()),
            None
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn playback_uses_the_notification_role() {
        assert_eq!(
            play_args(
                Path::new("/usr/share/sounds/freedesktop/stereo/bell.oga"),
                2.0
            ),
            [
                "pw-play",
                "--media-role",
                "Notification",
                "--volume",
                "1.000",
                "/usr/share/sounds/freedesktop/stereo/bell.oga"
            ]
        );
    }
}
