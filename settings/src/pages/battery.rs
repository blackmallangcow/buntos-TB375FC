// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 배터리: 남은 양과 충전 상태(UPower), 에너지 모드(power-profiles-daemon), 메뉴 막대의 배터리 표시.
//! 배터리가 없는 기기(데스크톱, 가상 머신)는 전원 어댑터와 에너지 모드만 보인다

use std::{cell::RefCell, rc::Rc, time::Duration};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use super::{Context, bind_switch, shell_settings};
use crate::{
    dbus::{self, Debounce, Props, Remote},
    widgets::{self, BadgeSize, Tint, badge},
};

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const UPOWER_DEVICE: &str = "org.freedesktop.UPower.Device";
/// power-profiles-daemon의 (이름, 경로, 인터페이스). 새 이름을 먼저, 없으면 옛 이름을 쓴다
const PROFILES: [(&str, &str, &str); 2] = [
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    ),
    (
        "net.hadess.PowerProfiles",
        "/net/hadess/PowerProfiles",
        "net.hadess.PowerProfiles",
    ),
];
/// 에너지 모드를 보이는 순서와 (이름, 설명) 메시지 ID
const MODES: [(&str, &str, &str); 3] = [
    (
        "power-saver",
        "battery-mode-power-saver",
        "battery-mode-power-saver-description",
    ),
    (
        "balanced",
        "battery-mode-balanced",
        "battery-mode-balanced-description",
    ),
    (
        "performance",
        "battery-mode-performance",
        "battery-mode-performance-description",
    ),
];
const REFRESH_DELAY: Duration = Duration::from_millis(300);

/// UPower 장치 종류와 상태 값
const TYPE_BATTERY: u32 = 2;
const STATE_CHARGING: u32 = 1;
const STATE_DISCHARGING: u32 = 2;
const STATE_FULL: u32 = 4;

#[derive(Clone, Debug, Default)]
struct Battery {
    percentage: f64,
    state: u32,
    time_to_empty: i64,
    time_to_full: i64,
    /// 설계 용량 대비 지금 최대 용량 (%)
    capacity: Option<f64>,
}

struct Profiles {
    remote: Remote,
    interface: &'static str,
    available: Vec<String>,
    active: String,
    degraded: String,
}

struct BatteryPage {
    context: Rc<Context>,
    content: gtk::Box,
    upower: RefCell<Option<Remote>>,
    profiles: RefCell<Option<Profiles>>,
    debounce: Debounce,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let (scroller, column) = widgets::page();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .build();
    column.append(&content);
    // 메뉴 막대 묶음은 다시 그리지 않으므로 따로 둔다
    menu_bar_group(context, &column);

    let page = Rc::new(BatteryPage {
        context: context.clone(),
        content,
        upower: RefCell::new(None),
        profiles: RefCell::new(None),
        debounce: Debounce::default(),
        subscriptions: RefCell::default(),
    });
    glib::spawn_future_local(page.clone().start());
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

fn menu_bar_group(context: &Context, column: &gtk::Box) {
    let tr = &context.tr;
    let Some(settings) = shell_settings() else {
        return;
    };
    let list = widgets::group(column, Some(&tr.get("battery-menu-bar")), None);
    let (row, switch) = widgets::switch_row(&tr.get("battery-show-in-menu-bar"), None);
    bind_switch(&settings, "show-battery", &switch);
    list.append(&row);
    let (row, switch) = widgets::switch_row(&tr.get("battery-show-percentage"), None);
    bind_switch(&settings, "show-battery-percentage", &switch);
    list.append(&row);
}

/// 초를 "2시간 15분"처럼
pub fn duration_text(tr: &Translations, seconds: i64) -> String {
    let minutes = (seconds.max(0) + 30) / 60;
    let mut args = FluentArgs::new();
    args.set("hours", minutes / 60);
    args.set("minutes", minutes % 60);
    if minutes >= 60 {
        tr.get_with("duration-hours-minutes", &args)
    } else {
        tr.get_with("duration-minutes", &args)
    }
}

impl BatteryPage {
    async fn start(self: Rc<Self>) {
        let Ok(bus) = dbus::system().await else {
            self.render(None, false);
            return;
        };
        let mut subscriptions = Vec::new();
        for sender in [UPOWER, PROFILES[0].0, PROFILES[1].0] {
            let weak = Rc::downgrade(&self);
            subscriptions.push(dbus::subscribe(
                &bus,
                Some(sender),
                Some(dbus::PROPERTIES),
                Some("PropertiesChanged"),
                None,
                move |_| {
                    if let Some(page) = weak.upgrade() {
                        page.schedule_refresh();
                    }
                },
            ));
        }
        self.subscriptions.replace(subscriptions);
        self.upower
            .replace(Some(Remote::new(&bus, UPOWER, DISPLAY_DEVICE)));
        self.refresh().await;
    }

    fn schedule_refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.debounce.run(REFRESH_DELAY, move || {
            if let Some(page) = weak.upgrade() {
                glib::spawn_future_local(async move { page.refresh().await });
            }
        });
    }

    async fn refresh(self: &Rc<Self>) {
        let Some(display) = self.upower.borrow().clone() else {
            return;
        };
        let mut battery = None;
        let mut on_battery = false;
        if let Ok(props) = display.get_all(UPOWER_DEVICE).await
            && props.u32("Type") == TYPE_BATTERY
            && props.bool("IsPresent")
        {
            let mut found = Battery {
                percentage: props.get::<f64>("Percentage").unwrap_or(0.0),
                state: props.u32("State"),
                time_to_empty: props.get::<i64>("TimeToEmpty").unwrap_or(0),
                time_to_full: props.get::<i64>("TimeToFull").unwrap_or(0),
                capacity: None,
            };
            found.capacity = battery_capacity(&display).await;
            battery = Some(found);
        }
        if let Ok(props) = display.at(UPOWER_PATH).get_all(UPOWER).await {
            on_battery = props.bool("OnBattery");
        }
        self.profiles.replace(read_profiles(&display.bus).await);
        self.render(battery, on_battery);
    }

    fn render(self: &Rc<Self>, battery: Option<Battery>, on_battery: bool) {
        let tr = &self.context.tr;
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        let hero = widgets::group(&self.content, None, None);
        let (icon, tint, description) = match &battery {
            Some(battery) => (
                level_icon(battery),
                Tint::Green,
                status_text(tr, battery, on_battery),
            ),
            None => (
                "ac-adapter-symbolic",
                Tint::Gray,
                tr.get("battery-no-battery"),
            ),
        };
        hero.append(&widgets::hero_row(
            &badge(icon, tint, BadgeSize::Medium),
            &tr.get("page-battery"),
            &description,
            None,
        ));

        if let Some(battery) = &battery {
            let list = widgets::group(&self.content, Some(&tr.get("battery-status")), None);
            let level = gtk::LevelBar::builder()
                .min_value(0.0)
                .max_value(100.0)
                .value(battery.percentage)
                .width_request(160)
                .valign(gtk::Align::Center)
                .build();
            let percent = gtk::Label::builder()
                .label(format!("{:.0}%", battery.percentage))
                .css_classes(["row-value"])
                .build();
            let (row, accessories) = widgets::rich_row(None, &tr.get("battery-level"), None);
            accessories.append(&level);
            accessories.append(&percent);
            list.append(&row);
            list.append(&widgets::info_row(
                &tr.get("battery-state"),
                &status_text(tr, battery, on_battery),
            ));
            if let Some(capacity) = battery.capacity {
                let mut args = FluentArgs::new();
                args.set("capacity", format!("{capacity:.0}"));
                let id = if capacity >= 80.0 {
                    "battery-health-normal"
                } else {
                    "battery-health-service"
                };
                list.append(&widgets::info_row(
                    &tr.get("battery-health"),
                    &tr.get_with(id, &args),
                ));
            }
        } else {
            let list = widgets::group(&self.content, None, None);
            let icon = widgets::symbol("ac-adapter-symbolic");
            let (row, accessories) = widgets::rich_row(
                Some(icon.upcast_ref()),
                &tr.get("battery-power-source"),
                None,
            );
            accessories.append(
                &gtk::Label::builder()
                    .label(tr.get("battery-power-adapter"))
                    .css_classes(["row-value"])
                    .build(),
            );
            list.append(&row);
        }
        self.render_modes();
    }

    /// 에너지 모드: 하나만 고르는 행들
    fn render_modes(self: &Rc<Self>) {
        let tr = &self.context.tr;
        let profiles = self.profiles.borrow();
        let Some(profiles) = profiles.as_ref() else {
            widgets::group(
                &self.content,
                Some(&tr.get("battery-modes")),
                Some(&tr.get("battery-modes-unavailable")),
            );
            return;
        };
        let degraded = (!profiles.degraded.is_empty()).then(|| tr.get("battery-mode-degraded"));
        let list = widgets::group(
            &self.content,
            Some(&tr.get("battery-modes")),
            degraded.as_deref(),
        );
        let mut first: Option<gtk::CheckButton> = None;
        for (value, title, description) in MODES {
            if !profiles
                .available
                .iter()
                .any(|available| available == value)
            {
                continue;
            }
            let check = gtk::CheckButton::builder()
                .active(profiles.active == value)
                .valign(gtk::Align::Center)
                .build();
            match &first {
                Some(first) => check.set_group(Some(first)),
                None => first = Some(check.clone()),
            }
            let (row, _) = widgets::rich_row(
                Some(check.upcast_ref()),
                &tr.get(title),
                Some(&tr.get(description)),
            );
            row.set_activatable(true);
            let page = self.clone();
            check.connect_toggled(move |check| {
                if check.is_active() {
                    page.set_mode(value);
                }
            });
            list.append(&row);
        }
        list.connect_row_activated(|_, row| {
            if let Some(check) = row
                .child()
                .and_then(|content| content.first_child())
                .and_downcast::<gtk::CheckButton>()
            {
                check.set_active(true);
            }
        });
    }

    fn set_mode(self: &Rc<Self>, value: &'static str) {
        let target = self
            .profiles
            .borrow()
            .as_ref()
            .map(|profiles| (profiles.remote.clone(), profiles.interface));
        let Some((remote, interface)) = target else {
            return;
        };
        if self
            .profiles
            .borrow()
            .as_ref()
            .is_some_and(|profiles| profiles.active == value)
        {
            return;
        }
        let page = self.clone();
        glib::spawn_future_local(async move {
            if let Err(err) = remote
                .set(interface, "ActiveProfile", value.to_variant())
                .await
            {
                page.context
                    .notify_error("battery-mode-error", dbus::message(&err));
            }
        });
    }
}

/// 배터리 장치의 최대 용량. 여러 개면 첫 번째
async fn battery_capacity(display: &Remote) -> Option<f64> {
    let reply = display
        .at(UPOWER_PATH)
        .call(UPOWER, "EnumerateDevices", None)
        .await
        .ok()?;
    for path in dbus::paths(&reply.child_value(0)) {
        let Ok(props) = display.at(path).get_all(UPOWER_DEVICE).await else {
            continue;
        };
        if props.u32("Type") == TYPE_BATTERY && props.bool("PowerSupply") {
            let capacity = props.get::<f64>("Capacity").unwrap_or(0.0);
            return (capacity > 0.0).then_some(capacity);
        }
    }
    None
}

async fn read_profiles(bus: &gio::DBusConnection) -> Option<Profiles> {
    for (name, path, interface) in PROFILES {
        let remote = Remote::new(bus, name, path);
        let Ok(props) = remote.get_all(interface).await else {
            continue;
        };
        let available = props
            .value("Profiles")
            .map(|list| {
                list.iter()
                    .map(|entry| Props::new(entry).string("Profile"))
                    .collect()
            })
            .unwrap_or_default();
        return Some(Profiles {
            remote,
            interface,
            available,
            active: props.string("ActiveProfile"),
            degraded: props.string("PerformanceDegraded"),
        });
    }
    None
}

fn level_icon(battery: &Battery) -> &'static str {
    let charging = battery.state == STATE_CHARGING;
    match (battery.percentage as u32, charging) {
        (_, _) if battery.state == STATE_FULL => "battery-full-charged-symbolic",
        (80.., false) => "battery-full-symbolic",
        (80.., true) => "battery-full-charging-symbolic",
        (40.., false) => "battery-good-symbolic",
        (40.., true) => "battery-good-charging-symbolic",
        (15.., false) => "battery-low-symbolic",
        (15.., true) => "battery-low-charging-symbolic",
        (_, false) => "battery-caution-symbolic",
        (_, true) => "battery-caution-charging-symbolic",
    }
}

fn status_text(tr: &Translations, battery: &Battery, on_battery: bool) -> String {
    let mut args = FluentArgs::new();
    args.set("percent", format!("{:.0}", battery.percentage));
    match battery.state {
        STATE_CHARGING if battery.time_to_full > 0 => {
            args.set("time", duration_text(tr, battery.time_to_full));
            tr.get_with("battery-charging-time", &args)
        }
        STATE_CHARGING => tr.get_with("battery-charging", &args),
        STATE_FULL => tr.get_with("battery-full", &args),
        STATE_DISCHARGING if battery.time_to_empty > 0 => {
            args.set("time", duration_text(tr, battery.time_to_empty));
            tr.get_with("battery-discharging-time", &args)
        }
        _ if on_battery => tr.get_with("battery-discharging", &args),
        _ => tr.get_with("battery-not-charging", &args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KO: &str = include_str!("../../../i18n/ko/bunt-settings.ftl");

    #[test]
    fn durations_read_naturally() {
        let tr = Translations::from_source("ko", KO);
        assert_eq!(duration_text(&tr, 45 * 60), "45분");
        assert_eq!(duration_text(&tr, 2 * 3600 + 15 * 60), "2시간 15분");
    }

    #[test]
    fn charging_icons_follow_level() {
        let battery = |percentage, state| Battery {
            percentage,
            state,
            ..Battery::default()
        };
        assert_eq!(
            level_icon(&battery(90.0, STATE_DISCHARGING)),
            "battery-full-symbolic"
        );
        assert_eq!(
            level_icon(&battery(50.0, STATE_CHARGING)),
            "battery-good-charging-symbolic"
        );
        assert_eq!(
            level_icon(&battery(5.0, STATE_DISCHARGING)),
            "battery-caution-symbolic"
        );
        assert_eq!(
            level_icon(&battery(100.0, STATE_FULL)),
            "battery-full-charged-symbolic"
        );
    }
}
