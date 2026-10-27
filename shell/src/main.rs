// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-shell: 메뉴바, Dock, 알림 (wlr-layer-shell 클라이언트, ARCHITECTURE 5.2)

mod appmenu;
mod bluetooth;
mod config;
mod control_center;
mod dbus;
mod dock;
mod extras;
mod input_source;
mod launcher;
mod menubar;
mod network;
mod notification_center;
mod notifications;
mod power;
mod sound;
mod status_items;
mod toplevels;
mod wallpaper;

use std::rc::Rc;

use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

use crate::{
    bluetooth::Bluetooth, config::Config, control_center::Services, input_source::InputSources,
    launcher::Launcher, network::Network, notifications::Notifications, power::Power, sound::Sound,
    toplevels::Toplevels,
};

const APP_ID: &str = "org.buntos.Shell";
/// 공통 디자인 토큰(theme/tokens.css) 다음에 셸 스타일
const STYLE: &str = concat!(
    include_str!("../../theme/tokens.css"),
    include_str!("../data/style.css")
);
const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../i18n/ko/bunt-shell.ftl")),
    ("en", include_str!("../../i18n/en/bunt-shell.ftl")),
];
/// GNOME 공용 데스크톱 설정. 시스템 설정 앱이 시계 형식과 밝게·어둡게를 여기에 쓴다
const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
/// 셸 설정 (data/org.buntos.shell.gschema.xml). 시스템 설정의 메뉴 막대 페이지가 바꾼다
const SHELL_SCHEMA: &str = "org.buntos.shell";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(|app| {
        let config = Config::load();
        let tr = Rc::new(Translations::load(TRANSLATIONS));
        load_style();
        let toplevels =
            gtk::gdk::Display::default().and_then(|display| Toplevels::connect(&display));
        if toplevels.is_none() {
            eprintln!(
                "bunt-shell: the window list is unavailable; app menus and Dock indicators are off"
            );
        }
        wallpaper::build(app);
        let notifications = Notifications::start();
        let services = Services {
            network: Network::start(),
            bluetooth: Bluetooth::start(),
            power: Power::start(),
            sound: Sound::start(),
        };
        let input_sources = InputSources::start();
        let menubar = menubar::build(
            app,
            &tr,
            toplevels.as_ref(),
            &notifications,
            &services,
            &input_sources,
        );
        let launcher = Launcher::new(app, &tr, toplevels.as_ref());
        let dock = dock::build(app, &config, &tr, toplevels.as_ref(), &launcher);
        let banner = notification_center::banner(app, &notifications, &tr);
        if let Some(toplevels) = &toplevels {
            hide_dock_in_fullscreen(&dock, toplevels);
        }
        follow_color_scheme(vec![menubar, dock, banner, launcher.window().clone()]);
    });
    app.run()
}

fn load_style() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn layer_window(app: &gtk::Application, name: &str) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .css_classes([name])
        .build();
    window.init_layer_shell();
    window.set_namespace(Some(name));
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::None);
    window
}

/// 공용 데스크톱 설정. 스키마가 설치되지 않았으면 None이고, 이때는 기본값을 쓴다.
fn interface_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(INTERFACE_SCHEMA, true)?;
    Some(gio::Settings::new(INTERFACE_SCHEMA))
}

/// 셸 설정. 스키마가 설치되지 않았으면(개발 중 이미지 밖에서 실행) None이다
pub fn shell_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(SHELL_SCHEMA, true)?;
    Some(gio::Settings::new(SHELL_SCHEMA))
}

/// 앞에 있는 창이 전체 화면이면 Dock을 숨긴다 (macOS와 같다)
fn hide_dock_in_fullscreen(dock: &gtk::ApplicationWindow, toplevels: &Toplevels) {
    let dock = dock.clone();
    toplevels.subscribe(move |toplevels| {
        let fullscreen = toplevels.active().is_some_and(|active| active.fullscreen);
        dock.set_visible(!fullscreen);
    });
}

/// 시스템 설정에서 고른 밝게·어둡게(color-scheme)를 Dock, 알림 카드, 메뉴에 적용한다.
/// 메뉴바 자체는 macOS처럼 늘 흰 글씨를 쓴다.
fn follow_color_scheme(windows: Vec<gtk::ApplicationWindow>) {
    let Some(settings) = interface_settings() else {
        return;
    };
    apply_color_scheme(&settings, &windows);
    // 변경 알림은 설정 객체가 살아 있을 때만 온다. 처리기가 객체를 붙들어 셸이 끝날 때까지 둔다
    let keep_alive = settings.clone();
    settings.connect_changed(Some("color-scheme"), move |settings, _| {
        let _ = &keep_alive;
        apply_color_scheme(settings, &windows);
    });
}

/// 메뉴 같은 GTK 기본 위젯은 prefer-dark로, 자체 CSS는 창의 dark 클래스로 바꾼다
fn apply_color_scheme(settings: &gio::Settings, windows: &[gtk::ApplicationWindow]) {
    let dark = settings.string("color-scheme") == "prefer-dark";
    if let Some(gtk_settings) = gtk::Settings::default() {
        gtk_settings.set_gtk_application_prefer_dark_theme(dark);
    }
    for window in windows {
        if dark {
            window.add_css_class("dark");
        } else {
            window.remove_css_class("dark");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_the_same_messages() {
        bunt_i18n::check_same_messages(TRANSLATIONS).unwrap();
    }
}
