// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 마우스 (macOS 26과 같은 이름과 순서): 이동 속도, 자연스러운 스크롤, 보조 클릭, 두 번 클릭 속도,
//! 스크롤 속도. macOS는 손쉬운 사용에 두는 포인터 크기를 여기에 함께 두고, 연결된 마우스를 보인다.
//! 두 번 클릭 속도는 GTK가 읽는 공용 설정(org.gnome.desktop.peripherals.mouse)이고, 나머지는
//! 컴포지터 API(org.buntos.Compositor1)가 연결된 장치와 커서에 쓴다

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gtk4::{self as gtk, gio, glib, prelude::*};

use super::{Context, has_key, interface_settings, settings_for};
use crate::{
    dbus::{self, Debounce, Remote},
    widgets::{self, BadgeSize, Tint, badge},
};

const COMPOSITOR: &str = "org.buntos.Compositor";
const COMPOSITOR_PATH: &str = "/org/buntos/Compositor";
const COMPOSITOR_INTERFACE: &str = "org.buntos.Compositor1";
const MOUSE_SCHEMA: &str = "org.gnome.desktop.peripherals.mouse";
const ICON: &str = "input-mouse-symbolic";

/// 이동 속도 눈금 (libinput 가속, 가운데가 기본)
const SPEEDS: [f64; 9] = [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0];
/// 두 번 클릭으로 볼 간격 눈금 (밀리초). 왼쪽이 느리다
const DOUBLE_CLICKS: [i32; 9] = [1000, 800, 650, 500, 400, 330, 270, 220, 180];
/// 스크롤 속도 눈금 (배수)
const SCROLL_SPEEDS: [f64; 8] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
/// 포인터 크기 눈금 (논리 픽셀)
const CURSOR_SIZES: [u32; 9] = [16, 20, 24, 28, 32, 40, 48, 56, 64];
/// 드롭다운 항목 순서대로 보조 클릭이 왼쪽 단추인지 (오른쪽 클릭, 왼쪽 클릭)
const SECONDARY_LABELS: [&str; 2] = ["mouse-click-right", "mouse-click-left"];
const APPLY_DELAY: Duration = Duration::from_millis(250);

struct MousePage {
    context: Rc<Context>,
    compositor: RefCell<Option<Remote>>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    speed: gtk::Scale,
    natural: gtk::Switch,
    secondary: gtk::DropDown,
    scroll: gtk::Scale,
    size: gtk::Scale,
    devices: gtk::ListBox,
    /// 컴포지터에서 읽은 값을 위젯에 넣는 동안은 다시 보내지 않는다
    loading: Cell<bool>,
    debounce: Debounce,
}

/// 가장 가까운 눈금의 번호
fn nearest<T: Copy + Into<f64>>(steps: &[T], value: f64) -> usize {
    let distance = |step: T| (step.into() - value).abs();
    (0..steps.len())
        .min_by(|&a, &b| distance(steps[a]).total_cmp(&distance(steps[b])))
        .unwrap_or(0)
}

fn step<T: Copy>(steps: &[T], scale: &gtk::Scale) -> T {
    steps[(scale.value().round().max(0.0) as usize).min(steps.len() - 1)]
}

/// 장치 이름을 알 수 없으면(빈 이름, "HID 093a:2510"처럼 번호뿐인 이름) None이다
fn known_name(name: &str) -> Option<&str> {
    let name = name.trim();
    let numbers_only = name.strip_prefix("HID ").is_some_and(|ids| {
        ids.split(':').count() == 2 && ids.chars().all(|c| c == ':' || c.is_ascii_hexdigit())
    });
    (!name.is_empty() && !numbers_only).then_some(name)
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Gray, BadgeSize::Medium),
        &tr.get("page-mouse"),
        &tr.get("mouse-description"),
        None,
    ));

    let behavior = widgets::group(&column, None, None);
    let speed = widgets::step_scale(SPEEDS.len());
    behavior.append(&widgets::slider_row(
        &tr.get("mouse-speed"),
        &speed,
        &tr.get("mouse-slow"),
        &tr.get("mouse-fast"),
    ));
    let (row, natural) = widgets::switch_row(
        &tr.get("mouse-natural-scroll"),
        Some(&tr.get("mouse-natural-scroll-description")),
    );
    behavior.append(&row);
    let labels = SECONDARY_LABELS.map(|id| tr.get(id));
    let secondary = gtk::DropDown::from_strings(&labels.each_ref().map(String::as_str));
    secondary.set_valign(gtk::Align::Center);
    behavior.append(&widgets::row(&tr.get("mouse-secondary-click"), &secondary));
    let double_click = widgets::step_scale(DOUBLE_CLICKS.len());
    behavior.append(&widgets::slider_row(
        &tr.get("mouse-double-click"),
        &double_click,
        &tr.get("mouse-slow"),
        &tr.get("mouse-fast"),
    ));
    let scroll = widgets::step_scale(SCROLL_SPEEDS.len());
    behavior.append(&widgets::slider_row(
        &tr.get("mouse-scroll-speed"),
        &scroll,
        &tr.get("mouse-slow"),
        &tr.get("mouse-fast"),
    ));

    let pointer = widgets::group(&column, Some(&tr.get("mouse-pointer")), None);
    let size = widgets::step_scale(CURSOR_SIZES.len());
    pointer.append(&widgets::slider_row(
        &tr.get("mouse-pointer-size"),
        &size,
        &tr.get("mouse-small"),
        &tr.get("mouse-large"),
    ));

    let devices = widgets::group(&column, Some(&tr.get("mouse-devices")), None);

    bind_double_click(&double_click);

    let page = Rc::new(MousePage {
        context: context.clone(),
        compositor: RefCell::new(None),
        subscription: RefCell::new(None),
        speed: speed.clone(),
        natural: natural.clone(),
        secondary: secondary.clone(),
        scroll: scroll.clone(),
        size: size.clone(),
        devices,
        loading: Cell::new(false),
        debounce: Debounce::default(),
    });
    page.set_available(false);

    for scale in [&speed, &scroll, &size] {
        let weak = Rc::downgrade(&page);
        scale.connect_change_value(move |scale, _, value| {
            // 눈금에 맞춘다
            scale.set_value(value.round());
            if let Some(page) = weak.upgrade() {
                page.schedule_apply();
            }
            glib::Propagation::Stop
        });
    }
    let weak = Rc::downgrade(&page);
    natural.connect_active_notify(move |_| {
        if let Some(page) = weak.upgrade() {
            page.schedule_apply();
        }
    });
    let weak = Rc::downgrade(&page);
    secondary.connect_selected_notify(move |_| {
        if let Some(page) = weak.upgrade() {
            page.schedule_apply();
        }
    });
    // 컴포지터가 나중에 뜰 수 있으니 보일 때마다 다시 읽는다
    let weak = Rc::downgrade(&page);
    widgets::while_mapped(
        &scroller,
        move || {
            if let Some(page) = weak.upgrade() {
                glib::spawn_future_local(async move { page.start().await });
            }
        },
        || {},
    );
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

/// 두 번 클릭 속도는 GTK가 읽는 공용 설정에 바로 쓴다
fn bind_double_click(scale: &gtk::Scale) {
    let settings = settings_for(MOUSE_SCHEMA).filter(|settings| has_key(settings, "double-click"));
    let Some(settings) = settings else {
        scale.set_sensitive(false);
        return;
    };
    scale.set_value(nearest(&DOUBLE_CLICKS, f64::from(settings.int("double-click"))) as f64);
    scale.connect_change_value(move |scale, _, value| {
        scale.set_value(value.round());
        let _ = settings.set_int("double-click", step(&DOUBLE_CLICKS, scale));
        glib::Propagation::Stop
    });
}

impl MousePage {
    async fn start(self: Rc<Self>) {
        let Ok(bus) = dbus::session().await else {
            return;
        };
        if self.subscription.borrow().is_none() {
            let weak = Rc::downgrade(&self);
            let subscription = dbus::subscribe(
                &bus,
                Some(COMPOSITOR),
                Some(COMPOSITOR_INTERFACE),
                Some("PointerDevicesChanged"),
                None,
                move |_| {
                    if let Some(page) = weak.upgrade() {
                        glib::spawn_future_local(async move { page.refresh_devices().await });
                    }
                },
            );
            self.subscription.replace(Some(subscription));
        }
        let compositor = Remote::new(&bus, COMPOSITOR, COMPOSITOR_PATH);
        self.compositor.replace(Some(compositor.clone()));
        self.refresh_settings(&compositor).await;
        self.refresh_devices().await;
    }

    async fn refresh_settings(&self, compositor: &Remote) {
        let reply = compositor
            .call(COMPOSITOR_INTERFACE, "GetPointer", None)
            .await;
        let Ok(reply) = reply else {
            self.set_available(false);
            return;
        };
        self.loading.set(true);
        let speed = reply.child_value(0).get::<f64>().unwrap_or(0.0);
        let natural = reply.child_value(1).get::<bool>().unwrap_or(false);
        let left_handed = reply.child_value(2).get::<bool>().unwrap_or(false);
        let scroll = reply.child_value(3).get::<f64>().unwrap_or(1.0);
        let size = reply.child_value(4).get::<u32>().unwrap_or(24);
        self.speed.set_value(nearest(&SPEEDS, speed) as f64);
        self.natural.set_active(natural);
        self.secondary.set_selected(u32::from(left_handed));
        self.scroll
            .set_value(nearest(&SCROLL_SPEEDS, scroll) as f64);
        self.size
            .set_value(nearest(&CURSOR_SIZES, f64::from(size)) as f64);
        self.loading.set(false);
        self.set_available(true);
    }

    fn set_available(&self, available: bool) {
        for widget in [
            self.speed.upcast_ref::<gtk::Widget>(),
            self.natural.upcast_ref(),
            self.secondary.upcast_ref(),
            self.scroll.upcast_ref(),
            self.size.upcast_ref(),
        ] {
            widget.set_sensitive(available);
        }
    }

    async fn refresh_devices(&self) {
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        let devices = match compositor
            .call(COMPOSITOR_INTERFACE, "GetPointerDevices", None)
            .await
        {
            Ok(reply) => reply.child_value(0).iter().collect::<Vec<_>>(),
            Err(_) => Vec::new(),
        };
        let tr = &self.context.tr;
        while let Some(row) = self.devices.first_child() {
            self.devices.remove(&row);
        }
        if devices.is_empty() {
            self.devices
                .append(&widgets::info_row(&tr.get("mouse-no-devices"), ""));
            return;
        }
        for device in devices {
            let text = |key| dbus::lookup(&device, key).and_then(|value| value.get::<String>());
            let name = text("name").unwrap_or_default();
            let bus = text("bus").unwrap_or_default();
            let touchpad = dbus::lookup(&device, "touchpad")
                .and_then(|value| value.get::<bool>())
                .unwrap_or(false);
            // 이름을 알 수 없는 장치는 macOS처럼 연결 방식으로 부른다
            let title = known_name(&name)
                .map(str::to_owned)
                .unwrap_or_else(|| tr.get("mouse-device-unknown"));
            let subtitle = if touchpad {
                Some(tr.get("mouse-device-touchpad"))
            } else {
                match bus.as_str() {
                    "usb" => Some(tr.get("mouse-device-usb")),
                    "bluetooth" => Some(tr.get("mouse-device-bluetooth")),
                    _ => None,
                }
            };
            let leading = badge(ICON, Tint::Gray, BadgeSize::Small);
            let (row, _) =
                widgets::rich_row(Some(leading.upcast_ref()), &title, subtitle.as_deref());
            self.devices.append(&row);
        }
    }

    fn schedule_apply(self: &Rc<Self>) {
        if self.loading.get() {
            return;
        }
        let weak = Rc::downgrade(self);
        self.debounce.run(APPLY_DELAY, move || {
            let Some(page) = weak.upgrade() else {
                return;
            };
            let Some(compositor) = page.compositor.borrow().clone() else {
                return;
            };
            let size = step(&CURSOR_SIZES, &page.size);
            // GTK 앱이 커서를 직접 그릴 때도 같은 크기를 쓰게 한다
            if let Some(settings) =
                interface_settings().filter(|settings| has_key(settings, "cursor-size"))
            {
                let _ = settings.set_int("cursor-size", size as i32);
            }
            let args = (
                step(&SPEEDS, &page.speed),
                page.natural.is_active(),
                page.secondary.selected() == 1,
                step(&SCROLL_SPEEDS, &page.scroll),
                size,
            )
                .to_variant();
            glib::spawn_future_local(async move {
                if let Err(err) = compositor
                    .call(COMPOSITOR_INTERFACE, "SetPointer", Some(args))
                    .await
                {
                    page.context
                        .notify_error("mouse-error", dbus::message(&err));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_names_fall_back_to_the_connection() {
        assert_eq!(known_name("Logitech M705"), Some("Logitech M705"));
        assert_eq!(known_name("  "), None);
        assert_eq!(known_name("HID 093a:2510"), None);
        assert_eq!(known_name("HID Mouse"), Some("HID Mouse"));
    }

    #[test]
    fn values_snap_to_the_nearest_step() {
        assert_eq!(nearest(&SPEEDS, 0.1), 4);
        assert_eq!(nearest(&CURSOR_SIZES, 25.0), 2);
        assert_eq!(nearest(&DOUBLE_CLICKS, 400.0), 4);
        assert_eq!(nearest(&SCROLL_SPEEDS, 1.0), 3);
    }
}
