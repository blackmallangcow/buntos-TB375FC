// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-settings: 시스템 설정. macOS 26 시스템 설정을 본뜬 화면으로 Wi-Fi, Bluetooth, 네트워크,
//! 배터리, 화면 모드, 메뉴 막대, 배경화면, 디스플레이, 키보드, 언어, 날짜와 시간, 정보를 바꾼다.
//! ADR-001의 잠정안에 따라 libadwaita 없이 GTK4와 자체 CSS로 만든다.

mod dbus;
mod network_manager;
mod pages;
mod system;
mod widgets;
mod window;

use std::{cell::RefCell, rc::Rc};

use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{pages::Page, window::SettingsWindow};

const APP_ID: &str = "org.buntos.Settings";
/// 공통 디자인 토큰(theme/tokens.css) 다음에 이 앱의 CSS. AI 에이전트 페이지의 CSS는 따로 둔다
const STYLE: &str = concat!(
    include_str!("../../theme/tokens.css"),
    include_str!("../data/style.css"),
    include_str!("../data/agent.css")
);
/// 페이지가 많은 문자열은 따로 둔다 (AI 에이전트)
const TRANSLATIONS: &Sources = &[
    (
        "ko",
        concat!(
            include_str!("../../i18n/ko/bunt-settings.ftl"),
            include_str!("../../i18n/ko/bunt-settings-agent.ftl"),
        ),
    ),
    (
        "en",
        concat!(
            include_str!("../../i18n/en/bunt-settings.ftl"),
            include_str!("../../i18n/en/bunt-settings-agent.ftl"),
        ),
    ),
];

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_startup(|app| {
        load_style();
        // 메뉴바의 앱 메뉴("시스템 설정 종료")가 D-Bus로 부르는 동작
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &gtk::Application, _, _| app.quit())
            .build();
        app.add_action_entries([quit]);
        app.set_accels_for_action("app.quit", &["<Control>q"]);
    });

    let tr = Rc::new(Translations::load(TRANSLATIONS));
    let window: RefCell<Option<SettingsWindow>> = RefCell::new(None);
    // 이미 떠 있으면 새로 실행한 쪽의 인자가 이쪽으로 넘어온다
    app.connect_command_line(move |app, command_line| {
        // 인자는 처음 보여 줄 페이지 이름이다 (예: bunt-settings about)
        let page = command_line
            .arguments()
            .get(1)
            .and_then(|argument| argument.to_str())
            .and_then(Page::from_name);
        window
            .borrow_mut()
            .get_or_insert_with(|| SettingsWindow::new(app, &tr))
            .show(page);
        glib::ExitCode::SUCCESS
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_the_same_messages() {
        bunt_i18n::check_same_messages(TRANSLATIONS).unwrap();
    }
}
