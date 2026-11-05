// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 모든 앱 (macOS 26의 Spotlight 설정에 해당한다. ⌘스페이스로 여는 패널이 같다): 클립보드 기록을 켜고 끄기, 보관 기간(30분,
//! 8시간, 7일), 클립보드 기록 지우기. 기록은 셸의 메모리에만 있어 이 페이지는 설정(org.buntos.shell)만 바꾼다. 지우기도
//! 키 하나를 늘려 셸에 알린다. 설정 앱이 기록을 읽는 길은 없다

use std::rc::Rc;

use gtk4::{self as gtk, gio, prelude::*};

use super::{Context, bind_choice, bind_switch, has_key, shell_settings};
use crate::widgets::{self, BadgeSize, Sheet, Tint, badge};

const ICON: &str = "system-search-symbolic";
/// 드롭다운 항목 순서대로의 clipboard-history-period 값 (macOS의 차례)
const PERIODS: [&str; 3] = ["30m", "8h", "7d"];
const PERIOD_LABELS: [&str; 3] = [
    "launcher-clipboard-period-30m",
    "launcher-clipboard-period-8h",
    "launcher-clipboard-period-7d",
];
const ENABLED_KEY: &str = "clipboard-history";
const PERIOD_KEY: &str = "clipboard-history-period";
const CLEAR_KEY: &str = "clipboard-history-clear";

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Graphite, BadgeSize::Medium),
        &tr.get("page-launcher"),
        &tr.get("launcher-description"),
        None,
    ));

    match shell_settings() {
        Some(settings) => {
            let group = widgets::group(
                &column,
                Some(&tr.get("launcher-clipboard")),
                Some(&tr.get("launcher-clipboard-description")),
            );
            let (row, enabled) = widgets::switch_row(&tr.get("launcher-clipboard-history"), None);
            bind_switch(&settings, ENABLED_KEY, &enabled);
            group.append(&row);

            let labels = PERIOD_LABELS.map(|id| tr.get(id));
            let period = gtk::DropDown::from_strings(&labels.each_ref().map(String::as_str));
            period.set_valign(gtk::Align::Center);
            bind_choice(&settings, PERIOD_KEY, &period, &PERIODS);
            group.append(&widgets::row(&tr.get("launcher-clipboard-period"), &period));

            let footer = widgets::footer(&column);
            let clear = gtk::Button::with_label(&tr.get("launcher-clipboard-clear"));
            clear.set_sensitive(has_key(&settings, CLEAR_KEY));
            let page = context.clone();
            clear.connect_clicked(move |_| confirm_clear(&page, &settings));
            footer.append(&clear);
        }
        None => column.append(&widgets::placeholder(
            ICON,
            Tint::Gray,
            &tr.get("launcher-unavailable-title"),
            &tr.get("menu-bar-unavailable-description"),
        )),
    }
    scroller.upcast()
}

/// 지우기 전에 한 번 더 묻는다 (macOS와 같다)
fn confirm_clear(context: &Rc<Context>, settings: &gio::Settings) {
    let tr = &context.tr;
    let sheet = Sheet::new(
        context.window().as_ref(),
        Some(&badge(ICON, Tint::Graphite, BadgeSize::Medium)),
        &tr.get("launcher-clipboard-clear-title"),
        Some(&tr.get("launcher-clipboard-clear-description")),
        Some(&tr.get("cancel")),
        &tr.get("launcher-clipboard-clear-confirm"),
    );
    let (settings, window) = (settings.clone(), sheet.window.clone());
    sheet.confirm.connect_clicked(move |_| {
        let requests = settings.uint(CLEAR_KEY).wrapping_add(1);
        if let Err(err) = settings.set_uint(CLEAR_KEY, requests) {
            eprintln!("bunt-settings: {CLEAR_KEY}: {err}");
        }
        window.close();
    });
    sheet.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 드롭다운이 고르는 값이 셸 스키마의 선택지에 있다 (없으면 GSettings가 값을 받지 않는다)
    #[test]
    fn choices_match_the_shell_schema() {
        let schema = include_str!("../../../shell/data/org.buntos.shell.gschema.xml");
        for value in PERIODS {
            assert!(
                schema.contains(&format!("<choice value=\"{value}\"/>")),
                "{value}"
            );
        }
        for key in [ENABLED_KEY, PERIOD_KEY, CLEAR_KEY] {
            assert!(schema.contains(&format!("<key name=\"{key}\"")), "{key}");
        }
    }
}
