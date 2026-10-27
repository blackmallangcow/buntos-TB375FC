// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴바의 네트워크 아이콘. NetworkManager가 알려 주는 연결 방식(유선, Wi-Fi)과 상태를
//! 아이콘으로 보이고, 누르면 상태를 글로 보인다. NetworkManager가 없으면 아이콘을 숨긴다.

use std::{cell::RefCell, rc::Rc};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

const NM_NAME: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_INTERFACE: &str = "org.freedesktop.NetworkManager";
const ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const ACCESS_POINT_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const WIRELESS: &str = "802-11-wireless";

/// NetworkManager의 NMState 값
const CONNECTING: u32 = 40;
const CONNECTED_LOCAL: u32 = 50;
const CONNECTED_GLOBAL: u32 = 70;

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

struct Indicator {
    tr: Rc<Translations>,
    button: gtk::MenuButton,
    image: gtk::Image,
    label: gtk::Label,
    manager: RefCell<Option<gio::DBusProxy>>,
    /// 주 연결이 Wi-Fi일 때 그 접속 지점. 신호 세기가 바뀌면 알려 준다
    access_point: RefCell<Option<gio::DBusProxy>>,
}

pub fn indicator(tr: &Rc<Translations>) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name("network-offline-symbolic");
    let label = gtk::Label::builder()
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(8)
        .margin_end(8)
        .build();
    let popover = gtk::Popover::builder()
        .child(&label)
        .has_arrow(false)
        .build();
    let button = gtk::MenuButton::builder()
        .child(&image)
        .popover(&popover)
        .css_classes(["status-item"])
        .visible(false)
        .build();
    let indicator = Rc::new(Indicator {
        tr: tr.clone(),
        button: button.clone(),
        image,
        label,
        manager: RefCell::new(None),
        access_point: RefCell::new(None),
    });
    gio::DBusProxy::for_bus(
        gio::BusType::System,
        gio::DBusProxyFlags::NONE,
        None,
        NM_NAME,
        NM_PATH,
        NM_INTERFACE,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(proxy) => indicator.watch(proxy),
            Err(err) => eprintln!("bunt-shell: NetworkManager: {err}"),
        },
    );
    button
}

impl Indicator {
    fn watch(self: Rc<Self>, proxy: gio::DBusProxy) {
        let weak = Rc::downgrade(&self);
        proxy.connect_local("g-properties-changed", false, move |_| {
            if let Some(indicator) = weak.upgrade() {
                indicator.refresh();
            }
            None
        });
        // NetworkManager가 나중에 켜지거나 다시 시작해도 따라간다
        let weak = Rc::downgrade(&self);
        proxy.connect_notify_local(Some("g-name-owner"), move |_, _| {
            if let Some(indicator) = weak.upgrade() {
                indicator.refresh();
            }
        });
        *self.manager.borrow_mut() = Some(proxy);
        self.refresh();
    }

    fn property<T: glib::variant::FromVariant>(proxy: &gio::DBusProxy, name: &str) -> Option<T> {
        proxy.cached_property(name)?.get()
    }

    fn refresh(self: &Rc<Self>) {
        let Some(manager) = self.manager.borrow().clone() else {
            return;
        };
        if manager.name_owner().is_none() {
            self.button.set_visible(false);
            return;
        }
        let state: u32 = Self::property(&manager, "State").unwrap_or(0);
        let kind: String = Self::property(&manager, "PrimaryConnectionType").unwrap_or_default();
        if kind == WIRELESS {
            self.follow_access_point(&manager);
        } else {
            self.access_point.replace(None);
        }

        let access_point = self.access_point.borrow();
        let strength: Option<u8> = access_point
            .as_ref()
            .and_then(|ap| Self::property(ap, "Strength"));
        let ssid: Option<Vec<u8>> = access_point
            .as_ref()
            .and_then(|ap| Self::property(ap, "Ssid"));
        let status = describe(state, &kind, strength);
        let mut args = FluentArgs::new();
        args.set(
            "ssid",
            ssid.map(|ssid| String::from_utf8_lossy(&ssid).into_owned())
                .unwrap_or_default(),
        );
        self.image.set_icon_name(Some(status.icon));
        self.label
            .set_text(&self.tr.get_with(status.message, &args));
        self.button.set_visible(true);
    }

    /// 주 연결(Wi-Fi)이 쓰는 접속 지점을 찾아 신호 세기를 받는다
    fn follow_access_point(self: &Rc<Self>, manager: &gio::DBusProxy) {
        let Some(primary) =
            Self::property::<glib::variant::ObjectPath>(manager, "PrimaryConnection")
        else {
            return;
        };
        let connection = manager.connection();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Ok(active) = gio::DBusProxy::new_future(
                &connection,
                gio::DBusProxyFlags::NONE,
                None,
                Some(NM_NAME),
                primary.as_str(),
                ACTIVE_INTERFACE,
            )
            .await
            else {
                return;
            };
            let Some(ap_path) =
                Self::property::<glib::variant::ObjectPath>(&active, "SpecificObject")
            else {
                return;
            };
            let Ok(access_point) = gio::DBusProxy::new_future(
                &connection,
                gio::DBusProxyFlags::NONE,
                None,
                Some(NM_NAME),
                ap_path.as_str(),
                ACCESS_POINT_INTERFACE,
            )
            .await
            else {
                return;
            };
            let Some(indicator) = weak.upgrade() else {
                return;
            };
            let same = indicator
                .access_point
                .borrow()
                .as_ref()
                .is_some_and(|current| current.object_path() == access_point.object_path());
            if same {
                return;
            }
            let weak = Rc::downgrade(&indicator);
            access_point.connect_local("g-properties-changed", false, move |_| {
                if let Some(indicator) = weak.upgrade() {
                    indicator.refresh();
                }
                None
            });
            indicator.access_point.replace(Some(access_point));
            indicator.refresh();
        });
    }
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
