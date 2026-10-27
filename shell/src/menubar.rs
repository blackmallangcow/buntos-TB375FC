// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴바: 로고 메뉴(시스템 설정, 다시 시작, 시스템 종료)와 시계

use std::rc::Rc;

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::{interface_settings, layer_window};

const SETTINGS_APP: &str = "org.buntos.Settings.desktop";

pub fn build(app: &gtk::Application, tr: &Rc<Translations>) -> gtk::ApplicationWindow {
    let window = layer_window(app, "bunt-menubar");
    for edge in [Edge::Top, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.auto_exclusive_zone_enable();

    add_actions(app, tr);
    let bar = gtk::CenterBox::builder().css_classes(["bar"]).build();
    bar.set_start_widget(Some(&logo_menu(tr)));
    bar.set_end_widget(Some(&clock(tr)));

    window.set_child(Some(&bar));
    window.present();
    window
}

fn logo_menu(tr: &Translations) -> gtk::MenuButton {
    let system = gio::Menu::new();
    system.append(Some(&tr.get("menu-about")), Some("app.about"));
    system.append(Some(&tr.get("menu-settings")), Some("app.settings"));
    let power = gio::Menu::new();
    power.append(Some(&tr.get("menu-restart")), Some("app.restart"));
    power.append(Some(&tr.get("menu-power-off")), Some("app.power-off"));
    let menu = gio::Menu::new();
    menu.append_section(None, &system);
    menu.append_section(None, &power);

    // 레이블을 자식으로 두면 펼침 화살표가 붙지 않는다
    let label = gtk::Label::new(Some(&tr.get("menubar-logo")));
    gtk::MenuButton::builder()
        .child(&label)
        .menu_model(&menu)
        .css_classes(["logo"])
        .build()
}

fn add_actions(app: &gtk::Application, tr: &Rc<Translations>) {
    let about = gio::ActionEntry::builder("about")
        .activate(|_, _, _| open_settings(Some("about")))
        .build();
    let settings = gio::ActionEntry::builder("settings")
        .activate(|_, _, _| open_settings(None))
        .build();
    let restart_tr = tr.clone();
    let restart = gio::ActionEntry::builder("restart")
        .activate(move |_, _, _| confirm_power(&restart_tr, Power::Restart))
        .build();
    let power_off_tr = tr.clone();
    let power_off = gio::ActionEntry::builder("power-off")
        .activate(move |_, _, _| confirm_power(&power_off_tr, Power::PowerOff))
        .build();
    app.add_action_entries([about, settings, restart, power_off]);
}

/// 시스템 설정 앱을 연다. page는 .desktop 파일의 동작 이름이다 (예: "about").
fn open_settings(page: Option<&str>) {
    let Some(info) = gio_unix::DesktopAppInfo::new(SETTINGS_APP) else {
        eprintln!("bunt-shell: {SETTINGS_APP} is not installed");
        return;
    };
    let context = gtk::gdk::Display::default().map(|display| display.app_launch_context());
    match page {
        Some(action) => info.launch_action(action, context.as_ref()),
        None => {
            if let Err(err) = info.launch(&[], context.as_ref()) {
                eprintln!("bunt-shell: failed to start {SETTINGS_APP}: {err}");
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Power {
    Restart,
    PowerOff,
}

impl Power {
    /// org.freedesktop.login1.Manager의 메서드
    fn method(self) -> &'static str {
        match self {
            Self::Restart => "Reboot",
            Self::PowerOff => "PowerOff",
        }
    }

    /// 확인 질문과 확인 버튼의 Fluent 메시지 ID
    fn messages(self) -> (&'static str, &'static str) {
        match self {
            Self::Restart => ("restart-confirm", "dialog-restart"),
            Self::PowerOff => ("power-off-confirm", "dialog-power-off"),
        }
    }
}

fn confirm_power(tr: &Translations, power: Power) {
    let (question, button) = power.messages();
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(tr.get(question))
        .buttons(vec![tr.get("dialog-cancel"), tr.get(button)])
        .cancel_button(0)
        .default_button(1)
        .build();
    dialog.choose(
        None::<&gtk::Window>,
        gio::Cancellable::NONE,
        move |choice| {
            if choice == Ok(1) {
                request_power(power);
            }
        },
    );
}

/// logind에 다시 시작이나 종료를 요청한다. 허용 여부는 polkit이 정한다.
fn request_power(power: Power) {
    let bus = match gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
        Ok(bus) => bus,
        Err(err) => {
            eprintln!("bunt-shell: no system bus: {err}");
            return;
        }
    };
    bus.call(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        power.method(),
        // interactive: polkit이 인증을 물을 수 있게 한다
        Some(&(true,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| {
            if let Err(err) = result {
                eprintln!("bunt-shell: {} failed: {err}", power.method());
            }
        },
    );
}

fn clock(tr: &Translations) -> gtk::Label {
    let label = gtk::Label::builder().css_classes(["clock"]).build();
    let formats = [
        tr.get("menubar-clock-format-12h"),
        tr.get("menubar-clock-format-24h"),
    ];
    let interface = interface_settings();
    // 매초 다시 읽으므로 시스템 설정에서 바꾼 시계 형식과 시간대가 바로 반영된다
    let update = move |label: &gtk::Label| {
        let use_24h = interface
            .as_ref()
            .is_some_and(|settings| settings.string("clock-format") == "24h");
        let format = &formats[usize::from(use_24h)];
        if let Ok(text) = glib::DateTime::now_local().and_then(|now| now.format(format)) {
            label.set_text(&text);
        }
    };
    update(&label);
    let ticking = label.clone();
    glib::timeout_add_seconds_local(1, move || {
        update(&ticking);
        glib::ControlFlow::Continue
    });
    label
}
