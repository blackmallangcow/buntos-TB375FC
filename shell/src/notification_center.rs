// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 알림 센터와 알림 배너. 알림 센터는 메뉴바의 날짜와 시간을 누르면 열리며, 받은 알림과
//! 달력 위젯을 보인다. 새 알림은 화면 오른쪽 위에 배너로 잠깐 보인다.

use std::{cell::RefCell, rc::Rc};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{
    control_center::do_not_disturb,
    notifications::{Change, Notification, Notifications},
};

/// 배너가 보이는 시간 (초)
const BANNER_SECONDS: u32 = 5;
const PANEL_WIDTH: i32 = 360;
/// 알림 목록이 이보다 길면 스크롤한다
const LIST_MAX_HEIGHT: i32 = 440;

/// 알림 카드: 아이콘, 앱 이름, 시간, 제목, 본문, 동작 단추, 닫기 단추.
/// 카드를 누르면 앱에 알리고 치운다.
fn card(notification: &Notification, notifications: &Notifications, tr: &Translations) -> gtk::Box {
    let id = notification.id;
    let icon = match &notification.icon {
        Some(icon) => gtk::Image::from_gicon(icon),
        None => gtk::Image::from_icon_name("dialog-information-symbolic"),
    };
    icon.set_pixel_size(32);
    icon.set_valign(gtk::Align::Start);

    let header = gtk::Box::builder().spacing(6).build();
    header.append(
        &gtk::Label::builder()
            .label(&notification.app_name)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["card-app"])
            .build(),
    );
    header.append(
        &gtk::Label::builder()
            .label(relative_time(&notification.time, tr))
            .css_classes(["card-time"])
            .build(),
    );
    let close = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["card-close"])
        .build();
    let dismiss = notifications.clone();
    close.connect_clicked(move |_| dismiss.dismiss(id));
    header.append(&close);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .build();
    text.append(&header);
    if !notification.summary.is_empty() {
        text.append(
            &gtk::Label::builder()
                .label(&notification.summary)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .css_classes(["card-summary"])
                .build(),
        );
    }
    if !notification.body.is_empty() {
        text.append(
            &gtk::Label::builder()
                .label(&notification.body)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .lines(4)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["card-body"])
                .build(),
        );
    }
    if !notification.actions.is_empty() {
        let buttons = gtk::Box::builder().spacing(6).margin_top(4).build();
        for (key, label) in &notification.actions {
            let button = gtk::Button::with_label(label);
            let (invoke, key) = (notifications.clone(), key.clone());
            button.connect_clicked(move |_| invoke.invoke_action(id, &key));
            buttons.append(&button);
        }
        text.append(&buttons);
    }

    let card = gtk::Box::builder()
        .spacing(10)
        .css_classes(["notification-card"])
        .build();
    card.append(&icon);
    card.append(&text);
    let click = gtk::GestureClick::new();
    let activate = notifications.clone();
    click.connect_released(move |_, _, _, _| activate.activate(id));
    card.add_controller(click);
    card
}

/// "지금", "5분 전", "3시간 전", 하루가 지나면 날짜
fn relative_time(time: &glib::DateTime, tr: &Translations) -> String {
    let Ok(now) = glib::DateTime::now_local() else {
        return String::new();
    };
    let minutes = now.difference(time).as_minutes();
    let mut args = FluentArgs::new();
    match minutes {
        ..1 => tr.get("notifications-now"),
        1..60 => {
            args.set("minutes", minutes);
            tr.get_with("notifications-minutes-ago", &args)
        }
        60..1440 => {
            args.set("hours", minutes / 60);
            tr.get_with("notifications-hours-ago", &args)
        }
        _ => time
            .format(&tr.get("notifications-date-format"))
            .map(|text| text.to_string())
            .unwrap_or_default(),
    }
}

/// 오늘 날짜와 이번 달 달력
fn calendar_widget() -> gtk::Box {
    let today = glib::DateTime::now_local().ok();
    let format = |pattern: &str| {
        today
            .as_ref()
            .and_then(|now| now.format(pattern).ok())
            .map(|text| text.to_string())
            .unwrap_or_default()
    };
    let date = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Start)
        .build();
    date.append(
        &gtk::Label::builder()
            .label(format("%A"))
            .xalign(0.0)
            .css_classes(["widget-weekday"])
            .build(),
    );
    date.append(
        &gtk::Label::builder()
            .label(format("%-d"))
            .xalign(0.0)
            .css_classes(["widget-day"])
            .build(),
    );
    let widget = gtk::Box::builder()
        .spacing(12)
        .css_classes(["notification-card", "calendar-widget"])
        .build();
    widget.append(&date);
    widget.append(&gtk::Calendar::builder().hexpand(true).build());
    widget
}

/// 메뉴바의 날짜와 시간 단추에 붙는 알림 센터
pub fn popover(notifications: &Notifications, tr: &Rc<Translations>) -> gtk::Popover {
    let clear = gtk::Button::builder()
        .label(tr.get("notifications-clear"))
        .css_classes(["flat", "center-clear"])
        .build();
    let cleared = notifications.clone();
    clear.connect_clicked(move |_| cleared.clear());
    let header = gtk::Box::builder().build();
    header.append(
        &gtk::Label::builder()
            .label(tr.get("notifications-title"))
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["center-title"])
            .build(),
    );
    header.append(&clear);

    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&list)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(LIST_MAX_HEIGHT)
        .build();

    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .width_request(PANEL_WIDTH)
        .css_classes(["notification-center"])
        .build();
    panel.append(&header);
    panel.append(&scroller);
    let calendar = RefCell::new(calendar_widget());
    panel.append(&*calendar.borrow());

    let popover = gtk::Popover::builder()
        .child(&panel)
        .has_arrow(false)
        .build();
    // 바탕은 style.css에서 지운다 (카드만 떠 보이게)
    popover.add_css_class("notification-center-popover");

    let fill = {
        let (notifications, tr, panel) = (notifications.clone(), tr.clone(), panel.clone());
        move || {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let items = notifications.items();
            clear.set_visible(!items.is_empty());
            if items.is_empty() {
                list.append(
                    &gtk::Label::builder()
                        .label(tr.get("notifications-empty"))
                        .css_classes(["center-empty"])
                        .build(),
                );
            }
            for item in &items {
                list.append(&card(item, &notifications, &tr));
            }
            // 날짜가 바뀌었을 수 있으므로 달력도 새로 만든다
            let fresh = calendar_widget();
            panel.remove(&*calendar.borrow());
            panel.append(&fresh);
            calendar.replace(fresh);
        }
    };
    let fill = Rc::new(fill);
    let on_change = fill.clone();
    notifications.subscribe(move |_, _| on_change());
    // 열 때마다 "5분 전" 같은 시간을 새로 쓴다
    popover.connect_show(move |_| fill());
    popover
}

/// 새 알림을 화면 오른쪽 위에 잠깐 보이는 창
pub fn banner(
    app: &gtk::Application,
    notifications: &Notifications,
    tr: &Rc<Translations>,
) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .css_classes(["bunt-banner"])
        .build();
    window.init_layer_shell();
    window.set_namespace(Some("bunt-banner"));
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    // 메뉴바 바로 아래
    window.set_margin(Edge::Top, 36);
    window.set_margin(Edge::Right, 8);

    // 배너마다 번호를 매겨 마지막 배너만 숨긴다
    let shown = Rc::new(std::cell::Cell::new(0u32));
    let tr = tr.clone();
    let banner = window.clone();
    notifications.subscribe(move |notifications, change| {
        let Change::Added(id) = change else {
            // 보이던 알림이 치워지면 배너도 내린다
            if notifications.get(shown.get()).is_none() {
                window.set_visible(false);
            }
            return;
        };
        let Some(notification) = notifications.get(*id) else {
            return;
        };
        // 방해 금지 중에는 배너 없이 알림 센터에만 쌓는다
        if do_not_disturb() {
            return;
        }
        let content = card(&notification, notifications, &tr);
        content.set_width_request(PANEL_WIDTH);
        window.set_child(Some(&content));
        window.present();
        shown.set(*id);
        let (window, shown, id) = (window.clone(), shown.clone(), *id);
        glib::timeout_add_seconds_local_once(BANNER_SECONDS, move || {
            if shown.get() == id {
                window.set_visible(false);
            }
        });
    });
    banner
}

#[cfg(test)]
mod tests {
    use super::*;

    const KO: &str = include_str!("../../i18n/ko/bunt-shell.ftl");

    #[test]
    fn recent_times_are_relative() {
        let tr = Translations::from_source("ko", KO);
        let now = glib::DateTime::now_local().unwrap();
        assert_eq!(relative_time(&now, &tr), "지금");
        let earlier = now.add_minutes(-5).unwrap();
        assert_eq!(relative_time(&earlier, &tr), "5분 전");
        let hours = now.add_hours(-3).unwrap();
        assert_eq!(relative_time(&hours, &tr), "3시간 전");
    }
}
