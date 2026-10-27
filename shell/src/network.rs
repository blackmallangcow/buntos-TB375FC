// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 네트워크(Wi-Fi) 항목. NetworkManager가 알려 주는 연결 방식(유선, Wi-Fi)과 상태를
//! 아이콘으로 보인다. Wi-Fi 장치가 있으면 누를 때 macOS의 Wi-Fi 메뉴처럼 켜고 끄는 스위치,
//! 연결된 네트워크, 알고 있는 네트워크, 기타 네트워크가 나오고, 없으면 연결 상태를 글로 보인다.
//! 암호가 필요한 새 네트워크는 시스템 설정의 Wi-Fi 페이지에서 암호를 받는다.
//! NetworkManager가 없으면 항목을 숨긴다.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    dbus,
    extras::{self, Listeners, Menu, Row, Visibility},
    menubar::open_settings,
};

const NM_NAME: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_INTERFACE: &str = "org.freedesktop.NetworkManager";
const ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS_INTERFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings";
const SETTINGS_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings";
const CONNECTION_INTERFACE: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const WIRELESS: &str = "802-11-wireless";
/// NMDeviceType: Wi-Fi
const DEVICE_TYPE_WIFI: u32 = 2;

/// NetworkManager의 NMState 값
const CONNECTING: u32 = 40;
const CONNECTED_LOCAL: u32 = 50;
const CONNECTED_GLOBAL: u32 = 70;

/// 메뉴에 보이는 다른 네트워크 수
const MAX_NETWORKS: usize = 8;

/// 아이콘 이름과 상태 문장
#[derive(Debug, PartialEq, Eq)]
struct Status {
    icon: &'static str,
    message: &'static str,
}

/// NetworkManager의 상태, 주 연결 방식, Wi-Fi 신호 세기(0~100)로 아이콘과 문장을 고른다
fn describe(state: u32, kind: &str, strength: Option<u8>) -> Status {
    let wireless = kind == WIRELESS;
    let status = |icon, message| Status { icon, message };
    match state {
        CONNECTED_GLOBAL.. if wireless => {
            status(signal_icon(strength.unwrap_or(0)), "network-wireless")
        }
        CONNECTED_GLOBAL.. if kind == "802-3-ethernet" => {
            status("network-wired-symbolic", "network-wired")
        }
        CONNECTED_GLOBAL.. => status("network-wired-symbolic", "network-other"),
        CONNECTED_LOCAL.. if wireless => {
            status("network-wireless-no-route-symbolic", "network-limited")
        }
        CONNECTED_LOCAL.. => status("network-wired-no-route-symbolic", "network-limited"),
        CONNECTING.. if wireless => {
            status("network-wireless-acquiring-symbolic", "network-connecting")
        }
        CONNECTING.. => status("network-wired-acquiring-symbolic", "network-connecting"),
        _ => status("network-offline-symbolic", "network-disconnected"),
    }
}

/// GNOME과 같은 구간으로 Wi-Fi 신호 아이콘을 고른다
fn signal_icon(strength: u8) -> &'static str {
    match strength {
        81.. => "network-wireless-signal-excellent-symbolic",
        56.. => "network-wireless-signal-good-symbolic",
        31.. => "network-wireless-signal-ok-symbolic",
        6.. => "network-wireless-signal-weak-symbolic",
        _ => "network-wireless-signal-none-symbolic",
    }
}

/// 검색된 Wi-Fi 네트워크 (같은 이름의 접속 지점은 가장 센 것 하나)
#[derive(Debug, Clone, PartialEq)]
pub struct AccessPoint {
    pub path: String,
    pub ssid: String,
    pub strength: u8,
    pub secure: bool,
}

/// 네트워크 상태. 복제해도 같은 상태를 가리킨다. 제어 센터의 Wi-Fi 칸도 같은 것을 쓴다
#[derive(Clone)]
pub struct Network(Rc<Inner>);

struct Inner {
    manager: RefCell<Option<gio::DBusProxy>>,
    /// 주 연결이 Wi-Fi일 때 그 접속 지점. 신호 세기가 바뀌면 알려 준다
    access_point: RefCell<Option<gio::DBusProxy>>,
    /// 첫 Wi-Fi 장치의 경로. 없으면 None
    wifi_device: RefCell<Option<String>>,
    listeners: Listeners<Network>,
}

impl Network {
    pub fn start() -> Self {
        let network = Self(Rc::new(Inner {
            manager: RefCell::default(),
            access_point: RefCell::default(),
            wifi_device: RefCell::default(),
            listeners: RefCell::default(),
        }));
        let weak = Rc::downgrade(&network.0);
        dbus::watch(
            gio::BusType::System,
            NM_NAME,
            NM_PATH,
            NM_INTERFACE,
            move |proxy| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.manager.replace(Some(proxy.clone()));
                let network = Network(inner);
                network.follow_access_point();
                network.find_wifi_device();
                network.notify();
            },
        );
        network
    }

    fn notify(&self) {
        for listener in self.0.listeners.borrow().iter() {
            listener(self);
        }
    }

    pub fn subscribe(&self, listener: impl Fn(&Network) + 'static) {
        listener(self);
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    fn manager(&self) -> Option<gio::DBusProxy> {
        self.0.manager.borrow().clone().filter(dbus::is_running)
    }

    pub fn available(&self) -> bool {
        self.manager().is_some()
    }

    fn status(&self) -> Status {
        let Some(manager) = self.manager() else {
            return describe(0, "", None);
        };
        let state: u32 = dbus::property(&manager, "State").unwrap_or(0);
        let kind: String = dbus::property(&manager, "PrimaryConnectionType").unwrap_or_default();
        describe(state, &kind, self.strength())
    }

    fn strength(&self) -> Option<u8> {
        dbus::property(self.0.access_point.borrow().as_ref()?, "Strength")
    }

    /// 연결된 Wi-Fi 이름
    pub fn ssid(&self) -> Option<String> {
        let ssid: Vec<u8> = dbus::property(self.0.access_point.borrow().as_ref()?, "Ssid")?;
        Some(String::from_utf8_lossy(&ssid).into_owned())
    }

    pub fn has_wifi(&self) -> bool {
        self.0.wifi_device.borrow().is_some()
    }

    pub fn wifi_enabled(&self) -> bool {
        self.manager()
            .and_then(|manager| dbus::property(&manager, "WirelessEnabled"))
            .unwrap_or(false)
    }

    pub fn set_wifi_enabled(&self, on: bool) {
        if let Some(manager) = self.manager() {
            dbus::set_property(&manager, "WirelessEnabled", on.to_variant());
        }
    }

    /// 주 연결(Wi-Fi)이 쓰는 접속 지점을 찾아 신호 세기를 받는다
    fn follow_access_point(&self) {
        let Some(manager) = self.manager() else {
            self.0.access_point.replace(None);
            return;
        };
        let kind: String = dbus::property(&manager, "PrimaryConnectionType").unwrap_or_default();
        let primary: Option<glib::variant::ObjectPath> =
            dbus::property(&manager, "PrimaryConnection");
        let Some(primary) = primary.filter(|_| kind == WIRELESS) else {
            self.0.access_point.replace(None);
            return;
        };
        let connection = manager.connection();
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let Some(ap_path) = object_property(
                &connection,
                primary.as_str(),
                ACTIVE_INTERFACE,
                "SpecificObject",
            )
            .await
            else {
                return;
            };
            let Ok(access_point) = gio::DBusProxy::new_future(
                &connection,
                gio::DBusProxyFlags::NONE,
                None,
                Some(NM_NAME),
                &ap_path,
                ACCESS_POINT_INTERFACE,
            )
            .await
            else {
                return;
            };
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let same = inner
                .access_point
                .borrow()
                .as_ref()
                .is_some_and(|current| current.object_path() == access_point.object_path());
            if same {
                return;
            }
            let weak = Rc::downgrade(&inner);
            access_point.connect_local("g-properties-changed", false, move |_| {
                if let Some(inner) = weak.upgrade() {
                    Network(inner).notify();
                }
                None
            });
            inner.access_point.replace(Some(access_point));
            Network(inner).notify();
        });
    }

    /// 장치 목록에서 첫 Wi-Fi 장치를 찾는다
    fn find_wifi_device(&self) {
        let Some(manager) = self.manager() else {
            self.0.wifi_device.replace(None);
            return;
        };
        let devices: Vec<glib::variant::ObjectPath> =
            dbus::property(&manager, "Devices").unwrap_or_default();
        let connection = manager.connection();
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let mut found = None;
            for device in devices {
                let kind =
                    property_value(&connection, device.as_str(), DEVICE_INTERFACE, "DeviceType")
                        .await
                        .and_then(|value| value.get::<u32>());
                if kind == Some(DEVICE_TYPE_WIFI) {
                    found = Some(device.as_str().to_owned());
                    break;
                }
            }
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if *inner.wifi_device.borrow() != found {
                inner.wifi_device.replace(found);
                Network(inner).notify();
            }
        });
    }

    /// 주변 네트워크를 다시 찾게 한다 (결과는 몇 초 뒤 목록에 들어온다)
    pub fn request_scan(&self) {
        let (Some(manager), Some(device)) = (self.manager(), self.0.wifi_device.borrow().clone())
        else {
            return;
        };
        let options: HashMap<String, glib::Variant> = HashMap::new();
        manager.connection().call(
            Some(NM_NAME),
            &device,
            WIRELESS_INTERFACE,
            "RequestScan",
            Some(&(options,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            |_| {},
        );
    }

    /// 검색된 네트워크, 신호가 센 순서
    pub async fn access_points(&self) -> Vec<AccessPoint> {
        let (Some(manager), Some(device)) = (self.manager(), self.0.wifi_device.borrow().clone())
        else {
            return Vec::new();
        };
        let connection = manager.connection();
        let Some(paths) = property_value(&connection, &device, WIRELESS_INTERFACE, "AccessPoints")
            .await
            .and_then(|value| value.get::<Vec<glib::variant::ObjectPath>>())
        else {
            return Vec::new();
        };
        let mut found: Vec<AccessPoint> = Vec::new();
        for path in paths {
            let Some(props) =
                all_properties(&connection, path.as_str(), ACCESS_POINT_INTERFACE).await
            else {
                continue;
            };
            let get_u32 = |name: &str| props.get(name).and_then(|value| value.get::<u32>());
            let ssid = props
                .get("Ssid")
                .and_then(|value| value.get::<Vec<u8>>())
                .map(|ssid| String::from_utf8_lossy(&ssid).into_owned())
                .unwrap_or_default();
            if ssid.is_empty() {
                continue;
            }
            let strength = props
                .get("Strength")
                .and_then(|value| value.get::<u8>())
                .unwrap_or(0);
            // 암호화 플래그(WPA, RSN)나 개인정보 플래그가 있으면 암호가 필요하다
            let secure = get_u32("WpaFlags").unwrap_or(0) != 0
                || get_u32("RsnFlags").unwrap_or(0) != 0
                || get_u32("Flags").unwrap_or(0) & 1 != 0;
            match found.iter_mut().find(|ap| ap.ssid == ssid) {
                Some(existing) if existing.strength >= strength => {}
                Some(existing) => {
                    *existing = AccessPoint {
                        path: path.as_str().to_owned(),
                        ssid,
                        strength,
                        secure,
                    }
                }
                None => found.push(AccessPoint {
                    path: path.as_str().to_owned(),
                    ssid,
                    strength,
                    secure,
                }),
            }
        }
        found.sort_by_key(|point| std::cmp::Reverse(point.strength));
        found
    }

    /// 저장된 Wi-Fi 연결: SSID → 연결 경로
    pub async fn saved_networks(&self) -> HashMap<String, String> {
        let mut saved = HashMap::new();
        let Some(manager) = self.manager() else {
            return saved;
        };
        let connection = manager.connection();
        let Ok(reply) = connection
            .call_future(
                Some(NM_NAME),
                SETTINGS_PATH,
                SETTINGS_INTERFACE,
                "ListConnections",
                None,
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await
        else {
            return saved;
        };
        let paths: Vec<glib::variant::ObjectPath> = reply.child_value(0).get().unwrap_or_default();
        for path in paths {
            let Ok(reply) = connection
                .call_future(
                    Some(NM_NAME),
                    path.as_str(),
                    CONNECTION_INTERFACE,
                    "GetSettings",
                    None,
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await
            else {
                continue;
            };
            let settings: HashMap<String, HashMap<String, glib::Variant>> =
                reply.child_value(0).get().unwrap_or_default();
            let ssid = settings
                .get(WIRELESS)
                .and_then(|wireless| wireless.get("ssid"))
                .and_then(|ssid| ssid.get::<Vec<u8>>());
            if let Some(ssid) = ssid {
                saved.insert(
                    String::from_utf8_lossy(&ssid).into_owned(),
                    path.as_str().to_owned(),
                );
            }
        }
        saved
    }

    /// 네트워크에 연결한다. 저장된 연결이 있거나 암호가 없는 네트워크만 바로 연결하고,
    /// 암호가 필요한 새 네트워크는 시스템 설정의 Wi-Fi 페이지를 연다
    pub fn join(&self, access_point: &AccessPoint, saved: Option<&str>) {
        let (Some(manager), Some(device)) = (self.manager(), self.0.wifi_device.borrow().clone())
        else {
            return;
        };
        let device = glib::variant::ObjectPath::try_from(device).ok();
        let ap = glib::variant::ObjectPath::try_from(access_point.path.clone()).ok();
        let (Some(device), Some(ap)) = (device, ap) else {
            return;
        };
        match saved {
            Some(saved) => {
                let Ok(saved) = glib::variant::ObjectPath::try_from(saved.to_owned()) else {
                    return;
                };
                dbus::call(
                    &manager,
                    "ActivateConnection",
                    Some(&(saved, device, ap).to_variant()),
                );
            }
            None if !access_point.secure => {
                let settings: HashMap<String, HashMap<String, glib::Variant>> = HashMap::new();
                dbus::call(
                    &manager,
                    "AddAndActivateConnection",
                    Some(&(settings, device, ap).to_variant()),
                );
            }
            None => open_settings(Some("wifi")),
        }
    }
}

/// 객체 하나의 속성 값 (org.freedesktop.DBus.Properties.Get)
async fn property_value(
    connection: &gio::DBusConnection,
    path: &str,
    interface: &str,
    name: &str,
) -> Option<glib::Variant> {
    let reply = connection
        .call_future(
            Some(NM_NAME),
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(interface, name).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .ok()?;
    reply.child_value(0).as_variant()
}

async fn object_property(
    connection: &gio::DBusConnection,
    path: &str,
    interface: &str,
    name: &str,
) -> Option<String> {
    let value = property_value(connection, path, interface, name).await?;
    Some(
        value
            .get::<glib::variant::ObjectPath>()?
            .as_str()
            .to_owned(),
    )
}

async fn all_properties(
    connection: &gio::DBusConnection,
    path: &str,
    interface: &str,
) -> Option<HashMap<String, glib::Variant>> {
    let reply = connection
        .call_future(
            Some(NM_NAME),
            path,
            "org.freedesktop.DBus.Properties",
            "GetAll",
            Some(&(interface,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .ok()?;
    reply.child_value(0).get()
}

pub fn extra(tr: &Rc<Translations>, network: &Network) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name("network-offline-symbolic");
    let menu = Menu::new();
    let button = extras::button(&image, &menu, &tr.get("network-title"));
    let visibility = Visibility::new(&button, "show-wifi");
    let tooltip_tr = tr.clone();
    network.subscribe(move |network| {
        visibility.set_available(network.available());
        let status = network.status();
        image.set_icon_name(Some(status.icon));
        let mut args = FluentArgs::new();
        args.set("ssid", network.ssid().unwrap_or_default());
        image.set_tooltip_text(Some(&tooltip_tr.get_with(status.message, &args)));
    });

    let tr = tr.clone();
    let network = network.clone();
    menu.on_show(move |menu| fill_menu(menu, &tr, &network));
    button
}

fn fill_menu(menu: &Menu, tr: &Rc<Translations>, network: &Network) {
    if !network.has_wifi() {
        // 유선만 있는 기기: 상태와 설정 열기
        menu.title(&tr.get("network-title"));
        let mut args = FluentArgs::new();
        args.set("ssid", network.ssid().unwrap_or_default());
        menu.text(&tr.get_with(network.status().message, &args));
        menu.separator();
        menu.action(&tr.get("network-settings"), || {
            open_settings(Some("network"))
        });
        return;
    }

    let enabled = network.wifi_enabled();
    let switcher = network.clone();
    menu.title_switch(&tr.get("wifi-title"), enabled, move |on| {
        switcher.set_wifi_enabled(on)
    });
    if enabled {
        network.request_scan();
        // 검색 결과와 저장된 연결은 D-Bus로 읽은 뒤 채운다
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        menu.append(&list);
        let (menu, tr, network) = (menu.clone(), tr.clone(), network.clone());
        glib::spawn_future_local(async move {
            let points = network.access_points().await;
            let saved = network.saved_networks().await;
            let current = network.ssid();
            fill_networks(
                &menu,
                &list,
                &tr,
                &network,
                &points,
                &saved,
                current.as_deref(),
            );
        });
    } else {
        // 끄면 목록 없이 설정만
        menu.separator();
        menu.action(&tr.get("wifi-settings"), || open_settings(Some("wifi")));
    }
}

fn fill_networks(
    menu: &Menu,
    list: &gtk::Box,
    tr: &Translations,
    network: &Network,
    points: &[AccessPoint],
    saved: &HashMap<String, String>,
    current: Option<&str>,
) {
    // 그사이 메뉴를 닫았다 다시 열었으면 이 목록은 이미 치워졌다
    if list.parent().is_none() {
        return;
    }
    let section = |text: &str| {
        list.append(
            &gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .css_classes(["extra-section"])
                .build(),
        );
    };
    let add = |point: &AccessPoint, connected: bool| {
        let mut trailing = Vec::new();
        if point.secure {
            trailing.push("changes-prevent-symbolic");
        }
        trailing.push(signal_icon(point.strength));
        let (network, point_owned, saved_path) = (
            network.clone(),
            point.clone(),
            saved.get(&point.ssid).cloned(),
        );
        let row = menu.item_button(
            Row {
                icon: "network-wireless-symbolic",
                title: &point.ssid,
                subtitle: None,
                active: connected,
                trailing: &trailing,
            },
            move || {
                if !connected {
                    network.join(&point_owned, saved_path.as_deref());
                }
            },
        );
        list.append(&row);
    };

    if let Some(point) = current.and_then(|ssid| points.iter().find(|p| p.ssid == ssid)) {
        add(point, true);
    }
    let others = points
        .iter()
        .filter(|point| Some(point.ssid.as_str()) != current);
    let (known, unknown): (Vec<&AccessPoint>, Vec<&AccessPoint>) =
        others.partition(|point| saved.contains_key(&point.ssid));
    if !known.is_empty() {
        section(&tr.get("wifi-known"));
        for point in known.iter().take(MAX_NETWORKS) {
            add(point, false);
        }
    }
    if !unknown.is_empty() {
        section(&tr.get("wifi-other"));
        for point in unknown.iter().take(MAX_NETWORKS) {
            add(point, false);
        }
    }
    if points.is_empty() {
        section(&tr.get("wifi-none"));
    }
    menu.separator();
    menu.action(&tr.get("wifi-settings"), || open_settings(Some("wifi")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wired_and_wireless_connections_have_their_own_icons() {
        assert_eq!(
            describe(70, "802-3-ethernet", None).icon,
            "network-wired-symbolic"
        );
        assert_eq!(
            describe(70, WIRELESS, Some(90)).icon,
            "network-wireless-signal-excellent-symbolic"
        );
        assert_eq!(
            describe(70, WIRELESS, Some(40)).icon,
            "network-wireless-signal-ok-symbolic"
        );
    }

    #[test]
    fn partial_and_missing_connections_are_shown() {
        assert_eq!(
            describe(60, "802-3-ethernet", None).message,
            "network-limited"
        );
        assert_eq!(describe(40, "", None).message, "network-connecting");
        assert_eq!(
            describe(20, "", None),
            Status {
                icon: "network-offline-symbolic",
                message: "network-disconnected"
            }
        );
    }
}
