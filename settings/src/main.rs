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

use std::{cell::RefCell, ffi::OsString, path::PathBuf, rc::Rc};

use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    pages::{ClientChoice, Page},
    window::SettingsWindow,
};

const APP_ID: &str = "org.buntos.Settings";
/// 공통 디자인 토큰(theme/tokens.css)과 모든 앱의 창 단추(theme/gtk4-apps.css) 다음에 이 앱의 CSS.
/// 사용자 및 그룹, AI 에이전트 페이지와 키보드 단축키 시트의 CSS는 따로 둔다
const STYLE: &str = concat!(
    include_str!("../../theme/tokens.css"),
    include_str!("../../theme/gtk4-apps.css"),
    include_str!("../data/style.css"),
    include_str!("../data/users.css"),
    include_str!("../data/agent.css"),
    include_str!("../data/shortcuts.css")
);
/// 페이지가 많은 문자열은 따로 둔다 (사용자 및 그룹, AI 에이전트, 키보드 단축키)
const TRANSLATIONS: &Sources = &[
    (
        "ko",
        concat!(
            include_str!("../../i18n/ko/bunt-settings.ftl"),
            include_str!("../../i18n/ko/bunt-settings-users.ftl"),
            include_str!("../../i18n/ko/bunt-settings-agent.ftl"),
            include_str!("../../i18n/ko/bunt-settings-shortcuts.ftl"),
        ),
    ),
    (
        "en",
        concat!(
            include_str!("../../i18n/en/bunt-settings.ftl"),
            include_str!("../../i18n/en/bunt-settings-users.ftl"),
            include_str!("../../i18n/en/bunt-settings-agent.ftl"),
            include_str!("../../i18n/en/bunt-settings-shortcuts.ftl"),
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
        let arguments = Arguments::parse(&command_line.arguments());
        let mut window = window.borrow_mut();
        let window = window.get_or_insert_with(|| SettingsWindow::new(app, &tr));
        match arguments.client {
            Some(client) => window.choose_client(client),
            None => window.show(arguments.page),
        }
        glib::ExitCode::SUCCESS
    });
    app.run()
}

/// 명령줄: `bunt-settings [페이지] [--add-client <실행 파일> [--script <스크립트>]]`
/// (예: `bunt-settings about`). --add-client는 셸의 AI 접근 요청 창이 넘기며, AI 에이전트 페이지에서
/// 그 AI를 고른 채로 연다
#[derive(Debug, Default, PartialEq)]
struct Arguments {
    page: Option<Page>,
    client: Option<ClientChoice>,
}

impl Arguments {
    fn parse(arguments: &[OsString]) -> Self {
        let mut page = None;
        let mut exe = None;
        let mut script = None;
        let mut rest = arguments.iter().skip(1);
        while let Some(argument) = rest.next() {
            match argument.to_str() {
                Some("--add-client") => exe = rest.next().map(PathBuf::from),
                Some("--script") => script = rest.next().map(PathBuf::from),
                Some(name) => page = page.or(Page::from_name(name)),
                None => {}
            }
        }
        let client = exe
            .filter(|exe| bunt_access::valid_path(exe))
            .map(|exe| ClientChoice {
                exe,
                script: script.filter(|script| bunt_access::valid_path(script)),
            });
        Self { page, client }
    }
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

    #[test]
    fn reads_the_page_and_the_ai_to_choose() {
        let parse = |arguments: &[&str]| {
            let arguments: Vec<OsString> = arguments.iter().map(OsString::from).collect();
            Arguments::parse(&arguments)
        };
        assert_eq!(parse(&["bunt-settings"]), Arguments::default());
        assert_eq!(parse(&["bunt-settings", "about"]).page, Some(Page::About));
        let arguments = parse(&[
            "bunt-settings",
            "agent",
            "--add-client",
            "/usr/bin/node",
            "--script",
            "/opt/ai/cli.js",
        ]);
        assert_eq!(arguments.page, Some(Page::Agent));
        assert_eq!(
            arguments.client,
            Some(ClientChoice {
                exe: "/usr/bin/node".into(),
                script: Some("/opt/ai/cli.js".into()),
            })
        );
        // 상대 경로와 값이 빠진 옵션은 받지 않는다
        assert_eq!(
            parse(&["bunt-settings", "--add-client", "claude"]).client,
            None
        );
        assert_eq!(
            parse(&["bunt-settings", "agent", "--add-client"]).client,
            None
        );
    }
}
