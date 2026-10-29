// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-shell: 메뉴바, Dock, 알림 (wlr-layer-shell 클라이언트, ARCHITECTURE 5.2)

mod agent_access;
mod appmenu;
mod bluetooth;
mod compositor;
mod config;
mod control_center;
mod dbus;
mod dock;
mod extras;
mod input_source;
mod launcher;
mod menubar;
mod minimized;
mod network;
mod notification_center;
mod notifications;
mod polkit_agent;
mod power;
mod screenshot;
mod sound;
mod status_items;
mod switcher;
mod toplevels;
mod wallpaper;
// 로고 메뉴의 강제 종료, 잠자기, 화면 잠금, 로그아웃
mod session;
// 시스템 서비스가 사람에게 묻는 창: Wi-Fi 암호(NetworkManager 비밀 에이전트)와 그 틀
mod secret_agent;
mod service_dialog;

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
/// 셸 설정 (data/org.buntos.shell.gschema.xml). 시스템 설정의 메뉴 막대, 데스크탑 및 Dock 페이지가 바꾼다
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
        // 세션의 관리자 인증(pkexec, 시간대, 기기 이름, AI 에이전트 권한)을 묻는 창
        polkit_agent::start(app, &tr);
        // 허용하지 않은 AI가 이 컴퓨터를 제어하려 하면 묻고 시스템 설정으로 바로 보내는 창
        agent_access::start(app, &tr);
        // 저장된 Wi-Fi의 암호가 바뀌었거나 새 네트워크에 암호가 필요할 때 NetworkManager가 묻는 창
        secret_agent::start(app, &tr);
        let services = Services {
            network: Network::start(),
            bluetooth: Bluetooth::start(),
            power: Power::start(),
            sound: Sound::start(),
        };
        let input_sources = InputSources::start();
        // 로고 메뉴의 강제 종료…, 잠자기, 화면 잠금, 로그아웃… 동작. 메뉴 막대보다 먼저 단다
        let session = session::Session::new(app, &tr);
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
        // ⇧⌘5 스크린샷 도구 막대와 찍은 뒤 화면 모서리의 미리 보기
        let screenshot_toolbar = screenshot::Toolbar::new(app, &tr);
        let screenshot_preview = screenshot::Thumbnail::new(app, &tr);
        if let Some(settings) = shell_settings() {
            compositor::share_panel_settings(&settings);
            screenshot::share_options(&settings);
        }
        compositor::follow_shortcuts(&launcher, &session, shell_settings(), &screenshot_toolbar);
        // ⌘⇥ 앱 전환 창. 키는 컴포지터가 받고 이 창은 보이기만 한다
        let switcher = switcher::Switcher::start(app);
        follow_color_scheme(vec![
            menubar,
            dock,
            banner,
            launcher.window().clone(),
            switcher.window().clone(),
            screenshot_toolbar.window().clone(),
            screenshot_preview.window().clone(),
        ]);
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
    // 제목은 보이지 않지만 접근성 이름이 된다. 에이전트는 이것으로 인증 창을 알아보고 건드리지 않는다
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(name)
        .css_classes([name])
        .build();
    window.init_layer_shell();
    window.set_namespace(Some(name));
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::None);
    window
}

/// 레이어 창이 입력을 받는 곳을 그 창의 자식 위젯이 그리는 자리로 줄인다. 그림자를 그리려고 둔 둘레의
/// 투명한 여백이 뒤 창의 클릭을 가로채지 않게 한다(macOS의 그림자는 클릭을 받지 않는다). 창 크기가 바뀔
/// 때마다 다시 잰다. 크기를 바꿀 수 없는 창은 내용이 바뀌면 크기가 바뀐다
pub fn clip_input_to_child(window: &gtk::ApplicationWindow) {
    window.connect_realize(|window| {
        let Some(surface) = window.surface() else {
            return;
        };
        let window = window.downgrade();
        // GTK가 같은 신호에서 창을 먼저 배치하므로 자식의 자리는 새 크기의 것이다
        surface.connect_layout(move |surface, _, _| {
            let Some(window) = window.upgrade() else {
                return;
            };
            let Some(bounds) = window
                .child()
                .and_then(|child| child.compute_bounds(&window))
            else {
                return;
            };
            let (dx, dy) = window.surface_transform();
            let area = gtk::cairo::RectangleInt::new(
                (f64::from(bounds.x()) + dx).floor() as i32,
                (f64::from(bounds.y()) + dy).floor() as i32,
                f64::from(bounds.width()).ceil() as i32,
                f64::from(bounds.height()).ceil() as i32,
            );
            surface.set_input_region(Some(&gtk::cairo::Region::create_rectangle(&area)));
        });
    });
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
