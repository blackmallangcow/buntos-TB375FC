// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대: 자동으로 가리기와 배경(macOS 26과 같은 이름과 순서), 메뉴 막대 오른쪽에 보일
//! 항목(macOS의 메뉴 막대 컨트롤)과 앱이 넣는 메뉴 막대 항목(StatusNotifierItem) 허용.
//! bunt-shell이 같은 설정(org.buntos.shell)을 바로 따른다. "최대화하거나 전체 화면일 때"는
//! macOS의 "데스크탑에서만" 대신 buntOS가 둔 값이다

use std::rc::Rc;

use gtk4::{self as gtk, prelude::*};

use super::{Context, bind_choice, bind_switch, shell_settings};
use crate::widgets::{self, BadgeSize, Tint, badge};

/// 드롭다운 항목 순서대로의 menu-bar-autohide 값 (macOS와 같은 순서)
const AUTOHIDE: [&str; 4] = ["always", "maximized", "fullscreen", "never"];
const AUTOHIDE_LABELS: [&str; 4] = [
    "autohide-always",
    "autohide-maximized",
    "autohide-fullscreen",
    "autohide-never",
];

/// (설정 키, 아이콘, 배지 색, 이름 메시지 ID)
const CONTROLS: [(&str, &str, Tint, &str); 5] = [
    (
        "show-wifi",
        "network-wireless-symbolic",
        Tint::Blue,
        "page-wifi",
    ),
    (
        "show-bluetooth",
        "bluetooth-active-symbolic",
        Tint::Blue,
        "page-bluetooth",
    ),
    (
        "show-sound",
        "audio-volume-high-symbolic",
        Tint::Pink,
        "menu-bar-sound",
    ),
    (
        "show-battery",
        "battery-full-symbolic",
        Tint::Green,
        "page-battery",
    ),
    (
        "show-input-source",
        "input-keyboard-symbolic",
        Tint::Gray,
        "menu-bar-input-source",
    ),
];

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let Some(settings) = shell_settings() else {
        column.append(&widgets::placeholder(
            "open-menu-symbolic",
            Tint::Gray,
            &tr.get("menu-bar-unavailable-title"),
            &tr.get("menu-bar-unavailable-description"),
        ));
        return scroller.upcast();
    };

    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge("open-menu-symbolic", Tint::Graphite, BadgeSize::Medium),
        &tr.get("page-menu-bar"),
        &tr.get("menu-bar-description"),
        None,
    ));

    let behavior = widgets::group(
        &column,
        None,
        Some(&tr.get("menu-bar-autohide-description")),
    );
    let labels = AUTOHIDE_LABELS.map(|id| tr.get(id));
    let autohide = gtk::DropDown::from_strings(&labels.each_ref().map(String::as_str));
    autohide.set_valign(gtk::Align::Center);
    bind_choice(&settings, "menu-bar-autohide", &autohide, &AUTOHIDE);
    behavior.append(&widgets::row(&tr.get("menu-bar-autohide"), &autohide));
    let (row, background) = widgets::switch_row(&tr.get("menu-bar-background"), None);
    bind_switch(&settings, "menu-bar-background", &background);
    behavior.append(&row);

    let controls = widgets::group(
        &column,
        Some(&tr.get("menu-bar-controls")),
        Some(&tr.get("menu-bar-controls-description")),
    );
    for (key, icon, tint, title) in CONTROLS {
        let leading = badge(icon, tint, BadgeSize::Small);
        let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let (row, accessories) =
            widgets::rich_row(Some(leading.upcast_ref()), &tr.get(title), None);
        accessories.append(&switch);
        bind_switch(&settings, key, &switch);
        controls.append(&row);
        if key == "show-battery" {
            // 배터리 아래에 남은 양 보기를 들여 놓는다
            let (row, percentage) = widgets::switch_row(&tr.get("battery-show-percentage"), None);
            row.add_css_class("indented-row");
            bind_switch(&settings, "show-battery-percentage", &percentage);
            switch
                .bind_property("active", &percentage, "sensitive")
                .sync_create()
                .build();
            controls.append(&row);
        }
    }

    let apps = widgets::group(
        &column,
        Some(&tr.get("menu-bar-apps")),
        Some(&tr.get("menu-bar-apps-description")),
    );
    let (row, switch) = widgets::switch_row(&tr.get("menu-bar-allow-apps"), None);
    bind_switch(&settings, "show-status-items", &switch);
    apps.append(&row);
    scroller.upcast()
}
