// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 알림 센터와 알림 배너. 알림 센터는 메뉴바의 날짜와 시간을 누르면 열리며, 받은 알림과
//! 달력 위젯을 보인다. 새 알림은 화면 오른쪽 위에 배너로 잠깐 보인다.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{
    clip_input_to_child,
    control_center::do_not_disturb,
    notifications::{Change, Notification, Notifications},
};

/// 배너가 보이는 시간 (초)
const BANNER_SECONDS: u32 = 5;
/// 알림 카드의 너비. 배너와 알림 센터에서 같다
const CARD_WIDTH: i32 = 348;
/// 알림 센터 판의 너비: 카드와 좌우 안쪽 여백(style.css .notification-center의 10px, 카드 그림자 자리)
const PANEL_WIDTH: i32 = CARD_WIDTH + 2 * 10;
/// 배너 카드의 좌우 바깥 여백(style.css .notification-card.banner, 그림자 자리). GTK의 크기 요청은
/// 바깥 여백을 포함한다
const BANNER_MARGIN: i32 = 10;
/// 알림 목록이 이보다 길면 스크롤한다
const LIST_MAX_HEIGHT: i32 = 440;
/// 접은 카드에 보이는 제목과 본문의 줄 수. macOS처럼 넘치면 "…"로 자른다
const SUMMARY_LINES: i32 = 1;
const BODY_LINES: i32 = 2;
/// 펼친 카드의 제목과 본문의 줄 수 상한. 아주 긴 본문도 카드가 화면을 넘지 않는다. 줄 수를 두지 않으면
/// (-1) Pango가 말줄임하는 글을 한 줄로 자르므로 늘 줄 수를 준다
const EXPANDED_SUMMARY_LINES: i32 = 4;
const EXPANDED_BODY_LINES: i32 = 12;
/// 카드 안 글 칸의 너비: 카드에서 안쪽 여백(style.css의 --space-3, 양쪽), 아이콘(32px), 간격(10px)을 뺀 것
const TEXT_WIDTH: i32 = CARD_WIDTH - 2 * 12 - 32 - 10;

/// 알림 카드: 아이콘, 앱 이름, 시간, 제목, 본문, 동작 단추, 펼침·닫기 단추. 카드를 누르면 앱에 알리고
/// 치운다. 접은 카드는 제목 한 줄과 본문 두 줄까지라 긴 알림도 카드가 길어지지 않는다. 글이 넘치거나
/// 동작 단추가 있으면 펼침 단추가 보이고, 펼치면 글을 다 보이고 동작 단추를 꺼낸다(macOS와 같다)
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
    let (more, less) = (
        tr.get("notifications-expand"),
        tr.get("notifications-collapse"),
    );
    let expand = gtk::Button::builder()
        .icon_name("pan-down-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(&more)
        .visible(!notification.actions.is_empty())
        .css_classes(["card-expand"])
        .build();
    expand.update_property(&[gtk::accessible::Property::Label(&more)]);
    header.append(&expand);
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
    let line = |label: &str, lines: i32, class: &str| {
        gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(lines)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            // 줄바꿈하는 글의 자연 너비는 한 줄로 쓴 너비라, 내용에 크기를 맞추는 배너가 옆으로 늘었다.
            // 너비는 카드가 정한다
            .max_width_chars(1)
            .css_classes([class])
            .build()
    };
    let summary = (!notification.summary.is_empty())
        .then(|| line(&notification.summary, SUMMARY_LINES, "card-summary"));
    let body =
        (!notification.body.is_empty()).then(|| line(&notification.body, BODY_LINES, "card-body"));
    for label in summary.iter().chain(&body) {
        text.append(label);
    }
    // 동작 단추는 펼쳤을 때만 보인다
    let buttons = (!notification.actions.is_empty()).then(|| {
        let buttons = gtk::Box::builder()
            .spacing(6)
            .margin_top(4)
            .visible(false)
            .build();
        for (key, label) in &notification.actions {
            let button = gtk::Button::with_label(label);
            let (invoke, key) = (notifications.clone(), key.clone());
            button.connect_clicked(move |_| invoke.invoke_action(id, &key));
            buttons.append(&button);
        }
        text.append(&buttons);
        buttons
    });

    let expanded = Cell::new(false);
    {
        let (summary, body, buttons) = (summary.clone(), body.clone(), buttons.clone());
        expand.connect_clicked(move |expand| {
            let open = !expanded.get();
            expanded.set(open);
            if let Some(summary) = &summary {
                summary.set_lines(if open {
                    EXPANDED_SUMMARY_LINES
                } else {
                    SUMMARY_LINES
                });
            }
            if let Some(body) = &body {
                body.set_lines(if open {
                    EXPANDED_BODY_LINES
                } else {
                    BODY_LINES
                });
            }
            if let Some(buttons) = &buttons {
                buttons.set_visible(open);
            }
            let name = if open { &less } else { &more };
            expand.set_icon_name(if open {
                "pan-up-symbolic"
            } else {
                "pan-down-symbolic"
            });
            expand.set_tooltip_text(Some(name));
            expand.update_property(&[gtk::accessible::Property::Label(name)]);
        });
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
    // 글이 넘치는지는 카드가 창에 들어가 글꼴(CSS)이 정해진 뒤에 잰다
    card.connect_realize(move |_| {
        let cut = [(&summary, SUMMARY_LINES), (&body, BODY_LINES)]
            .into_iter()
            .any(|(label, lines)| label.as_ref().is_some_and(|label| overflows(label, lines)));
        if cut {
            expand.set_visible(true);
        }
    });
    card
}

/// 글이 `lines`줄에서 잘리는가: 펼친 줄 수로 잰 높이가 더 크다
fn overflows(label: &gtk::Label, lines: i32) -> bool {
    let height = |label: &gtk::Label| label.measure(gtk::Orientation::Vertical, TEXT_WIDTH).1;
    label.set_lines(EXPANDED_BODY_LINES);
    let full = height(label);
    label.set_lines(lines);
    full > height(label)
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
    // 창을 카드에 맞춘다. 크기를 바꿀 수 있는 창이면 gtk4-layer-shell이 처음 받은 크기를 계속 써서
    // 배너가 화면 너비만큼 늘어났다. 카드 둘레의 여백(style.css .banner)이 그림자 자리이자 메뉴 막대
    // 아래 8px의 틈이다. 레이어 여백은 메뉴 막대의 배타 영역 다음부터 재므로 두지 않는다
    window.set_resizable(false);
    clip_input_to_child(&window);

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
        content.set_width_request(CARD_WIDTH + 2 * BANNER_MARGIN);
        content.add_css_class("banner");
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
