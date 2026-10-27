// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 모든 앱 목록: Dock의 앱 단추를 누르면 Dock 위에 뜬다. 설치된 앱을 이름순으로 보이고,
//! 이름으로 찾아(Enter는 첫 결과) 눌러서 실행한다. 바깥을 누르거나 Esc를 누르면 닫힌다.

use std::{cell::RefCell, rc::Rc};

use gtk4::{self as gtk, gio, prelude::*};

use crate::dock::{app_button_content, launch};

/// Dock 단추의 아이콘 (shell/data/icons)
const ICON: &str = "org.buntos.Apps";
const TILE_ICON_SIZE: i32 = 64;
const COLUMNS: u32 = 6;

pub fn button(tooltip: &str, search_hint: &str, icon_size: i32) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name(ICON);
    image.set_pixel_size(icon_size);
    // 실행 중 표시 점 자리를 비워 다른 Dock 단추와 높이를 맞춘다
    let (content, _dot) = app_button_content(&image);
    let popover = gtk::Popover::builder().has_arrow(false).build();
    // css_classes로 정하면 팝오버의 기본 클래스(background)가 빠져 바탕이 그려지지 않는다
    popover.add_css_class("launcher");
    let search_hint = search_hint.to_owned();
    // 열 때마다 앱 목록을 새로 읽는다 (그사이 설치·삭제된 앱 반영)
    popover.connect_show(move |popover| {
        let (content, search) = launcher_content(popover, &search_hint);
        popover.set_child(Some(&content));
        search.grab_focus();
    });
    gtk::MenuButton::builder()
        .child(&content)
        .popover(&popover)
        .direction(gtk::ArrowType::Up)
        .tooltip_text(tooltip)
        .css_classes(["dock-item"])
        .build()
}

/// 검색 상자와 앱 격자
fn launcher_content(popover: &gtk::Popover, search_hint: &str) -> (gtk::Box, gtk::SearchEntry) {
    let mut apps: Vec<gio::AppInfo> = gio::AppInfo::all()
        .into_iter()
        .filter(|info| info.should_show())
        .collect();
    apps.sort_by_cached_key(|info| info.display_name().to_lowercase());

    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(COLUMNS)
        .max_children_per_line(COLUMNS)
        .row_spacing(12)
        .column_spacing(4)
        .valign(gtk::Align::Start)
        .build();
    // 검색에 쓰는 글자: 이름, 영문 이름, 키워드, 실행 파일 (소문자)
    let keys: Vec<String> = apps.iter().map(search_key).collect();
    for info in &apps {
        grid.append(&tile(info, popover));
    }

    let query = Rc::new(RefCell::new(String::new()));
    let matcher = {
        let query = query.clone();
        move |index: i32| {
            let query = query.borrow();
            usize::try_from(index)
                .ok()
                .and_then(|index| keys.get(index))
                .is_some_and(|key| key.contains(query.as_str()))
        }
    };
    let matcher = Rc::new(matcher);
    let filter = matcher.clone();
    grid.set_filter_func(move |child| filter(child.index()));

    let search = gtk::SearchEntry::builder()
        .placeholder_text(search_hint)
        .css_classes(["launcher-search"])
        .build();
    let refilter = grid.clone();
    search.connect_search_changed(move |search| {
        *query.borrow_mut() = search.text().trim().to_lowercase();
        refilter.invalidate_filter();
    });
    // Enter는 보이는 첫 앱을 실행한다
    let (first_grid, first_apps) = (grid.clone(), apps.clone());
    let popdown = popover.clone();
    search.connect_activate(move |_| {
        let index = (0..first_apps.len()).find(|index| matcher(*index as i32));
        if let Some(info) = index.and_then(|index| first_apps.get(index)) {
            launch(info, &first_grid);
            popdown.popdown();
        }
    });

    let scroller = gtk::ScrolledWindow::builder()
        .child(&grid)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(420)
        .min_content_width(680)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .width_request(720)
        .build();
    content.append(&search);
    content.append(&scroller);
    (content, search)
}

fn search_key(info: &gio::AppInfo) -> String {
    let mut key = format!("{} {}", info.display_name(), info.name());
    if let Some(desktop) = info.downcast_ref::<gio_unix::DesktopAppInfo>() {
        for keyword in desktop.keywords() {
            key.push(' ');
            key.push_str(&keyword);
        }
    }
    key.push(' ');
    key.push_str(&info.executable().to_string_lossy());
    key.to_lowercase()
}

fn tile(info: &gio::AppInfo, popover: &gtk::Popover) -> gtk::Button {
    let icon = match info.icon() {
        Some(icon) => gtk::Image::from_gicon(&icon),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    icon.set_pixel_size(TILE_ICON_SIZE);
    let label = gtk::Label::builder()
        .label(info.display_name())
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .justify(gtk::Justification::Center)
        .max_width_chars(12)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    content.append(&icon);
    content.append(&label);
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(info.display_name())
        .css_classes(["launcher-tile"])
        .build();
    let (info, popover) = (info.clone(), popover.clone());
    button.connect_clicked(move |button| {
        launch(&info, button);
        popover.popdown();
    });
    button
}
