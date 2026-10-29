// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 날짜와 큰 시각 (macOS 26의 로그인 창과 잠금 화면). 로그인 화면(login.rs)과 잠금 화면(bunt-lock)이 함께 쓴다

use std::rc::Rc;

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};

/// 날짜와 큰 시각. 분이 바뀔 때마다 고친다
pub fn clock(tr: &Rc<Translations>) -> gtk::Box {
    let date = gtk::Label::builder().css_classes(["greeter-date"]).build();
    let time = gtk::Label::builder().css_classes(["greeter-time"]).build();
    let clock = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .css_classes(["greeter-clock"])
        .build();
    clock.append(&date);
    clock.append(&time);
    let twelve_hour = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
        .is_none_or(|_| {
            gio::Settings::new("org.gnome.desktop.interface").string("clock-format") != "24h"
        });
    let formats = Rc::new((
        tr.get("clock-date-format"),
        tr.get(if twelve_hour {
            "clock-time-format-12h"
        } else {
            "clock-time-format-24h"
        }),
    ));
    tick(date.downgrade(), time.downgrade(), formats);
    clock
}

fn tick(
    date: glib::WeakRef<gtk::Label>,
    time: glib::WeakRef<gtk::Label>,
    formats: Rc<(String, String)>,
) {
    let (Some(date_label), Some(time_label)) = (date.upgrade(), time.upgrade()) else {
        return;
    };
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };
    if let Ok(text) = now.format(&formats.0) {
        date_label.set_text(&text);
    }
    if let Ok(text) = now.format(&formats.1) {
        time_label.set_text(&text);
    }
    let delay = 60u32.saturating_sub(now.second() as u32).max(1);
    glib::timeout_add_seconds_local_once(delay, move || tick(date, time, formats));
}
