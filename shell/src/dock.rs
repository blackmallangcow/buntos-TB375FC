// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dock: 화면 아래에서 앱을 실행한다. 실행 중인 앱은 아이콘 아래에 점이 붙고, 누르면 새로
//! 띄우지 않고 그 앱의 창을 앞으로 가져온다. Dock에 없는 앱이 실행 중이면 구분선 뒤에 보인다.
//! 최소화한 창은 macOS처럼 오른쪽 끝, 구분선 뒤에 창 그림으로 놓이고 누르면 돌아온다(minimized.rs).
//! 그 칸의 자리를 컴포지터에 알려 최소화 효과가 창을 칸으로 보내고 칸에서 꺼내게 한다.
//! macOS의 Dock처럼 화면이 좁거나 앱이 많아 넘치면 아이콘을 줄여 화면 안에 둔다.
//! 크기, 투명도, 실행 중 표시는 시스템 설정의 "데스크탑 및 Dock"을 따른다. 자동으로 숨고 아래쪽
//! 가장자리에서 나오는 것은 컴포지터가 한다(comp/src/panels.rs).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::{
    apps, clip_input_to_child,
    config::{Builtin, Config, DockItem},
    launcher::{self, Launcher},
    layer_window,
    minimized::{self, Minimized},
    shell_settings,
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
    let raised = toplevels.is_some_and(|toplevels| apps::activate(toplevels, info));
    if raised {
        launcher::remember(info);
    } else {
        launch(info, widget);
    }
}

/// Dock 단추의 내용: 아이콘(최소화 칸은 창 그림)과 그 아래 실행 중 표시 점
pub fn app_button_content(image: &impl IsA<gtk::Widget>) -> (gtk::Box, gtk::Box) {
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

/// Dock 판의 가로 바깥 여백(그림자 자리 28px), 안쪽 여백(10px), 테두리를 양쪽 더한 것 (style.css)
const DOCK_CHROME: i32 = 2 * 28 + 2 * 10 + 2;
/// 단추 안에서 아이콘 밖의 너비(좌우 4px)와 단추 사이 간격 (style.css, build의 spacing)
const ITEM_PADDING: i32 = 8;
const ITEM_SPACING: i32 = 6;
/// 실행 중인 앱·최소화한 창 앞 구분선(선과 좌우 여백)과 그 뒤 간격
const SEPARATOR_WIDTH: i32 = 5 + ITEM_SPACING;
/// 최소화한 창 그림 위 앱 아이콘의 크기 (창 그림 크기에 대한 비율, 5분의 2)
const BADGE_RATIO: (i32, i32) = (2, 5);
/// 화면 가장자리에 남기는 틈
const SCREEN_MARGIN: i32 = 8;
/// 아무리 좁아도 이보다 작게 줄이지 않는다
const MIN_ICON_SIZE: i32 = 24;
/// 최소화 칸의 자리가 정해지기를 기다리는 프레임 수. 넘으면 그때의 자리를 알린다
const SLOT_FRAMES: u32 = 60;

struct AppItem {
    /// 이 앱의 창이 알리는 앱 ID들(apps.rs). 설치된 앱이 아니면 그 창의 앱 ID 하나다
    ids: Vec<String>,
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
    let ids = match &info {
        Some(info) => apps::window_ids(info),
        None => vec![app_id.to_owned()],
    };
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
    let item = AppItem { ids, dot };
    (button, item)
}

/// 최소화한 창 단추: 창 그림의 오른쪽 아래에 앱 아이콘을 얹는다(macOS). 누르면 그 창이 돌아온다.
/// 창 그림은 아이콘처럼 아이콘 크기의 정사각형 안에 비율을 지켜 그린다(fit_icons가 함께 줄인다).
/// 그림이 없으면(컴포지터가 그리지 못했으면) 앱 아이콘만 그린다. 칸의 자리를 잴 그림(창 그림이나 앱
/// 아이콘)도 돌려준다
fn minimized_button(window: &Minimized, icon_size: i32) -> (gtk::Button, gtk::Image) {
    let info = apps::for_window(&window.app_id);
    let overlay = gtk::Overlay::new();
    let picture = match &window.thumbnail {
        Some(texture) => {
            let picture = gtk::Image::from_paintable(Some(texture));
            picture.set_pixel_size(icon_size);
            picture.add_css_class("dock-window");
            overlay.set_child(Some(&picture));
            let badge = app_image(info.as_ref(), badge_size(icon_size));
            badge.add_css_class("dock-badge");
            badge.set_halign(gtk::Align::End);
            badge.set_valign(gtk::Align::End);
            overlay.add_overlay(&badge);
            picture
        }
        None => {
            let icon = app_image(info.as_ref(), icon_size);
            overlay.set_child(Some(&icon));
            icon
        }
    };
    // 실행 중 표시 점은 늘 비워 두지만 자리는 남겨 다른 아이콘과 높이를 맞춘다
    let (content, _dot) = app_button_content(&overlay);
    let name = match window.title.is_empty() {
        false => window.title.clone(),
        true => info
            .as_ref()
            .map(|info| info.name().to_string())
            .unwrap_or_else(|| window.app_id.clone()),
    };
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(&name)
        .css_classes(["dock-item", "dock-minimized"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&name)]);
    let id = window.id;
    button.connect_clicked(move |_| minimized::restore(id));
    (button, picture)
}

/// 최소화 칸의 자리를 컴포지터에 알린다(minimized::report_slots). GTK는 다음 프레임에 자리를 정하므로
/// 프레임마다 재어 두 프레임 잇달아 같으면 알린다. 칸이 바뀌거나(최소화, 되돌리기) 아이콘 크기가 바뀌거나
/// 앞의 앱이 늘어 칸이 옮겨 갈 때마다 다시 잰다
struct SlotReporter {
    window: gtk::ApplicationWindow,
    /// 최소화 번호와 칸의 그림
    slots: RefCell<Vec<(u32, gtk::Image)>>,
    /// 마지막으로 알린 자리. 같으면 다시 보내지 않는다
    sent: RefCell<Option<Vec<minimized::Slot>>>,
    tick: RefCell<Option<gtk::TickCallbackId>>,
}

impl SlotReporter {
    fn new(window: &gtk::ApplicationWindow) -> Rc<Self> {
        Rc::new(Self {
            window: window.clone(),
            slots: RefCell::default(),
            sent: RefCell::default(),
            tick: RefCell::default(),
        })
    }

    /// 다음 프레임부터 자리를 잰다. 재던 중이면 처음부터 다시 잰다
    fn schedule(self: &Rc<Self>) {
        if let Some(tick) = self.tick.take() {
            tick.remove();
        }
        let weak = Rc::downgrade(self);
        let previous: RefCell<Option<Vec<minimized::Slot>>> = RefCell::default();
        let frames = Cell::new(0);
        let tick = self.window.add_tick_callback(move |_, _| {
            let Some(reporter) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let now = reporter.measure();
            frames.set(frames.get() + 1);
            if (now.is_some() && *previous.borrow() == now) || frames.get() >= SLOT_FRAMES {
                // 이 처리기는 끝나며 스스로 빠지므로 번호만 버린다
                reporter.tick.take();
                if let Some(slots) = now.or_else(|| previous.take()) {
                    reporter.send(slots);
                }
                return glib::ControlFlow::Break;
            }
            previous.replace(now);
            glib::ControlFlow::Continue
        });
        self.tick.replace(Some(tick));
    }

    /// 칸의 자리(Dock 표면 안 좌표). 아직 자리를 받지 못한 칸이 있으면 None
    fn measure(&self) -> Option<Vec<minimized::Slot>> {
        let (dx, dy) = self.window.surface_transform();
        self.slots
            .borrow()
            .iter()
            .map(|(id, image)| {
                let bounds = image.compute_bounds(&self.window)?;
                slot_in(
                    *id,
                    (
                        f64::from(bounds.x()) + dx,
                        f64::from(bounds.y()) + dy,
                        f64::from(bounds.width()),
                        f64::from(bounds.height()),
                    ),
                    image.pixel_size(),
                )
            })
            .collect()
    }

    fn send(&self, slots: Vec<minimized::Slot>) {
        if self.sent.borrow().as_ref() == Some(&slots) {
            return;
        }
        minimized::report_slots(&slots);
        self.sent.replace(Some(slots));
    }
}

/// 그림 위젯의 자리(`bounds`: x, y, 너비, 높이)에서 그림이 그려지는 칸: 가운데의 `pixel_size` 정사각형
/// (GtkImage는 그림을 그 크기로 가운데에 그린다). 자리가 아직 없으면 None
fn slot_in(
    id: u32,
    (x, y, width, height): (f64, f64, f64, f64),
    pixel_size: i32,
) -> Option<minimized::Slot> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let side = f64::from(pixel_size.max(1)).min(width).min(height);
    Some((
        id,
        (x + (width - side) / 2.0).round() as i32,
        (y + (height - side) / 2.0).round() as i32,
        side.round() as i32,
        side.round() as i32,
    ))
}

/// 창 그림 위 앱 아이콘의 크기
fn badge_size(icon_size: i32) -> i32 {
    (icon_size * BADGE_RATIO.0 / BADGE_RATIO.1).max(12)
}

fn app_image(info: Option<&gio_unix::DesktopAppInfo>, icon_size: i32) -> gtk::Image {
    let image = match info.and_then(|info| info.icon()) {
        Some(icon) => gtk::Image::from_gicon(&icon),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    image.set_pixel_size(icon_size);
    image
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
    // 판 둘레의 그림자 여백은 클릭을 받지 않는다
    clip_input_to_child(&window);
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
                    app_button(image, &name, apps::stem(app), Some(info), toplevels);
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
    // 최소화한 창: 오른쪽 끝, 구분선 뒤 (macOS Dock의 오른쪽 칸)
    let minimized_separator = gtk::Separator::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["dock-separator"])
        .visible(false)
        .build();
    let minimized_box = gtk::Box::builder().spacing(6).build();
    dock.append(&minimized_separator);
    dock.append(&minimized_box);
    // 아이콘 크기를 다시 맞추면 최소화 칸도 옮겨 가므로 칸의 자리를 다시 잰다
    let reporter = SlotReporter::new(&window);
    let fit: Rc<dyn Fn()> = {
        let (window, dock) = (window.clone(), dock.clone());
        let (icon_size, reporter) = (icon_size.clone(), reporter.clone());
        Rc::new(move || {
            fit_icons(&window, &dock, icon_size.get());
            reporter.schedule();
        })
    };
    {
        let (fit, icon_size) = (fit.clone(), icon_size.clone());
        minimized::follow(move |windows| {
            while let Some(child) = minimized_box.first_child() {
                minimized_box.remove(&child);
            }
            let mut slots = Vec::new();
            for window in &windows {
                let (button, picture) = minimized_button(window, icon_size.get());
                minimized_box.append(&button);
                slots.push((window.id, picture));
            }
            reporter.slots.replace(slots);
            minimized_separator.set_visible(!windows.is_empty());
            fit();
        });
    }

    if let Some(toplevels) = toplevels {
        let pinned = Rc::new(pinned);
        let clicks = toplevels.clone();
        let (fit, icon_size) = (fit.clone(), icon_size.clone());
        // 지금 구분선 뒤에 놓인 앱. 창 제목만 바뀌는 것처럼 목록이 같으면 단추를 다시 만들지 않는다
        let shown: RefCell<Vec<String>> = RefCell::default();
        toplevels.subscribe(move |toplevels| {
            let open = toplevels.list();
            for item in pinned.iter() {
                let on = open
                    .iter()
                    .any(|toplevel| apps::same_app(&item.ids, &toplevel.app_id));
                item.dot.set_opacity(if on { 1.0 } else { 0.0 });
            }
            let mut extra: Vec<String> = Vec::new();
            for toplevel in open {
                let id = toplevel.app_id;
                if !id.is_empty()
                    && !pinned.iter().any(|item| apps::same_app(&item.ids, &id))
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
                let info = apps::for_window(app_id);
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

/// Dock이 화면 너비에 들어가도록 아이콘 크기를 정한다. 최소화한 창 그림도 아이콘 하나로 센다
fn fit_icons(window: &gtk::ApplicationWindow, dock: &gtk::Box, max: i32) {
    let display = WidgetExt::display(window);
    let monitor = window
        .surface()
        .and_then(|surface| display.monitor_at_surface(&surface))
        .or_else(|| display.monitors().item(0).and_downcast());
    let Some(monitor) = monitor else {
        return;
    };
    let mut parts = DockParts::default();
    parts.collect(dock.upcast_ref());
    let size = icon_size_for(
        monitor.geometry().width(),
        parts.icons.len() as i32,
        parts.separators,
        max,
    );
    for image in parts.icons {
        if image.pixel_size() != size {
            image.set_pixel_size(size);
        }
    }
    for badge in parts.badges {
        badge.set_pixel_size(badge_size(size));
    }
}

/// 아이콘 `items`개와 구분선 `separators`개가 너비 `screen`인 화면에 들어가는 가장 큰 아이콘 크기
/// (`max`까지)
fn icon_size_for(screen: i32, items: i32, separators: i32, max: i32) -> i32 {
    if items <= 0 {
        return max;
    }
    let fixed = DOCK_CHROME
        + 2 * SCREEN_MARGIN
        + (items - 1) * ITEM_SPACING
        + items * ITEM_PADDING
        + separators * SEPARATOR_WIDTH;
    ((screen - fixed) / items).clamp(MIN_ICON_SIZE.min(max), max)
}

/// Dock에서 크기를 맞출 것: 아이콘(앱 아이콘과 최소화한 창 그림), 창 그림 위 앱 아이콘, 보이는
/// 구분선 수
#[derive(Default)]
struct DockParts {
    icons: Vec<gtk::Image>,
    badges: Vec<gtk::Image>,
    separators: i32,
}

impl DockParts {
    fn collect(&mut self, widget: &gtk::Widget) {
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(image) = current.downcast_ref::<gtk::Image>() {
                match image.has_css_class("dock-badge") {
                    true => self.badges.push(image.clone()),
                    false => self.icons.push(image.clone()),
                }
            } else if current.is::<gtk::Separator>() {
                self.separators += i32::from(current.is_visible());
            } else {
                self.collect(&current);
            }
            child = current.next_sibling();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_shrink_only_when_the_dock_would_overflow() {
        // 1024×640처럼 보이는 화면에는 기본 Dock(앱 10개)이 그대로 들어간다
        assert_eq!(icon_size_for(1024, 10, 0, 48), 48);
        // 좁은 화면이나 실행 중인 앱이 많으면 줄인다
        assert_eq!(icon_size_for(640, 10, 0, 48), 41);
        assert_eq!(icon_size_for(1024, 16, 1, 48), 43);
        // 최소화한 창 칸의 구분선도 자리를 차지한다
        assert_eq!(icon_size_for(1013, 16, 1, 48), 43);
        assert_eq!(icon_size_for(1013, 16, 2, 48), 42);
        // 그래도 24보다 작게는 줄이지 않는다
        assert_eq!(icon_size_for(400, 20, 1, 48), 24);
        assert_eq!(icon_size_for(1024, 0, 0, 48), 48);
    }

    #[test]
    fn slots_are_the_centered_square_the_picture_is_drawn_in() {
        // 그림 위젯이 아이콘보다 넓게 자리를 받아도 그림은 가운데 48px 정사각형에 그려진다
        assert_eq!(
            slot_in(7, (100.0, 20.5, 60.0, 48.0), 48),
            Some((7, 106, 21, 48, 48))
        );
        assert_eq!(
            slot_in(7, (0.0, 0.0, 32.0, 32.0), 48),
            Some((7, 0, 0, 32, 32))
        );
        // 아직 자리를 받지 못했다
        assert_eq!(slot_in(7, (0.0, 0.0, 0.0, 48.0), 48), None);
    }

    #[test]
    fn badges_are_two_fifths_of_the_picture() {
        assert_eq!(badge_size(48), 19);
        assert_eq!(badge_size(96), 38);
        // 작은 Dock에서도 알아볼 수 있게 12px보다 작게는 줄이지 않는다
        assert_eq!(badge_size(24), 12);
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
}
