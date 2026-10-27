// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 날짜와 시간: 시간대와 자동 시간 맞추기는 timedated로, 시계 형식은 GNOME 공용 설정으로 바꾼다

use std::rc::Rc;

use gtk4::{self as gtk, gio, glib, prelude::*};

use super::{Context, interface_settings};
use crate::{
    system::TIMEDATE,
    widgets::{group, page, row},
};

pub fn build(context: &Rc<Context>) -> gtk::ScrolledWindow {
    let tr = &context.tr;
    let (scroller, column) = page(&tr.get("page-datetime"));
    let list = group(&column, None, None);
    list.append(&automatic_row(context));
    list.append(&row(
        &tr.get("datetime-timezone"),
        &timezone_choice(context),
    ));
    if let Some(settings) = interface_settings() {
        list.append(&row(
            &tr.get("datetime-24h"),
            &clock_format_switch(&settings),
        ));
    }
    scroller
}

/// 시간대 목록과 지금 시간대는 timedated에서 받아 온다. 목록에서 이름으로 찾을 수 있다
fn timezone_choice(context: &Rc<Context>) -> gtk::DropDown {
    let expression = gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    );
    let dropdown = gtk::DropDown::builder()
        .expression(&expression)
        .enable_search(true)
        .search_match_mode(gtk::StringFilterMatchMode::Substring)
        .sensitive(false)
        .build();

    let context = context.clone();
    glib::spawn_future_local(glib::clone!(
        #[weak]
        dropdown,
        async move {
            let zones = match TIMEDATE.call("ListTimezones", &().to_variant()).await {
                Ok(reply) => reply
                    .child_value(0)
                    .get::<Vec<String>>()
                    .unwrap_or_default(),
                Err(err) => {
                    context.notify_error("datetime-timezone-error", err.message());
                    return;
                }
            };
            let current = TIMEDATE
                .property("Timezone")
                .await
                .ok()
                .and_then(|value| value.get::<String>());
            let names: Vec<&str> = zones.iter().map(String::as_str).collect();
            dropdown.set_model(Some(&gtk::StringList::new(&names)));
            if let Some(index) = zones.iter().position(|zone| Some(zone) == current.as_ref()) {
                dropdown.set_selected(index as u32);
            }
            dropdown.set_sensitive(true);

            // 목록을 채운 뒤에 연결해야 처음 고른 항목으로 다시 바꾸지 않는다
            dropdown.connect_selected_notify(move |dropdown| {
                let Some(zone) = dropdown.selected_item().and_downcast::<gtk::StringObject>()
                else {
                    return;
                };
                let context = context.clone();
                glib::spawn_future_local(async move {
                    let args = (zone.string().as_str(), false).to_variant();
                    if let Err(err) = TIMEDATE.call("SetTimezone", &args).await {
                        context.notify_error("datetime-timezone-error", err.message());
                    }
                });
            });
        }
    ));
    dropdown
}

/// 시간 동기화 서비스(systemd-timesyncd)가 있을 때만 보인다.
fn automatic_row(context: &Rc<Context>) -> gtk::ListBoxRow {
    let switch = gtk::Switch::new();
    let automatic = row(&context.tr.get("datetime-automatic"), &switch);
    automatic.set_visible(false);

    let context = context.clone();
    glib::spawn_future_local(glib::clone!(
        #[weak]
        automatic,
        #[weak]
        switch,
        async move {
            let flag = |name| async move {
                TIMEDATE
                    .property(name)
                    .await
                    .ok()
                    .and_then(|value| value.get::<bool>())
            };
            if flag("CanNTP").await != Some(true) {
                return;
            }
            switch.set_active(flag("NTP").await == Some(true));
            automatic.set_visible(true);
            switch.connect_active_notify(move |switch| {
                let context = context.clone();
                let args = (switch.is_active(), false).to_variant();
                glib::spawn_future_local(async move {
                    if let Err(err) = TIMEDATE.call("SetNTP", &args).await {
                        context.notify_error("datetime-automatic-error", err.message());
                    }
                });
            });
        }
    ));
    automatic
}

fn clock_format_switch(settings: &gio::Settings) -> gtk::Switch {
    let switch = gtk::Switch::new();
    settings
        .bind("clock-format", &switch, "active")
        .mapping(|value, _| Some((value.str() == Some("24h")).to_value()))
        .set_mapping(|value, _| {
            let use_24h = value.get::<bool>().ok()?;
            Some(if use_24h { "24h" } else { "12h" }.to_variant())
        })
        .build();
    switch
}
