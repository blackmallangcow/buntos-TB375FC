// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 배터리(전원) 항목. 배터리가 있으면 남은 양과 충전 상태를, 없으면(데스크톱, VM)
//! 전원 어댑터를 아이콘으로 보인다. 누르면 상태와 전원 모드(저전력 모드, 자동, 고성능)가 나오고,
//! 모드는 고르는 즉시 적용된다 (UPower, power-profiles-daemon).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    dbus,
    extras::{self, Listeners, Menu, Visibility},
    menubar::open_settings,
    shell_settings,
};

const UPOWER: &str = "org.freedesktop.UPower";
const UPOWER_PATH: &str = "/org/freedesktop/UPower";
const DISPLAY_DEVICE: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";
const PROFILES: &str = "org.freedesktop.UPower.PowerProfiles";
const PROFILES_PATH: &str = "/org/freedesktop/UPower/PowerProfiles";

/// UPower Device.Type: 배터리
const TYPE_BATTERY: u32 = 2;
/// UPower Device.State
const CHARGING: u32 = 1;
const EMPTY: u32 = 3;
const FULLY_CHARGED: u32 = 4;
const PENDING_CHARGE: u32 = 5;
/// 충전이 이보다 오래 남으면 시간 대신 "10시간 이상"이라고 적는다 (macOS와 같다)
const LONG_CHARGE_SECONDS: i64 = 10 * 3600;

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
    /// UPower의 OnBattery: 전원 없이 배터리로 돈다
    pub on_battery: bool,
}

/// 아이콘과 메뉴가 보이는 전원 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Charging,
    Full,
    /// 전원에 연결됐지만 충전되지 않는다. 입력보다 많이 쓰거나 충전이 멈췄을 때다
    NotCharging,
    OnBattery,
}

impl Battery {
    /// UPower는 전원이 꽂혀 있어도 배터리에서 전류가 나오면 State를 Discharging이라고 한다.
    /// 그래서 배터리 사용 중인지는 OnBattery로 가린다. OnBattery는 Discharging일 때만 참이라
    /// Empty는 따로 본다
    pub fn status(&self) -> Status {
        match self.state {
            CHARGING => Status::Charging,
            FULLY_CHARGED => Status::Full,
            PENDING_CHARGE => Status::NotCharging,
            EMPTY => Status::OnBattery,
            _ if self.on_battery => Status::OnBattery,
            _ => Status::NotCharging,
        }
    }
}

/// 배터리 아이콘: 10% 단위 (Adwaita의 battery-level-N 아이콘). 번개는 충전할 때만, 플러그는
/// 연결됐지만 충전하지 않을 때만 그린다 (macOS와 같다)
pub fn battery_icon(battery: &Battery) -> String {
    let level = ((battery.percentage / 10.0).round() as u32 * 10).min(100);
    match battery.status() {
        Status::Full => "battery-level-100-charged-symbolic".into(),
        // Adwaita의 충전 중 아이콘은 90까지다
        Status::Charging => format!("battery-level-{}-charging-symbolic", level.min(90)),
        Status::NotCharging => format!("battery-level-{level}-plugged-in-symbolic"),
        Status::OnBattery => format!("battery-level-{level}-symbolic"),
    }
}

/// 전원 상태. 복제해도 같은 상태를 가리킨다. 제어 센터도 같은 것을 쓴다
#[derive(Clone)]
pub struct Power(Rc<Inner>);

struct Inner {
    /// 배터리가 없으면 None (전원 어댑터)
    battery: RefCell<Option<Battery>>,
    on_battery: Cell<bool>,
    profiles: RefCell<Option<gio::DBusProxy>>,
    listeners: Listeners<Power>,
}

impl Power {
    pub fn start() -> Self {
        let power = Self(Rc::new(Inner {
            battery: RefCell::default(),
            on_battery: Cell::default(),
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
                    // OnBattery는 다른 객체의 속성이라 battery()가 채운다
                    ..Battery::default()
                });
                inner.battery.replace(battery);
                Power(inner).notify();
            },
        );
        let weak = Rc::downgrade(&power.0);
        dbus::watch(
            gio::BusType::System,
            UPOWER,
            UPOWER_PATH,
            UPOWER,
            move |proxy| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.on_battery.set(
                    dbus::is_running(proxy)
                        && dbus::property::<bool>(proxy, "OnBattery").unwrap_or(false),
                );
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
        let mut battery = self.0.battery.borrow().clone()?;
        battery.on_battery = self.0.on_battery.get();
        Some(battery)
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

/// "2시간 15분" 같은 남은 시간. 분으로 반올림하고 "0분"으로는 적지 않는다
fn duration_text(tr: &Translations, seconds: i64) -> String {
    let minutes = ((seconds + 30) / 60).max(1);
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
            let id = match battery.status() {
                Status::Charging => "power-battery-percent-charging",
                Status::Full | Status::NotCharging => "power-battery-percent-connected",
                Status::OnBattery => "power-battery-percent",
            };
            tr.get_with(id, &args)
        }
        None => tr.get("power-adapter"),
    };
    image.set_tooltip_text(Some(&text));
}

/// 메뉴의 전원 공급원 줄
fn source_text(tr: &Translations, battery: &Battery) -> String {
    tr.get(match battery.status() {
        Status::OnBattery => "power-source-battery",
        _ => "power-source-adapter",
    })
}

/// 메뉴의 상태 줄. 배터리로 돌 때 남은 시간을 모르면 줄이 없다
fn status_text(tr: &Translations, battery: &Battery) -> Option<String> {
    let mut args = FluentArgs::new();
    match battery.status() {
        Status::Charging if battery.time_to_full > LONG_CHARGE_SECONDS => {
            Some(tr.get("power-charging-long"))
        }
        Status::Charging if battery.time_to_full > 0 => {
            args.set("time", duration_text(tr, battery.time_to_full));
            Some(tr.get_with("power-charging-time", &args))
        }
        Status::Charging => Some(tr.get("power-charging")),
        Status::Full => Some(tr.get("power-fully-charged")),
        Status::NotCharging => Some(tr.get("power-not-charging")),
        Status::OnBattery if battery.time_to_empty > 0 => {
            args.set("time", duration_text(tr, battery.time_to_empty));
            Some(tr.get_with("power-remaining", &args))
        }
        Status::OnBattery => None,
    }
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
            menu.text(&source_text(tr, &battery));
            if let Some(status) = status_text(tr, &battery) {
                menu.text(&status);
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

    const DISCHARGING: u32 = 2;
    const KO: &str = include_str!("../../i18n/ko/bunt-shell.ftl");
    const EN: &str = include_str!("../../i18n/en/bunt-shell.ftl");

    fn battery(percentage: f64, state: u32, on_battery: bool) -> Battery {
        Battery {
            percentage,
            state,
            on_battery,
            ..Battery::default()
        }
    }

    #[test]
    fn status_tells_a_plugged_in_battery_from_one_in_use() {
        let status = |state, on_battery| battery(50.0, state, on_battery).status();
        assert_eq!(status(CHARGING, false), Status::Charging);
        assert_eq!(status(FULLY_CHARGED, false), Status::Full);
        assert_eq!(status(PENDING_CHARGE, false), Status::NotCharging);
        assert_eq!(status(DISCHARGING, true), Status::OnBattery);
        // 전원이 꽂혀 있는데 배터리에서 전류가 나온다 (입력보다 많이 쓸 때)
        assert_eq!(status(DISCHARGING, false), Status::NotCharging);
        assert_eq!(status(EMPTY, false), Status::OnBattery);
    }

    #[test]
    fn battery_icons_follow_level_and_status() {
        let icon =
            |percentage, state, on_battery| battery_icon(&battery(percentage, state, on_battery));
        assert_eq!(icon(47.0, DISCHARGING, true), "battery-level-50-symbolic");
        assert_eq!(icon(3.0, DISCHARGING, true), "battery-level-0-symbolic");
        assert_eq!(
            icon(64.0, CHARGING, false),
            "battery-level-60-charging-symbolic"
        );
        // Adwaita에 battery-level-100-charging은 없다
        assert_eq!(
            icon(97.0, CHARGING, false),
            "battery-level-90-charging-symbolic"
        );
        assert_eq!(
            icon(100.0, FULLY_CHARGED, false),
            "battery-level-100-charged-symbolic"
        );
        assert_eq!(
            icon(64.0, PENDING_CHARGE, false),
            "battery-level-60-plugged-in-symbolic"
        );
        assert_eq!(
            icon(64.0, DISCHARGING, false),
            "battery-level-60-plugged-in-symbolic"
        );
    }

    #[test]
    fn durations_round_to_minutes_and_are_never_zero() {
        let tr = Translations::from_source("ko", KO);
        assert_eq!(duration_text(&tr, 80 * 60), "1시간 20분");
        assert_eq!(duration_text(&tr, 2 * 3600), "2시간");
        assert_eq!(duration_text(&tr, 45 * 60 + 40), "46분");
        assert_eq!(duration_text(&tr, 10), "1분");
    }

    #[test]
    fn menu_reads_like_macos() {
        let tr = Translations::from_source("ko", KO);
        let lines = |battery: Battery| {
            (
                source_text(&tr, &battery),
                status_text(&tr, &battery).unwrap_or_default(),
            )
        };
        let adapter = "전원 공급원: 전원 어댑터";
        assert_eq!(
            lines(Battery {
                time_to_full: 80 * 60,
                ..battery(40.0, CHARGING, false)
            }),
            (
                adapter.into(),
                "충전 중, 충전 완료까지 1시간 20분 남음".into()
            )
        );
        assert_eq!(
            lines(Battery {
                time_to_full: 16 * 3600,
                ..battery(19.0, CHARGING, false)
            }),
            (
                adapter.into(),
                "충전 중, 충전 완료까지 10시간 이상 남음".into()
            )
        );
        assert_eq!(
            lines(battery(40.0, CHARGING, false)),
            (adapter.into(), "충전 중".into())
        );
        assert_eq!(
            lines(battery(100.0, FULLY_CHARGED, false)),
            (adapter.into(), "충전 완료됨".into())
        );
        assert_eq!(
            lines(Battery {
                time_to_empty: 3 * 3600,
                ..battery(60.0, DISCHARGING, false)
            }),
            (adapter.into(), "배터리 충전 중이 아님".into())
        );
        assert_eq!(
            lines(Battery {
                time_to_empty: 228 * 60,
                ..battery(60.0, DISCHARGING, true)
            }),
            ("전원 공급원: 배터리".into(), "남은 시간 3시간 48분".into())
        );
        assert_eq!(
            lines(battery(60.0, DISCHARGING, true)),
            ("전원 공급원: 배터리".into(), String::new())
        );
    }

    #[test]
    fn english_menu_has_every_status() {
        let tr = Translations::from_source("en", EN);
        let text = |battery: Battery| status_text(&tr, &battery).unwrap_or_default();
        assert_eq!(
            text(Battery {
                time_to_full: 80 * 60,
                ..battery(40.0, CHARGING, false)
            }),
            "Charging, 1 hr 20 min until fully charged"
        );
        assert_eq!(
            text(Battery {
                time_to_full: 16 * 3600,
                ..battery(19.0, CHARGING, false)
            }),
            "Charging, more than 10 hours until fully charged"
        );
        assert_eq!(text(battery(100.0, FULLY_CHARGED, false)), "Fully Charged");
        assert_eq!(
            text(battery(60.0, PENDING_CHARGE, false)),
            "Battery Is Not Charging"
        );
    }
}
