// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dock의 최소화 칸에 놓을 창. 컴포지터(org.buntos.Compositor1)가 최소화한 창의 번호, 앱, 제목과
//! 내리기 전에 뜬 창 그림을 준다(GetMinimizedWindows). 창을 최소화하거나, 최소화한 창이 돌아오거나
//! 닫히면 알린다(MinimizedWindowsChanged). Dock에서 누른 창은 컴포지터가 되돌린다(RestoreMinimized).
//! 최소화 효과(지니, 크기 조절)는 컴포지터가 그린다. 셸은 고른 효과(SetMinimizeEffect)와 Dock의 칸 자리
//! (SetMinimizedSlots)를 알린다.

use std::rc::Rc;

use gtk4::{gdk, gio, glib, prelude::*};

use crate::dbus;

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
/// 셸 설정 키 (data/org.buntos.shell.gschema.xml)
const EFFECT_KEY: &str = "minimize-effect";

/// Dock의 최소화 칸 하나: 최소화 번호와 Dock 표면 안의 (x, y, 너비, 높이) 논리 좌표
pub type Slot = (u32, i32, i32, i32, i32);

/// 최소화한 창 하나
pub struct Minimized {
    pub id: u32,
    pub app_id: String,
    pub title: String,
    /// 내리기 전의 창 그림. 컴포지터가 그리지 못했으면 없다
    pub thumbnail: Option<gdk::Texture>,
}

/// 최소화한 창 목록(먼저 내린 창이 앞)을 처음 한 번, 그 뒤로는 바뀔 때마다 `changed`에 준다
pub fn follow(changed: impl Fn(Vec<Minimized>) + 'static) {
    let changed: Rc<dyn Fn(Vec<Minimized>)> = Rc::new(changed);
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
            let (bus, on_change) = (connection.clone(), changed.clone());
            let subscription = connection.subscribe_to_signal(
                Some(NAME),
                Some(INTERFACE),
                Some("MinimizedWindowsChanged"),
                Some(PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |_| fetch(&bus, on_change.clone()),
            );
            // 셸이 끝날 때까지 받는다
            std::mem::forget(subscription);
            fetch(&connection, changed);
        },
    );
}

/// 컴포지터에 시스템 설정의 최소화 효과를 알린다. 컴포지터가 (다시) 뜰 때와 값이 바뀔 때마다 보낸다
pub fn share_effect(settings: &gio::Settings) {
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
            // 처리기가 프록시와 설정 객체를 붙들어 셸이 끝날 때까지 둔다
            let changed = proxy.clone();
            settings.connect_changed(Some(EFFECT_KEY), move |settings, _| {
                send_effect(&changed, settings)
            });
            let keep_alive = settings.clone();
            proxy.connect_notify_local(Some("g-name-owner"), move |proxy, _| {
                send_effect(proxy, &keep_alive);
            });
            // 변경 알림은 처리기를 단 뒤 한 번 읽어야 온다
            send_effect(&proxy, &settings);
        },
    );
}

fn send_effect(proxy: &gio::DBusProxy, settings: &gio::Settings) {
    let args = (settings.string(EFFECT_KEY).to_string(),).to_variant();
    if dbus::is_running(proxy) {
        dbus::call(proxy, "SetMinimizeEffect", Some(&args));
    }
}

/// Dock의 최소화 칸 자리를 컴포지터에 알린다. 최소화 효과가 창을 이 칸으로 보내고 이 칸에서 꺼낸다
pub fn report_slots(slots: &[Slot]) {
    let args = (slots.to_vec(),).to_variant();
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let Ok(connection) = result else {
                return;
            };
            connection.call(
                Some(NAME),
                PATH,
                INTERFACE,
                "SetMinimizedSlots",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                |result| {
                    if let Err(err) = result {
                        eprintln!("bunt-shell: {INTERFACE}.SetMinimizedSlots: {err}");
                    }
                },
            );
        },
    );
}

/// 최소화한 창을 제자리에 놓고 포커스를 준다
pub fn restore(id: u32) {
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let Ok(connection) = result else {
                return;
            };
            connection.call(
                Some(NAME),
                PATH,
                INTERFACE,
                "RestoreMinimized",
                Some(&(id,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                |result| {
                    if let Err(err) = result {
                        eprintln!("bunt-shell: {INTERFACE}.RestoreMinimized: {err}");
                    }
                },
            );
        },
    );
}

fn fetch(connection: &gio::DBusConnection, changed: Rc<dyn Fn(Vec<Minimized>)>) {
    connection.call(
        Some(NAME),
        PATH,
        INTERFACE,
        "GetMinimizedWindows",
        None,
        glib::VariantTy::new("(a(ussuuay))").ok(),
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(reply) => changed(parse(&reply.child_value(0))),
            Err(err) => eprintln!("bunt-shell: {INTERFACE}.GetMinimizedWindows: {err}"),
        },
    );
}

/// GetMinimizedWindows의 답(a(ussuuay): 번호, 앱 ID, 제목, 그림 너비, 높이, 픽셀)을 읽는다
fn parse(entries: &glib::Variant) -> Vec<Minimized> {
    entries
        .iter()
        .filter_map(|entry| {
            let text = |index| entry.child_value(index).get::<String>().unwrap_or_default();
            Some(Minimized {
                id: entry.child_value(0).get::<u32>()?,
                app_id: text(1),
                title: text(2),
                thumbnail: thumbnail(
                    entry.child_value(3).get::<u32>()?,
                    entry.child_value(4).get::<u32>()?,
                    &entry.child_value(5),
                ),
            })
        })
        .collect()
}

/// 창 그림: 알파를 미리 곱한 RGBA, 위 줄부터, 줄 너비는 width × 4. 크기가 0이면 그림이 없다
fn thumbnail(width: u32, height: u32, pixels: &glib::Variant) -> Option<gdk::Texture> {
    let bytes = pixels.data_as_bytes();
    let stride = width as usize * 4;
    if width == 0 || height == 0 || bytes.len() < stride * height as usize {
        return None;
    }
    let texture = gdk::MemoryTexture::new(
        width as i32,
        height as i32,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &bytes,
        stride,
    );
    Some(texture.upcast())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u32, title: &str, width: u32, height: u32, pixels: &[u8]) -> glib::Variant {
        // 픽셀은 ay다 (Variant를 넣으면 v가 된다)
        (
            id,
            "org.gnome.TextEditor",
            title,
            width,
            height,
            pixels.to_vec(),
        )
            .to_variant()
    }

    #[test]
    fn windows_come_in_order_and_pictures_are_optional() {
        let entries = glib::Variant::array_from_iter_with_type(
            glib::VariantTy::new("(ussuuay)").unwrap(),
            [
                entry(7, "메모", 0, 0, &[]),
                entry(9, "그림 있음", 1, 1, &[255; 4]),
            ],
        );
        let windows = parse(&entries);
        assert_eq!(windows.len(), 2);
        assert_eq!(
            (
                windows[0].id,
                windows[0].app_id.as_str(),
                windows[0].title.as_str()
            ),
            (7, "org.gnome.TextEditor", "메모")
        );
        assert!(windows[0].thumbnail.is_none());
        assert_eq!(windows[1].id, 9);
        assert!(windows[1].thumbnail.is_some());
    }

    #[test]
    fn slots_go_out_as_the_compositor_reads_them() {
        let slots: Vec<Slot> = vec![(3, 700, 20, 48, 48)];
        let args = (slots,).to_variant();
        assert_eq!(args.type_().as_str(), "(a(uiiii))");
    }

    #[test]
    fn the_effect_key_is_in_the_settings_schema() {
        let schema = include_str!("../data/org.buntos.shell.gschema.xml");
        assert!(schema.contains(&format!("<key name=\"{EFFECT_KEY}\"")));
        for value in ["genie", "scale"] {
            assert!(
                schema.contains(&format!("<choice value=\"{value}\"/>")),
                "{value}"
            );
        }
    }

    #[test]
    fn short_pixel_data_gives_no_picture() {
        let pixels = glib::Variant::array_from_fixed_array(&[0u8; 12]);
        assert!(thumbnail(2, 2, &pixels).is_none());
        assert!(thumbnail(0, 2, &pixels).is_none());
    }
}
