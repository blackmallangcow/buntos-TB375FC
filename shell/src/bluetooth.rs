// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 Bluetooth 항목 (BlueZ). 켜고 끄는 스위치, 내 기기(연결된 기기는 강조), 근처 기기가
//! 나온다. 내 기기를 누르면 연결하거나 연결을 끊고, 근처 기기를 누르면 짝지은 뒤 연결한다(확인 번호와
//! PIN은 셸의 페어링 에이전트 pairing_agent.rs가 묻는다). 근처 기기는 메뉴가 열린 동안만 찾는다.
//! macOS 26 메뉴에는 근처 기기가 없다(DESIGN-SPEC 15장). 어댑터가 없으면 항목을 숨긴다.
//! macOS처럼 기본으로는 메뉴 막대에 보이지 않고(show-bluetooth), 제어 센터에 있다.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

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
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// 페어링은 상대 기기와 페어링 창의 답을 기다린다 (BlueZ가 에이전트를 60초까지 기다린다)
const PAIR_TIMEOUT_MS: i32 = 60_000;
/// 연결은 몇 초 걸릴 수 있다
const CONNECT_TIMEOUT_MS: i32 = 30_000;
/// 메뉴에 보이는 근처 기기 수
const MAX_NEARBY: usize = 8;
/// 기기가 보이거나 바뀐 뒤 목록을 다시 읽기까지. 찾는 동안 신호 세기(RSSI) 알림이 잦다
const NEARBY_DELAY: Duration = Duration::from_millis(500);

type Interfaces = HashMap<String, HashMap<String, glib::Variant>>;

/// 기기 하나 (내 기기 또는 근처 기기)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub path: String,
    pub name: String,
    pub icon: String,
    pub connected: bool,
}

/// 어댑터의 기기들
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Devices {
    /// 짝지은 기기, 연결된 것이 먼저
    pub mine: Vec<Device>,
    /// 짝짓지 않은 기기, 이름순
    pub nearby: Vec<Device>,
}

impl Devices {
    /// GetManagedObjects의 결과에서 어댑터(경로가 prefix로 시작) 아래의 기기를 고른다.
    /// 근처 기기는 이름(Name)이 알려진 것만 보인다. BlueZ의 Alias는 이름이 없으면 주소가 되는데,
    /// macOS도 이름 없는 기기는 보이지 않는다
    fn from_objects(objects: Vec<(String, Interfaces)>, prefix: &str) -> Self {
        let mut devices = Self::default();
        for (path, interfaces) in objects {
            let Some(device) = interfaces.get(DEVICE_INTERFACE) else {
                continue;
            };
            if !path.starts_with(prefix) {
                continue;
            }
            let get_bool = |name: &str| {
                device
                    .get(name)
                    .and_then(|value| value.get::<bool>())
                    .unwrap_or(false)
            };
            let text = |name: &str| {
                device
                    .get(name)
                    .and_then(|value| value.get::<String>())
                    .filter(|text| !text.is_empty())
            };
            let paired = get_bool("Paired");
            if !paired && text("Name").is_none() {
                continue;
            }
            let entry = Device {
                path,
                name: text("Alias").or_else(|| text("Name")).unwrap_or_default(),
                icon: text("Icon").unwrap_or_default(),
                connected: get_bool("Connected"),
            };
            if paired {
                devices.mine.push(entry);
            } else {
                devices.nearby.push(entry);
            }
        }
        devices
            .mine
            .sort_by_key(|device| (!device.connected, device.name.to_lowercase()));
        devices
            .nearby
            .sort_by_key(|device| device.name.to_lowercase());
        devices
    }
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
        let weak = Rc::downgrade(&bluetooth.0);
        glib::spawn_future_local(async move {
            // 버스 연결을 먼저 얻는다. gio::bus_watch_name은 시스템 버스에 붙지 못하면(컨테이너, 이미지
            // 밖에서 실행) 사라짐 처리기에 연결 대신 NULL을 주고, gio-rs가 그것을 unwrap해 셸이 죽었다
            let bus = match gio::bus_get_future(gio::BusType::System).await {
                Ok(bus) => bus,
                Err(err) => {
                    eprintln!("bunt-shell: no system bus, Bluetooth is unavailable: {err}");
                    return;
                }
            };
            // bluetoothd가 나중에 켜지거나 다시 시작해도 따라간다
            let (appeared, vanished) = (weak.clone(), weak);
            dbus::watch_name(
                &bus,
                BLUEZ,
                move |connection| {
                    if let Some(inner) = appeared.upgrade() {
                        Bluetooth(inner).find_adapter(connection);
                    }
                },
                move || {
                    if let Some(inner) = vanished.upgrade() {
                        inner.adapter.replace(None);
                        Bluetooth(inner).notify();
                    }
                },
            );
        });
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

    /// 어댑터의 짝지은 기기와 근처 기기
    pub async fn devices(&self) -> Devices {
        let Some(adapter) = self.0.adapter.borrow().clone() else {
            return Devices::default();
        };
        let prefix = format!("{}/", adapter.object_path());
        Devices::from_objects(managed_objects(&adapter.connection()).await, &prefix)
    }

    /// 근처 기기 찾기를 켜고 끈다. BlueZ는 찾기를 부른 프로그램마다 따로 세므로 시스템 설정이
    /// 찾는 중이어도 서로 끄지 않는다. 이미 찾는 중이라는 거절 같은 것은 무시한다
    fn set_discovering(&self, on: bool) {
        let Some(adapter) = self.0.adapter.borrow().clone() else {
            return;
        };
        let method = if on {
            "StartDiscovery"
        } else {
            "StopDiscovery"
        };
        adapter.call(
            method,
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            |_| {},
        );
    }

    /// BlueZ의 기기가 생기거나 없어지거나 속성이 바뀔 때마다 changed를 부른다.
    /// 돌려준 구독을 버리면 멈춘다
    fn watch_devices(&self, changed: impl Fn() + 'static) -> Vec<gio::SignalSubscription> {
        let Some(adapter) = self.0.adapter.borrow().clone() else {
            return Vec::new();
        };
        let connection = adapter.connection();
        let changed = Rc::new(changed);
        [
            (OBJECT_MANAGER, "InterfacesAdded"),
            (OBJECT_MANAGER, "InterfacesRemoved"),
            (PROPERTIES, "PropertiesChanged"),
        ]
        .into_iter()
        .map(|(interface, member)| {
            let changed = changed.clone();
            connection.subscribe_to_signal(
                Some(BLUEZ),
                Some(interface),
                Some(member),
                None,
                None,
                gio::DBusSignalFlags::NONE,
                move |_| changed(),
            )
        })
        .collect()
    }

    /// 근처 기기를 짝짓고 믿는 기기로 둔 뒤 연결한다. 확인 번호나 PIN은 BlueZ가 기본 에이전트인
    /// 셸의 페어링 에이전트를 거쳐 사람에게 묻는다
    pub fn pair(&self, device: &Device) {
        let Some(adapter) = self.0.adapter.borrow().clone() else {
            return;
        };
        let connection = adapter.connection();
        let path = device.path.clone();
        glib::spawn_future_local(async move {
            let call = |interface: &'static str,
                        method: &'static str,
                        args: Option<glib::Variant>,
                        timeout: i32| {
                connection.call_future(
                    Some(BLUEZ),
                    &path,
                    interface,
                    method,
                    args.as_ref(),
                    None,
                    gio::DBusCallFlags::NONE,
                    timeout,
                )
            };
            let result = async {
                call(DEVICE_INTERFACE, "Pair", None, PAIR_TIMEOUT_MS).await?;
                let trusted = (DEVICE_INTERFACE, "Trusted", true.to_variant()).to_variant();
                call(PROPERTIES, "Set", Some(trusted), -1).await?;
                call(DEVICE_INTERFACE, "Connect", None, CONNECT_TIMEOUT_MS).await
            }
            .await;
            if let Err(err) = result {
                eprintln!("bunt-shell: Bluetooth: {err}");
            }
        });
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

/// BlueZ의 기기 종류(Icon 속성)에 맞는 기호 아이콘. 페어링 창(pairing_agent.rs)도 쓴다
pub(crate) fn device_icon(icon: &str) -> &'static str {
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

    // 메뉴가 열린 동안 근처 기기를 찾고 기기 알림을 받는다. 닫으면 둘 다 멈춘다
    let discovery: Rc<RefCell<Vec<gio::SignalSubscription>>> = Rc::default();
    let (stopper, watching) = (bluetooth.clone(), discovery.clone());
    menu.popover().connect_closed(move |_| {
        if !watching.take().is_empty() {
            stopper.set_discovering(false);
        }
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
            let shown: Rc<RefCell<Devices>> = Rc::default();
            bluetooth.set_discovering(true);
            let pending = Rc::new(Cell::new(false));
            let (watch_menu, watch_tr, watch_bluetooth, watch_list, watch_shown) = (
                menu.clone(),
                tr.clone(),
                bluetooth.clone(),
                list.clone(),
                shown.clone(),
            );
            discovery.replace(bluetooth.watch_devices(move || {
                // 알림이 몰려와도 한 번만 다시 읽는다
                if pending.replace(true) {
                    return;
                }
                let (menu, tr, bluetooth, list, shown, pending) = (
                    watch_menu.clone(),
                    watch_tr.clone(),
                    watch_bluetooth.clone(),
                    watch_list.clone(),
                    watch_shown.clone(),
                    pending.clone(),
                );
                glib::spawn_future_local(async move {
                    glib::timeout_future(NEARBY_DELAY).await;
                    pending.set(false);
                    let devices = bluetooth.devices().await;
                    // 신호 세기만 바뀌었으면 그대로 둔다 (가리킨 줄이 깜빡이지 않게)
                    if list.parent().is_some() && *shown.borrow() != devices {
                        fill_devices(&menu, &list, &tr, &bluetooth, &devices);
                        shown.replace(devices);
                    }
                });
            }));
            let (menu, tr, bluetooth) = (menu.clone(), tr.clone(), bluetooth.clone());
            glib::spawn_future_local(async move {
                let devices = bluetooth.devices().await;
                if list.parent().is_none() {
                    return;
                }
                fill_devices(&menu, &list, &tr, &bluetooth, &devices);
                shown.replace(devices);
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

/// 내 기기와 근처 기기 줄을 (다시) 채운다
fn fill_devices(
    menu: &Menu,
    list: &gtk::Box,
    tr: &Translations,
    bluetooth: &Bluetooth,
    devices: &Devices,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    list.append(
        &gtk::Label::builder()
            .label(tr.get("bluetooth-devices"))
            .xalign(0.0)
            .css_classes(["extra-section"])
            .build(),
    );
    if devices.mine.is_empty() {
        list.append(
            &gtk::Label::builder()
                .label(tr.get("bluetooth-no-devices"))
                .xalign(0.0)
                .css_classes(["extra-text"])
                .build(),
        );
    }
    for device in &devices.mine {
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

    // 근처 기기: 찾는 동안 제목 옆에서 돈다
    let header = gtk::Box::builder()
        .spacing(6)
        .css_classes(["extra-section"])
        .build();
    header.append(&gtk::Label::new(Some(&tr.get("bluetooth-nearby"))));
    header.append(&gtk::Spinner::builder().spinning(true).build());
    list.append(&header);
    for device in devices.nearby.iter().take(MAX_NEARBY) {
        let pairer = bluetooth.clone();
        let target = device.clone();
        list.append(&menu.item_button(
            Row {
                icon: device_icon(&device.icon),
                title: &device.name,
                subtitle: None,
                active: false,
                trailing: &[],
            },
            move || pairer.pair(&target),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(props: &[(&str, glib::Variant)]) -> Interfaces {
        let props = props
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect();
        Interfaces::from([(DEVICE_INTERFACE.to_owned(), props)])
    }

    #[test]
    fn devices_split_into_mine_and_named_nearby_ones() {
        let objects = vec![
            (
                "/org/bluez/hci0".to_owned(),
                Interfaces::from([(ADAPTER_INTERFACE.to_owned(), HashMap::new())]),
            ),
            (
                "/org/bluez/hci0/dev_1".to_owned(),
                device(&[
                    ("Paired", true.to_variant()),
                    ("Alias", "Magic Keyboard".to_variant()),
                    ("Icon", "input-keyboard".to_variant()),
                ]),
            ),
            (
                "/org/bluez/hci0/dev_2".to_owned(),
                device(&[
                    ("Paired", true.to_variant()),
                    ("Connected", true.to_variant()),
                    ("Alias", "AirPods Pro".to_variant()),
                ]),
            ),
            (
                "/org/bluez/hci0/dev_3".to_owned(),
                device(&[
                    ("Alias", "JBL Flip 6".to_variant()),
                    ("Name", "JBL Flip 6".to_variant()),
                ]),
            ),
            // 이름 없는 기기의 Alias는 주소다. 근처 기기에 보이지 않는다
            (
                "/org/bluez/hci0/dev_4".to_owned(),
                device(&[("Alias", "AA-BB-CC-DD-EE-FF".to_variant())]),
            ),
            (
                "/org/bluez/hci0/dev_5".to_owned(),
                device(&[
                    ("Alias", "alpha".to_variant()),
                    ("Name", "alpha".to_variant()),
                ]),
            ),
            // 다른 어댑터의 기기
            (
                "/org/bluez/hci1/dev_6".to_owned(),
                device(&[
                    ("Paired", true.to_variant()),
                    ("Alias", "Other".to_variant()),
                ]),
            ),
        ];
        let devices = Devices::from_objects(objects, "/org/bluez/hci0/");
        let names = |list: &[Device]| list.iter().map(|d| d.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&devices.mine), ["AirPods Pro", "Magic Keyboard"]);
        assert!(devices.mine[0].connected);
        assert_eq!(devices.mine[1].icon, "input-keyboard");
        assert_eq!(names(&devices.nearby), ["alpha", "JBL Flip 6"]);
    }

    #[test]
    fn unchanged_devices_compare_equal() {
        let objects = || {
            vec![(
                "/org/bluez/hci0/dev_1".to_owned(),
                device(&[
                    ("Name", "Speaker".to_variant()),
                    ("RSSI", (-60i16).to_variant()),
                ]),
            )]
        };
        let mut moved = objects();
        // 신호 세기만 바뀐 알림은 목록을 다시 그리지 않는다
        moved[0]
            .1
            .get_mut(DEVICE_INTERFACE)
            .unwrap()
            .insert("RSSI".to_owned(), (-40i16).to_variant());
        assert_eq!(
            Devices::from_objects(objects(), "/org/bluez/hci0/"),
            Devices::from_objects(moved, "/org/bluez/hci0/")
        );
    }
}
