// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설정 화면의 틀: 페이지, 묶음(카드), 행, 알림. 모양은 data/style.css에 있다

use std::{cell::Cell, rc::Rc};

use gtk4::{self as gtk, glib, prelude::*};

/// 알림이 보이는 시간 (초)
const NOTICE_SECONDS: u32 = 4;

/// 제목 아래로 묶음을 쌓는 페이지. 스크롤되는 위젯과 묶음을 넣을 세로 상자를 돌려준다
pub fn page(title: &str) -> (gtk::ScrolledWindow, gtk::Box) {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .css_classes(["settings-page"])
        .build();
    column.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["settings-page-title"])
            .build(),
    );
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .child(&column)
        .build();
    (scroller, column)
}

/// 행을 담는 카드. 제목과 설명은 카드 위아래에 놓는다
pub fn group(column: &gtk::Box, title: Option<&str>, description: Option<&str>) -> gtk::ListBox {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    if let Some(title) = title {
        section.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .css_classes(["settings-group-title"])
                .build(),
        );
    }
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["settings-group"])
        .build();
    section.append(&list);
    if let Some(description) = description {
        section.append(&description_label(description));
    }
    column.append(&section);
    list
}

pub fn description_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["settings-description", "dim-label"])
        .build()
}

/// 왼쪽에 이름, 오른쪽 끝에 조작 위젯이 있는 행. 위젯이 남는 폭을 차지해 긴 값도 줄이 덜 바뀐다
pub fn row(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let content = gtk::Box::builder().spacing(12).build();
    content.append(&gtk::Label::builder().label(title).xalign(0.0).build());
    control.set_hexpand(true);
    control.set_halign(gtk::Align::End);
    control.set_valign(gtk::Align::Center);
    content.append(control);
    gtk::ListBoxRow::builder()
        .child(&content)
        .activatable(false)
        .build()
}

/// 읽기 전용 값을 보여 주는 행. 값은 선택해 복사할 수 있다
pub fn info_row(title: &str, value: &str) -> gtk::ListBoxRow {
    let value = gtk::Label::builder()
        .label(value)
        .selectable(true)
        .wrap(true)
        .xalign(1.0)
        .justify(gtk::Justification::Right)
        .max_width_chars(48)
        .css_classes(["dim-label"])
        .build();
    row(title, &value)
}

/// 창 아래쪽에 잠깐 보이는 알림
#[derive(Clone)]
pub struct Notice {
    revealer: gtk::Revealer,
    label: gtk::Label,
    /// 마지막 알림만 숨기도록 알림마다 번호를 매긴다
    generation: Rc<Cell<u32>>,
}

impl Notice {
    pub fn new() -> Self {
        let label = gtk::Label::builder()
            .wrap(true)
            .css_classes(["settings-notice"])
            .build();
        let revealer = gtk::Revealer::builder()
            .child(&label)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::End)
            .margin_bottom(24)
            .can_target(false)
            .build();
        Self {
            revealer,
            label,
            generation: Rc::new(Cell::new(0)),
        }
    }

    pub fn widget(&self) -> &gtk::Revealer {
        &self.revealer
    }

    pub fn show(&self, message: &str) {
        self.label.set_text(message);
        self.revealer.set_reveal_child(true);
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let notice = self.clone();
        glib::timeout_add_seconds_local_once(NOTICE_SECONDS, move || {
            if notice.generation.get() == generation {
                notice.revealer.set_reveal_child(false);
            }
        });
    }
}
