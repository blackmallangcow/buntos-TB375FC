// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설정 화면의 틀: 페이지, 묶음, 행, 아이콘 배지, 대화상자, 알림.
//! 모양은 macOS 26 시스템 설정을 따르며 data/style.css와 theme/tokens.css에 있다

use std::{cell::Cell, rc::Rc};

use gtk4::{self as gtk, gio, glib, prelude::*};

/// 알림이 보이는 시간 (초)
const NOTICE_SECONDS: u32 = 4;

/// 아이콘 배지의 바탕색 (theme/tokens.css의 시스템 색)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tint {
    Blue,
    Green,
    Pink,
    Cyan,
    Indigo,
    Gray,
    Graphite,
}

impl Tint {
    fn class(self) -> &'static str {
        match self {
            Self::Blue => "tint-blue",
            Self::Green => "tint-green",
            Self::Pink => "tint-pink",
            Self::Cyan => "tint-cyan",
            Self::Indigo => "tint-indigo",
            Self::Gray => "tint-gray",
            Self::Graphite => "tint-graphite",
        }
    }
}

/// 배지 크기: 사이드바(20), 페이지 머리(32), 큰 그림(64)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeSize {
    Small,
    Medium,
    Large,
}

impl BadgeSize {
    fn pixels(self) -> (i32, i32) {
        // (배지, 기호)
        match self {
            Self::Small => (20, 14),
            Self::Medium => (32, 20),
            Self::Large => (64, 40),
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }
}

/// 색 있는 둥근 사각형 위의 흰 기호. macOS 설정의 아이콘 모양이다
pub fn badge(icon: &str, tint: Tint, size: BadgeSize) -> gtk::Box {
    let (outer, inner) = size.pixels();
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(inner);
    image.set_hexpand(true);
    image.set_vexpand(true);
    // 안의 기호가 늘어나도 배지는 제 크기를 지킨다
    let badge = gtk::Box::builder()
        .width_request(outer)
        .height_request(outer)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(false)
        .vexpand(false)
        .css_classes(["badge", size.class(), tint.class()])
        .build();
    badge.append(&image);
    badge
}

/// 스크롤되는 페이지와 묶음을 쌓을 세로 상자. 페이지 제목은 창 위쪽 막대에 보인다
pub fn page() -> (gtk::ScrolledWindow, gtk::Box) {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .css_classes(["settings-page"])
        .build();
    // 창이 넓어도 행이 너무 길어지지 않게 가운데에 둔다
    let clamp = gtk::Box::builder()
        .halign(gtk::Align::Fill)
        .hexpand(true)
        .build();
    column.set_hexpand(true);
    column.set_width_request(420);
    clamp.append(&column);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .child(&clamp)
        .build();
    (scroller, column)
}

/// 행을 담는 둥근 묶음. 제목은 위에, 설명은 아래에 놓는다
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

/// 묶음을 제목·설명과 함께 숨기거나 보인다
pub fn set_group_visible(list: &gtk::ListBox, visible: bool) {
    if let Some(section) = list.parent() {
        section.set_visible(visible);
    }
}

/// 묶음의 행을 모두 지운다
pub fn clear(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

pub fn description_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["settings-description"])
        .build()
}

/// 묶음 아래 오른쪽에 놓는 단추 줄 (예: "기타...")
pub fn footer(column: &gtk::Box) -> gtk::Box {
    let footer = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::End)
        .css_classes(["settings-footer"])
        .build();
    column.append(&footer);
    footer
}

/// 왼쪽에 (아이콘,) 제목과 설명, 오른쪽 끝에 부속을 놓는 행. 부속을 넣을 상자를 함께 돌려준다
pub fn rich_row(
    leading: Option<&gtk::Widget>,
    title: &str,
    subtitle: Option<&str>,
) -> (gtk::ListBoxRow, gtk::Box) {
    let below = subtitle.map(|text| subtitle_label(text).upcast::<gtk::Widget>());
    row_parts(leading, title, below.as_ref())
}

/// rich_row와 같고, 제목 아래에 글 대신 위젯(예: 상태 줄)을 놓는다
pub fn row_parts(
    leading: Option<&gtk::Widget>,
    title: &str,
    below: Option<&gtk::Widget>,
) -> (gtk::ListBoxRow, gtk::Box) {
    let content = gtk::Box::builder().spacing(10).build();
    if let Some(leading) = leading {
        leading.set_valign(gtk::Align::Center);
        content.append(leading);
    }
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    text.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["row-title"])
            .build(),
    );
    if let Some(below) = below {
        text.append(below);
    }
    content.append(&text);
    let accessories = gtk::Box::builder()
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();
    content.append(&accessories);
    let row = gtk::ListBoxRow::builder()
        .child(&content)
        .activatable(false)
        .build();
    (row, accessories)
}

pub fn subtitle_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["row-subtitle"])
        .build()
}

/// 왼쪽에 이름, 오른쪽 끝에 조작 위젯이 있는 행
pub fn row(title: &str, control: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let (row, accessories) = rich_row(None, title, None);
    accessories.append(control);
    row
}

/// 읽기 전용 값을 보여 주는 행. 값은 선택해 복사할 수 있다
pub fn info_row(title: &str, value: &str) -> gtk::ListBoxRow {
    let value = gtk::Label::builder()
        .label(value)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .xalign(1.0)
        .justify(gtk::Justification::Right)
        .max_width_chars(40)
        .css_classes(["row-value"])
        .build();
    let content = gtk::Box::builder().spacing(12).build();
    content.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .valign(gtk::Align::Start)
            .css_classes(["row-title"])
            .build(),
    );
    value.set_hexpand(true);
    value.set_halign(gtk::Align::End);
    value.set_valign(gtk::Align::Start);
    content.append(&value);
    gtk::ListBoxRow::builder()
        .child(&content)
        .activatable(false)
        .build()
}

/// 스위치 행
pub fn switch_row(title: &str, subtitle: Option<&str>) -> (gtk::ListBoxRow, gtk::Switch) {
    let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
    let (row, accessories) = rich_row(None, title, subtitle);
    accessories.append(&switch);
    (row, switch)
}

/// 페이지 맨 위 묶음의 머리 행: 배지, 제목, 설명, 오른쪽 조작(예: 켜고 끄는 스위치)
pub fn hero_row(
    badge: &gtk::Box,
    title: &str,
    description: &str,
    control: Option<&gtk::Widget>,
) -> gtk::ListBoxRow {
    let content = gtk::Box::builder().spacing(12).build();
    badge.set_valign(gtk::Align::Start);
    content.append(badge);
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    text.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["hero-title"])
            .build(),
    );
    text.append(
        &gtk::Label::builder()
            .label(description)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .css_classes(["hero-description"])
            .build(),
    );
    content.append(&text);
    if let Some(control) = control {
        control.set_valign(gtk::Align::Start);
        content.append(control);
    }
    gtk::ListBoxRow::builder()
        .child(&content)
        .activatable(false)
        .css_classes(["hero-row"])
        .build()
}

/// 상태 점의 색
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Good,
    Busy,
    Off,
}

/// "● 연결됨" 같은 상태 줄
pub fn status_line(status: Status, text: &str) -> gtk::Box {
    let dot = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes([
            "status-dot",
            match status {
                Status::Good => "good",
                Status::Busy => "busy",
                Status::Off => "off",
            },
        ])
        .build();
    let line = gtk::Box::builder().spacing(6).build();
    line.append(&dot);
    line.append(
        &gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .css_classes(["row-subtitle"])
            .build(),
    );
    line
}

/// 작은 기호 (자물쇠, 신호 세기 등)
pub fn symbol(icon: &str) -> gtk::Image {
    let image = gtk::Image::from_icon_name(icon);
    image.add_css_class("row-symbol");
    image
}

/// 원 안의 "⋯" 단추. 누르면 메뉴를 연다
pub fn more_button(menu: &gio::Menu) -> gtk::MenuButton {
    gtk::MenuButton::builder()
        .icon_name("view-more-horizontal-symbolic")
        .menu_model(menu)
        .valign(gtk::Align::Center)
        .css_classes(["more-button"])
        .build()
}

/// 기능을 쓸 수 없을 때 페이지 가운데에 보이는 안내
pub fn placeholder(icon: &str, tint: Tint, title: &str, description: &str) -> gtk::Box {
    let placeholder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .css_classes(["placeholder"])
        .build();
    let badge = badge(icon, tint, BadgeSize::Large);
    badge.set_margin_bottom(6);
    placeholder.append(&badge);
    placeholder.append(
        &gtk::Label::builder()
            .label(title)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .css_classes(["placeholder-title"])
            .build(),
    );
    placeholder.append(
        &gtk::Label::builder()
            .label(description)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .max_width_chars(48)
            .css_classes(["placeholder-description"])
            .build(),
    );
    placeholder
}

/// 작은 모달 창 (macOS의 시트). 배지와 제목·설명 아래에 내용, 맨 아래에 취소·확인 단추를 둔다.
/// 확인 단추가 기본 단추라 입력 칸에서 Enter를 누르면 눌린다
pub struct Sheet {
    pub window: gtk::Window,
    pub content: gtk::Box,
    pub confirm: gtk::Button,
}

impl Sheet {
    pub fn new(
        parent: Option<&gtk::Window>,
        badge: Option<&gtk::Box>,
        title: &str,
        description: Option<&str>,
        cancel_label: Option<&str>,
        confirm_label: &str,
    ) -> Self {
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(false)
            .default_width(420)
            .css_classes(["sheet"])
            .build();
        // 제목 표시줄 없이 내용만 보인다. 숨긴 위젯을 제목 표시줄로 두면 GTK가 기본 막대를 만들지 않는다
        let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        titlebar.set_visible(false);
        window.set_titlebar(Some(&titlebar));
        if let Some(parent) = parent {
            window.set_transient_for(Some(parent));
            // 앱에 묶어야 창 목록에 앱 ID(org.buntos.Settings)로 보인다 (메뉴바 앱 이름, Dock)
            window.set_application(parent.application().as_ref());
            if parent.has_css_class("dark") {
                window.add_css_class("dark");
            }
        }

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(14)
            .css_classes(["sheet-body"])
            .build();
        let header = gtk::Box::builder().spacing(14).build();
        if let Some(badge) = badge {
            badge.set_valign(gtk::Align::Start);
            header.append(badge);
        }
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .build();
        text.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .max_width_chars(40)
                .css_classes(["sheet-title"])
                .build(),
        );
        if let Some(description) = description {
            text.append(
                &gtk::Label::builder()
                    .label(description)
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .max_width_chars(48)
                    .css_classes(["sheet-description"])
                    .build(),
            );
        }
        header.append(&text);
        body.append(&header);

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        body.append(&content);

        let confirm = gtk::Button::builder()
            .label(confirm_label)
            .css_classes(["suggested-action"])
            .build();
        let buttons = gtk::Box::builder()
            .spacing(8)
            .halign(gtk::Align::End)
            .margin_top(6)
            .build();
        if let Some(cancel_label) = cancel_label {
            let cancel = gtk::Button::with_label(cancel_label);
            cancel.connect_clicked(glib::clone!(
                #[weak]
                window,
                move |_| window.close()
            ));
            buttons.append(&cancel);
        }
        buttons.append(&confirm);
        body.append(&buttons);
        window.set_child(Some(&body));
        window.set_default_widget(Some(&confirm));
        let escape = gtk::ShortcutController::new();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(|widget, _| {
                if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                    window.close();
                }
                glib::Propagation::Stop
            })),
        ));
        window.add_controller(escape);

        Self {
            window,
            content,
            confirm,
        }
    }

    /// 이름표가 붙은 입력 칸을 넣는다
    pub fn add_entry(&self, label: &str, password: bool) -> gtk::Entry {
        let entry = gtk::Entry::builder()
            .activates_default(true)
            .hexpand(true)
            .visibility(!password)
            .build();
        if password {
            entry.set_input_purpose(gtk::InputPurpose::Password);
        }
        let line = gtk::Box::builder().spacing(10).build();
        line.append(
            &gtk::Label::builder()
                .label(label)
                .xalign(1.0)
                .width_chars(8)
                .css_classes(["sheet-label"])
                .build(),
        );
        line.append(&entry);
        self.content.append(&line);
        entry
    }

    pub fn present(&self) {
        self.window.present();
    }
}

/// 위젯이 없어질 때까지 값(페이지 상태)을 붙든다
pub fn keep_alive<T: 'static>(widget: &impl IsA<gtk::Widget>, value: T) {
    widget.as_ref().connect_destroy(move |_| {
        let _ = &value;
    });
}

/// 위젯이 화면에 보이는 동안만 주기적으로 일을 한다 (예: Wi-Fi 검색, Bluetooth 기기 찾기).
/// 보이기 시작할 때 start, 가려질 때 stop을 부른다
pub fn while_mapped(
    widget: &impl IsA<gtk::Widget>,
    start: impl Fn() + 'static,
    stop: impl Fn() + 'static,
) {
    widget.as_ref().connect_map(move |_| start());
    widget.as_ref().connect_unmap(move |_| stop());
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
