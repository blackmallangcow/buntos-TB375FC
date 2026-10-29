// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 알림 서버 (freedesktop.org Desktop Notifications 1.2, org.freedesktop.Notifications).
//! 앱과 포털이 보낸 알림을 받아 모아 두고, 배너와 알림 센터(notification_center.rs)가 보여 준다.
//! 앱마다의 알림 설정(알림 허용, 알림 스타일, 소리, 알림 센터에 보기)은 시스템 설정의 알림 페이지가 정하고
//! notification_policy.rs가 읽는다. 앱은 desktop-entry 힌트로 안다.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use gtk4::{gio, glib, prelude::*};

use crate::{
    control_center::do_not_disturb,
    notification_policy::{self, Banners, Policies},
};

const BUS_NAME: &str = "org.freedesktop.Notifications";
const OBJECT_PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";
const INTROSPECTION: &str = r#"<node>
  <interface name="org.freedesktop.Notifications">
    <method name="GetCapabilities">
      <arg name="capabilities" type="as" direction="out"/>
    </method>
    <method name="Notify">
      <arg name="app_name" type="s" direction="in"/>
      <arg name="replaces_id" type="u" direction="in"/>
      <arg name="app_icon" type="s" direction="in"/>
      <arg name="summary" type="s" direction="in"/>
      <arg name="body" type="s" direction="in"/>
      <arg name="actions" type="as" direction="in"/>
      <arg name="hints" type="a{sv}" direction="in"/>
      <arg name="expire_timeout" type="i" direction="in"/>
      <arg name="id" type="u" direction="out"/>
    </method>
    <method name="CloseNotification">
      <arg name="id" type="u" direction="in"/>
    </method>
    <method name="GetServerInformation">
      <arg name="name" type="s" direction="out"/>
      <arg name="vendor" type="s" direction="out"/>
      <arg name="version" type="s" direction="out"/>
      <arg name="spec_version" type="s" direction="out"/>
    </method>
    <signal name="NotificationClosed">
      <arg name="id" type="u"/>
      <arg name="reason" type="u"/>
    </signal>
    <signal name="ActionInvoked">
      <arg name="id" type="u"/>
      <arg name="action_key" type="s"/>
    </signal>
  </interface>
</node>"#;

/// 알림을 누르면 실행되는 동작의 키 (명세의 예약어)
const DEFAULT_ACTION: &str = "default";

/// NotificationClosed의 이유 값
#[derive(Clone, Copy)]
enum CloseReason {
    /// 알림 센터에 두지 않는 알림의 배너가 사라졌다
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
}

#[derive(Clone, Debug)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub icon: Option<gio::Icon>,
    pub summary: String,
    pub body: String,
    /// 알림을 누르면 앱에 알릴지
    pub has_default_action: bool,
    /// 단추로 보일 (키, 이름)
    pub actions: Vec<(String, String)>,
    pub time: glib::DateTime,
    /// 앱의 알림 스타일 (없음, 배너, 알림)
    pub banners: Banners,
    /// 알림 센터에 보이는지. 아니면 배너가 사라질 때 치운다
    pub in_center: bool,
}

pub enum Change {
    Added(u32),
    Removed,
}

/// 받은 알림 목록. 복제해도 같은 목록을 가리킨다
#[derive(Clone)]
pub struct Notifications(Rc<Inner>);

type Listener = Box<dyn Fn(&Notifications, &Change)>;

struct Inner {
    /// 오래된 것부터
    items: RefCell<Vec<Notification>>,
    next_id: Cell<u32>,
    connection: RefCell<Option<gio::DBusConnection>>,
    listeners: RefCell<Vec<Listener>>,
    /// 앱마다의 알림 설정
    policies: Policies,
}

impl Notifications {
    /// 세션 버스에서 알림 서버 이름을 얻고 요청을 받기 시작한다.
    pub fn start() -> Self {
        let notifications = Self(Rc::new(Inner {
            items: RefCell::default(),
            next_id: Cell::new(1),
            connection: RefCell::default(),
            listeners: RefCell::default(),
            policies: Policies::default(),
        }));
        let server = notifications.clone();
        gio::bus_own_name(
            gio::BusType::Session,
            BUS_NAME,
            gio::BusNameOwnerFlags::NONE,
            move |connection, _| server.register(connection),
            |_, _| {},
            |_, name| {
                eprintln!(
                    "bunt-shell: could not own {name}; another notification server is running"
                )
            },
        );
        notifications
    }

    fn register(&self, connection: gio::DBusConnection) {
        let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).expect("valid introspection XML");
        let interface = node.lookup_interface(INTERFACE).expect("interface in XML");
        let server = self.clone();
        let registration = connection
            .register_object(OBJECT_PATH, &interface)
            .method_call(move |_, _, _, _, method, params, invocation| {
                server.handle_call(method, &params, invocation);
            })
            .build();
        if let Err(err) = registration {
            eprintln!("bunt-shell: could not register the notification server: {err}");
            return;
        }
        self.0.connection.replace(Some(connection));
    }

    fn handle_call(
        &self,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        match method {
            "GetCapabilities" => {
                let capabilities = vec!["actions", "body", "persistence"];
                invocation.return_value(Some(&(capabilities,).to_variant()));
            }
            "Notify" => match self.notify(params) {
                Some(id) => invocation.return_value(Some(&(id,).to_variant())),
                None => invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs",
                    "unexpected arguments",
                ),
            },
            "CloseNotification" => {
                if let Some((id,)) = params.get::<(u32,)>() {
                    self.close(id, CloseReason::Closed);
                }
                invocation.return_value(None);
            }
            "GetServerInformation" => {
                let info = ("bunt-shell", "buntOS", env!("CARGO_PKG_VERSION"), "1.2");
                invocation.return_value(Some(&info.to_variant()));
            }
            _ => invocation
                .return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method"),
        }
    }

    fn notify(&self, params: &glib::Variant) -> Option<u32> {
        type Args = (
            String,
            u32,
            String,
            String,
            String,
            Vec<String>,
            HashMap<String, glib::Variant>,
            i32,
        );
        let (app_name, replaces_id, app_icon, summary, body, actions, hints, _timeout): Args =
            params.get()?;
        let text_hint = |key: &str| hints.get(key).and_then(|value| value.get::<String>());
        // 데스크톱 ID. 명세는 .desktop을 뗀 이름이지만 붙여 보내는 앱도 받는다
        let app_id = text_hint("desktop-entry")
            .map(|entry| entry.trim_end_matches(".desktop").to_owned())
            .filter(|entry| !entry.is_empty());
        let desktop_entry = app_id
            .as_deref()
            .and_then(|entry| gio_unix::DesktopAppInfo::new(&format!("{entry}.desktop")));
        let image_path = hints
            .get("image-path")
            .and_then(|value| value.get::<String>());
        let icon = icon_from_name(&app_icon)
            .or_else(|| image_path.as_deref().and_then(icon_from_name))
            .or_else(|| desktop_entry.as_ref().and_then(|info| info.icon()));
        // .desktop 파일이 있으면 그 지역화된 이름을 쓴다
        let app_name = desktop_entry
            .as_ref()
            .map(|info| info.name().to_string())
            .unwrap_or(app_name);
        // 동작은 키와 이름이 번갈아 온다
        let pairs: Vec<(String, String)> = actions
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[key, label]| (key.clone(), label.clone()))
            .collect();

        let replaced = replaces_id != 0
            && self
                .0
                .items
                .borrow()
                .iter()
                .any(|item| item.id == replaces_id);
        let id = if replaced {
            replaces_id
        } else {
            let id = self.0.next_id.get();
            self.0.next_id.set(id.wrapping_add(1).max(1));
            id
        };
        // 시스템 설정의 알림 페이지가 정한 이 앱의 설정. 허용하지 않은 앱의 알림은 받기만 한다
        let policy = self.0.policies.for_app(app_id.as_deref());
        if !policy.enabled {
            return Some(id);
        }
        // 방해 금지 중에는 소리도 내지 않는다
        if policy.sound && !do_not_disturb() {
            let suppress = hints
                .get("suppress-sound")
                .and_then(|value| value.get::<bool>())
                .unwrap_or(false);
            notification_policy::play_sound(
                text_hint("sound-file").as_deref(),
                text_hint("sound-name").as_deref(),
                suppress,
            );
        }
        // 배너도 없고 알림 센터에도 두지 않으면 보일 곳이 없다
        if policy.banners == Banners::None && !policy.in_center {
            return Some(id);
        }
        let notification = Notification {
            id,
            app_name,
            icon,
            summary,
            body,
            has_default_action: pairs.iter().any(|(key, _)| key == DEFAULT_ACTION),
            actions: pairs
                .into_iter()
                .filter(|(key, _)| key != DEFAULT_ACTION)
                .collect(),
            time: glib::DateTime::now_local().ok()?,
            banners: policy.banners,
            in_center: policy.in_center,
        };
        {
            let mut items = self.0.items.borrow_mut();
            items.retain(|item| item.id != id);
            items.push(notification);
        }
        self.emit_change(&Change::Added(id));
        Some(id)
    }

    /// 알림 센터에 보일 알림, 새것부터. 알림 센터에 두지 않는 앱의 알림은 배너로만 보인다
    pub fn items(&self) -> Vec<Notification> {
        self.0
            .items
            .borrow()
            .iter()
            .rev()
            .filter(|item| item.in_center)
            .cloned()
            .collect()
    }

    pub fn get(&self, id: u32) -> Option<Notification> {
        self.0
            .items
            .borrow()
            .iter()
            .find(|item| item.id == id)
            .cloned()
    }

    pub fn subscribe(&self, listener: impl Fn(&Notifications, &Change) + 'static) {
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    /// 사용자가 알림을 눌렀다. 앱에 기본 동작을 알리고 알림을 치운다
    pub fn activate(&self, id: u32) {
        if self.get(id).is_some_and(|item| item.has_default_action) {
            self.invoke(id, DEFAULT_ACTION);
        }
        self.close(id, CloseReason::Dismissed);
    }

    /// 사용자가 알림의 단추를 눌렀다
    pub fn invoke_action(&self, id: u32, key: &str) {
        self.invoke(id, key);
        self.close(id, CloseReason::Dismissed);
    }

    pub fn dismiss(&self, id: u32) {
        self.close(id, CloseReason::Dismissed);
    }

    /// 배너가 사라졌다. 알림 센터에 두지 않는 알림이면 치운다
    pub fn banner_hidden(&self, id: u32) {
        if self.get(id).is_some_and(|item| !item.in_center) {
            self.close(id, CloseReason::Expired);
        }
    }

    pub fn clear(&self) {
        let ids: Vec<u32> = self.0.items.borrow().iter().map(|item| item.id).collect();
        for id in ids {
            self.close(id, CloseReason::Dismissed);
        }
    }

    fn close(&self, id: u32, reason: CloseReason) {
        let removed = {
            let mut items = self.0.items.borrow_mut();
            let before = items.len();
            items.retain(|item| item.id != id);
            items.len() != before
        };
        if removed {
            self.emit("NotificationClosed", &(id, reason as u32).to_variant());
            self.emit_change(&Change::Removed);
        }
    }

    fn invoke(&self, id: u32, key: &str) {
        self.emit("ActionInvoked", &(id, key).to_variant());
    }

    fn emit(&self, signal: &str, args: &glib::Variant) {
        if let Some(connection) = self.0.connection.borrow().as_ref()
            && let Err(err) =
                connection.emit_signal(None, OBJECT_PATH, INTERFACE, signal, Some(args))
        {
            eprintln!("bunt-shell: could not emit {signal}: {err}");
        }
    }

    fn emit_change(&self, change: &Change) {
        for listener in self.0.listeners.borrow().iter() {
            listener(self, change);
        }
    }
}

/// 아이콘 이름이나 파일 경로(file:// URI 포함)를 아이콘으로 바꾼다
fn icon_from_name(name: &str) -> Option<gio::Icon> {
    if name.is_empty() {
        None
    } else if name.starts_with('/') {
        Some(gio::FileIcon::new(&gio::File::for_path(name)).upcast())
    } else if name.starts_with("file://") {
        Some(gio::FileIcon::new(&gio::File::for_uri(name)).upcast())
    } else {
        Some(gio::ThemedIcon::new(name).upcast())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_come_from_names_and_paths() {
        assert!(icon_from_name("").is_none());
        assert!(
            icon_from_name("/usr/share/pixmaps/a.png")
                .is_some_and(|icon| icon.is::<gio::FileIcon>())
        );
        assert!(icon_from_name("mail-unread").is_some_and(|icon| icon.is::<gio::ThemedIcon>()));
    }

    #[test]
    fn introspection_matches_the_interface() {
        let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).unwrap();
        let interface = node.lookup_interface(INTERFACE).unwrap();
        assert!(interface.lookup_method("Notify").is_some());
        assert!(interface.lookup_signal("ActionInvoked").is_some());
    }
}
