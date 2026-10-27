// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 제어 센터 (macOS 26). 메뉴 막대의 제어 센터 아이콘을 누르면 유리 판 위에 모듈이 나온다:
//! 연결(Wi-Fi, Bluetooth), 방해 금지, 화면 모드(밝게·어둡게), 저전력 모드, 디스플레이 밝기,
//! 사운드 음량. 장치나 서비스가 없는 모듈은 빠진다. 방해 금지 중에는 메뉴 막대에 달 아이콘이
//! 보인다.

use std::{path::PathBuf, rc::Rc};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, prelude::*};

use crate::{
    bluetooth::Bluetooth,
    extras::{self, Menu},
    interface_settings,
    menubar::open_settings,
    network::Network,
    power::Power,
    sound::{self, Sound},
};

const ICON: &str = "org.buntos.ControlCenter-symbolic";
const FOCUS_ICON: &str = "org.buntos.Focus-symbolic";
const APPEARANCE_ICON: &str = "org.buntos.Appearance-symbolic";
const LOW_POWER_ICON: &str = "org.buntos.LowPower-symbolic";
/// 알림 배너를 보일지 (끄면 방해 금지). GNOME 공용 설정이라 다른 데스크톱 도구와 같은 값이다
const NOTIFICATIONS_SCHEMA: &str = "org.gnome.desktop.notifications";

/// 제어 센터가 다루는 상태들
#[derive(Clone)]
pub struct Services {
    pub network: Network,
    pub bluetooth: Bluetooth,
    pub power: Power,
    pub sound: Sound,
}

pub fn notification_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(NOTIFICATIONS_SCHEMA, true)?;
    Some(gio::Settings::new(NOTIFICATIONS_SCHEMA))
}

/// 방해 금지 중인지
pub fn do_not_disturb() -> bool {
    notification_settings().is_some_and(|settings| !settings.boolean("show-banners"))
}

fn set_do_not_disturb(on: bool) {
    if let Some(settings) = notification_settings()
        && let Err(err) = settings.set_boolean("show-banners", !on)
    {
        eprintln!("bunt-shell: failed to change Do Not Disturb: {err}");
    }
}

fn dark_mode() -> bool {
    interface_settings().is_some_and(|settings| settings.string("color-scheme") == "prefer-dark")
}

fn set_dark_mode(on: bool) {
    let scheme = if on { "prefer-dark" } else { "default" };
    if let Some(settings) = interface_settings()
        && let Err(err) = settings.set_string("color-scheme", scheme)
    {
        eprintln!("bunt-shell: failed to change the appearance: {err}");
    }
}

/// 내장 화면의 밝기 (/sys/class/backlight). 바꾸기는 logind가 한다
struct Backlight {
    name: String,
    max: u32,
}

impl Backlight {
    fn find() -> Option<Self> {
        let entry = std::fs::read_dir("/sys/class/backlight")
            .ok()?
            .flatten()
            .next()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let max = read_number(entry.path().join("max_brightness"))?;
        (max > 0).then_some(Self { name, max })
    }

    fn get(&self) -> f64 {
        let path = PathBuf::from("/sys/class/backlight")
            .join(&self.name)
            .join("brightness");
        f64::from(read_number(path).unwrap_or(self.max)) / f64::from(self.max)
    }

    fn set(&self, fraction: f64) {
        let value = (fraction.clamp(0.01, 1.0) * f64::from(self.max)).round() as u32;
        let Ok(bus) = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) else {
            return;
        };
        bus.call(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            Some(&("backlight", self.name.as_str(), value).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            |result| {
                if let Err(err) = result {
                    eprintln!("bunt-shell: SetBrightness: {err}");
                }
            },
        );
    }
}

fn read_number(path: PathBuf) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// 켜고 끄는 칸의 모양
#[derive(Clone, Copy)]
enum Shape {
    /// 동그라미 오른쪽에 이름과 상태 (연결, 방해 금지)
    Row,
    /// 동그라미 아래에 이름 (작은 모듈)
    Tile,
}

/// 누르면 켜고 끄는 칸. macOS 제어 센터처럼 동그라미뿐 아니라 칸 어디를 눌러도 된다.
/// 켜면 동그라미를 강조색으로 채우고 상태 글자(켤 때, 끌 때)를 바꾼다
fn toggle(
    icon: &str,
    title: &str,
    status: Option<(String, String)>,
    on: bool,
    shape: Shape,
    toggled: impl Fn(bool) + 'static,
) -> gtk::Button {
    let circle = gtk::Image::from_icon_name(icon);
    circle.add_css_class("cc-toggle");
    if on {
        circle.add_css_class("on");
    }
    let status_label = status.as_ref().map(|(on_text, off_text)| {
        gtk::Label::builder()
            .label(if on { on_text } else { off_text })
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(12)
            .css_classes(["cc-status"])
            .build()
    });
    let content = match shape {
        Shape::Row => {
            let text = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .valign(gtk::Align::Center)
                .build();
            text.append(
                &gtk::Label::builder()
                    .label(title)
                    .xalign(0.0)
                    .css_classes(["cc-title"])
                    .build(),
            );
            if let Some(label) = &status_label {
                text.append(label);
            }
            let row = gtk::Box::builder().spacing(8).build();
            row.append(&circle);
            row.append(&text);
            row
        }
        Shape::Tile => {
            let tile = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .build();
            circle.set_halign(gtk::Align::Center);
            tile.append(&circle);
            tile.append(
                &gtk::Label::builder()
                    .label(title)
                    .justify(gtk::Justification::Center)
                    .wrap(true)
                    .css_classes(["cc-caption"])
                    .build(),
            );
            tile
        }
    };
    let button = gtk::Button::builder()
        .child(&content)
        .css_classes(["cc-button"])
        .build();
    button.connect_clicked(move |_| {
        let on = !circle.has_css_class("on");
        if on {
            circle.add_css_class("on");
        } else {
            circle.remove_css_class("on");
        }
        if let (Some(label), Some((on_text, off_text))) = (&status_label, &status) {
            label.set_text(if on { on_text } else { off_text });
        }
        toggled(on);
    });
    button
}

fn module() -> gtk::Box {
    gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["cc-module"])
        .build()
}

/// 칸 하나로 된 모듈 (방해 금지, 다크 모드, 저전력 모드): 모듈 전체가 단추다
fn module_button(button: gtk::Button, small: bool) -> gtk::Button {
    button.add_css_class("cc-module");
    if small {
        button.add_css_class("small");
    }
    button
}

fn fill(menu: &Menu, tr: &Rc<Translations>, services: &Services) {
    let grid = gtk::Grid::builder()
        .column_homogeneous(true)
        .row_spacing(10)
        .column_spacing(10)
        .css_classes(["control-center"])
        .build();
    let on_off = || (tr.get("cc-on"), tr.get("cc-off"));

    // 연결: Wi-Fi, Bluetooth
    let connectivity = module();
    let network = &services.network;
    if network.has_wifi() {
        let enabled = network.wifi_enabled();
        let (on_text, off_text) = on_off();
        let status = (network.ssid().unwrap_or(on_text), off_text);
        let switcher = network.clone();
        connectivity.append(&toggle(
            "network-wireless-symbolic",
            &tr.get("wifi-title"),
            Some(status),
            enabled,
            Shape::Row,
            move |on| switcher.set_wifi_enabled(on),
        ));
    } else if network.available() {
        let wired = toggle(
            "network-wired-symbolic",
            &tr.get("network-title"),
            Some((tr.get("cc-wired"), tr.get("cc-wired"))),
            true,
            Shape::Row,
            |_| {},
        );
        wired.set_sensitive(false);
        connectivity.append(&wired);
    }
    let bluetooth = &services.bluetooth;
    let status = if bluetooth.available() {
        on_off()
    } else {
        let unavailable = tr.get("cc-unavailable");
        (unavailable.clone(), unavailable)
    };
    let switcher = bluetooth.clone();
    let bluetooth_toggle = toggle(
        "bluetooth-symbolic",
        &tr.get("bluetooth-title"),
        Some(status),
        bluetooth.powered(),
        Shape::Row,
        move |on| switcher.set_powered(on),
    );
    bluetooth_toggle.set_sensitive(bluetooth.available());
    connectivity.append(&bluetooth_toggle);
    grid.attach(&connectivity, 0, 0, 2, 2);

    // 방해 금지
    let focus = toggle(
        FOCUS_ICON,
        &tr.get("cc-focus"),
        Some(on_off()),
        do_not_disturb(),
        Shape::Row,
        set_do_not_disturb,
    );
    grid.attach(&module_button(focus, false), 2, 0, 2, 1);

    // 다크 모드, 저전력 모드
    let appearance = toggle(
        APPEARANCE_ICON,
        &tr.get("cc-dark-mode"),
        None,
        dark_mode(),
        Shape::Tile,
        set_dark_mode,
    );
    grid.attach(&module_button(appearance, true), 2, 1, 1, 1);
    let power = services.power.clone();
    if power.modes().iter().any(|mode| mode == "power-saver") {
        let setter = power.clone();
        let low_power = toggle(
            LOW_POWER_ICON,
            &tr.get("cc-low-power"),
            None,
            power.low_power(),
            Shape::Tile,
            move |on| setter.set_mode(if on { "power-saver" } else { "balanced" }),
        );
        grid.attach(&module_button(low_power, true), 3, 1, 1, 1);
    }

    // 디스플레이 밝기 (내장 화면만)
    let mut row = 2;
    if let Some(backlight) = Backlight::find() {
        let display = module();
        display.append(
            &gtk::Label::builder()
                .label(tr.get("cc-display"))
                .xalign(0.0)
                .css_classes(["cc-title"])
                .build(),
        );
        let scale = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .adjustment(&gtk::Adjustment::new(
                backlight.get(),
                0.0,
                1.0,
                0.01,
                0.1,
                0.0,
            ))
            .hexpand(true)
            .build();
        scale.connect_value_changed(move |scale| backlight.set(scale.value()));
        let slider = gtk::Box::builder()
            .spacing(6)
            .css_classes(["slider-row"])
            .build();
        slider.append(&gtk::Image::from_icon_name("display-brightness-symbolic"));
        slider.append(&scale);
        display.append(&slider);
        grid.attach(&display, 0, row, 4, 1);
        row += 1;
    }

    // 사운드
    if services.sound.volume().is_some() {
        let audio = module();
        audio.append(
            &gtk::Label::builder()
                .label(tr.get("sound-title"))
                .xalign(0.0)
                .css_classes(["cc-title"])
                .build(),
        );
        audio.append(&sound::slider(&services.sound));
        grid.attach(&audio, 0, row, 4, 1);
    }

    menu.append(&grid);
    menu.action(&tr.get("cc-edit"), || open_settings(Some("menu-bar")));
}

pub fn extra(tr: &Rc<Translations>, services: &Services) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name(ICON);
    let menu = Menu::new();
    menu.popover().add_css_class("control-center-popover");
    let button = extras::button(&image, &menu, &tr.get("cc-title"));
    let (tr, services) = (tr.clone(), services.clone());
    menu.on_show(move |menu| {
        services.sound.refresh();
        fill(menu, &tr, &services);
    });
    button
}

/// 방해 금지 중에만 보이는 달 아이콘. 누르면 끌 수 있다
pub fn focus_extra(tr: &Rc<Translations>) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name(FOCUS_ICON);
    let menu = Menu::new();
    let button = extras::button(&image, &menu, &tr.get("cc-focus"));
    let title = tr.get("cc-focus");
    menu.on_show(move |menu| {
        menu.title_switch(&title, do_not_disturb(), set_do_not_disturb);
    });
    let Some(settings) = notification_settings() else {
        button.set_visible(false);
        return button;
    };
    button.set_visible(!settings.boolean("show-banners"));
    let shown = button.clone();
    let keep = settings.clone();
    settings.connect_changed(Some("show-banners"), move |settings, _| {
        let _ = &keep;
        shown.set_visible(!settings.boolean("show-banners"));
    });
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_read_from_sysfs_files() {
        let dir = std::env::temp_dir().join(format!("bunt-shell-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("max_brightness");
        std::fs::write(&file, "937\n").unwrap();
        assert_eq!(read_number(file), Some(937));
        assert_eq!(read_number(dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
