// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! D-Bus 도우미. 메뉴 막대 항목이 시스템 서비스(UPower, BlueZ, NetworkManager)와 세션
//! 서비스(fcitx5)의 속성을 지켜보고 바꾸는 데 쓴다.

use std::rc::Rc;

use gtk4::{gio, glib, prelude::*};

/// 서비스의 객체 하나를 지켜본다. 처음 연결되면 한 번, 그 뒤로는 속성이 바뀌거나 서비스가
/// 켜지고 꺼질 때마다 changed를 부른다. 서비스가 꺼져 있으면 프록시의 name_owner()가 None이다.
/// 프록시는 셸이 끝날 때까지 산다.
pub fn watch(
    bus: gio::BusType,
    name: &str,
    path: &str,
    interface: &str,
    changed: impl Fn(&gio::DBusProxy) + 'static,
) {
    let service = name.to_owned();
    gio::DBusProxy::for_bus(
        bus,
        gio::DBusProxyFlags::NONE,
        None,
        name,
        path,
        interface,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(proxy) => {
                let changed = Rc::new(changed);
                let on_change = changed.clone();
                // 처리기가 프록시를 붙들어 프록시가 사라지지 않게 한다
                let keep = proxy.clone();
                proxy.connect_local("g-properties-changed", false, move |_| {
                    on_change(&keep);
                    None
                });
                let on_owner = changed.clone();
                proxy.connect_notify_local(Some("g-name-owner"), move |proxy, _| on_owner(proxy));
                changed(&proxy);
            }
            Err(err) => eprintln!("bunt-shell: {service}: {err}"),
        },
    );
}

/// 캐시된 속성 값
pub fn property<T: glib::variant::FromVariant>(proxy: &gio::DBusProxy, name: &str) -> Option<T> {
    proxy.cached_property(name)?.get()
}

/// 서비스가 떠 있는지
pub fn is_running(proxy: &gio::DBusProxy) -> bool {
    proxy.name_owner().is_some()
}

/// 속성을 바꾼다 (org.freedesktop.DBus.Properties.Set). 바뀐 값은 속성 변경 알림으로 돌아온다
pub fn set_property(proxy: &gio::DBusProxy, name: &str, value: glib::Variant) {
    let args = set_property_args(proxy.interface_name().as_str(), name, value);
    call(proxy, "org.freedesktop.DBus.Properties.Set", Some(&args));
}

/// Properties.Set의 인자 (ssv). 튜플의 to_variant가 값을 v로 한 번 감싼다. 예전에는 값을 미리
/// Variant::from_variant로 감싸 v 안에 v가 들어갔고, 서비스가 "Expected type 'b' but got 'v'"로
/// 거절했다(Bluetooth 켜기, Wi-Fi 켜기, 전원 모드)
fn set_property_args(interface: &str, name: &str, value: glib::Variant) -> glib::Variant {
    (interface, name, value).to_variant()
}

/// 메서드를 부르고 결과는 기다리지 않는다. 실패하면 기록만 남긴다
pub fn call(proxy: &gio::DBusProxy, method: &str, args: Option<&glib::Variant>) {
    let label = format!("{}.{method}", proxy.interface_name());
    proxy.call(
        method,
        args,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| {
            if let Err(err) = result {
                eprintln!("bunt-shell: {label}: {err}");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_values_are_wrapped_once() {
        let args = set_property_args("org.bluez.Adapter1", "Powered", true.to_variant());
        assert_eq!(args.type_().as_str(), "(ssv)");
        assert_eq!(
            args.print(true),
            "('org.bluez.Adapter1', 'Powered', <true>)"
        );
    }
}
