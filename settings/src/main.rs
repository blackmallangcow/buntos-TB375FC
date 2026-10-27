// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-settings: 시스템 설정 (정보, 모양, 날짜와 시간, 언어).
//! ADR-001의 잠정안에 따라 libadwaita 없이 GTK4와 자체 CSS로 만든다.

mod pages;
mod system;
mod widgets;
mod window;

use std::{cell::RefCell, rc::Rc};

use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{pages::Page, window::SettingsWindow};

const APP_ID: &str = "org.buntos.Settings";
const STYLE: &str = include_str!("../data/style.css");
const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../i18n/ko/bunt-settings.ftl")),
    ("en", include_str!("../../i18n/en/bunt-settings.ftl")),
];

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_startup(|_| load_style());

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
