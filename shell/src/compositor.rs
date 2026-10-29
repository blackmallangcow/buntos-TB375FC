// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터에 메뉴 막대·Dock 설정을 알린다 (org.buntos.Compositor1.SetPanels). 컴포지터가 이 값으로
//! 막대를 숨기고 꺼내며(macOS의 WindowServer처럼), 최대화·전체 화면 창이 막대 자리까지 채울지와
//! 메뉴 막대 뒤를 흐릴지 정한다. 컴포지터의 D-Bus 서비스가 (다시) 뜰 때와 값이 바뀔 때마다 보낸다.
//! 컴포지터가 받은 단축키 가운데 셸이 하는 것(ShortcutActivated: 모든 앱, Dock 가리기, ⇧⌘5)도
//! 여기서 받는다.

use gtk4::{gio, prelude::*};

use crate::{dbus, launcher::Launcher, screenshot};

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

/// 컴포지터가 알린 단축키를 한다: 모든 앱 보기(⌘스페이스, 검색 칸에 포커스), Dock 가리기
/// 켜기/끄기(⌥⌘D), 스크린샷 도구 막대(⇧⌘5). 끈 Dock 가리기는 기본값처럼 "전체 화면일 때"만
/// 숨긴다 (macOS의 끈 상태)
pub fn follow_shortcuts(
    launcher: &Launcher,
    settings: Option<gio::Settings>,
    screenshot_toolbar: &screenshot::Toolbar,
) {
    let (launcher, screenshot_toolbar) = (launcher.clone(), screenshot_toolbar.clone());
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let connection = match result {
                Ok(connection) => connection,
                Err(err) => {
                    eprintln!("bunt-shell: session bus: {err}");
                    return;
                }
            };
            let subscription = connection.subscribe_to_signal(
                Some(NAME),
                Some(INTERFACE),
                Some("ShortcutActivated"),
                Some(PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    let Some((id,)) = signal.parameters.get::<(String,)>() else {
                        return;
                    };
                    match id.as_str() {
                        "launcher.show" => launcher.toggle(),
                        "screenshot.toolbar" => screenshot_toolbar.toggle(),
                        "launcher.dock-autohide" => {
                            if let Some(settings) = &settings {
                                let next = match settings.string(KEYS[1]).as_str() {
                                    "always" => "maximized",
                                    _ => "always",
                                };
                                if let Err(err) = settings.set_string(KEYS[1], next) {
                                    eprintln!("bunt-shell: {}: {err}", KEYS[1]);
                                }
                            }
                        }
                        _ => {}
                    }
                },
            );
            // 셸이 끝날 때까지 받는다
            std::mem::forget(subscription);
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
