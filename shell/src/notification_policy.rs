// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 알림 설정. 시스템 설정의 알림 페이지가 바꾸고 알림 서버(notifications.rs)와 배너·알림 센터
//! (notification_center.rs)가 따른다. macOS 시스템 설정 > 알림의 앱별 설정과 미리 보기를 GNOME과 같은
//! GSettings에 둔다.
//! - 전체: org.gnome.desktop.notifications. show-banners를 끄면 방해 금지다(제어 센터). application-children는
//!   알림 설정 목록에 오른 앱이다
//! - 앱별: org.gnome.desktop.notifications.application, 경로 /org/gnome/desktop/notifications/application/<앱>/.
//!   enable(알림 허용), show-banners(배너를 보일지), enable-sound-alerts(소리 재생)
//! - GNOME 키에 없는 macOS 설정은 같은 앱 이름의 buntOS 경로(/org/buntos/shell/app-notifications/<앱>/,
//!   org.buntos.shell.app-notifications)에 둔다: alerts(알림 스타일 "알림": 닫을 때까지 남는다), show-in-center
//! - 미리 보기 보기, 경고음과 그 음량: org.buntos.shell notification-previews, alert-sound, alert-volume
//! - <앱>은 데스크톱 ID(.desktop을 뗀 것)를 GNOME 셸처럼 정규화한 이름이다([`canonical_id`]). 알림을 처음 보낸
//!   앱은 GNOME 셸처럼 application-children에 더하고 application-id를 적어, 시스템 설정의 목록에 오른다

use std::{cell::RefCell, collections::HashMap, path::PathBuf};

use bunt_sound::theme;
use gtk4::{gio, glib, prelude::*};

const NOTIFICATIONS_SCHEMA: &str = "org.gnome.desktop.notifications";
const APP_SCHEMA: &str = "org.gnome.desktop.notifications.application";
const APP_PATH: &str = "/org/gnome/desktop/notifications/application/";
/// macOS에만 있는 앱별 설정 (shell/data/org.buntos.shell.gschema.xml)
const EXTRA_SCHEMA: &str = "org.buntos.shell.app-notifications";
const EXTRA_PATH: &str = "/org/buntos/shell/app-notifications/";
/// 사운드 테마 이름 (GNOME 공용 설정)
const SOUND_SCHEMA: &str = "org.gnome.desktop.sound";

/// 배너를 어떻게 보일지 (macOS의 알림 스타일)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Banners {
    /// 없음: 알림 센터에만 둔다
    None,
    /// 배너: 잠깐 보이고 사라진다
    Banner,
    /// 알림: 사람이 닫을 때까지 남는다
    Alert,
}

/// 앱 하나의 알림 설정
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// 알림 허용. 끄면 알림을 받기만 하고 보이지도 소리를 내지도 않는다
    pub enabled: bool,
    pub banners: Banners,
    /// 알림 소리 재생
    pub sound: bool,
    /// 알림 센터에 보기. 끄면 배너가 사라질 때 알림도 치운다
    pub in_center: bool,
}

impl Default for Policy {
    /// 앱을 모르거나 설정이 없을 때 (GNOME 스키마의 기본값과 같다)
    fn default() -> Self {
        Self {
            enabled: true,
            banners: Banners::Banner,
            sound: true,
            in_center: true,
        }
    }
}

/// 미리 보기 보기 (macOS의 항상, 잠금 해제 시, 안 함)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Previews {
    Always,
    /// 잠금 화면에서만 숨긴다. 잠금 화면이 생기기 전에는 늘 보인다
    WhenUnlocked,
    Never,
}

impl Previews {
    fn from_key(value: &str) -> Self {
        match value {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::WhenUnlocked,
        }
    }

    /// 알림 카드에 제목과 본문을 보일지 (잠금 화면이 아닐 때)
    pub fn shown(self) -> bool {
        self != Self::Never
    }
}

/// GNOME 셸의 앱 ID 정규화(messageTray.js의 _canonicalizeId): 소문자로 바꾸고, 영문 소문자·숫자·'-'가 아닌
/// 글자는 '-'로, 이어진 '-'는 하나로 줄인다. 시스템 설정(settings/src/pages/notifications.rs)도 같은 규칙을 쓴다
pub fn canonical_id(app_id: &str) -> String {
    let mut out = String::with_capacity(app_id.len());
    for char in app_id.trim_end_matches(".desktop").chars() {
        let char = char.to_ascii_lowercase();
        let char = if char.is_ascii_lowercase() || char.is_ascii_digit() || char == '-' {
            char
        } else {
            '-'
        };
        if !(char == '-' && out.ends_with('-')) {
            out.push(char);
        }
    }
    out
}

/// 설치된 스키마만 연다. 스키마가 없으면 GSettings가 프로세스를 끝내므로 먼저 확인한다
fn settings(schema: &str, path: Option<&str>) -> Option<gio::Settings> {
    let found = gio::SettingsSchemaSource::default()?.lookup(schema, true)?;
    Some(match path {
        Some(path) if found.path().is_none() => gio::Settings::with_path(schema, path),
        Some(_) => return None,
        None => gio::Settings::new(schema),
    })
}

/// 앱 하나의 설정 객체들
struct App {
    gnome: gio::Settings,
    extra: Option<gio::Settings>,
}

/// 앱별 알림 설정. 앱마다 설정 객체를 한 번 만들어 둔다
#[derive(Default)]
pub struct Policies {
    apps: RefCell<HashMap<String, Option<App>>>,
}

impl Policies {
    /// 앱(데스크톱 ID)의 설정. 앱을 모르거나(데스크톱 항목이 없는 알림) 스키마가 없으면 기본값이다
    pub fn for_app(&self, app_id: Option<&str>) -> Policy {
        let Some(app_id) = app_id.filter(|id| !id.is_empty()) else {
            return Policy::default();
        };
        let id = canonical_id(app_id);
        if id.is_empty() {
            return Policy::default();
        }
        let mut apps = self.apps.borrow_mut();
        let app = apps.entry(id.clone()).or_insert_with(|| {
            let gnome = settings(APP_SCHEMA, Some(&format!("{APP_PATH}{id}/")))?;
            let extra = settings(EXTRA_SCHEMA, Some(&format!("{EXTRA_PATH}{id}/")));
            remember(&gnome, &id, app_id);
            Some(App { gnome, extra })
        });
        let Some(app) = app else {
            return Policy::default();
        };
        let alerts = app
            .extra
            .as_ref()
            .is_some_and(|extra| extra.boolean("alerts"));
        Policy {
            enabled: app.gnome.boolean("enable"),
            banners: match (app.gnome.boolean("show-banners"), alerts) {
                (false, _) => Banners::None,
                (true, false) => Banners::Banner,
                (true, true) => Banners::Alert,
            },
            sound: app.gnome.boolean("enable-sound-alerts"),
            in_center: app
                .extra
                .as_ref()
                .is_none_or(|extra| extra.boolean("show-in-center")),
        }
    }
}

/// 알림 설정 목록에 앱을 올린다: application-id를 적고 application-children에 더한다
/// (GNOME 셸의 NotificationApplicationPolicy.store와 같다)
fn remember(app: &gio::Settings, canonical: &str, app_id: &str) {
    let desktop = format!("{}.desktop", app_id.trim_end_matches(".desktop"));
    if app.string("application-id") != desktop
        && let Err(err) = app.set_string("application-id", &desktop)
    {
        eprintln!("bunt-shell: could not remember notification settings for {desktop}: {err}");
    }
    let Some(all) = settings(NOTIFICATIONS_SCHEMA, None) else {
        return;
    };
    let mut children: Vec<glib::GString> = all.strv("application-children").into_iter().collect();
    if children.iter().any(|child| child == canonical) {
        return;
    }
    children.push(canonical.into());
    let children: Vec<&str> = children.iter().map(|child| child.as_str()).collect();
    if let Err(err) = all.set_strv("application-children", children) {
        eprintln!("bunt-shell: could not list {canonical} in notification settings: {err}");
    }
}

/// 미리 보기 보기
pub fn previews() -> Previews {
    crate::shell_settings()
        .filter(|shell| has_key(shell, "notification-previews"))
        .map_or(Previews::WhenUnlocked, |shell| {
            Previews::from_key(&shell.string("notification-previews"))
        })
}

fn has_key(settings: &gio::Settings, key: &str) -> bool {
    settings
        .settings_schema()
        .is_some_and(|schema| schema.has_key(key))
}

/// 알림 소리 파일: 앱이 정한 소리(sound-file, sound-name)가 먼저이고, 없으면 경고음(alert-sound)이다.
/// suppress-sound면 없다. 경고음이 빈 값("없음")이면 앱이 정하지 않은 알림은 조용하다
pub fn sound_file(
    sound_file_hint: Option<&str>,
    sound_name_hint: Option<&str>,
    suppress: bool,
    alert_sound: &str,
    sound_theme: &str,
    dirs: &[PathBuf],
) -> Option<PathBuf> {
    if suppress {
        return None;
    }
    if let Some(file) = sound_file_hint.and_then(theme::file_from_hint) {
        return Some(file);
    }
    if let Some(found) = sound_name_hint.and_then(|name| theme::find(name, sound_theme, dirs)) {
        return Some(found);
    }
    theme::find(alert_sound, sound_theme, dirs)
}

/// 알림 소리를 낸다. 음량은 알림 음량(alert-volume, 사람이 보는 세제곱 눈금)을 스트림 음량(선형)으로 바꿔 쓴다.
/// 출력 음량에 곱해진다(macOS의 알림 음량과 같다)
pub fn play_sound(sound_file_hint: Option<&str>, sound_name_hint: Option<&str>, suppress: bool) {
    let shell = crate::shell_settings();
    let alert_sound = shell
        .as_ref()
        .filter(|shell| has_key(shell, "alert-sound"))
        .map_or_else(
            || theme::DEFAULT_ALERT_SOUND.to_owned(),
            |shell| shell.string("alert-sound").to_string(),
        );
    let level = shell
        .as_ref()
        .filter(|shell| has_key(shell, "alert-volume"))
        .map_or(1.0, |shell| shell.double("alert-volume"));
    let sound_theme = settings(SOUND_SCHEMA, None).map_or_else(
        || theme::FALLBACK_THEME.to_owned(),
        |sound| sound.string("theme-name").to_string(),
    );
    let Some(file) = sound_file(
        sound_file_hint,
        sound_name_hint,
        suppress,
        &alert_sound,
        &sound_theme,
        &theme::data_dirs(),
    ) else {
        return;
    };
    if let Err(err) = bunt_sound::run::play(&file, level.clamp(0.0, 1.0).powi(3)) {
        eprintln!("bunt-shell: could not play {}: {err}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_are_canonicalized_like_gnome_shell() {
        assert_eq!(canonical_id("org.gnome.Nautilus"), "org-gnome-nautilus");
        assert_eq!(
            canonical_id("org.gnome.Nautilus.desktop"),
            "org-gnome-nautilus"
        );
        assert_eq!(canonical_id("firefox-esr"), "firefox-esr");
        // 이어진 '-'는 하나로 줄인다
        assert_eq!(canonical_id("my_._app"), "my-app");
        assert_eq!(canonical_id("한글앱"), "-");
        assert_eq!(canonical_id(""), "");
    }

    #[test]
    fn previews_default_to_when_unlocked() {
        assert_eq!(Previews::from_key("always"), Previews::Always);
        assert_eq!(Previews::from_key("never"), Previews::Never);
        assert_eq!(Previews::from_key("when-unlocked"), Previews::WhenUnlocked);
        assert_eq!(Previews::from_key("unknown"), Previews::WhenUnlocked);
        assert!(Previews::WhenUnlocked.shown());
        assert!(!Previews::Never.shown());
    }

    #[test]
    fn unknown_apps_use_the_defaults() {
        let policies = Policies::default();
        assert_eq!(policies.for_app(None), Policy::default());
        assert_eq!(policies.for_app(Some("")), Policy::default());
    }

    #[test]
    fn app_sounds_come_before_the_alert_sound() {
        let root = std::env::temp_dir().join(format!("bunt-shell-sounds-{}", std::process::id()));
        let stereo = root.join("sounds/freedesktop/stereo");
        std::fs::create_dir_all(&stereo).unwrap();
        for name in ["message.oga", "bell.oga"] {
            std::fs::write(stereo.join(name), b"OggS").unwrap();
        }
        let file = root.join("app.oga");
        std::fs::write(&file, b"OggS").unwrap();
        let dirs = [root.join("sounds")];
        let find = |file_hint: Option<&str>, name: Option<&str>, suppress, alert: &str| {
            sound_file(file_hint, name, suppress, alert, "freedesktop", &dirs)
        };
        assert_eq!(
            find(None, None, false, "bell"),
            Some(stereo.join("bell.oga"))
        );
        assert_eq!(
            find(None, Some("message-new-email"), false, "bell"),
            Some(stereo.join("message.oga"))
        );
        assert_eq!(
            find(file.to_str(), Some("message"), false, "bell"),
            Some(file.clone())
        );
        // 앱이 소리를 막았거나 경고음이 "없음"이면 조용하다
        assert_eq!(find(None, Some("message"), true, "bell"), None);
        assert_eq!(find(None, None, false, ""), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
