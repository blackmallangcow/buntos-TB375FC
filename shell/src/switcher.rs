// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! ⌘⇥ 앱 전환 창 (macOS의 앱 전환기). ⌘를 누른 채 ⇥를 누르는 동안 컴포지터가 최근에 쓴 차례의 앱과
//! 고른 앱을 알리면(AppSwitcherChanged) 화면 가운데에 앱 아이콘을 늘어놓고 고른 앱을 칠한다. ⌘를
//! 떼면 컴포지터가 그 앱으로 바꾸고 숨기라고 알린다. 키는 컴포지터가 받으므로 이 창은 키보드를 받지
//! 않는다. 포인터를 움직여 가리키면 그 앱을 고르고, 누르면 그 앱으로 바꾼다(SelectSwitcherApp).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{Layer, LayerShell};

use crate::{clip_input_to_child, layer_window};

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";

/// ⌘⇥를 빨리 눌렀다 떼면(바로 앞 앱으로 가기) 창을 띄우지 않는다
const SHOW_DELAY: Duration = Duration::from_millis(120);
/// 아이콘 크기. 앱이 많아 화면에 다 들어가지 않으면 MIN_ICON_SIZE까지 줄인다
const ICON_SIZE: i32 = 96;
const MIN_ICON_SIZE: i32 = 48;
/// 칸 하나의 아이콘 밖 너비(좌우 여백), 칸 사이, 판의 좌우 여백과 그림자 자리 (style.css)
const TILE_PADDING: i32 = 2 * 12;
const TILE_SPACING: i32 = 4;
const PANEL_CHROME: i32 = 2 * 16 + 2 * 28;
/// 화면 가장자리에 남기는 틈
const SCREEN_MARGIN: i32 = 40;
/// 이만큼 움직여야 가리킨 것으로 본다. 창이 포인터 밑에 나타날 때 오는 움직임은 고르지 않는다
const POINTER_SLOP: f64 = 2.0;

struct Inner {
    window: gtk::ApplicationWindow,
    row: gtk::Box,
    /// 지금 늘어놓은 앱 (최근 차례)
    apps: RefCell<Vec<String>>,
    /// 칸마다 앱 이름 (고른 칸에만 보인다)
    names: RefCell<Vec<gtk::Label>>,
    selected: Cell<i32>,
    /// 띄우기를 기다리는 중
    pending: RefCell<Option<glib::SourceId>>,
    /// 창이 뜬 뒤 처음 본 포인터 자리
    pointer_start: Cell<Option<(f64, f64)>>,
}

#[derive(Clone)]
pub struct Switcher(Rc<Inner>);

impl Switcher {
    /// 전환 창을 만들고 컴포지터의 알림을 받기 시작한다
    pub fn start(app: &gtk::Application) -> Self {
        let window = layer_window(app, "bunt-switcher");
        window.set_layer(Layer::Overlay);
        // 앵커가 없으면 화면 가운데에 뜬다. 크기는 내용에 맞추고, 판 둘레의 그림자 여백은 누르기를
        // 받지 않는다
        window.set_resizable(false);
        clip_input_to_child(&window);
        let row = gtk::Box::builder()
            .spacing(TILE_SPACING)
            .css_classes(["switcher-panel"])
            .build();
        window.set_child(Some(&row));
        let switcher = Self(Rc::new(Inner {
            window,
            row,
            apps: RefCell::default(),
            names: RefCell::default(),
            selected: Cell::new(-1),
            pending: RefCell::default(),
            pointer_start: Cell::new(None),
        }));
        switcher.follow();
        switcher
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    /// 컴포지터의 알림을 받는다. 처리기가 전환 창을 붙들어 셸이 끝날 때까지 둔다
    fn follow(&self) {
        let switcher = self.clone();
        gio::bus_get(
            gio::BusType::Session,
            gio::Cancellable::NONE,
            move |result| {
                let connection = match result {
                    Ok(connection) => connection,
                    Err(err) => {
                        eprintln!("bunt-shell: session bus: {err}");
                        return;
                    }
                };
                let subscription = connection.subscribe_to_signal(
                    Some(NAME),
                    Some(INTERFACE),
                    Some("AppSwitcherChanged"),
                    Some(PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let Some((apps, selected)) =
                            signal.parameters.get::<(Vec<String>, i32)>()
                        {
                            switcher.update(apps, selected);
                        }
                    },
                );
                // 셸이 끝날 때까지 받는다
                std::mem::forget(subscription);
            },
        );
    }

    /// 컴포지터가 알린 상태로 바꾼다. 고른 앱이 없으면(-1) 숨긴다
    fn update(&self, apps: Vec<String>, selected: i32) {
        let inner = &self.0;
        if selected < 0 || apps.is_empty() {
            if let Some(pending) = inner.pending.take() {
                pending.remove();
            }
            inner.window.set_visible(false);
            inner.apps.borrow_mut().clear();
            inner.selected.set(-1);
            return;
        }
        if *inner.apps.borrow() != apps {
            self.fill(&apps);
            inner.apps.replace(apps);
        }
        self.select(selected);
        if inner.window.is_visible() || inner.pending.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        let pending = glib::timeout_add_local_once(SHOW_DELAY, move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.pending.take();
            if inner.selected.get() >= 0 {
                inner.pointer_start.set(None);
                inner.window.present();
            }
        });
        inner.pending.replace(Some(pending));
    }

    /// 앱마다 칸(아이콘과 그 아래 이름)을 늘어놓는다
    fn fill(&self, apps: &[String]) {
        let inner = &self.0;
        while let Some(child) = inner.row.first_child() {
            inner.row.remove(&child);
        }
        let size = icon_size_for(self.screen_width(), apps.len() as i32);
        let mut names = Vec::new();
        for (index, app_id) in apps.iter().enumerate() {
            let info = crate::apps::for_window(app_id);
            let image = match info.as_ref().and_then(|info| info.icon()) {
                Some(icon) => gtk::Image::from_gicon(&icon),
                None => gtk::Image::from_icon_name("application-x-executable"),
            };
            image.set_pixel_size(size);
            let name = info
                .as_ref()
                .map(|info| info.name().to_string())
                .unwrap_or_else(|| app_id.clone());
            let label = gtk::Label::builder()
                .label(&name)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(1)
                .css_classes(["switcher-name"])
                .build();
            label.set_size_request(size, -1);
            label.set_opacity(0.0);
            let tile = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .css_classes(["switcher-tile"])
                .build();
            tile.append(&image);
            tile.append(&label);
            tile.update_property(&[gtk::accessible::Property::Label(&name)]);
            self.follow_pointer(&tile, index as u32);
            inner.row.append(&tile);
            names.push(label);
        }
        inner.names.replace(names);
    }

    /// 고른 칸을 칠하고 그 칸의 이름만 보인다
    fn select(&self, selected: i32) {
        let inner = &self.0;
        inner.selected.set(selected);
        let mut child = inner.row.first_child();
        let mut index = 0;
        while let Some(tile) = child {
            if index == selected {
                tile.add_css_class("selected");
            } else {
                tile.remove_css_class("selected");
            }
            if let Some(label) = inner.names.borrow().get(index as usize) {
                label.set_opacity(if index == selected { 1.0 } else { 0.0 });
            }
            child = tile.next_sibling();
            index += 1;
        }
    }

    /// 포인터를 움직여 가리키면 그 앱을 고르고, 누르면 그 앱으로 바꾼다
    fn follow_pointer(&self, tile: &gtk::Box, index: u32) {
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&self.0);
        motion.connect_motion(move |controller, x, y| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // 창 좌표로 바꿔 처음 자리와 견준다 (칸마다 좌표가 다르다)
            let Some(point) = controller.widget().and_then(|tile| {
                tile.compute_point(
                    &inner.window,
                    &gtk::graphene::Point::new(x as f32, y as f32),
                )
            }) else {
                return;
            };
            let here = (f64::from(point.x()), f64::from(point.y()));
            match inner.pointer_start.get() {
                None => inner.pointer_start.set(Some(here)),
                Some(start) if moved(start, here) && inner.selected.get() != index as i32 => {
                    select_app(index, false);
                }
                Some(_) => {}
            }
        });
        tile.add_controller(motion);
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| select_app(index, true));
        tile.add_controller(click);
    }

    /// 앱 칸이 들어갈 화면 너비 (논리 픽셀)
    fn screen_width(&self) -> i32 {
        let display = WidgetExt::display(&self.0.window);
        self.0
            .window
            .surface()
            .and_then(|surface| display.monitor_at_surface(&surface))
            .or_else(|| display.monitors().item(0).and_downcast::<gdk::Monitor>())
            .map_or(1280, |monitor| monitor.geometry().width())
    }
}

/// 처음 자리에서 POINTER_SLOP보다 멀리 움직였는지
fn moved(start: (f64, f64), here: (f64, f64)) -> bool {
    (here.0 - start.0).abs() > POINTER_SLOP || (here.1 - start.1).abs() > POINTER_SLOP
}

/// 앱 `count`개가 너비 `screen`인 화면에 들어가는 가장 큰 아이콘 크기
fn icon_size_for(screen: i32, count: i32) -> i32 {
    if count <= 0 {
        return ICON_SIZE;
    }
    let fixed =
        PANEL_CHROME + 2 * SCREEN_MARGIN + (count - 1) * TILE_SPACING + count * TILE_PADDING;
    ((screen - fixed) / count).clamp(MIN_ICON_SIZE, ICON_SIZE)
}

/// 컴포지터에 그 앱을 고르게 한다. `activate`면 전환을 마치고 그 앱으로 바꾼다
fn select_app(index: u32, activate: bool) {
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let Ok(connection) = result else {
                return;
            };
            connection.call(
                Some(NAME),
                PATH,
                INTERFACE,
                "SelectSwitcherApp",
                Some(&(index, activate).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                |result| {
                    if let Err(err) = result {
                        eprintln!("bunt-shell: {INTERFACE}.SelectSwitcherApp: {err}");
                    }
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_shrink_to_fit_many_apps() {
        // 1280px 화면에 앱 다섯은 그대로, 열둘은 줄여서 들어간다
        assert_eq!(icon_size_for(1280, 5), ICON_SIZE);
        assert_eq!(icon_size_for(1280, 12), 65);
        // 아무리 많아도 MIN_ICON_SIZE보다 작게는 줄이지 않는다
        assert_eq!(icon_size_for(1024, 30), MIN_ICON_SIZE);
        assert_eq!(icon_size_for(1280, 0), ICON_SIZE);
    }

    #[test]
    fn a_small_wobble_is_not_pointing() {
        assert!(!moved((10.0, 10.0), (11.5, 9.0)));
        assert!(moved((10.0, 10.0), (13.0, 10.0)));
        assert!(moved((10.0, 10.0), (10.0, 6.0)));
    }
}
