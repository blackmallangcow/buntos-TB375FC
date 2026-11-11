// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 트랙패드 (macOS 26과 같은 이름과 순서): 위의 분할 단추로 "포인트 및 클릭"(이동 속도, 보조 클릭,
//! 탭하여 클릭하기)과 "스크롤 및 확대/축소"(자연스러운 스크롤)를 오간다. 컴포지터 API
//! (org.buntos.Compositor1의 GetTouchpad, SetTouchpad)가 연결된 트랙패드에 쓴다. 자연스러운 스크롤은
//! macOS처럼 마우스와 같은 값이다. 탭하여 클릭하기는 macOS와 달리 기본으로 켜져 있다. 확대/축소,
//! 회전, 추가 제스처, 강제 클릭은 아직 없다

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gtk4::{self as gtk, gio, glib, prelude::*};

use super::Context;
use crate::{
    dbus::{self, Debounce, Remote},
    widgets::{self, BadgeSize, Tint, badge},
};

const COMPOSITOR: &str = "org.buntos.Compositor";
const COMPOSITOR_PATH: &str = "/org/buntos/Compositor";
const COMPOSITOR_INTERFACE: &str = "org.buntos.Compositor1";
const ICON: &str = "input-touchpad-symbolic";

/// 이동 속도 눈금 (libinput 가속, 가운데가 기본). 마우스 페이지와 같다
const SPEEDS: [f64; 9] = [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0];
/// 드롭다운 항목 순서대로 보조 클릭의 컴포지터 이름과 Fluent 메시지 ID
const SECONDARY: [(&str, &str); 2] = [
    ("two-fingers", "trackpad-secondary-two-fingers"),
    ("bottom-right", "trackpad-secondary-bottom-right"),
];
const APPLY_DELAY: Duration = Duration::from_millis(250);

struct TrackpadPage {
    context: Rc<Context>,
    compositor: RefCell<Option<Remote>>,
    subscription: RefCell<Option<gio::SignalSubscription>>,
    speed: gtk::Scale,
    secondary: gtk::DropDown,
    tap: gtk::Switch,
    natural: gtk::Switch,
    /// 연결된 트랙패드가 없을 때만 보이는 안내
    missing: gtk::ListBox,
    /// 컴포지터에서 읽은 값을 위젯에 넣는 동안은 다시 보내지 않는다
    loading: Cell<bool>,
    debounce: Debounce,
}

/// 가장 가까운 눈금의 번호
fn nearest(steps: &[f64], value: f64) -> usize {
    (0..steps.len())
        .min_by(|&a, &b| {
            (steps[a] - value)
                .abs()
                .total_cmp(&(steps[b] - value).abs())
        })
        .unwrap_or(0)
}

fn step(steps: &[f64], scale: &gtk::Scale) -> f64 {
    steps[(scale.value().round().max(0.0) as usize).min(steps.len() - 1)]
}

/// 컴포지터의 보조 클릭 이름에 해당하는 드롭다운 번호. 모르는 이름은 첫 항목(두 손가락)이다
fn secondary_index(name: &str) -> u32 {
    SECONDARY
        .iter()
        .position(|(id, _)| *id == name)
        .unwrap_or(0) as u32
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Gray, BadgeSize::Medium),
        &tr.get("page-trackpad"),
        &tr.get("trackpad-description"),
        None,
    ));

    let missing = widgets::group(&column, None, None);
    missing.append(&widgets::info_row(&tr.get("trackpad-none"), ""));
    widgets::set_group_visible(&missing, false);

    // macOS처럼 위의 분할 단추로 묶음을 오간다 (사운드 페이지의 출력·입력과 같다)
    let tabs = widgets::group(&column, None, None);
    let segments = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .css_classes(["segmented"])
        .build();
    let point_button = gtk::ToggleButton::builder()
        .label(tr.get("trackpad-point-click"))
        .active(true)
        .build();
    let scroll_button = gtk::ToggleButton::builder()
        .label(tr.get("trackpad-scroll-zoom"))
        .group(&point_button)
        .build();
    segments.append(&point_button);
    segments.append(&scroll_button);
    tabs.append(
        &gtk::ListBoxRow::builder()
            .child(&segments)
            .activatable(false)
            .css_classes(["segmented-row"])
            .build(),
    );

    let point = widgets::group(&column, None, None);
    let speed = widgets::step_scale(SPEEDS.len());
    point.append(&widgets::slider_row(
        &tr.get("trackpad-speed"),
        &speed,
        &tr.get("mouse-slow"),
        &tr.get("mouse-fast"),
    ));
    let labels = SECONDARY.map(|(_, id)| tr.get(id));
    let secondary = gtk::DropDown::from_strings(&labels.each_ref().map(String::as_str));
    secondary.set_valign(gtk::Align::Center);
    point.append(&widgets::row(
        &tr.get("trackpad-secondary-click"),
        &secondary,
    ));
    let (row, tap) = widgets::switch_row(
        &tr.get("trackpad-tap-to-click"),
        Some(&tr.get("trackpad-tap-to-click-description")),
    );
    point.append(&row);

    let scroll = widgets::group(&column, None, None);
    let (row, natural) = widgets::switch_row(
        &tr.get("trackpad-natural-scroll"),
        Some(&tr.get("trackpad-natural-scroll-description")),
    );
    scroll.append(&row);
    widgets::set_group_visible(&scroll, false);

    let shown = (point.clone(), scroll.clone());
    point_button.connect_toggled(move |button| {
        widgets::set_group_visible(&shown.0, button.is_active());
        widgets::set_group_visible(&shown.1, !button.is_active());
    });

    let page = Rc::new(TrackpadPage {
        context: context.clone(),
        compositor: RefCell::new(None),
        subscription: RefCell::new(None),
        speed: speed.clone(),
        secondary: secondary.clone(),
        tap: tap.clone(),
        natural: natural.clone(),
        missing,
        loading: Cell::new(false),
        debounce: Debounce::default(),
    });
    page.set_available(false);

    let weak = Rc::downgrade(&page);
    speed.connect_change_value(move |scale, _, value| {
        // 눈금에 맞춘다
        scale.set_value(value.round());
        if let Some(page) = weak.upgrade() {
            page.schedule_apply();
        }
        glib::Propagation::Stop
    });
    for switch in [&tap, &natural] {
        let weak = Rc::downgrade(&page);
        switch.connect_active_notify(move |_| {
            if let Some(page) = weak.upgrade() {
                page.schedule_apply();
            }
        });
    }
    let weak = Rc::downgrade(&page);
    secondary.connect_selected_notify(move |_| {
        if let Some(page) = weak.upgrade() {
            page.schedule_apply();
        }
    });
    // 컴포지터가 나중에 뜰 수 있고 마우스 페이지가 자연스러운 스크롤을 바꿀 수 있으니 보일 때마다
    // 다시 읽는다
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

impl TrackpadPage {
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
            .call(COMPOSITOR_INTERFACE, "GetTouchpad", None)
            .await;
        let Ok(reply) = reply else {
            self.set_available(false);
            return;
        };
        self.loading.set(true);
        let tap = reply.child_value(0).get::<bool>().unwrap_or(true);
        let secondary = reply.child_value(1).get::<String>().unwrap_or_default();
        let speed = reply.child_value(2).get::<f64>().unwrap_or(0.0);
        let natural = reply.child_value(3).get::<bool>().unwrap_or(false);
        self.tap.set_active(tap);
        self.secondary.set_selected(secondary_index(&secondary));
        self.speed.set_value(nearest(&SPEEDS, speed) as f64);
        self.natural.set_active(natural);
        self.loading.set(false);
        self.set_available(true);
    }

    fn set_available(&self, available: bool) {
        for widget in [
            self.speed.upcast_ref::<gtk::Widget>(),
            self.secondary.upcast_ref(),
            self.tap.upcast_ref(),
            self.natural.upcast_ref(),
        ] {
            widget.set_sensitive(available);
        }
    }

    /// 연결된 트랙패드가 없으면 안내를 보인다. 설정은 그대로 두고, 트랙패드를 연결하면 쓴다
    async fn refresh_devices(&self) {
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        let devices = match compositor
            .call(COMPOSITOR_INTERFACE, "GetPointerDevices", None)
            .await
        {
            Ok(reply) => reply.child_value(0).iter().collect::<Vec<_>>(),
            Err(_) => return,
        };
        let connected = devices.iter().any(|device| {
            dbus::lookup(device, "touchpad")
                .and_then(|value| value.get::<bool>())
                .unwrap_or(false)
        });
        widgets::set_group_visible(&self.missing, !connected);
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
            let index = (page.secondary.selected() as usize).min(SECONDARY.len() - 1);
            let args = (
                page.tap.is_active(),
                SECONDARY[index].0,
                step(&SPEEDS, &page.speed),
                page.natural.is_active(),
            )
                .to_variant();
            glib::spawn_future_local(async move {
                if let Err(err) = compositor
                    .call(COMPOSITOR_INTERFACE, "SetTouchpad", Some(args))
                    .await
                {
                    page.context
                        .notify_error("trackpad-error", dbus::message(&err));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_names_map_to_the_dropdown() {
        assert_eq!(secondary_index("two-fingers"), 0);
        assert_eq!(secondary_index("bottom-right"), 1);
        // 모르는 이름은 macOS의 기본(두 손가락)이다
        assert_eq!(secondary_index("off"), 0);
    }

    #[test]
    fn speeds_snap_to_the_nearest_step() {
        assert_eq!(nearest(&SPEEDS, 0.1), 4);
        assert_eq!(nearest(&SPEEDS, -1.0), 0);
        assert_eq!(nearest(&SPEEDS, 0.9), 8);
    }
}
