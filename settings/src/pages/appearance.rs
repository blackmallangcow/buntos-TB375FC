// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 모양: 밝게·어둡게. GNOME 공용 설정에 쓰므로 bunt-shell과 GTK 앱이 함께 바뀐다
//! (GTK 앱은 xdg-desktop-portal을 거쳐 읽는다).
//! 강조 색은 아직 넣지 않았다. 지금 쓰는 GTK 포털이 강조 색을 앱에 전하지 않기 때문이다

use std::rc::Rc;

use gtk4::{self as gtk, gio, prelude::*};

use super::{Context, interface_settings};
use crate::widgets::{description_label, group, page, row};

/// (color-scheme 값, 버튼 이름의 Fluent 메시지 ID).
/// 밝게는 "default"(앱이 고르는 기본값), 어둡게는 "prefer-dark"로 적는다. GNOME 설정과 같은 방식이다
const STYLES: [(&str, &str); 2] = [
    ("default", "appearance-style-light"),
    ("prefer-dark", "appearance-style-dark"),
];

pub fn build(context: &Rc<Context>) -> gtk::ScrolledWindow {
    let tr = &context.tr;
    let (scroller, column) = page(&tr.get("page-appearance"));
    let Some(settings) = interface_settings() else {
        column.append(&description_label(&tr.get("appearance-unavailable")));
        return scroller;
    };

    let list = group(&column, None, None);
    list.append(&row(
        &tr.get("appearance-style"),
        &style_switcher(context, &settings),
    ));
    scroller
}

/// 하나만 눌리는 버튼 묶음. 누른 버튼의 값을 color-scheme에 적는다
fn style_switcher(context: &Context, settings: &gio::Settings) -> gtk::Box {
    // prefer-light도 밝게로 보여 준다
    let dark = settings.string("color-scheme") == "prefer-dark";
    let linked = gtk::Box::builder().css_classes(["linked"]).build();
    let mut first: Option<gtk::ToggleButton> = None;
    for (value, label_id) in STYLES {
        let button = gtk::ToggleButton::builder()
            .label(context.tr.get(label_id))
            .active((value == "prefer-dark") == dark)
            .build();
        match &first {
            Some(first) => button.set_group(Some(first)),
            None => first = Some(button.clone()),
        }
        let settings = settings.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                let _ = settings.set_string("color-scheme", value);
            }
        });
        linked.append(&button);
    }
    linked
}
