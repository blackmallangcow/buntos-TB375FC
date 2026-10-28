// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dock: 화면 아래에서 앱을 실행한다. 실행 중인 앱은 아이콘 아래에 점이 붙고, 누르면 새로
//! 띄우지 않고 그 앱의 창을 앞으로 가져온다. Dock에 없는 앱이 실행 중이면 구분선 뒤에 보인다.
//! macOS의 Dock처럼 화면이 좁거나 앱이 많아 넘치면 아이콘을 줄여 화면 안에 둔다.
//! 크기, 투명도, 실행 중 표시는 시스템 설정의 "데스크탑 및 Dock"을 따른다. 자동으로 숨고 아래쪽
//! 가장자리에서 나오는 것은 컴포지터가 한다(comp/src/panels.rs).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gdk, gio, prelude::*};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::{
    config::{Builtin, Config, DockItem},
    launcher::{self, Launcher},
    layer_window, shell_settings,
    toplevels::Toplevels,
};

/// 셸 설정 키 (data/org.buntos.shell.gschema.xml)
const SIZE_KEY: &str = "dock-icon-size";
const TRANSPARENCY_KEY: &str = "dock-transparency";
const INDICATORS_KEY: &str = "dock-show-indicators";
/// 설정 스키마가 없을 때(이미지 밖에서 실행하는 개발 환경)의 아이콘 크기
const DEFAULT_ICON_SIZE: i32 = 48;

/// 앱을 실행한다. D-Bus로 활성화하는 앱이면 GIO가 D-Bus로 띄운다
pub fn launch(info: &impl IsA<gio::AppInfo>, widget: &impl IsA<gtk::Widget>) {
    let context = widget.display().app_launch_context();
    match info.launch(&[], Some(&context)) {
        Ok(()) => launcher::remember(info),
        Err(err) => {
            let name = info.as_ref().display_name();
            eprintln!("bunt-shell: failed to start {name}: {err}");
        }
    }
}

/// 앱이 실행 중이면 그 창을 앞으로 가져오고, 아니면 실행한다
pub fn activate_or_launch(
    info: &impl IsA<gio::AppInfo>,
    toplevels: Option<&Toplevels>,
    widget: &impl IsA<gtk::Widget>,
) {
    let app_id = info.as_ref().id().map(|id| app_id_of(&id).to_owned());
    let raised = toplevels
        .zip(app_id)
        .is_some_and(|(toplevels, app_id)| toplevels.activate_app(&app_id));
    if raised {
        launcher::remember(info);
    } else {
        launch(info, widget);
    }
}

/// Dock 단추의 내용: 아이콘과 그 아래 실행 중 표시 점
pub fn app_button_content(image: &gtk::Image) -> (gtk::Box, gtk::Box) {
    let dot = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .css_classes(["running-dot"])
        .build();
    dot.set_opacity(0.0);
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    content.append(image);
    content.append(&dot);
    (content, dot)
}

/// Dock 판의 가로 바깥 여백(--space-4), 안쪽 여백(10px), 테두리를 양쪽 더한 것 (style.css)
const DOCK_CHROME: i32 = 2 * 16 + 2 * 10 + 2;
/// 단추 안에서 아이콘 밖의 너비(좌우 4px)와 단추 사이 간격 (style.css, build의 spacing)
const ITEM_PADDING: i32 = 8;
const ITEM_SPACING: i32 = 6;
/// 실행 중인 앱 앞 구분선(선과 좌우 여백)과 그 뒤 간격
const SEPARATOR_WIDTH: i32 = 5 + ITEM_SPACING;
/// 화면 가장자리에 남기는 틈
const SCREEN_MARGIN: i32 = 8;
/// 아무리 좁아도 이보다 작게 줄이지 않는다
const MIN_ICON_SIZE: i32 = 24;

struct AppItem {
    app_id: String,
    dot: gtk::Box,
}

/// 앱 단추. 앱이 실행 중이면 그 창을 앞으로 가져오고, 아니면 실행한다
fn app_button(
    image: gtk::Image,
    name: &str,
    app_id: &str,
    info: Option<gio_unix::DesktopAppInfo>,
    toplevels: Option<&Toplevels>,
) -> (gtk::Button, AppItem) {
    let (content, dot) = app_button_content(&image);
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(name)
        .css_classes(["dock-item"])
        .build();
    let (target, toplevels) = (app_id.to_owned(), toplevels.cloned());
    button.connect_clicked(move |button| match &info {
        Some(info) => activate_or_launch(info, toplevels.as_ref(), button),
        // .desktop 파일이 없는 실행 중인 앱: 창만 앞으로 가져온다
        None => {
            if let Some(toplevels) = &toplevels {
                toplevels.activate_app(&target);
            }
        }
    });
    let item = AppItem {
        app_id: app_id.to_owned(),
        dot,
    };
    (button, item)
}

fn app_image(info: Option<&gio_unix::DesktopAppInfo>, icon_size: i32) -> gtk::Image {
    let image = match info.and_then(|info| info.icon()) {
        Some(icon) => gtk::Image::from_gicon(&icon),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    image.set_pixel_size(icon_size);
    image
}

/// .desktop 파일 ID에서 앱 ID를 얻는다 (Wayland의 app_id와 같다)
fn app_id_of(desktop_id: &str) -> &str {
    desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id)
}

pub fn build(
    app: &gtk::Application,
    config: &Config,
    tr: &Translations,
    toplevels: Option<&Toplevels>,
    launcher: &Launcher,
) -> gtk::ApplicationWindow {
    let window = layer_window(app, "bunt-dock");
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Bottom, config.dock.margin_bottom);
    window.auto_exclusive_zone_enable();
    // gtk4-layer-shell은 표면 크기를 전체 화면 창의 크기처럼 GTK에 알리고, GTK는 다음 알림이 올
    // 때까지 그 크기를 쓴다. 크기를 바꿀 수 있는 창이면 최소 크기보다 작을 때만 늘려서, 아이콘이
    // 작아져도 Dock은 줄지 않는다. 크기를 바꿀 수 없는 창은 그 크기를 내용의 크기로 맞춘다
    window.set_resizable(false);

    let settings = shell_settings();
    // 고른 크기. 화면에 다 들어가지 않으면 fit_icons가 이보다 줄인다
    let icon_size = Rc::new(Cell::new(
        settings
            .as_ref()
            .map_or(DEFAULT_ICON_SIZE, |settings| settings.int(SIZE_KEY)),
    ));
    let dock = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .css_classes(["dock"])
        .build();
    let mut pinned: Vec<AppItem> = Vec::new();
    for item in &config.dock.items {
        match item {
            DockItem::Builtin {
                builtin: Builtin::Apps,
                label,
            } => {
                dock.append(&launcher::dock_button(
                    launcher,
                    &tr.get(label),
                    icon_size.get(),
                ));
            }
            DockItem::App { app, label } => {
                let Some(info) = gio_unix::DesktopAppInfo::new(app) else {
                    eprintln!("bunt-shell: {app} is not installed; skipping it");
                    continue;
                };
                let name = match label {
                    Some(id) => tr.get(id),
                    None => info.name().into(),
                };
                let image = app_image(Some(&info), icon_size.get());
                let (button, item) =
                    app_button(image, &name, app_id_of(app), Some(info), toplevels);
                dock.append(&button);
                pinned.push(item);
            }
        }
    }

    let separator = gtk::Separator::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["dock-separator"])
        .visible(false)
        .build();
    let running = gtk::Box::builder().spacing(6).build();
    dock.append(&separator);
    dock.append(&running);
    let fit: Rc<dyn Fn()> = {
        let (window, dock, separator) = (window.clone(), dock.clone(), separator.clone());
        let icon_size = icon_size.clone();
        Rc::new(move || fit_icons(&window, &dock, &separator, icon_size.get()))
    };

    if let Some(toplevels) = toplevels {
        let pinned = Rc::new(pinned);
        let clicks = toplevels.clone();
        let (fit, icon_size) = (fit.clone(), icon_size.clone());
        // 지금 구분선 뒤에 놓인 앱. 창 제목만 바뀌는 것처럼 목록이 같으면 단추를 다시 만들지 않는다
        let shown: RefCell<Vec<String>> = RefCell::default();
        toplevels.subscribe(move |toplevels| {
            for item in pinned.iter() {
                let on = toplevels.is_running(&item.app_id);
                item.dot.set_opacity(if on { 1.0 } else { 0.0 });
            }
            let mut extra: Vec<String> = Vec::new();
            for toplevel in toplevels.list() {
                let id = toplevel.app_id;
                if !id.is_empty()
                    && !pinned.iter().any(|item| item.app_id == id)
                    && !extra.contains(&id)
                {
                    extra.push(id);
                }
            }
            if *shown.borrow() == extra {
                return;
            }
            // Dock에 없는 실행 중인 앱을 구분선 뒤에 다시 늘어놓는다
            while let Some(child) = running.first_child() {
                running.remove(&child);
            }
            for app_id in &extra {
                let info = gio_unix::DesktopAppInfo::new(&format!("{app_id}.desktop"));
                let name = info
                    .as_ref()
                    .map(|info| info.name().to_string())
                    .unwrap_or_else(|| app_id.clone());
                let image = app_image(info.as_ref(), icon_size.get());
                let (button, item) = app_button(image, &name, app_id, info, Some(&clicks));
                item.dot.set_opacity(1.0);
                running.append(&button);
            }
            separator.set_visible(!extra.is_empty());
            shown.replace(extra);
            // 실행 중인 앱이 늘거나 줄면 화면에 들도록 다시 맞춘다
            fit();
        });
    }

    window.set_child(Some(&dock));
    if let Some(settings) = &settings {
        follow_settings(settings, &dock, &icon_size, &fit);
    }
    window.present();
    fit();
    follow_monitors(&WidgetExt::display(&window), &fit);
    window
}

/// 설정 키 하나가 바뀌었을 때 할 일
type SettingChanged = dyn Fn(&gio::Settings, &str);

/// 시스템 설정의 Dock 크기, 투명도, 실행 중 표시를 따른다
fn follow_settings(
    settings: &gio::Settings,
    dock: &gtk::Box,
    icon_size: &Rc<Cell<i32>>,
    fit: &Rc<dyn Fn()>,
) {
    // 투명도는 style.css의 .dock 바탕을 덮어쓴다
    let glass = gtk::CssProvider::new();
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &glass,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
    let apply: Rc<SettingChanged> = {
        let (dock, icon_size, fit) = (dock.clone(), icon_size.clone(), fit.clone());
        Rc::new(move |settings, key| match key {
            SIZE_KEY => {
                icon_size.set(settings.int(SIZE_KEY));
                fit();
            }
            TRANSPARENCY_KEY => glass.load_from_string(&glass_css(settings.double(key))),
            INDICATORS_KEY if settings.boolean(key) => dock.remove_css_class("no-indicators"),
            INDICATORS_KEY => dock.add_css_class("no-indicators"),
            _ => {}
        })
    };
    for key in [SIZE_KEY, TRANSPARENCY_KEY, INDICATORS_KEY] {
        // 처리기가 설정 객체를 붙들어 셸이 끝날 때까지 둔다
        let (changed, keep_alive) = (apply.clone(), settings.clone());
        settings.connect_changed(Some(key), move |settings, key| {
            let _ = &keep_alive;
            changed(settings, key);
        });
        // 변경 알림은 처리기를 단 뒤 한 번 읽어야 온다
        apply(settings, key);
    }
}

/// Dock 유리의 바탕. 투명도가 0이면 불투명하고, 1이면 바탕 없이 테두리와 그림자만 남는다.
/// bunt-comp는 알파가 0.35~0.5보다 옅은 곳은 흐리지 않으므로 아주 투명하면 뒤가 그대로 비친다
fn glass_css(transparency: f64) -> String {
    let alpha = 1.0 - transparency.clamp(0.0, 1.0);
    format!(".dock {{ background-color: alpha(var(--glass-tint), {alpha:.2}); }}")
}

/// 화면 크기나 배율이 바뀌면(모니터의 논리 크기) `fit`을 부른다
fn follow_monitors(display: &gtk::gdk::Display, fit: &Rc<dyn Fn()>) {
    let watch = {
        let fit = fit.clone();
        move |monitor: &gtk::gdk::Monitor| {
            let fit = fit.clone();
            monitor.connect_geometry_notify(move |_| fit());
        }
    };
    let monitors = display.monitors();
    for monitor in monitors.iter::<gtk::gdk::Monitor>().flatten() {
        watch(&monitor);
    }
    let fit = fit.clone();
    monitors.connect_items_changed(move |monitors, position, _removed, added| {
        for index in position..position + added {
            if let Some(monitor) = monitors.item(index).and_downcast::<gtk::gdk::Monitor>() {
                watch(&monitor);
            }
        }
        fit();
    });
}

/// Dock이 화면 너비에 들어가도록 아이콘 크기를 정한다
fn fit_icons(
    window: &gtk::ApplicationWindow,
    dock: &gtk::Box,
    separator: &gtk::Separator,
    max: i32,
) {
    let display = WidgetExt::display(window);
    let monitor = window
        .surface()
        .and_then(|surface| display.monitor_at_surface(&surface))
        .or_else(|| display.monitors().item(0).and_downcast());
    let Some(monitor) = monitor else {
        return;
    };
    let images = images_in(dock.upcast_ref());
    let size = icon_size_for(
        monitor.geometry().width(),
        images.len() as i32,
        separator.is_visible(),
        max,
    );
    for image in images {
        if image.pixel_size() != size {
            image.set_pixel_size(size);
        }
    }
}

/// 아이콘 `items`개가 너비 `screen`인 화면에 들어가는 가장 큰 아이콘 크기 (`max`까지)
fn icon_size_for(screen: i32, items: i32, separator: bool, max: i32) -> i32 {
    if items <= 0 {
        return max;
    }
    let fixed = DOCK_CHROME
        + 2 * SCREEN_MARGIN
        + (items - 1) * ITEM_SPACING
        + items * ITEM_PADDING
        + if separator { SEPARATOR_WIDTH } else { 0 };
    ((screen - fixed) / items).clamp(MIN_ICON_SIZE.min(max), max)
}

/// 위젯 아래의 모든 아이콘
fn images_in(widget: &gtk::Widget) -> Vec<gtk::Image> {
    let mut images = Vec::new();
    let mut child = widget.first_child();
    while let Some(current) = child {
        match current.downcast_ref::<gtk::Image>() {
            Some(image) => images.push(image.clone()),
            None => images.extend(images_in(&current)),
        }
        child = current.next_sibling();
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_shrink_only_when_the_dock_would_overflow() {
        // 1024×640처럼 보이는 화면에는 기본 Dock(앱 10개)이 그대로 들어간다
        assert_eq!(icon_size_for(1024, 10, false, 48), 48);
        // 좁은 화면이나 실행 중인 앱이 많으면 줄인다
        assert_eq!(icon_size_for(640, 10, false, 48), 43);
        assert_eq!(icon_size_for(1024, 16, true, 48), 45);
        // 그래도 24보다 작게는 줄이지 않는다
        assert_eq!(icon_size_for(400, 20, true, 48), 24);
        assert_eq!(icon_size_for(1024, 0, false, 48), 48);
    }

    #[test]
    fn transparency_sets_the_glass_alpha() {
        // 기본값 0.4는 style.css의 --glass(60%)와 같다
        assert_eq!(
            glass_css(0.4),
            ".dock { background-color: alpha(var(--glass-tint), 0.60); }"
        );
        assert!(glass_css(0.0).contains("1.00"));
        assert!(glass_css(2.0).contains("0.00"));
    }

    #[test]
    fn app_ids_come_from_desktop_ids() {
        assert_eq!(
            app_id_of("org.gnome.TextEditor.desktop"),
            "org.gnome.TextEditor"
        );
        assert_eq!(app_id_of("foot.desktop"), "foot");
    }
}
