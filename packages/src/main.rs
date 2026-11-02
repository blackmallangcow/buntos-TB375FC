// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-packages: 패키지 매니저. 인터넷에서 받은 설치 파일(.deb, AppImage, Flatpak)을 터미널 없이 설치하고 지운다.
//! 파일을 두 번 누르면 macOS의 설치 프로그램 같은 설치 창이 뜨고, 앱을 그냥 열면 설치한 것의 목록이 뜬다(ARCHITECTURE
//! 5.5절). 시스템 설정처럼 GTK4와 자체 CSS로 만든다. 앱을 찾아 받는 일은 앱 스토어(GNOME Software)가 한다

mod appimage;
mod flatpak;
mod installed;
mod installer;
mod manager;
mod package;
mod packagekit;
mod ui;

use std::rc::Rc;

use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

const APP_ID: &str = "org.buntos.PackageManager";
/// 공통 디자인 토큰(theme/tokens.css)과 모든 앱의 창 단추(theme/gtk4-apps.css) 다음에 이 앱의 CSS
const STYLE: &str = concat!(
    include_str!("../../theme/tokens.css"),
    include_str!("../../theme/gtk4-apps.css"),
    include_str!("../data/style.css")
);
const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../i18n/ko/bunt-packages.ftl")),
    ("en", include_str!("../../i18n/en/bunt-packages.ftl")),
];

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    app.connect_startup(|app| {
        load_style();
        ui::follow_input_mode();
        // 메뉴바의 앱 메뉴("패키지 매니저 종료")가 D-Bus로 부르는 동작. 창을 닫아 끝낸다: 설치하는 창은 닫히지 않는다
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &gtk::Application, _, _| {
                for window in app.windows() {
                    window.close();
                }
            })
            .build();
        app.add_action_entries([quit]);
        app.set_accels_for_action("app.quit", &["<Control>q"]);
        app.set_accels_for_action("window.close", &["<Control>w"]);
    });

    let tr = Rc::new(Translations::load(TRANSLATIONS));
    // 앱을 그냥 열면 목록, 파일을 넘기면 파일마다 설치 창이다. 이미 떠 있으면 새로 실행한 쪽의 요청이 이쪽으로 온다
    let manager = Rc::new(manager::Manager::new(&tr));
    let on_activate = manager.clone();
    app.connect_activate(move |app| on_activate.present(app));
    let on_open = tr.clone();
    app.connect_open(move |app, files, _| {
        for file in files {
            installer::open(app, &on_open, file);
        }
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
