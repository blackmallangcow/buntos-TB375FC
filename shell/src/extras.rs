// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대 오른쪽 항목(Apple HIG의 menu bar extra)의 공통 모양. 항목은 메뉴 막대의 아이콘
//! 단추와, 누르면 열리는 유리 메뉴로 되어 있다. 메뉴는 열 때마다 내용을 새로 그린다.
//! 어느 항목을 메뉴 막대에 보일지는 사용자가 시스템 설정의 메뉴 막대 페이지에서 정한다
//! (HIG: "Let people — not your app — decide").

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk4::{self as gtk, gio, prelude::*};

use crate::shell_settings;

/// 상태가 바뀔 때 부르는 처리기들 (Network, Power 같은 상태 객체가 쓴다)
pub type Listeners<T> = std::cell::RefCell<Vec<Box<dyn Fn(&T)>>>;

/// 메뉴 막대 항목의 메뉴. 복제해도 같은 메뉴를 가리킨다
#[derive(Clone)]
pub struct Menu {
    popover: gtk::Popover,
    content: gtk::Box,
    /// 열기 직전에 내용을 그리는 함수 (on_show)
    fill: Rc<RefCell<Option<Fill>>>,
}

type Fill = Rc<dyn Fn(&Menu)>;

/// 메뉴의 한 줄: 동그란 아이콘, 이름, 설명, 오른쪽 아이콘들 (Wi-Fi 네트워크, Bluetooth 기기)
pub struct Row<'a> {
    pub icon: &'a str,
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    /// 켜져 있거나 연결된 것. 아이콘 동그라미를 강조색으로 채운다
    pub active: bool,
    pub trailing: &'a [&'a str],
}

impl Menu {
    pub fn new() -> Self {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["extra-menu"])
            .build();
        let popover = gtk::Popover::builder()
            .child(&content)
            .has_arrow(false)
            .build();
        // css_classes로 정하면 기본 클래스(background)가 빠진다
        popover.add_css_class("extra-popover");
        follow_pointer(&content);
        Self {
            popover,
            content,
            fill: Rc::default(),
        }
    }

    pub fn popover(&self) -> &gtk::Popover {
        &self.popover
    }

    /// 열 때마다 fill로 내용을 다시 그린다. 팝오버가 뜨기 전에(메뉴 막대 단추의 create-popup)
    /// 그린다. 뜬 뒤에 그리면 크기가 바뀌어 팝오버가 다시 배치되고, 그 사이 GTK가 옛 위치로
    /// 포인터를 계산해 아래쪽 항목이 가리킨 것처럼 칠해진다
    pub fn on_show(&self, fill: impl Fn(&Menu) + 'static) {
        self.fill.replace(Some(Rc::new(fill)));
    }

    fn refill(&self) {
        self.content.remove_css_class("pointer-moved");
        let fill = self.fill.borrow().clone();
        if let Some(fill) = fill {
            self.clear();
            fill(self);
        }
    }

    pub fn clear(&self) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
    }

    pub fn append(&self, widget: &impl IsA<gtk::Widget>) {
        self.content.append(widget);
    }

    /// 굵은 제목. 오른쪽에 스위치를 둘 수 있게 줄을 돌려준다
    pub fn title(&self, text: &str) -> gtk::Box {
        let row = gtk::Box::builder()
            .spacing(8)
            .css_classes(["extra-title"])
            .build();
        row.append(
            &gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        self.content.append(&row);
        row
    }

    /// 제목과 켜고 끄는 스위치 (Wi-Fi, Bluetooth)
    pub fn title_switch(&self, text: &str, active: bool, toggled: impl Fn(bool) + 'static) {
        let row = self.title(text);
        let switch = gtk::Switch::builder()
            .active(active)
            .valign(gtk::Align::Center)
            .build();
        switch.connect_state_set(move |_, on| {
            toggled(on);
            gtk4::glib::Propagation::Proceed
        });
        row.append(&switch);
    }

    /// 묶음 머리글 (작고 흐린 글씨)
    pub fn section(&self, text: &str) {
        self.content.append(
            &gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .css_classes(["extra-section"])
                .build(),
        );
    }

    /// 설명 글 (상태, 남은 시간)
    pub fn text(&self, text: &str) {
        self.content.append(
            &gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["extra-text"])
                .build(),
        );
    }

    pub fn separator(&self) {
        self.content.append(
            &gtk::Separator::builder()
                .css_classes(["extra-separator"])
                .build(),
        );
    }

    /// 누르면 메뉴를 닫고 activate를 부르는 단추. 메뉴에 붙이지는 않는다
    fn row_button(
        &self,
        child: &impl IsA<gtk::Widget>,
        activate: impl Fn() + 'static,
    ) -> gtk::Button {
        let button = gtk::Button::builder()
            .child(child)
            .css_classes(["extra-row"])
            .build();
        let popover = self.popover.clone();
        button.connect_clicked(move |_| {
            popover.popdown();
            activate();
        });
        button
    }

    fn button(&self, child: &impl IsA<gtk::Widget>, activate: impl Fn() + 'static) -> gtk::Button {
        let button = self.row_button(child, activate);
        self.content.append(&button);
        button
    }

    /// 한 줄 단추를 만든다. 메뉴에 붙이지는 않으므로 나중에 채우는 목록 상자에 넣는다
    pub fn item_button(&self, row: Row, activate: impl Fn() + 'static) -> gtk::Button {
        let icon = gtk::Image::from_icon_name(row.icon);
        icon.add_css_class("row-icon");
        if row.active {
            icon.add_css_class("active");
        }
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        text.append(
            &gtk::Label::builder()
                .label(row.title)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(28)
                .build(),
        );
        if let Some(subtitle) = row.subtitle {
            text.append(
                &gtk::Label::builder()
                    .label(subtitle)
                    .xalign(0.0)
                    .css_classes(["extra-subtitle"])
                    .build(),
            );
        }
        let content = gtk::Box::builder().spacing(10).build();
        content.append(&icon);
        content.append(&text);
        for name in row.trailing {
            let image = gtk::Image::from_icon_name(name);
            image.add_css_class("row-trailing");
            content.append(&image);
        }
        self.row_button(&content, activate)
    }

    /// 고를 수 있는 것 하나. 고른 것 앞에 체크 표시 (입력 소스, 전원 모드)
    pub fn check(&self, text: &str, checked: bool, activate: impl Fn() + 'static) -> gtk::Button {
        let mark = gtk::Image::from_icon_name("object-select-symbolic");
        mark.add_css_class("row-check");
        mark.set_opacity(if checked { 1.0 } else { 0.0 });
        let content = gtk::Box::builder().spacing(6).build();
        content.append(&mark);
        content.append(
            &gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        self.button(&content, activate)
    }

    /// 글자만 있는 동작 ("Wi-Fi 설정...")
    pub fn action(&self, text: &str, activate: impl Fn() + 'static) -> gtk::Button {
        let label = gtk::Label::builder().label(text).xalign(0.0).build();
        self.button(&label, activate)
    }
}

/// 포인터가 메뉴 안에서 실제로 움직인 뒤에만 가리킨 줄을 칠하게 한다 (pointer-moved 클래스).
/// 메뉴가 뜰 때 GTK가 포인터가 메뉴 위에 있다고 보고 아래쪽 줄에 hover를 줄 때가 있다.
/// 그 처음 위치에서 벗어나는 움직임이 와야 실제 포인터로 본다. macOS도 포인터가 메뉴에
/// 들어오기 전에는 항목을 칠하지 않는다
fn follow_pointer(content: &gtk::Box) {
    let motion = gtk::EventControllerMotion::new();
    let start: Rc<Cell<Option<(f64, f64)>>> = Rc::default();
    let entered = start.clone();
    motion.connect_enter(move |_, x, y| entered.set(Some((x, y))));
    let moved = start.clone();
    motion.connect_leave(move |_| moved.set(None));
    let target = content.clone();
    motion.connect_motion(move |_, x, y| match start.get() {
        Some((x0, y0)) if (x - x0).abs() + (y - y0).abs() < 2.0 => {}
        _ => target.add_css_class("pointer-moved"),
    });
    content.add_controller(motion);
}

/// 메뉴 막대에 놓는 단추
pub fn button(child: &impl IsA<gtk::Widget>, menu: &Menu, tooltip: &str) -> gtk::MenuButton {
    let button = gtk::MenuButton::builder()
        .child(child)
        .popover(menu.popover())
        .tooltip_text(tooltip)
        .css_classes(["status-item"])
        .build();
    let menu = menu.clone();
    button.set_create_popup_func(move |_| menu.refill());
    button
}

/// 메뉴 막대 항목을 보일지: 사용자가 켰고(org.buntos.shell의 키) 장치나 서비스가 있을 때.
/// 복제해도 같은 상태를 가리킨다
#[derive(Clone)]
pub struct Visibility(Rc<VisibilityInner>);

struct VisibilityInner {
    widget: gtk::Widget,
    settings: Option<gio::Settings>,
    key: &'static str,
    available: Cell<bool>,
}

impl Visibility {
    /// 처음에는 장치가 없는 것으로 보고 숨긴다
    pub fn new(widget: &impl IsA<gtk::Widget>, key: &'static str) -> Self {
        let visibility = Self(Rc::new(VisibilityInner {
            widget: widget.clone().upcast(),
            settings: shell_settings(),
            key,
            available: Cell::new(false),
        }));
        if let Some(settings) = &visibility.0.settings {
            let weak = Rc::downgrade(&visibility.0);
            settings.connect_changed(Some(key), move |_, _| {
                if let Some(inner) = weak.upgrade() {
                    Visibility(inner).update();
                }
            });
        }
        visibility.update();
        visibility
    }

    pub fn set_available(&self, available: bool) {
        self.0.available.set(available);
        self.update();
    }

    fn update(&self) {
        let wanted = self
            .0
            .settings
            .as_ref()
            .is_none_or(|settings| settings.boolean(self.0.key));
        self.0.widget.set_visible(wanted && self.0.available.get());
    }
}
