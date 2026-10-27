// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! D-Bus 도우미. 시스템 서비스(NetworkManager, BlueZ, UPower 등)와 세션 서비스(컴포지터, fcitx5)의
//! 메서드를 부르고, 속성을 읽고 쓰고, 신호를 받는다

use std::{cell::Cell, rc::Rc, time::Duration};

use gtk4::{gio, glib, prelude::*};

pub const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
pub const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

/// 사용자에게 보일 오류 문장. "GDBus.Error:org.bluez.Error.Failed: " 같은 앞부분을 뗀다
pub fn message(error: &glib::Error) -> String {
    let text = error.message();
    match text.strip_prefix("GDBus.Error:") {
        Some(rest) => rest
            .split_once(": ")
            .map_or(rest, |(_, message)| message)
            .to_owned(),
        None => text.to_owned(),
    }
}

pub async fn system() -> Result<gio::DBusConnection, glib::Error> {
    gio::bus_get_future(gio::BusType::System).await
}

pub async fn session() -> Result<gio::DBusConnection, glib::Error> {
    gio::bus_get_future(gio::BusType::Session).await
}

/// 대상 객체 하나: 버스, 이름, 경로
#[derive(Clone, Debug)]
pub struct Remote {
    pub bus: gio::DBusConnection,
    pub name: &'static str,
    pub path: String,
}

impl Remote {
    pub fn new(bus: &gio::DBusConnection, name: &'static str, path: impl Into<String>) -> Self {
        Self {
            bus: bus.clone(),
            name,
            path: path.into(),
        }
    }

    /// 같은 서비스의 다른 객체
    pub fn at(&self, path: impl Into<String>) -> Self {
        Self::new(&self.bus, self.name, path)
    }

    pub async fn call(
        &self,
        interface: &str,
        method: &str,
        args: Option<glib::Variant>,
    ) -> Result<glib::Variant, glib::Error> {
        self.call_with_timeout(interface, method, args, -1).await
    }

    /// 오래 걸리는 요청(Bluetooth 페어링 등)은 시간 제한을 늘린다. 밀리초, -1이면 기본값(25초)
    pub async fn call_with_timeout(
        &self,
        interface: &str,
        method: &str,
        args: Option<glib::Variant>,
        timeout_ms: i32,
    ) -> Result<glib::Variant, glib::Error> {
        self.bus
            .call_future(
                Some(self.name),
                &self.path,
                interface,
                method,
                args.as_ref(),
                None,
                gio::DBusCallFlags::NONE,
                timeout_ms,
            )
            .await
    }

    pub async fn get_all(&self, interface: &str) -> Result<Props, glib::Error> {
        let reply = self
            .call(PROPERTIES, "GetAll", Some((interface,).to_variant()))
            .await?;
        Ok(Props::new(reply.child_value(0)))
    }

    pub async fn get(&self, interface: &str, property: &str) -> Result<glib::Variant, glib::Error> {
        let reply = self
            .call(PROPERTIES, "Get", Some((interface, property).to_variant()))
            .await?;
        // 답은 (v) 형식이다
        reply.child_value(0).as_variant().ok_or_else(|| {
            glib::Error::new(gio::IOErrorEnum::InvalidData, "unexpected D-Bus reply")
        })
    }

    pub async fn set(
        &self,
        interface: &str,
        property: &str,
        value: glib::Variant,
    ) -> Result<(), glib::Error> {
        self.call(
            PROPERTIES,
            "Set",
            Some((interface, property, value).to_variant()),
        )
        .await
        .map(|_| ())
    }
}

/// 문자열(또는 객체 경로) 열쇠 사전(a{s*}, a{o*})에서 값을 찾는다. 값이 v면 벗긴다
pub fn lookup(dict: &glib::Variant, key: &str) -> Option<glib::Variant> {
    dict.iter().find_map(|entry| {
        (entry.child_value(0).str() == Some(key)).then(|| {
            let value = entry.child_value(1);
            // as_variant는 v가 아닌 값에 GLib 경고를 남기므로 형식을 먼저 본다
            if value.is_type(glib::VariantTy::VARIANT) {
                value.as_variant().unwrap_or(value)
            } else {
                value
            }
        })
    })
}

/// 속성 사전(a{sv})
#[derive(Clone, Debug)]
pub struct Props(glib::Variant);

impl Props {
    pub fn new(dict: glib::Variant) -> Self {
        Self(dict)
    }

    pub fn value(&self, key: &str) -> Option<glib::Variant> {
        lookup(&self.0, key)
    }

    pub fn get<T: glib::variant::FromVariant>(&self, key: &str) -> Option<T> {
        self.value(key)?.get()
    }

    pub fn string(&self, key: &str) -> String {
        self.value(key)
            .and_then(|value| value.str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub fn bool(&self, key: &str) -> bool {
        self.get(key).unwrap_or(false)
    }

    pub fn u32(&self, key: &str) -> u32 {
        self.get(key).unwrap_or(0)
    }

    /// 객체 경로. "/"는 없음으로 본다
    pub fn path(&self, key: &str) -> Option<String> {
        let value = self.value(key)?;
        let path = value.str()?;
        (path != "/").then(|| path.to_owned())
    }

    pub fn bytes(&self, key: &str) -> Vec<u8> {
        self.get::<Vec<u8>>(key).unwrap_or_default()
    }
}

/// 객체 경로 배열(ao)
pub fn paths(value: &glib::Variant) -> Vec<String> {
    value
        .iter()
        .filter_map(|path| path.str().map(str::to_owned))
        .collect()
}

pub fn object_path(path: &str) -> glib::Variant {
    glib::variant::ObjectPath::try_from(path)
        .unwrap_or_else(|_| glib::variant::ObjectPath::try_from("/").expect("root object path"))
        .to_variant()
}

/// 서비스(sender)가 보내는 신호를 받는다. 돌려준 값을 놓으면 받지 않는다
pub fn subscribe(
    bus: &gio::DBusConnection,
    sender: Option<&str>,
    interface: Option<&str>,
    member: Option<&str>,
    path: Option<&str>,
    callback: impl Fn(gio::DBusSignalRef) + 'static,
) -> gio::SignalSubscription {
    bus.subscribe_to_signal(
        sender,
        interface,
        member,
        path,
        None,
        gio::DBusSignalFlags::NONE,
        callback,
    )
}

/// 신호가 몰려 와도 잠시 뒤 한 번만 다시 읽게 한다
#[derive(Clone, Default)]
pub struct Debounce(Rc<Cell<bool>>);

impl Debounce {
    pub fn run(&self, delay: Duration, action: impl FnOnce() + 'static) {
        if self.0.replace(true) {
            return;
        }
        let pending = self.0.clone();
        glib::timeout_add_local_once(delay, move || {
            pending.set(false);
            action();
        });
    }
}
