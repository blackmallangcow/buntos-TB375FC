// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 모드: 밝게·어둡게. GNOME 공용 설정에 쓰므로 bunt-shell과 GTK 앱이 함께 바뀐다
//! (GTK 앱은 xdg-desktop-portal을 거쳐 읽는다).
//! 강조 색은 아직 넣지 않았다. 지금 쓰는 GTK 포털이 강조 색을 앱에 전하지 않기 때문이다

use std::rc::Rc;

use gtk4::{self as gtk, gio, prelude::*};

use super::{Context, interface_settings};
use crate::widgets::{description_label, group, page};

/// (color-scheme 값, 이름의 Fluent 메시지 ID, 미리보기 CSS 클래스).
/// 밝게는 "default"(앱이 고르는 기본값), 어둡게는 "prefer-dark"로 적는다. GNOME 설정과 같은 방식이다
const STYLES: [(&str, &str, &str); 2] = [
    ("default", "appearance-style-light", "preview-light"),
    ("prefer-dark", "appearance-style-dark", "preview-dark"),
];

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = page();
    let Some(settings) = interface_settings() else {
        column.append(&description_label(&tr.get("appearance-unavailable")));
        return scroller.upcast();
    };

    let list = group(&column, None, Some(&tr.get("appearance-description")));
    let content = gtk::Box::builder().spacing(12).build();
    content.append(
        &gtk::Label::builder()
            .label(tr.get("appearance-style"))
            .xalign(0.0)
            .valign(gtk::Align::Start)
            .hexpand(true)
            .css_classes(["row-title"])
            .build(),
    );
    content.append(&style_choices(context, &settings));
    list.append(
        &gtk::ListBoxRow::builder()
            .child(&content)
            .activatable(false)
            .build(),
    );
    scroller.upcast()
}

/// 밝게·어둡게 미리보기 그림. 누른 쪽의 값을 color-scheme에 적는다
fn style_choices(context: &Context, settings: &gio::Settings) -> gtk::Box {
    let choices = gtk::Box::builder().spacing(16).build();
    let mut first: Option<gtk::ToggleButton> = None;
    // prefer-light도 밝게로 보여 준다
    let dark = settings.string("color-scheme") == "prefer-dark";
    for (value, label_id, preview_class) in STYLES {
        let preview = gtk::Box::builder()
            .width_request(96)
            .height_request(64)
            .hexpand(false)
            .css_classes(["appearance-preview", preview_class])
            .build();
        // 미리보기 안의 작은 창
        preview.append(
            &gtk::Box::builder()
                .hexpand(true)
                .vexpand(true)
                .css_classes(["preview-window"])
                .build(),
        );
        let button = gtk::ToggleButton::builder()
            .child(&preview)
            .active((value == "prefer-dark") == dark)
            .css_classes(["appearance-choice"])
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
        let choice = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        choice.append(&button);
        choice.append(
            &gtk::Label::builder()
                .label(context.tr.get(label_id))
                .css_classes(["choice-label"])
                .build(),
        );
        choices.append(&choice);
    }
    choices
}
