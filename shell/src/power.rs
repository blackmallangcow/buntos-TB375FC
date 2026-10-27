// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 배터리(전원) 항목. 배터리가 있으면 남은 양과 충전 상태를, 없으면(데스크톱, VM)
//! 전원 어댑터를 아이콘으로 보인다. 누르면 상태와 전원 모드(저전력 모드, 자동, 고성능)가 나오고,
//! 모드는 고르는 즉시 적용된다 (UPower, power-profiles-daemon).

use std::{cell::RefCell, rc::Rc};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    dbus,
    extras::{self, Listeners, Menu, Visibility},
    menubar::open_settings,
    shell_settings,
};

const UPOWER: &str = "org.freedesktop.UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// UPower Device.Type: 배터리
const TYPE_BATTERY: u32 = 2;
/// UPower Device.State
const CHARGING: u32 = 1;
const FULLY_CHARGED: u32 = 4;
const PENDING_CHARGE: u32 = 5;

/// 전원 모드: power-profiles-daemon의 프로필 이름과 메뉴 문구. 메뉴에 놓는 순서다
const MODES: [(&str, &str); 3] = [
    ("power-saver", "power-mode-low"),
    ("balanced", "power-mode-automatic"),
    ("performance", "power-mode-high"),
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Battery {
    pub percentage: f64,
    pub state: u32,
    /// 초. 모르면 0
    pub time_to_empty: i64,
    pub time_to_full: i64,
}

impl Battery {
    pub fn charging(&self) -> bool {
        matches!(self.state, CHARGING | FULLY_CHARGED | PENDING_CHARGE)
    }
}

/// 배터리 아이콘: 10% 단위 (Adwaita의 battery-level-N 아이콘)
pub fn battery_icon(battery: &Battery) -> String {
    if battery.state == FULLY_CHARGED {
        return "battery-level-100-charged-symbolic".into();
    }
    let level = ((battery.percentage / 10.0).round() as u32 * 10).min(100);
    let suffix = if battery.charging() { "-charging" } else { "" };
    format!("battery-level-{level}{suffix}-symbolic")
}

/// 전원 상태. 복제해도 같은 상태를 가리킨다. 제어 센터도 같은 것을 쓴다
#[derive(Clone)]
pub struct Power(Rc<Inner>);

struct Inner {
    /// 배터리가 없으면 None (전원 어댑터)
    battery: RefCell<Option<Battery>>,
    profiles: RefCell<Option<gio::DBusProxy>>,
    listeners: Listeners<Power>,
}

impl Power {
    pub fn start() -> Self {
        let power = Self(Rc::new(Inner {
            battery: RefCell::default(),
            profiles: RefCell::default(),
            listeners: RefCell::default(),
        }));
        let weak = Rc::downgrade(&power.0);
        dbus::watch(
            gio::BusType::System,
            UPOWER,
            DISPLAY_DEVICE,
            DEVICE_INTERFACE,
            move |proxy| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let present = dbus::is_running(proxy)
                    && dbus::property::<u32>(proxy, "Type") == Some(TYPE_BATTERY)
                    && dbus::property::<bool>(proxy, "IsPresent").unwrap_or(false);
                let battery = present.then(|| Battery {
                    percentage: dbus::property(proxy, "Percentage").unwrap_or(0.0),
                    state: dbus::property(proxy, "State").unwrap_or(0),
                    time_to_empty: dbus::property(proxy, "TimeToEmpty").unwrap_or(0),
                    time_to_full: dbus::property(proxy, "TimeToFull").unwrap_or(0),
                });
                inner.battery.replace(battery);
                Power(inner).notify();
            },
        );
        let weak = Rc::downgrade(&power.0);
        dbus::watch(
            gio::BusType::System,
            PROFILES,
            PROFILES_PATH,
            PROFILES,
            move |proxy| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.profiles.replace(Some(proxy.clone()));
                Power(inner).notify();
            },
        );
        power
    }

    fn notify(&self) {
        for listener in self.0.listeners.borrow().iter() {
            listener(self);
        }
    }

    pub fn subscribe(&self, listener: impl Fn(&Power) + 'static) {
        listener(self);
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    pub fn battery(&self) -> Option<Battery> {
        self.0.battery.borrow().clone()
    }

    /// power-profiles-daemon이 주는 프로필 이름들 (없으면 빈 목록)
    pub fn modes(&self) -> Vec<String> {
        let profiles = self.0.profiles.borrow();
        let Some(proxy) = profiles.as_ref().filter(|proxy| dbus::is_running(proxy)) else {
            return Vec::new();
        };
        let list: Vec<std::collections::HashMap<String, glib::Variant>> =
            dbus::property(proxy, "Profiles").unwrap_or_default();
        list.iter()
            .filter_map(|profile| profile.get("Profile")?.get::<String>())
            .collect()
    }

    pub fn active_mode(&self) -> Option<String> {
        let profiles = self.0.profiles.borrow();
        dbus::property(profiles.as_ref()?, "ActiveProfile")
    }

    pub fn set_mode(&self, mode: &str) {
        if let Some(proxy) = self.0.profiles.borrow().as_ref() {
            dbus::set_property(proxy, "ActiveProfile", mode.to_variant());
        }
    }

    /// 저전력 모드인지 (제어 센터의 칸)
    pub fn low_power(&self) -> bool {
        self.active_mode().as_deref() == Some("power-saver")
    }
}

/// "2시간 15분" 같은 남은 시간
fn duration_text(tr: &Translations, seconds: i64) -> String {
    let minutes = seconds / 60;
    let mut args = FluentArgs::new();
    args.set("hours", minutes / 60);
    args.set("minutes", minutes % 60);
    tr.get_with("power-duration", &args)
}

pub fn extra(tr: &Rc<Translations>, power: &Power) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name("ac-adapter-symbolic");
    let percent = gtk::Label::builder()
        .css_classes(["status-percent"])
        .visible(false)
        .build();
    let content = gtk::Box::builder().spacing(4).build();
    content.append(&percent);
    content.append(&image);
    let menu = Menu::new();
    let button = extras::button(&content, &menu, &tr.get("power-title"));
    let visibility = Visibility::new(&button, "show-battery");
    visibility.set_available(true);

    // 남은 양(%) 보기는 설정을 따른다
    let settings = shell_settings();
    let show_percent = {
        let settings = settings.clone();
        move || {
            settings
                .as_ref()
                .is_some_and(|settings| settings.boolean("show-battery-percentage"))
        }
    };
    let update = {
        let tr = tr.clone();
        let show_percent = show_percent.clone();
        move |power: &Power| {
            let _ = &visibility;
            match power.battery() {
                Some(battery) => {
                    image.set_icon_name(Some(&battery_icon(&battery)));
                    percent.set_text(&format!("{:.0}%", battery.percentage));
                    percent.set_visible(show_percent());
                    button_tooltip(&image, &tr, Some(&battery));
                }
                None => {
                    image.set_icon_name(Some("ac-adapter-symbolic"));
                    percent.set_visible(false);
                    button_tooltip(&image, &tr, None);
                }
            }
        }
    };
    let update = Rc::new(update);
    let on_change = update.clone();
    power.subscribe(move |power| on_change(power));
    if let Some(settings) = settings {
        let (power, keep) = (power.clone(), settings.clone());
        settings.connect_changed(Some("show-battery-percentage"), move |_, _| {
            let _ = &keep;
            update(&power);
        });
    }

    let tr = tr.clone();
    let state = power.clone();
    menu.on_show(move |menu| fill_menu(menu, &tr, &state));
    button
}

fn button_tooltip(image: &gtk::Image, tr: &Translations, battery: Option<&Battery>) {
    let text = match battery {
        Some(battery) => {
            let mut args = FluentArgs::new();
            args.set("percent", format!("{:.0}", battery.percentage));
            tr.get_with("power-battery-percent", &args)
        }
        None => tr.get("power-adapter"),
    };
    image.set_tooltip_text(Some(&text));
}

fn fill_menu(menu: &Menu, tr: &Rc<Translations>, power: &Power) {
    match power.battery() {
        Some(battery) => {
            let title = menu.title(&tr.get("power-battery"));
            title.append(
                &gtk::Label::builder()
                    .label(format!("{:.0}%", battery.percentage))
                    .css_classes(["extra-value"])
                    .build(),
            );
            let source = if battery.charging() {
                tr.get("power-source-adapter")
            } else {
                tr.get("power-source-battery")
            };
            menu.text(&source);
            let remaining = match battery.state {
                FULLY_CHARGED => Some(tr.get("power-fully-charged")),
                _ if battery.charging() && battery.time_to_full > 0 => {
                    let mut args = FluentArgs::new();
                    args.set("time", duration_text(tr, battery.time_to_full));
                    Some(tr.get_with("power-until-full", &args))
                }
                _ if battery.time_to_empty > 0 => {
                    let mut args = FluentArgs::new();
                    args.set("time", duration_text(tr, battery.time_to_empty));
                    Some(tr.get_with("power-remaining", &args))
                }
                _ => None,
            };
            if let Some(remaining) = remaining {
                menu.text(&remaining);
            }
        }
        None => {
            menu.title(&tr.get("power-title"));
            menu.text(&tr.get("power-adapter"));
        }
    }

    let modes = power.modes();
    if !modes.is_empty() {
        menu.separator();
        menu.section(&tr.get("power-mode"));
        let active = power.active_mode();
        for (profile, label) in MODES {
            if !modes.iter().any(|mode| mode == profile) {
                continue;
            }
            let power = power.clone();
            menu.check(
                &tr.get(label),
                active.as_deref() == Some(profile),
                move || power.set_mode(profile),
            );
        }
    }
    menu.separator();
    menu.action(&tr.get("power-settings"), || open_settings(Some("battery")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_icons_follow_level_and_charging() {
        let battery = |percentage, state| Battery {
            percentage,
            state,
            ..Battery::default()
        };
        assert_eq!(battery_icon(&battery(47.0, 2)), "battery-level-50-symbolic");
        assert_eq!(battery_icon(&battery(3.0, 2)), "battery-level-0-symbolic");
        assert_eq!(
            battery_icon(&battery(64.0, CHARGING)),
            "battery-level-60-charging-symbolic"
        );
        assert_eq!(
            battery_icon(&battery(100.0, FULLY_CHARGED)),
            "battery-level-100-charged-symbolic"
        );
    }
}
