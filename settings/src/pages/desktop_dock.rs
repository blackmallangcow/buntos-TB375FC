// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 데스크탑 및 Dock (macOS 26과 같은 순서): Dock 크기, 투명도, 자동으로 Dock 가리기, 실행 중
//! 표시, 기본 웹 브라우저. bunt-shell이 같은 설정(org.buntos.shell)을 바로 따른다. 투명도는
//! macOS에 없고 buntOS에서 더했다. 자동으로 Dock 가리기는 macOS의 켜고 끄는 "자동으로 Dock
//! 가리기와 보기" 대신 숨는 때를 고른다. 기본 웹 브라우저는 기본 앱 페이지와 같은 값이다

use std::rc::Rc;

use gtk4::{self as gtk, prelude::*};

use super::{Context, Page, bind_choice, bind_scale, bind_switch, default_apps, shell_settings};
use crate::widgets::{self, BadgeSize, Tint, badge};

/// 셸이 설치하는 아이콘 (shell/data/icons)
const ICON: &str = "org.buntos.Dock-symbolic";
/// 드롭다운 항목 순서대로의 dock-autohide 값 (항상, 전체 화면일 때, 안 함)
const AUTOHIDE: [&str; 3] = ["always", "fullscreen", "never"];
const AUTOHIDE_LABELS: [&str; 3] = ["autohide-always", "autohide-fullscreen", "autohide-never"];
/// dock-icon-size의 범위 (셸 스키마)
const SIZE_RANGE: (f64, f64) = (32.0, 96.0);

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Graphite, BadgeSize::Medium),
        &tr.get("page-desktop-dock"),
        &tr.get("desktop-dock-description"),
        None,
    ));

    match shell_settings() {
        Some(settings) => {
            let dock = widgets::group(
                &column,
                Some(&tr.get("desktop-dock-dock")),
                Some(&tr.get("desktop-dock-autohide-description")),
            );
            let size = slider(SIZE_RANGE, 1.0);
            size.set_round_digits(0);
            bind_scale(&settings, "dock-icon-size", &size);
            dock.append(&widgets::slider_row(
                &tr.get("desktop-dock-size"),
                &size,
                &tr.get("desktop-dock-small"),
                &tr.get("desktop-dock-large"),
            ));
            let transparency = slider((0.0, 1.0), 0.05);
            bind_scale(&settings, "dock-transparency", &transparency);
            dock.append(&widgets::slider_row(
                &tr.get("desktop-dock-transparency"),
                &transparency,
                &tr.get("desktop-dock-opaque"),
                &tr.get("desktop-dock-clear"),
            ));
            let labels = AUTOHIDE_LABELS.map(|id| tr.get(id));
            let autohide = gtk::DropDown::from_strings(&labels.each_ref().map(String::as_str));
            autohide.set_valign(gtk::Align::Center);
            bind_choice(&settings, "dock-autohide", &autohide, &AUTOHIDE);
            dock.append(&widgets::row(&tr.get("desktop-dock-autohide"), &autohide));
            let (row, indicators) = widgets::switch_row(&tr.get("desktop-dock-indicators"), None);
            bind_switch(&settings, "dock-show-indicators", &indicators);
            dock.append(&row);
        }
        None => column.append(&widgets::placeholder(
            ICON,
            Tint::Gray,
            &tr.get("desktop-dock-unavailable-title"),
            &tr.get("menu-bar-unavailable-description"),
        )),
    }

    // macOS처럼 기본 웹 브라우저를 여기에 두고, 나머지 종류는 기본 앱 페이지로 보낸다
    if let Some(row) =
        default_apps::category_row(context, &default_apps::DEFAULT_WEB_BROWSER, false)
    {
        let browser = widgets::group(&column, None, None);
        browser.append(&row);
        let footer = widgets::footer(&column);
        let more = gtk::Button::with_label(&tr.get("desktop-dock-default-apps"));
        let page = context.clone();
        more.connect_clicked(move |_| page.open(Page::DefaultApps));
        footer.append(&more);
    }
    scroller.upcast()
}

fn slider((low, high): (f64, f64), step: f64) -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, low, high, step);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_width_request(220);
    scale
}
