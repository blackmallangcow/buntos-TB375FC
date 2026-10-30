// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴바: 왼쪽에 로고 메뉴(macOS의 Apple 메뉴 순서: 이 시스템에 관하여, 시스템 설정, 강제 종료, 잠자기,
//! 다시 시작, 시스템 종료, 화면 잠금, 로그아웃)와 앞에 있는 앱의 메뉴,
//! 오른쪽에 메뉴 막대 항목(입력 소스, Bluetooth, 사운드, 배터리, Wi-Fi), 제어 센터,
//! 날짜·시간(누르면 알림 센터). macOS처럼 평소에는 배경이 투명하고 글씨가 흰색이며,
//! 앞에 있는 창이 화면을 채우면(최대화, 전체 화면) 배경이 검은색이 된다. "메뉴 막대 배경 보기"를
//! 켜면 늘 흐린 유리 바탕이다. 자동으로 숨고 위쪽 가장자리에서 나오는 것은 컴포지터가 한다
//! (comp/src/panels.rs, 설정은 compositor.rs가 알린다).

use std::rc::Rc;

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::{
    appmenu, bluetooth, control_center,
    control_center::Services,
    input_source::{self, InputSources},
    interface_settings, layer_window, menu_names, network, notification_center,
    notifications::Notifications,
    power, session, shell_settings, sound, status_items,
    toplevels::Toplevels,
};

/// 시스템 설정 앱의 실행 파일. 인자는 처음 보여 줄 페이지 이름이다
const SETTINGS_COMMAND: &str = "bunt-settings";
/// 로고 메뉴의 아이콘 (shell/data/icons). 없으면 글자로 보인다
const LOGO_ICON: &str = "org.buntos.Logo-symbolic";
/// 셸 설정 키: 메뉴 막대 배경 보기
const BACKGROUND_KEY: &str = "menu-bar-background";

pub fn build(
    app: &gtk::Application,
    tr: &Rc<Translations>,
    toplevels: Option<&Toplevels>,
    notifications: &Notifications,
    services: &Services,
    input_sources: &InputSources,
) -> gtk::ApplicationWindow {
    let window = layer_window(app, "bunt-menubar");
    for edge in [Edge::Top, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.auto_exclusive_zone_enable();

    add_actions(app, tr);
    let start = gtk::Box::builder().spacing(2).build();
    start.append(&logo_menu(tr));
    if let Some(toplevels) = toplevels {
        start.append(&appmenu::build(tr, toplevels));
    }
    // 오른쪽 항목은 macOS와 같은 순서다: ... 배터리, Wi-Fi, 제어 센터, 시계
    let end = gtk::Box::builder().spacing(2).build();
    // 앱이 넣는 항목은 시스템 항목 왼쪽에 둔다
    end.append(&status_items::build());
    end.append(&control_center::focus_extra(tr));
    end.append(&input_source::extra(tr, input_sources));
    end.append(&bluetooth::extra(tr, &services.bluetooth));
    end.append(&sound::extra(tr, &services.sound));
    end.append(&power::extra(tr, &services.power));
    end.append(&network::extra(tr, &services.network));
    end.append(&control_center::extra(tr, services));
    end.append(&clock(tr, notifications));

    let bar = gtk::CenterBox::builder().css_classes(["bar"]).build();
    bar.set_start_widget(Some(&start));
    bar.set_end_widget(Some(&end));
    if let Some(toplevels) = toplevels {
        let bar = bar.clone();
        toplevels.subscribe(move |toplevels| {
            let fills_screen = toplevels
                .active()
                .is_some_and(|active| active.maximized || active.fullscreen);
            if fills_screen {
                bar.add_css_class("solid");
            } else {
                bar.remove_css_class("solid");
            }
        });
    }

    window.set_child(Some(&bar));
    if let Some(settings) = shell_settings() {
        follow_background(&settings, &bar);
    }
    window.present();
    window
}

/// 메뉴 막대 배경 보기를 켜면 흐린 유리 바탕에 화면 모드의 글씨색을 쓴다 (macOS 26과 같다)
fn follow_background(settings: &gio::Settings, bar: &gtk::CenterBox) {
    let apply = |settings: &gio::Settings, bar: &gtk::CenterBox| {
        if settings.boolean(BACKGROUND_KEY) {
            bar.add_css_class("background");
        } else {
            bar.remove_css_class("background");
        }
    };
    // 처리기가 설정 객체를 붙들어 셸이 끝날 때까지 둔다
    let (keep_alive, target) = (settings.clone(), bar.clone());
    settings.connect_changed(Some(BACKGROUND_KEY), move |settings, _| {
        let _ = &keep_alive;
        apply(settings, &target);
    });
    apply(settings, bar);
}

/// 로고 메뉴의 묶음과 항목(메시지 ID, 동작). macOS 26의 Apple 메뉴 순서와 묶음이다. 마지막 묶음 끝에
/// "<별칭> 로그아웃…"(LOG_OUT_ACTION)이 붙는다. 강제 종료, 잠자기, 화면 잠금, 로그아웃 동작은 session.rs가 단다
const LOGO_MENU: [&[(&str, &str)]; 5] = [
    &[("menu-about", "app.about")],
    &[("menu-settings", "app.settings")],
    &[("menu-force-quit", "app.force-quit")],
    &[
        ("menu-sleep", "app.sleep"),
        ("menu-restart", "app.restart"),
        ("menu-power-off", "app.power-off"),
    ],
    &[("menu-lock-screen", "app.lock-screen")],
];
const LOG_OUT_ACTION: &str = "app.log-out";

/// 로고 메뉴의 모델과, 로그아웃 항목이 든 마지막 묶음
fn logo_menu_model(tr: &Translations, log_out: &str) -> (gio::Menu, gio::Menu) {
    let menu = gio::Menu::new();
    let mut account = gio::Menu::new();
    for items in LOGO_MENU {
        account = gio::Menu::new();
        for (id, action) in items {
            account.append(Some(&tr.get(id)), Some(action));
        }
        menu.append_section(None, &account);
    }
    account.append(Some(log_out), Some(LOG_OUT_ACTION));
    (menu, account)
}

/// 로고 메뉴 단추
fn logo_menu(tr: &Rc<Translations>) -> gtk::MenuButton {
    let log_out = session::log_out_label(tr);
    let (menu, account) = logo_menu_model(tr, &log_out);

    // 아이콘이나 레이블을 자식으로 두면 펼침 화살표가 붙지 않는다
    let has_logo = gtk::gdk::Display::default()
        .is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(LOGO_ICON));
    let child: gtk::Widget = if has_logo {
        gtk::Image::from_icon_name(LOGO_ICON).upcast()
    } else {
        gtk::Label::new(Some(&tr.get("menubar-logo"))).upcast()
    };
    let button = gtk::MenuButton::builder()
        .child(&child)
        .menu_model(&menu)
        .tooltip_text(tr.get("menubar-logo"))
        .css_classes(["logo"])
        .build();
    // 항목이 접근성 트리에 이름 없이 menuitem으로만 나와 에이전트가 순서로만 골랐다(GTK 4.24의 GtkModelButton)
    if let Some(popover) = button.popover().and_downcast::<gtk::PopoverMenu>() {
        menu_names::follow(&popover);
    }
    // 메뉴를 열 때마다 "<별칭> 로그아웃…"의 별칭을 다시 읽는다 (시스템 설정의 사용자 및 그룹에서 바뀐다)
    let tr = tr.clone();
    let shown = std::cell::RefCell::new(log_out);
    button.connect_active_notify(move |button| {
        if !button.is_active() {
            return;
        }
        let label = session::log_out_label(&tr);
        if *shown.borrow() != label {
            account.remove(account.n_items() - 1);
            account.append(Some(&label), Some(LOG_OUT_ACTION));
            shown.replace(label);
        }
    });
    button
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

/// 시스템 설정 앱을 연다. page는 페이지 이름이다 (예: "about", "wifi", "battery").
/// 이미 떠 있으면 그 창이 그 페이지로 바뀐다
pub fn open_settings(page: Option<&str>) {
    let command = match page {
        Some(page) => format!("{SETTINGS_COMMAND} {page}"),
        None => SETTINGS_COMMAND.to_owned(),
    };
    let info = match gio::AppInfo::create_from_commandline(
        &command,
        Some(SETTINGS_COMMAND),
        gio::AppInfoCreateFlags::NONE,
    ) {
        Ok(info) => info,
        Err(err) => {
            eprintln!("bunt-shell: {command}: {err}");
            return;
        }
    };
    let context = gtk::gdk::Display::default().map(|display| display.app_launch_context());
    if let Err(err) = info.launch(&[], context.as_ref()) {
        eprintln!("bunt-shell: failed to start {command}: {err}");
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

/// 날짜와 시간. 누르면 알림 센터가 열린다
fn clock(tr: &Rc<Translations>, notifications: &Notifications) -> gtk::MenuButton {
    let label = gtk::Label::new(None);
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
    gtk::MenuButton::builder()
        .child(&label)
        .popover(&notification_center::popover(notifications, tr))
        .css_classes(["clock"])
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 메뉴 모델의 묶음마다 (레이블, 동작)
    fn sections(menu: &gio::Menu) -> Vec<Vec<(String, String)>> {
        let text = |model: &gio::MenuModel, item, attribute| {
            model
                .item_attribute_value(item, attribute, Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>())
                .unwrap_or_default()
        };
        (0..menu.n_items())
            .filter_map(|index| menu.item_link(index, gio::MENU_LINK_SECTION))
            .map(|section| {
                (0..section.n_items())
                    .map(|item| {
                        (
                            text(&section, item, gio::MENU_ATTRIBUTE_LABEL),
                            text(&section, item, gio::MENU_ATTRIBUTE_ACTION),
                        )
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_logo_menu_follows_the_apple_menu() {
        let (language, source) = crate::TRANSLATIONS[0];
        assert_eq!(language, "ko");
        let tr = Translations::from_source(language, source);
        let (menu, account) = logo_menu_model(&tr, "버니 로그아웃…");
        let expected: Vec<Vec<(&str, &str)>> = vec![
            vec![("이 시스템에 관하여", "app.about")],
            vec![("시스템 설정…", "app.settings")],
            vec![("강제 종료…", "app.force-quit")],
            vec![
                ("잠자기", "app.sleep"),
                ("다시 시작…", "app.restart"),
                ("시스템 종료…", "app.power-off"),
            ],
            vec![
                ("화면 잠금", "app.lock-screen"),
                ("버니 로그아웃…", "app.log-out"),
            ],
        ];
        let expected: Vec<Vec<(String, String)>> = expected
            .into_iter()
            .map(|items| {
                items
                    .into_iter()
                    .map(|(label, action)| (label.to_owned(), action.to_owned()))
                    .collect()
            })
            .collect();
        assert_eq!(sections(&menu), expected);
        // 별칭이 바뀌면 고치는 묶음은 마지막 묶음이다
        assert_eq!(account.n_items(), 2);
    }
}
