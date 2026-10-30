// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 네트워크(Wi-Fi) 항목. NetworkManager가 알려 주는 연결 방식(유선, Wi-Fi)과 상태를
//! 아이콘으로 보인다. Wi-Fi 장치가 있으면 누를 때 macOS의 Wi-Fi 메뉴처럼 켜고 끄는 스위치,
//! 연결된 네트워크, 알고 있는 네트워크, 기타 네트워크가 나오고, 없으면 연결 상태를 글로 보인다.
//! 암호가 필요한 새 네트워크는 연결을 만들어 켜고, 암호는 NetworkManager가 셸의 비밀 에이전트
//! (secret_agent.rs)에게 묻는다. 사용자 이름이 필요한 회사·학교 네트워크(802.1X)나 에이전트가 없을 때는
//! 시스템 설정의 Wi-Fi 페이지를 연다. NetworkManager가 없으면 항목을 숨긴다.
//! Wi-Fi 장치가 여럿이면 시스템 설정처럼 핫스팟으로 쓰지 않는 장치를 쓰고, 이 기기의 핫스팟 연결은
//! 알고 있는 네트워크에 넣지 않는다. 연결된 Wi-Fi는 유선이 함께 연결돼 있어도 맨 위에 보인다(macOS와
//! 같다). 메뉴 막대 아이콘은 주 연결을 따른다.

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    dbus,
    extras::{self, Listeners, Menu, Row, Visibility},
    menubar::open_settings,
    secret_agent,
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
/// NM80211Mode: 접속 지점(이 기기가 여는 핫스팟)
const MODE_AP: u32 = 3;
/// NMDeviceState: 연결을 마쳤다
const DEVICE_ACTIVATED: u32 = 100;
/// NM80211ApSecurityFlags: 802.1X 키 관리 (회사·학교 네트워크)
const KEY_MGMT_802_1X: u32 = 0x200;

/// NMActiveConnectionState
const ACTIVE_ACTIVATED: u32 = 2;
const ACTIVE_DEACTIVATED: u32 = 4;
/// 새로 만든 연결이 끝나기를 지켜보는 최대 시간. NetworkManager는 비밀 에이전트에게 한 번에 120초까지
/// 묻고 틀린 암호면 다시 묻는다. 이 시간이 지나도 끝나지 않으면 연결을 지우지 않고 그대로 둔다
const JOIN_WATCH: Duration = Duration::from_secs(600);
const JOIN_POLL: Duration = Duration::from_millis(500);

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

/// SSID 바이트를 보이는 이름으로. UTF-8이 아닌 바이트와 제어 문자(NUL 등)는 U+FFFD로 바꾼다. 예전에는
/// NUL을 그대로 두어, 그런 이름을 GTK 글로 넘길 때 잘리거나 멈출 수 있었다. 비밀 에이전트도 같은 규칙으로
/// 메뉴 막대에서 고른 네트워크와 견준다
pub(crate) fn ssid_text(ssid: &[u8]) -> String {
    String::from_utf8_lossy(ssid)
        .chars()
        .map(|char| if char.is_control() { '\u{FFFD}' } else { char })
        .collect()
}

/// 검색된 Wi-Fi 네트워크 (같은 이름의 접속 지점은 가장 센 것 하나)
#[derive(Debug, Clone, PartialEq)]
pub struct AccessPoint {
    pub path: String,
    pub ssid: String,
    pub strength: u8,
    pub secure: bool,
    /// 회사·학교 네트워크(802.1X). 사용자 이름이 있어야 연결을 만들 수 있다
    pub enterprise: bool,
}

/// 메뉴에서 네트워크를 눌렀을 때 할 일
#[derive(Debug, PartialEq, Eq)]
enum Join {
    /// 저장된 연결을 켠다
    Saved,
    /// 새 연결을 만들어 켠다. 암호가 필요하면 NetworkManager가 비밀 에이전트에게 묻는다
    New,
    /// 시스템 설정의 Wi-Fi 페이지를 연다
    Settings,
}

fn join_kind(access_point: &AccessPoint, saved: bool, agent_registered: bool) -> Join {
    if saved {
        Join::Saved
    } else if access_point.enterprise || (access_point.secure && !agent_registered) {
        Join::Settings
    } else {
        Join::New
    }
}

/// 메뉴가 쓸 Wi-Fi 장치: (경로, 핫스팟으로 쓰이는지) 가운데 핫스팟이 아닌 첫 장치, 모두 핫스팟이면 첫
/// 장치. 시스템 설정의 Wi-Fi 페이지와 같다
fn choose_wifi_device(devices: &[(String, bool)]) -> Option<&str> {
    devices
        .iter()
        .min_by_key(|(_, hotspot)| *hotspot)
        .map(|(path, _)| path.as_str())
}

/// 연결된 Wi-Fi의 접속 지점: Wi-Fi 장치의 상태(NMDeviceState), 방식(NM80211Mode), 쓰는 접속
/// 지점(ActiveAccessPoint)으로 고른다. 연결을 마쳤고 핫스팟이 아니면 그 접속 지점이다. 주 연결이 무엇인지
/// (유선이 함께 연결돼 있는지)는 보지 않는다
fn connected_access_point(
    state: Option<u32>,
    mode: Option<u32>,
    active: Option<String>,
) -> Option<String> {
    if state != Some(DEVICE_ACTIVATED) || mode == Some(MODE_AP) {
        return None;
    }
    // 접속 지점이 없으면 NetworkManager는 "/"를 준다
    active.filter(|path| path != "/")
}

/// 저장된 연결(GetSettings)이 알고 있는 Wi-Fi 네트워크면 그 이름. 이 기기가 여는 핫스팟 연결(mode
/// "ap")은 알고 있는 네트워크가 아니다(시스템 설정의 Wi-Fi 페이지와 같다)
fn known_ssid(settings: &HashMap<String, HashMap<String, glib::Variant>>) -> Option<String> {
    let wireless = settings.get(WIRELESS)?;
    let mode = wireless.get("mode").and_then(|mode| mode.get::<String>());
    if mode.as_deref() == Some("ap") {
        return None;
    }
    let ssid = wireless.get("ssid")?.get::<Vec<u8>>()?;
    Some(ssid_text(&ssid))
}

/// 활성 연결의 상태(NMActiveConnectionState)로 본 연결 결과. 사라진 활성 연결(None)은 실패한 것이다.
/// 아직 연결 중이면 None
fn join_result(state: Option<u32>) -> Option<bool> {
    match state {
        Some(ACTIVE_ACTIVATED) => Some(true),
        Some(ACTIVE_DEACTIVATED) | None => Some(false),
        Some(_) => None,
    }
}

/// 네트워크 상태. 복제해도 같은 상태를 가리킨다. 제어 센터의 Wi-Fi 칸도 같은 것을 쓴다
#[derive(Clone)]
pub struct Network(Rc<Inner>);

struct Inner {
    manager: RefCell<Option<gio::DBusProxy>>,
    /// 메뉴가 쓰는 Wi-Fi 장치가 연결된 접속 지점(connected_access_point). 유선이 주 연결이어도 따른다.
    /// 신호 세기가 바뀌면 알려 준다
    access_point: RefCell<Option<gio::DBusProxy>>,
    /// 메뉴가 쓰는 Wi-Fi 장치의 경로(choose_wifi_device). 없으면 None
    wifi_device: RefCell<Option<String>>,
    /// 그 장치의 속성 변경 구독(watch_device). 장치가 바뀌면 다시 구독한다
    device_changes: RefCell<Option<gio::SignalSubscription>>,
    listeners: Listeners<Network>,
}

impl Network {
    pub fn start() -> Self {
        let network = Self(Rc::new(Inner {
            manager: RefCell::default(),
            access_point: RefCell::default(),
            wifi_device: RefCell::default(),
            device_changes: RefCell::default(),
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
                // 접속 지점은 고른 장치의 것이므로 장치를 먼저 고른다
                network.follow_wifi();
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
        Some(ssid_text(&ssid))
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

    /// NetworkManager가 바뀌면 메뉴가 쓸 Wi-Fi 장치와 그 장치가 연결된 접속 지점을 다시 읽는다
    fn follow_wifi(&self) {
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            if let Some(inner) = weak.upgrade() {
                let network = Network(inner);
                network.refresh_wifi_device().await;
                network.refresh_access_point().await;
            }
        });
    }

    /// 메뉴가 쓸 Wi-Fi 장치를 고르고, 바뀌었으면 그 장치를 지켜보고 알린다. 예전에는 첫 Wi-Fi 장치를 써서,
    /// 장치가 여럿이고 첫 장치가 핫스팟이면 그 장치로 연결하려다 핫스팟을 끊었다. 이제 핫스팟이 아닌 장치를
    /// 고른다
    async fn refresh_wifi_device(&self) {
        let manager = self.manager();
        let found = match &manager {
            Some(manager) => wifi_device(manager).await,
            None => None,
        };
        if *self.0.wifi_device.borrow() == found {
            return;
        }
        let changes = manager
            .zip(found.as_deref())
            .map(|(manager, device)| self.watch_device(&manager.connection(), device));
        self.0.device_changes.replace(changes);
        self.0.wifi_device.replace(found);
        self.notify();
    }

    /// 장치의 연결 상태, 방식(핫스팟), 접속 지점이 바뀌면 연결된 접속 지점을 다시 읽는다. 유선이 주 연결이면
    /// Wi-Fi가 연결되거나 끊겨도 NetworkManager 전체의 속성은 그대로일 수 있어 장치를 따로 지켜본다
    fn watch_device(
        &self,
        connection: &gio::DBusConnection,
        device: &str,
    ) -> gio::SignalSubscription {
        let weak = Rc::downgrade(&self.0);
        connection.subscribe_to_signal(
            Some(NM_NAME),
            Some("org.freedesktop.DBus.Properties"),
            Some("PropertiesChanged"),
            Some(device),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let changed: HashMap<String, glib::Variant> = signal
                    .parameters
                    .try_child_value(1)
                    .and_then(|changed| changed.get())
                    .unwrap_or_default();
                // 검색 시각(LastScan) 같은 잦은 변경은 건너뛴다
                let relevant = ["State", "Mode", "ActiveAccessPoint"]
                    .iter()
                    .any(|name| changed.contains_key(*name));
                if relevant && let Some(inner) = weak.upgrade() {
                    glib::spawn_future_local(async move {
                        Network(inner).refresh_access_point().await;
                    });
                }
            },
        )
    }

    /// 메뉴가 쓰는 Wi-Fi 장치가 연결된 접속 지점을 따라가 이름과 신호 세기를 받는다. 예전에는 주 연결이
    /// Wi-Fi일 때만 따라가, 유선이 함께 연결돼 있으면 연결된 Wi-Fi가 메뉴 맨 위에 오지 않고 알고 있는
    /// 네트워크 사이에 섞였다(macOS는 연결된 Wi-Fi를 늘 위에 보인다). 메뉴 막대 아이콘은 그대로 주 연결을 따른다
    async fn refresh_access_point(&self) {
        let device = self.0.wifi_device.borrow().clone();
        let (Some(manager), Some(device)) = (self.manager(), device) else {
            self.clear_access_point();
            return;
        };
        let connection = manager.connection();
        let state = property_value(&connection, &device, DEVICE_INTERFACE, "State")
            .await
            .and_then(|value| value.get::<u32>());
        let mode = property_value(&connection, &device, WIRELESS_INTERFACE, "Mode")
            .await
            .and_then(|value| value.get::<u32>());
        let active = object_property(
            &connection,
            &device,
            WIRELESS_INTERFACE,
            "ActiveAccessPoint",
        )
        .await;
        // 읽는 사이에 장치를 다시 골랐으면 그쪽이 읽는다
        if self.0.wifi_device.borrow().as_deref() != Some(device.as_str()) {
            return;
        }
        let Some(path) = connected_access_point(state, mode, active) else {
            self.clear_access_point();
            return;
        };
        let same = self
            .0
            .access_point
            .borrow()
            .as_ref()
            .is_some_and(|current| current.object_path() == path.as_str());
        if same {
            return;
        }
        let Ok(access_point) = gio::DBusProxy::new_future(
            &connection,
            gio::DBusProxyFlags::NONE,
            None,
            Some(NM_NAME),
            &path,
            ACCESS_POINT_INTERFACE,
        )
        .await
        else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        access_point.connect_local("g-properties-changed", false, move |_| {
            if let Some(inner) = weak.upgrade() {
                Network(inner).notify();
            }
            None
        });
        self.0.access_point.replace(Some(access_point));
        self.notify();
    }

    fn clear_access_point(&self) {
        if self.0.access_point.replace(None).is_some() {
            self.notify();
        }
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
                .map(|ssid| ssid_text(&ssid))
                .unwrap_or_default();
            if ssid.is_empty() {
                continue;
            }
            let strength = props
                .get("Strength")
                .and_then(|value| value.get::<u8>())
                .unwrap_or(0);
            // 암호화 플래그(WPA, RSN)나 개인정보 플래그가 있으면 암호가 필요하다
            let security = get_u32("WpaFlags").unwrap_or(0) | get_u32("RsnFlags").unwrap_or(0);
            let secure = security != 0 || get_u32("Flags").unwrap_or(0) & 1 != 0;
            let enterprise = security & KEY_MGMT_802_1X != 0;
            match found.iter_mut().find(|ap| ap.ssid == ssid) {
                Some(existing) if existing.strength >= strength => {}
                Some(existing) => {
                    *existing = AccessPoint {
                        path: path.as_str().to_owned(),
                        ssid,
                        strength,
                        secure,
                        enterprise,
                    }
                }
                None => found.push(AccessPoint {
                    path: path.as_str().to_owned(),
                    ssid,
                    strength,
                    secure,
                    enterprise,
                }),
            }
        }
        found.sort_by_key(|point| std::cmp::Reverse(point.strength));
        found
    }

    /// 저장된 Wi-Fi 연결: SSID → 연결 경로. 이 기기의 핫스팟 연결은 뺀다(known_ssid). 예전에는 핫스팟
    /// 연결도 알고 있는 네트워크로 세어, 다른 장치가 찾은 그 핫스팟을 누르면 핫스팟 연결을 켜려 했다
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
            if let Some(ssid) = known_ssid(&settings) {
                saved.insert(ssid, path.as_str().to_owned());
            }
        }
        saved
    }

    /// 네트워크에 연결한다. 저장된 연결은 그대로 켜고(암호가 바뀌었으면 NetworkManager가 비밀
    /// 에이전트에게 다시 묻는다), 새 네트워크는 연결을 만들어 켠다. 사용자 이름이 필요한 회사·학교
    /// 네트워크나, 비밀 에이전트가 없는데 암호가 필요한 네트워크는 시스템 설정의 Wi-Fi 페이지를 연다.
    /// 예전에는 암호가 필요한 새 네트워크를 모두 시스템 설정으로 보냈다
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
        let kind = join_kind(access_point, saved.is_some(), secret_agent::registered());
        if kind != Join::Settings {
            // 이 네트워크를 묻는 Wi-Fi 암호 창은 사람이 방금 고른 것이라 키보드를 바로 받는다
            secret_agent::expect_join(&access_point.ssid);
        }
        match kind {
            Join::Saved => {
                let Some(Ok(saved)) =
                    saved.map(|saved| glib::variant::ObjectPath::try_from(saved.to_owned()))
                else {
                    return;
                };
                dbus::call(
                    &manager,
                    "ActivateConnection",
                    Some(&(saved, device, ap).to_variant()),
                );
            }
            Join::New => {
                glib::spawn_future_local(add_and_join(manager.connection(), device, ap));
            }
            Join::Settings => open_settings(Some("wifi")),
        }
    }
}

/// 장치 목록에서 메뉴가 쓸 Wi-Fi 장치 (choose_wifi_device)
async fn wifi_device(manager: &gio::DBusProxy) -> Option<String> {
    let devices: Vec<glib::variant::ObjectPath> =
        dbus::property(manager, "Devices").unwrap_or_default();
    let connection = manager.connection();
    let mut wifi = Vec::new();
    for device in devices {
        let kind = property_value(&connection, device.as_str(), DEVICE_INTERFACE, "DeviceType")
            .await
            .and_then(|value| value.get::<u32>());
        if kind != Some(DEVICE_TYPE_WIFI) {
            continue;
        }
        let mode = property_value(&connection, device.as_str(), WIRELESS_INTERFACE, "Mode")
            .await
            .and_then(|value| value.get::<u32>());
        wifi.push((device.as_str().to_owned(), mode == Some(MODE_AP)));
    }
    choose_wifi_device(&wifi).map(str::to_owned)
}

/// 새 연결을 저장하고 켠다. 연결하지 못하면(암호 창에서 취소, 시간 초과) 만든 연결을 지워
/// 알고 있는 네트워크에 남기지 않는다. 틀린 암호는 NetworkManager가 비밀 에이전트에게 다시 묻는다
async fn add_and_join(
    connection: gio::DBusConnection,
    device: glib::variant::ObjectPath,
    ap: glib::variant::ObjectPath,
) {
    // 빈 설정을 주면 NetworkManager가 접속 지점에 맞춰 보안 방식까지 채운다
    let settings: HashMap<String, HashMap<String, glib::Variant>> = HashMap::new();
    let reply = connection
        .call_future(
            Some(NM_NAME),
            NM_PATH,
            NM_INTERFACE,
            "AddAndActivateConnection",
            Some(&(settings, device, ap).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    let reply = match reply {
        Ok(reply) => reply,
        Err(err) => {
            eprintln!("bunt-shell: Wi-Fi: {err}");
            return;
        }
    };
    let path = |index| {
        reply
            .child_value(index)
            .get::<glib::variant::ObjectPath>()
            .map(|path| path.as_str().to_owned())
    };
    let (Some(saved), Some(active)) = (path(0), path(1)) else {
        return;
    };
    if wait_for_join(&connection, &active).await != Some(false) {
        return;
    }
    let deleted = connection
        .call_future(
            Some(NM_NAME),
            &saved,
            CONNECTION_INTERFACE,
            "Delete",
            None,
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    if let Err(err) = deleted {
        eprintln!("bunt-shell: Wi-Fi: {err}");
    }
}

/// 활성 연결이 켜지거나 꺼질 때까지 기다린다. JOIN_WATCH가 지나도 연결 중이면 None
async fn wait_for_join(connection: &gio::DBusConnection, active: &str) -> Option<bool> {
    let started = std::time::Instant::now();
    while started.elapsed() < JOIN_WATCH {
        let state = property_value(connection, active, ACTIVE_INTERFACE, "State")
            .await
            .and_then(|value| value.get::<u32>());
        if let Some(joined) = join_result(state) {
            return Some(joined);
        }
        glib::timeout_future(JOIN_POLL).await;
    }
    None
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
        // 검색 결과와 저장된 연결은 D-Bus로 읽은 뒤 채운다
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        menu.append(&list);
        let (menu, tr, network) = (menu.clone(), tr.clone(), network.clone());
        glib::spawn_future_local(async move {
            // 열 때마다 장치를 다시 고른다. 핫스팟을 막 켠 장치의 Mode는 NetworkManager의 활성 연결
            // 목록이 바뀐 뒤에 바뀌어, 그 알림으로 고른 장치가 핫스팟일 수 있다. 맨 위에 둘 연결된
            // 네트워크도 그 장치에서 다시 읽는다
            network.refresh_wifi_device().await;
            network.refresh_access_point().await;
            network.request_scan();
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

/// 메뉴의 네트워크 차례: 연결된 네트워크(맨 위), 알고 있는 네트워크, 기타 네트워크. 연결된 네트워크는
/// 알고 있는 네트워크여도 아래에 다시 넣지 않는다
fn arrange<'a>(
    points: &'a [AccessPoint],
    saved: &HashMap<String, String>,
    current: Option<&str>,
) -> (
    Option<&'a AccessPoint>,
    Vec<&'a AccessPoint>,
    Vec<&'a AccessPoint>,
) {
    let connected = current.and_then(|ssid| points.iter().find(|point| point.ssid == ssid));
    let (known, unknown) = points
        .iter()
        .filter(|point| Some(point.ssid.as_str()) != current)
        .partition(|point| saved.contains_key(&point.ssid));
    (connected, known, unknown)
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

    let (connected, known, unknown) = arrange(points, saved, current);
    if let Some(point) = connected {
        add(point, true);
    }
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

    fn point(secure: bool, enterprise: bool) -> AccessPoint {
        AccessPoint {
            path: "/org/freedesktop/NetworkManager/AccessPoint/1".into(),
            ssid: "Lab".into(),
            strength: 70,
            secure,
            enterprise,
        }
    }

    #[test]
    fn new_secure_networks_ask_through_the_secret_agent() {
        assert_eq!(join_kind(&point(true, false), true, true), Join::Saved);
        assert_eq!(join_kind(&point(true, true), true, false), Join::Saved);
        assert_eq!(join_kind(&point(false, false), false, false), Join::New);
        assert_eq!(join_kind(&point(true, false), false, true), Join::New);
        // 에이전트가 없으면 예전처럼 시스템 설정에서 암호를 받는다
        assert_eq!(join_kind(&point(true, false), false, false), Join::Settings);
        // 802.1X는 사용자 이름이 있어야 연결을 만들 수 있다
        assert_eq!(join_kind(&point(true, true), false, true), Join::Settings);
    }

    #[test]
    fn ssids_keep_text_but_lose_control_characters() {
        assert_eq!(ssid_text("집 Wi-Fi".as_bytes()), "집 Wi-Fi");
        assert_eq!(ssid_text(b"Lab\0Guest"), "Lab\u{FFFD}Guest");
        assert_eq!(ssid_text(b"\xff\nx"), "\u{FFFD}\u{FFFD}x");
        assert_eq!(ssid_text(b""), "");
    }

    #[test]
    fn the_menu_uses_a_wifi_device_that_is_not_a_hotspot() {
        let device = |path: &str, hotspot| (path.to_owned(), hotspot);
        // 첫 장치(wlan0)가 핫스팟이면 그다음 장치로 연결한다
        let wifi = [
            device("/wlan0", true),
            device("/wlan1", false),
            device("/wlan2", false),
        ];
        assert_eq!(choose_wifi_device(&wifi), Some("/wlan1"));
        // 핫스팟이 없으면 첫 장치
        let wifi = [device("/wlan0", false), device("/wlan1", false)];
        assert_eq!(choose_wifi_device(&wifi), Some("/wlan0"));
        // 모두 핫스팟이면 시스템 설정처럼 첫 장치를 보인다
        let wifi = [device("/wlan0", true), device("/wlan1", true)];
        assert_eq!(choose_wifi_device(&wifi), Some("/wlan0"));
        assert_eq!(choose_wifi_device(&[]), None);
    }

    #[test]
    fn hotspot_connections_are_not_known_networks() {
        let saved = |wireless: &[(&str, glib::Variant)]| {
            let wireless: HashMap<String, glib::Variant> = wireless
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect();
            HashMap::from([(WIRELESS.to_owned(), wireless)])
        };
        let ssid = |name: &str| name.as_bytes().to_vec().to_variant();
        assert_eq!(
            known_ssid(&saved(&[
                ("ssid", ssid("Home")),
                ("mode", "infrastructure".to_variant())
            ])),
            Some("Home".into())
        );
        // mode가 없으면 infrastructure다
        assert_eq!(
            known_ssid(&saved(&[("ssid", ssid("Cafe"))])),
            Some("Cafe".into())
        );
        // 이 기기가 여는 핫스팟
        assert_eq!(
            known_ssid(&saved(&[
                ("ssid", ssid("Bunt")),
                ("mode", "ap".to_variant())
            ])),
            None
        );
        // 유선 연결, SSID 없는 연결
        assert_eq!(known_ssid(&HashMap::new()), None);
        assert_eq!(
            known_ssid(&saved(&[("mode", "infrastructure".to_variant())])),
            None
        );
    }

    #[test]
    fn the_connected_wifi_is_followed_whatever_the_primary_connection() {
        let ap = || Some("/org/freedesktop/NetworkManager/AccessPoint/3".to_owned());
        // 연결을 마친 장치의 접속 지점. 유선이 주 연결이어도 같다(주 연결은 보지 않는다)
        assert_eq!(
            connected_access_point(Some(DEVICE_ACTIVATED), Some(2), ap()),
            ap()
        );
        // 연결 중, 끊김, 핫스팟, 접속 지점 없음("/")
        assert_eq!(connected_access_point(Some(70), Some(2), ap()), None);
        assert_eq!(connected_access_point(Some(30), Some(2), None), None);
        assert_eq!(
            connected_access_point(Some(DEVICE_ACTIVATED), Some(MODE_AP), ap()),
            None
        );
        assert_eq!(
            connected_access_point(Some(DEVICE_ACTIVATED), Some(2), Some("/".into())),
            None
        );
        assert_eq!(connected_access_point(None, None, ap()), None);
    }

    #[test]
    fn the_connected_network_comes_first_and_only_once() {
        let named = |ssid: &str, strength| AccessPoint {
            path: format!("/ap/{ssid}"),
            ssid: ssid.into(),
            strength,
            secure: true,
            enterprise: false,
        };
        // 신호가 센 순서. 연결된 Office는 Home보다 약하다
        let points = [named("Cafe", 90), named("Home", 80), named("Office", 40)];
        let saved = HashMap::from([
            ("Home".to_owned(), "/settings/1".to_owned()),
            ("Office".to_owned(), "/settings/2".to_owned()),
        ]);
        let ssids = |points: Vec<&AccessPoint>| -> Vec<String> {
            points.into_iter().map(|point| point.ssid.clone()).collect()
        };
        let (connected, known, unknown) = arrange(&points, &saved, Some("Office"));
        assert_eq!(connected.map(|point| point.ssid.as_str()), Some("Office"));
        assert_eq!(ssids(known), ["Home"]);
        assert_eq!(ssids(unknown), ["Cafe"]);
        // 연결된 Wi-Fi가 없으면(꺼짐, 유선만) 알고 있는 네트워크 사이에 둔다
        let (connected, known, _) = arrange(&points, &saved, None);
        assert!(connected.is_none());
        assert_eq!(ssids(known), ["Home", "Office"]);
    }

    #[test]
    fn joins_end_when_the_active_connection_settles() {
        assert_eq!(join_result(Some(ACTIVE_ACTIVATED)), Some(true));
        assert_eq!(join_result(Some(ACTIVE_DEACTIVATED)), Some(false));
        assert_eq!(join_result(None), Some(false));
        // 활성화 중(1)과 끄는 중(3)은 더 기다린다
        assert_eq!(join_result(Some(1)), None);
        assert_eq!(join_result(Some(3)), None);
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
