// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 Bluetooth 항목 (BlueZ). 켜고 끄는 스위치와 내 기기(연결된 기기는 강조)가 나오고,
//! 기기를 누르면 연결하거나 연결을 끊는다. 새 기기 연결은 시스템 설정의 Bluetooth 페이지에서
//! 한다. 어댑터가 없으면 항목을 숨긴다. macOS처럼 기본으로는 메뉴 막대에 보이지 않고
//! (show-bluetooth), 제어 센터에 있다.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    dbus,
    extras::{self, Listeners, Menu, Row, Visibility},
    menubar::open_settings,
};

const BLUEZ: &str = "org.bluez";
const ADAPTER_INTERFACE: &str = "org.bluez.Adapter1";
const DEVICE_INTERFACE: &str = "org.bluez.Device1";

type Interfaces = HashMap<String, HashMap<String, glib::Variant>>;

/// 짝지은 기기
#[derive(Debug, Clone)]
pub struct Device {
    pub path: String,
    pub name: String,
    pub icon: String,
    pub connected: bool,
}

/// Bluetooth 상태. 복제해도 같은 상태를 가리킨다. 제어 센터의 칸도 같은 것을 쓴다
#[derive(Clone)]
pub struct Bluetooth(Rc<Inner>);

struct Inner {
    /// 첫 어댑터. 없거나 bluetoothd가 꺼져 있으면 None
    adapter: RefCell<Option<gio::DBusProxy>>,
    listeners: Listeners<Bluetooth>,
}

/// org.freedesktop.DBus.ObjectManager.GetManagedObjects
async fn managed_objects(connection: &gio::DBusConnection) -> Vec<(String, Interfaces)> {
    let Ok(reply) = connection
        .call_future(
            Some(BLUEZ),
            "/",
            "org.freedesktop.DBus.ObjectManager",
            "GetManagedObjects",
            None,
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
    else {
        return Vec::new();
    };
    let objects = reply.child_value(0);
    objects
        .iter()
        .filter_map(|entry| {
            let path = entry.child_value(0).get::<glib::variant::ObjectPath>()?;
            let interfaces: Interfaces = entry.child_value(1).get()?;
            Some((path.as_str().to_owned(), interfaces))
        })
        .collect()
}

impl Bluetooth {
    pub fn start() -> Self {
        let bluetooth = Self(Rc::new(Inner {
            adapter: RefCell::default(),
            listeners: RefCell::default(),
        }));
        // bluetoothd가 나중에 켜지거나 다시 시작해도 따라간다
        let (appeared, vanished) = (Rc::downgrade(&bluetooth.0), Rc::downgrade(&bluetooth.0));
        // 셸이 끝날 때까지 지켜본다 (bus_unwatch_name을 부르지 않는다)
        gio::bus_watch_name(
            gio::BusType::System,
            BLUEZ,
            gio::BusNameWatcherFlags::NONE,
            move |connection, _, _| {
                if let Some(inner) = appeared.upgrade() {
                    Bluetooth(inner).find_adapter(connection);
                }
            },
            move |_, _| {
                if let Some(inner) = vanished.upgrade() {
                    inner.adapter.replace(None);
                    Bluetooth(inner).notify();
                }
            },
        );
        bluetooth
    }

    fn find_adapter(&self, connection: gio::DBusConnection) {
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let objects = managed_objects(&connection).await;
            let Some((path, _)) = objects
                .into_iter()
                .find(|(_, interfaces)| interfaces.contains_key(ADAPTER_INTERFACE))
            else {
                return;
            };
            let Ok(adapter) = gio::DBusProxy::new_future(
                &connection,
                gio::DBusProxyFlags::NONE,
                None,
                Some(BLUEZ),
                &path,
                ADAPTER_INTERFACE,
            )
            .await
            else {
                return;
            };
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let weak = Rc::downgrade(&inner);
            adapter.connect_local("g-properties-changed", false, move |_| {
                if let Some(inner) = weak.upgrade() {
                    Bluetooth(inner).notify();
                }
                None
            });
            inner.adapter.replace(Some(adapter));
            Bluetooth(inner).notify();
        });
    }

    fn notify(&self) {
        for listener in self.0.listeners.borrow().iter() {
            listener(self);
        }
    }

    pub fn subscribe(&self, listener: impl Fn(&Bluetooth) + 'static) {
        listener(self);
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    pub fn available(&self) -> bool {
        self.0
            .adapter
            .borrow()
            .as_ref()
            .is_some_and(dbus::is_running)
    }

    pub fn powered(&self) -> bool {
        self.0
            .adapter
            .borrow()
            .as_ref()
            .and_then(|adapter| dbus::property(adapter, "Powered"))
            .unwrap_or(false)
    }

    pub fn set_powered(&self, on: bool) {
        if let Some(adapter) = self.0.adapter.borrow().as_ref() {
            dbus::set_property(adapter, "Powered", on.to_variant());
        }
    }

    /// 짝지은 기기, 연결된 것이 먼저
    pub async fn devices(&self) -> Vec<Device> {
        let Some(adapter) = self.0.adapter.borrow().clone() else {
            return Vec::new();
        };
        let prefix = format!("{}/", adapter.object_path());
        let mut devices: Vec<Device> = managed_objects(&adapter.connection())
            .await
            .into_iter()
            .filter(|(path, _)| path.starts_with(&prefix))
            .filter_map(|(path, interfaces)| {
                let device = interfaces.get(DEVICE_INTERFACE)?;
                let get_bool = |name: &str| {
                    device
                        .get(name)
                        .and_then(|value| value.get::<bool>())
                        .unwrap_or(false)
                };
                if !get_bool("Paired") {
                    return None;
                }
                let text = |name: &str| device.get(name).and_then(|value| value.get::<String>());
                Some(Device {
                    path,
                    name: text("Alias").or_else(|| text("Name")).unwrap_or_default(),
                    icon: text("Icon").unwrap_or_default(),
                    connected: get_bool("Connected"),
                })
            })
            .collect();
        devices.sort_by_key(|device| (!device.connected, device.name.to_lowercase()));
        devices
    }

    /// 기기를 연결하거나 연결을 끊는다
    pub fn toggle_device(&self, device: &Device) {
        let Some(adapter) = self.0.adapter.borrow().clone() else {
            return;
        };
        let method = if device.connected {
            "Disconnect"
        } else {
            "Connect"
        };
        adapter.connection().call(
            Some(BLUEZ),
            &device.path,
            DEVICE_INTERFACE,
            method,
            None,
            None,
            gio::DBusCallFlags::NONE,
            // 연결은 몇 초 걸릴 수 있다
            30_000,
            gio::Cancellable::NONE,
            |result| {
                if let Err(err) = result {
                    eprintln!("bunt-shell: Bluetooth: {err}");
                }
            },
        );
    }
}

/// BlueZ의 기기 종류(Icon 속성)에 맞는 기호 아이콘
fn device_icon(icon: &str) -> &'static str {
    match icon {
        "audio-headset" | "audio-headphones" | "audio-card" => "audio-headphones-symbolic",
        "input-keyboard" => "input-keyboard-symbolic",
        "input-mouse" => "input-mouse-symbolic",
        "input-gaming" => "input-gaming-symbolic",
        "phone" => "phone-symbolic",
        _ => "bluetooth-symbolic",
    }
}

pub fn extra(tr: &Rc<Translations>, bluetooth: &Bluetooth) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name("bluetooth-active-symbolic");
    let menu = Menu::new();
    let button = extras::button(&image, &menu, &tr.get("bluetooth-title"));
    let visibility = Visibility::new(&button, "show-bluetooth");
    bluetooth.subscribe(move |bluetooth| {
        visibility.set_available(bluetooth.available());
        image.set_icon_name(Some(if bluetooth.powered() {
            "bluetooth-active-symbolic"
        } else {
            "bluetooth-disabled-symbolic"
        }));
    });

    let tr = tr.clone();
    let bluetooth = bluetooth.clone();
    menu.on_show(move |menu| {
        let powered = bluetooth.powered();
        let switcher = bluetooth.clone();
        menu.title_switch(&tr.get("bluetooth-title"), powered, move |on| {
            switcher.set_powered(on)
        });
        if powered {
            let list = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .build();
            menu.append(&list);
            let (menu, tr, bluetooth) = (menu.clone(), tr.clone(), bluetooth.clone());
            glib::spawn_future_local(async move {
                let devices = bluetooth.devices().await;
                if list.parent().is_none() {
                    return;
                }
                list.append(
                    &gtk::Label::builder()
                        .label(tr.get("bluetooth-devices"))
                        .xalign(0.0)
                        .css_classes(["extra-section"])
                        .build(),
                );
                if devices.is_empty() {
                    list.append(
                        &gtk::Label::builder()
                            .label(tr.get("bluetooth-no-devices"))
                            .xalign(0.0)
                            .css_classes(["extra-text"])
                            .build(),
                    );
                }
                for device in devices {
                    let subtitle = device.connected.then(|| tr.get("bluetooth-connected"));
                    let toggler = bluetooth.clone();
                    let target = device.clone();
                    list.append(&menu.item_button(
                        Row {
                            icon: device_icon(&device.icon),
                            title: &device.name,
                            subtitle: subtitle.as_deref(),
                            active: device.connected,
                            trailing: &[],
                        },
                        move || toggler.toggle_device(&target),
                    ));
                }
                menu.separator();
                menu.action(&tr.get("bluetooth-settings"), || {
                    open_settings(Some("bluetooth"))
                });
            });
        } else {
            menu.separator();
            menu.action(&tr.get("bluetooth-settings"), || {
                open_settings(Some("bluetooth"))
            });
        }
    });
    button
}
