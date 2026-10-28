// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설정 페이지

mod about;
mod agent;
mod appearance;
mod battery;
mod bluetooth;
mod datetime;
mod default_apps;
mod desktop_dock;
mod display;
mod keyboard;
mod language;
mod menu_bar;
mod mouse;
mod network;
mod wallpaper;
mod wifi;

use std::{cell::RefCell, rc::Rc};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::widgets::{Notice, Tint};

/// GNOME 공용 데스크톱 설정. bunt-shell과 GTK·libadwaita 앱이 같은 값을 읽는다
const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
/// bunt-shell 설정 (shell/data/org.buntos.shell.gschema.xml): 메뉴 막대 항목
pub const SHELL_SCHEMA: &str = "org.buntos.shell";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Page {
    Wifi,
    Bluetooth,
    Network,
    Battery,
    Appearance,
    MenuBar,
    DesktopDock,
    Wallpaper,
    Display,
    Keyboard,
    Mouse,
    Language,
    DateTime,
    DefaultApps,
    Agent,
    About,
}

impl Page {
    /// 왼쪽 목록에 놓는 순서
    pub const ALL: [Page; 16] = [
        Page::Wifi,
        Page::Bluetooth,
        Page::Network,
        Page::Battery,
        Page::Appearance,
        Page::MenuBar,
        Page::DesktopDock,
        Page::Wallpaper,
        Page::Display,
        Page::Keyboard,
        Page::Mouse,
        Page::Language,
        Page::DateTime,
        Page::DefaultApps,
        Page::Agent,
        Page::About,
    ];

    /// 명령줄과 .desktop 파일의 동작에서 쓰는 이름. 셸 메뉴가 `bunt-settings <이름>`으로 연다
    pub fn name(self) -> &'static str {
        match self {
            Self::Wifi => "wifi",
            Self::Bluetooth => "bluetooth",
            Self::Network => "network",
            Self::Battery => "battery",
            Self::Appearance => "appearance",
            Self::MenuBar => "menu-bar",
            Self::DesktopDock => "desktop-dock",
            Self::Wallpaper => "wallpaper",
            Self::Display => "display",
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
            Self::Language => "language",
            Self::DateTime => "datetime",
            Self::DefaultApps => "default-apps",
            Self::Agent => "agent",
            Self::About => "about",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.name() == name)
    }

    /// 제목의 Fluent 메시지 ID
    pub fn title_id(self) -> &'static str {
        match self {
            Self::Wifi => "page-wifi",
            Self::Bluetooth => "page-bluetooth",
            Self::Network => "page-network",
            Self::Battery => "page-battery",
            Self::Appearance => "page-appearance",
            Self::MenuBar => "page-menu-bar",
            Self::DesktopDock => "page-desktop-dock",
            Self::Wallpaper => "page-wallpaper",
            Self::Display => "page-display",
            Self::Keyboard => "page-keyboard",
            Self::Mouse => "page-mouse",
            Self::Language => "page-language",
            Self::DateTime => "page-datetime",
            Self::DefaultApps => "page-default-apps",
            Self::Agent => "page-agent",
            Self::About => "page-about",
        }
    }

    /// 사이드바 검색에 쓰는 낱말 (Fluent 메시지 ID)
    pub fn keywords_id(self) -> String {
        format!("{}-keywords", self.title_id())
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Wifi => "network-wireless-symbolic",
            Self::Bluetooth => "bluetooth-active-symbolic",
            Self::Network => "network-workgroup-symbolic",
            Self::Battery => "battery-full-symbolic",
            Self::Appearance => "preferences-desktop-appearance-symbolic",
            Self::MenuBar => "open-menu-symbolic",
            // 셸이 설치하는 아이콘 (shell/data/icons)
            Self::DesktopDock => "org.buntos.Dock-symbolic",
            Self::Wallpaper => "preferences-desktop-wallpaper-symbolic",
            Self::Display => "video-display-symbolic",
            Self::Keyboard => "input-keyboard-symbolic",
            Self::Mouse => "input-mouse-symbolic",
            Self::Language => "preferences-desktop-locale-symbolic",
            Self::DateTime => "preferences-system-time-symbolic",
            Self::DefaultApps => "view-app-grid-symbolic",
            // 셸이 설치하는 아이콘 (shell/data/icons)
            Self::Agent => "org.buntos.Agent-symbolic",
            Self::About => "computer-symbolic",
        }
    }

    /// 아이콘 배지 색. macOS 시스템 설정의 색을 따른다
    pub fn tint(self) -> Tint {
        match self {
            Self::Wifi | Self::Bluetooth | Self::Network | Self::Display => Tint::Blue,
            Self::Battery => Tint::Green,
            Self::Appearance | Self::MenuBar | Self::DesktopDock => Tint::Graphite,
            Self::Wallpaper => Tint::Cyan,
            Self::DefaultApps => Tint::Blue,
            Self::Agent => Tint::Pink,
            Self::Keyboard | Self::Mouse | Self::DateTime | Self::About => Tint::Gray,
            Self::Language => Tint::Indigo,
        }
    }

    /// 사이드바 묶음 번호. 번호가 바뀌는 곳에 간격을 둔다
    pub fn section(self) -> u8 {
        match self {
            Self::Wifi | Self::Bluetooth | Self::Network | Self::Battery => 0,
            Self::Appearance
            | Self::MenuBar
            | Self::DesktopDock
            | Self::Wallpaper
            | Self::Display => 1,
            Self::Keyboard | Self::Mouse | Self::Language | Self::DateTime | Self::DefaultApps => 2,
            Self::Agent => 3,
            Self::About => 4,
        }
    }

    pub fn build(self, context: &Rc<Context>) -> gtk::Widget {
        match self {
            Self::Wifi => wifi::build(context),
            Self::Bluetooth => bluetooth::build(context),
            Self::Network => network::build(context),
            Self::Battery => battery::build(context),
            Self::Appearance => appearance::build(context),
            Self::MenuBar => menu_bar::build(context),
            Self::DesktopDock => desktop_dock::build(context),
            Self::Wallpaper => wallpaper::build(context),
            Self::Display => display::build(context),
            Self::Keyboard => keyboard::build(context),
            Self::Mouse => mouse::build(context),
            Self::Language => language::build(context),
            Self::DateTime => datetime::build(context),
            Self::DefaultApps => default_apps::build(context),
            Self::Agent => agent::build(context),
            Self::About => about::build(context),
        }
    }
}

type Navigate = Rc<dyn Fn(Page)>;

/// 페이지들이 함께 쓰는 것
pub struct Context {
    pub tr: Rc<Translations>,
    pub notice: Notice,
    window: glib::WeakRef<gtk::Window>,
    navigate: RefCell<Option<Navigate>>,
}

impl Context {
    pub fn new(tr: Rc<Translations>, notice: Notice, window: &gtk::Window) -> Self {
        Self {
            tr,
            notice,
            window: window.downgrade(),
            navigate: RefCell::new(None),
        }
    }

    /// 대화상자의 부모 창
    pub fn window(&self) -> Option<gtk::Window> {
        self.window.upgrade()
    }

    pub fn set_navigate(&self, navigate: impl Fn(Page) + 'static) {
        self.navigate.replace(Some(Rc::new(navigate)));
    }

    /// 다른 페이지로 간다 (예: 네트워크 페이지의 Wi-Fi 행)
    pub fn open(&self, page: Page) {
        let navigate = self.navigate.borrow().clone();
        if let Some(navigate) = navigate {
            navigate(page);
        }
    }

    /// 창 아래에 잠깐 알림을 띄운다.
    pub fn notify(&self, message: &str) {
        self.notice.show(message);
    }

    /// 실패 알림. 메시지의 $error 자리에 오류 내용을 넣는다
    pub fn notify_error(&self, message_id: &str, error: impl ToString) {
        let mut args = FluentArgs::new();
        args.set("error", error.to_string());
        self.notify(&self.tr.get_with(message_id, &args));
    }
}

/// 설치된 GSettings 스키마만 연다. 스키마가 없으면 GSettings가 프로세스를 끝내므로 먼저 확인한다
pub fn settings_for(schema: &str) -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(schema, true)?;
    Some(gio::Settings::new(schema))
}

/// 공용 데스크톱 설정. 스키마가 설치되지 않았으면 None이다.
pub fn interface_settings() -> Option<gio::Settings> {
    settings_for(INTERFACE_SCHEMA)
}

/// bunt-shell 설정. 셸이 없는 이미지에서는 None이다
pub fn shell_settings() -> Option<gio::Settings> {
    settings_for(SHELL_SCHEMA)
}

/// 키가 있는 스키마인지 (이전 버전 스키마에는 키가 없을 수 있다)
pub fn has_key(settings: &gio::Settings, key: &str) -> bool {
    settings
        .settings_schema()
        .is_some_and(|schema| schema.has_key(key))
}

/// GSettings의 참·거짓 키를 스위치와 묶는다. 다른 곳에서 바꿔도 따라간다
pub fn bind_switch(settings: &gio::Settings, key: &str, switch: &gtk::Switch) {
    if has_key(settings, key) {
        settings.bind(key, switch, "active").build();
    } else {
        switch.set_sensitive(false);
    }
}

/// GSettings의 문자열 키를 드롭다운과 묶는다. 드롭다운의 n번째 항목이 `values[n]`이다.
/// 다른 곳에서 바꿔도 따라간다
pub fn bind_choice(
    settings: &gio::Settings,
    key: &str,
    dropdown: &gtk::DropDown,
    values: &'static [&'static str],
) {
    if !has_key(settings, key) {
        dropdown.set_sensitive(false);
        return;
    }
    settings
        .bind(key, dropdown, "selected")
        .mapping(move |variant, _| {
            let value = variant.str()?;
            let index = values.iter().position(|choice| *choice == value)?;
            Some((index as u32).to_value())
        })
        .set_mapping(move |value, _| {
            let index = value.get::<u32>().ok()?;
            values.get(index as usize).map(|choice| choice.to_variant())
        })
        .build();
}

/// GSettings의 숫자 키(i 또는 d)를 슬라이더와 묶는다. 끄는 동안 바로바로 적으므로 셸이 따라 바뀐다
pub fn bind_scale(settings: &gio::Settings, key: &str, scale: &gtk::Scale) {
    if !has_key(settings, key) {
        scale.set_sensitive(false);
        return;
    }
    let integer = settings.value(key).is::<i32>();
    settings
        .bind(key, &scale.adjustment(), "value")
        .mapping(|variant, _| {
            let value = variant
                .get::<f64>()
                .or_else(|| variant.get::<i32>().map(f64::from))?;
            Some(value.to_value())
        })
        .set_mapping(move |value, _| {
            let value = value.get::<f64>().ok()?;
            Some(if integer {
                (value.round() as i32).to_variant()
            } else {
                value.to_variant()
            })
        })
        .build();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_names_round_trip() {
        for page in Page::ALL {
            assert_eq!(Page::from_name(page.name()), Some(page));
        }
        assert_eq!(Page::from_name("no-such-page"), None);
    }

    #[test]
    fn shell_menus_open_these_pages() {
        // bunt-shell 메뉴가 쓰는 이름. 바꾸면 셸도 함께 고친다
        for name in [
            "wifi",
            "bluetooth",
            "network",
            "battery",
            "appearance",
            "menu-bar",
            "wallpaper",
            "display",
            "keyboard",
            "language",
            "datetime",
            "about",
        ] {
            assert!(Page::from_name(name).is_some(), "{name}");
        }
    }

    #[test]
    fn sections_are_contiguous() {
        let sections: Vec<u8> = Page::ALL.iter().map(|page| page.section()).collect();
        assert!(sections.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
