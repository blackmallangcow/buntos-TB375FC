// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터에 메뉴 막대·Dock 설정을 알린다 (org.buntos.Compositor1.SetPanels). 컴포지터가 이 값으로
//! 막대를 숨기고 꺼내며(macOS의 WindowServer처럼), 최대화·전체 화면 창이 막대 자리까지 채울지와
//! 메뉴 막대 뒤를 흐릴지 정한다. 컴포지터의 D-Bus 서비스가 (다시) 뜰 때와 값이 바뀔 때마다 보낸다.

use gtk4::{gio, prelude::*};

use crate::dbus;

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";

/// 컴포지터가 알아야 하는 셸 설정 키 (SetPanels의 인자 순서)
const KEYS: [&str; 3] = ["menu-bar-autohide", "dock-autohide", "menu-bar-background"];

pub fn share_panel_settings(settings: &gio::Settings) {
    let settings = settings.clone();
    gio::DBusProxy::for_bus(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES | gio::DBusProxyFlags::DO_NOT_CONNECT_SIGNALS,
        None,
        NAME,
        PATH,
        INTERFACE,
        gio::Cancellable::NONE,
        move |result| {
            let proxy = match result {
                Ok(proxy) => proxy,
                Err(err) => {
                    eprintln!("bunt-shell: {NAME}: {err}");
                    return;
                }
            };
            for key in KEYS {
                // 처리기가 프록시와 설정 객체를 붙들어 셸이 끝날 때까지 둔다
                let proxy = proxy.clone();
                settings.connect_changed(Some(key), move |settings, _| send(&proxy, settings));
            }
            let keep_alive = settings.clone();
            proxy.connect_notify_local(Some("g-name-owner"), move |proxy, _| {
                send(proxy, &keep_alive);
            });
            // 변경 알림은 처리기를 단 뒤 한 번 읽어야 온다
            send(&proxy, &settings);
        },
    );
}

fn send(proxy: &gio::DBusProxy, settings: &gio::Settings) {
    let args = (
        settings.string(KEYS[0]).to_string(),
        settings.string(KEYS[1]).to_string(),
        settings.boolean(KEYS[2]),
    )
        .to_variant();
    if dbus::is_running(proxy) {
        dbus::call(proxy, "SetPanels", Some(&args));
    }
}
