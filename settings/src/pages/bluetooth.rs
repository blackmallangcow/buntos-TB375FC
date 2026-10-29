// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bluetooth: 켜고 끄기, 내 기기(연결·연결 해제·지우기), 주변 기기(찾기·연결). BlueZ D-Bus를 쓴다.
//! 페이지가 보이는 동안만 주변 기기를 찾는다. 이 앱은 페어링 에이전트를 두지 않으므로 Pair는
//! BlueZ의 기본 에이전트인 셸의 페어링 에이전트(Bluetooth 페어링 창)가 확인 번호와 PIN을 묻는다

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
    time::Duration,
};

use bunt_i18n::FluentArgs;
use gtk4::{self as gtk, gio, glib, prelude::*};

use super::Context;
use crate::{
    dbus::{self, Debounce, Props, Remote},
    widgets::{self, BadgeSize, Tint, badge},
};

const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const ICON: &str = "bluetooth-active-symbolic";
/// 페어링은 상대 기기를 기다리므로 오래 걸린다
const PAIR_TIMEOUT_MS: i32 = 60_000;
const REFRESH_DELAY: Duration = Duration::from_millis(300);

#[derive(Clone, Debug)]
struct Adapter {
    path: String,
    powered: bool,
    alias: String,
}

#[derive(Clone, Debug)]
struct Device {
    path: String,
    adapter: String,
    name: String,
    icon: String,
    paired: bool,
    connected: bool,
}

struct BluetoothPage {
    context: Rc<Context>,
    bus: RefCell<Option<gio::DBusConnection>>,
    placeholder: gtk::Box,
    body: gtk::Box,
    switch: gtk::Switch,
    discoverable: gtk::Label,
    mine: gtk::ListBox,
    nearby: gtk::ListBox,
    adapter: RefCell<Option<Adapter>>,
    devices: RefCell<Vec<Device>>,
    /// 연결·페어링 중인 기기 (단추 대신 돌아가는 표시)
    busy: RefCell<HashSet<String>>,
    debounce: Debounce,
    updating: Cell<bool>,
    visible: Cell<bool>,
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let placeholder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .vexpand(true)
        .visible(false)
        .build();
    // 처음 읽기 전에는 아무것도 보이지 않는다 (어댑터가 없는데 스위치가 잠깐 보이지 않게)
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .visible(false)
        .build();
    column.append(&placeholder);
    column.append(&body);

    let switch = gtk::Switch::new();
    let hero = widgets::group(&body, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Blue, BadgeSize::Medium),
        &tr.get("page-bluetooth"),
        &tr.get("bluetooth-description"),
        Some(switch.upcast_ref()),
    ));
    let discoverable = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["settings-description"])
        .build();
    if let Some(section) = hero.parent().and_downcast::<gtk::Box>() {
        section.append(&discoverable);
    }
    let mine = widgets::group(&body, Some(&tr.get("bluetooth-my-devices")), None);
    let nearby = widgets::group(&body, Some(&tr.get("bluetooth-nearby")), None);

    let page = Rc::new(BluetoothPage {
        context: context.clone(),
        bus: RefCell::new(None),
        placeholder,
        body,
        switch: switch.clone(),
        discoverable,
        mine,
        nearby,
        adapter: RefCell::new(None),
        devices: RefCell::default(),
        busy: RefCell::default(),
        debounce: Debounce::default(),
        updating: Cell::new(false),
        visible: Cell::new(false),
        subscriptions: RefCell::default(),
    });

    let weak = Rc::downgrade(&page);
    switch.connect_active_notify(move |switch| {
        let Some(page) = weak.upgrade() else {
            return;
        };
        if !page.updating.get() {
            page.set_powered(switch.is_active());
        }
    });
    let (start, stop) = (Rc::downgrade(&page), Rc::downgrade(&page));
    widgets::while_mapped(
        &scroller,
        move || {
            if let Some(page) = start.upgrade() {
                page.visible.set(true);
                page.discover(true);
            }
        },
        move || {
            if let Some(page) = stop.upgrade() {
                page.visible.set(false);
                page.discover(false);
            }
        },
    );

    glib::spawn_future_local(page.clone().start());
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

/// 커널이 아는 Bluetooth 어댑터(hci 장치)가 있는지
fn has_adapter_hardware() -> bool {
    std::fs::read_dir("/sys/class/bluetooth").is_ok_and(|mut entries| entries.next().is_some())
}

/// BlueZ의 Icon 값(예: audio-headset)을 기호 아이콘 이름으로
fn device_icon(icon: &str) -> String {
    const KNOWN: [&str; 12] = [
        "audio-headphones",
        "audio-headset",
        "audio-card",
        "computer",
        "input-gaming",
        "input-keyboard",
        "input-mouse",
        "input-tablet",
        "phone",
        "printer",
        "video-display",
        "multimedia-player",
    ];
    if KNOWN.contains(&icon) {
        format!("{icon}-symbolic")
    } else {
        ICON.to_owned()
    }
}

impl BluetoothPage {
    fn remote(&self, path: &str) -> Option<Remote> {
        let bus = self.bus.borrow().clone()?;
        Some(Remote::new(&bus, BLUEZ, path))
    }

    async fn start(self: Rc<Self>) {
        // 어댑터가 하나도 없으면 bluetoothd를 깨우지 않고 바로 안내한다 (깨우기를 기다리면 몇십 초 걸린다).
        // 나중에 USB 어댑터를 꽂으면 신호를 받아 다시 그린다
        if !has_adapter_hardware() {
            self.show_unavailable();
        }
        let Ok(bus) = dbus::system().await else {
            self.show_unavailable();
            return;
        };
        let mut subscriptions = Vec::new();
        for (interface, member) in [
            (dbus::OBJECT_MANAGER, "InterfacesAdded"),
            (dbus::OBJECT_MANAGER, "InterfacesRemoved"),
            (dbus::PROPERTIES, "PropertiesChanged"),
        ] {
            let weak = Rc::downgrade(&self);
            subscriptions.push(dbus::subscribe(
                &bus,
                Some(BLUEZ),
                Some(interface),
                Some(member),
                None,
                move |_| {
                    if let Some(page) = weak.upgrade() {
                        page.schedule_refresh();
                    }
                },
            ));
        }
        self.subscriptions.replace(subscriptions);
        self.bus.replace(Some(bus));
        self.refresh().await;
    }

    fn schedule_refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.debounce.run(REFRESH_DELAY, move || {
            if let Some(page) = weak.upgrade() {
                glib::spawn_future_local(async move { page.refresh().await });
            }
        });
    }

    async fn refresh(self: &Rc<Self>) {
        let Some(root) = self.remote("/") else {
            return;
        };
        if !has_adapter_hardware() {
            self.adapter.replace(None);
            self.show_unavailable();
            return;
        }
        let objects = match root
            .call(dbus::OBJECT_MANAGER, "GetManagedObjects", None)
            .await
        {
            Ok(reply) => reply.child_value(0),
            Err(_) => {
                // bluetoothd가 없다
                self.show_unavailable();
                return;
            }
        };
        let mut adapter = None;
        let mut devices = Vec::new();
        for entry in objects.iter() {
            let Some(path) = entry.child_value(0).str().map(str::to_owned) else {
                continue;
            };
            let interfaces = entry.child_value(1);
            if let Some(props) = dbus::lookup(&interfaces, ADAPTER).map(Props::new) {
                if adapter.is_none() {
                    adapter = Some(Adapter {
                        path: path.clone(),
                        powered: props.bool("Powered"),
                        alias: props.string("Alias"),
                    });
                }
            } else if let Some(props) = dbus::lookup(&interfaces, DEVICE).map(Props::new) {
                let name = props.string("Alias");
                let name = if name.is_empty() {
                    props.string("Name")
                } else {
                    name
                };
                devices.push(Device {
                    path,
                    adapter: props.path("Adapter").unwrap_or_default(),
                    name,
                    icon: props.string("Icon"),
                    paired: props.bool("Paired"),
                    connected: props.bool("Connected"),
                });
            }
        }
        let had_adapter = self.adapter.borrow().is_some();
        let was_powered = self
            .adapter
            .borrow()
            .as_ref()
            .is_some_and(|adapter| adapter.powered);
        if let Some(adapter) = &adapter {
            devices.retain(|device| device.adapter == adapter.path);
        }
        devices.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let powered = adapter.as_ref().is_some_and(|adapter| adapter.powered);
        self.adapter.replace(adapter);
        self.devices.replace(devices);
        self.render();
        // 켜지거나 어댑터가 새로 생기면 (보이는 동안) 주변 기기 찾기를 시작한다
        if powered && (!was_powered || !had_adapter) {
            self.discover(self.visible.get());
        }
    }

    fn show_unavailable(&self) {
        let tr = &self.context.tr;
        while let Some(child) = self.placeholder.first_child() {
            self.placeholder.remove(&child);
        }
        self.placeholder.append(&widgets::placeholder(
            "bluetooth-disabled-symbolic",
            Tint::Gray,
            &tr.get("bluetooth-unavailable-title"),
            &tr.get("bluetooth-unavailable-description"),
        ));
        self.placeholder.set_visible(true);
        self.body.set_visible(false);
    }

    fn render(self: &Rc<Self>) {
        let tr = &self.context.tr;
        let Some(adapter) = self.adapter.borrow().clone() else {
            self.show_unavailable();
            return;
        };
        self.placeholder.set_visible(false);
        self.body.set_visible(true);
        self.updating.set(true);
        self.switch.set_active(adapter.powered);
        self.updating.set(false);

        let mut args = FluentArgs::new();
        args.set("name", adapter.alias.clone());
        self.discoverable
            .set_text(&tr.get_with("bluetooth-discoverable", &args));
        self.discoverable.set_visible(adapter.powered);

        let devices = self.devices.borrow().clone();
        let mine: Vec<&Device> = devices.iter().filter(|device| device.paired).collect();
        // 이름이 없는 기기는 macOS처럼 보이지 않는다
        let nearby: Vec<&Device> = devices
            .iter()
            .filter(|device| !device.paired && !device.name.is_empty())
            .collect();
        widgets::set_group_visible(&self.mine, adapter.powered && !mine.is_empty());
        widgets::set_group_visible(&self.nearby, adapter.powered);

        widgets::clear(&self.mine);
        for device in mine {
            self.mine.append(&self.device_row(device));
        }
        widgets::clear(&self.nearby);
        if nearby.is_empty() {
            let spinner = gtk::Spinner::builder().spinning(true).build();
            let (row, _) = widgets::rich_row(
                Some(spinner.upcast_ref()),
                &tr.get("bluetooth-searching"),
                None,
            );
            self.nearby.append(&row);
        }
        for device in nearby {
            self.nearby.append(&self.device_row(device));
        }
    }

    fn device_row(self: &Rc<Self>, device: &Device) -> gtk::ListBoxRow {
        let tr = &self.context.tr;
        let icon = gtk::Image::from_icon_name(&device_icon(&device.icon));
        icon.set_pixel_size(18);
        let circle = gtk::Box::builder()
            .width_request(30)
            .height_request(30)
            .hexpand(false)
            .css_classes(["device-icon"])
            .build();
        icon.set_hexpand(true);
        circle.append(&icon);
        let subtitle = if !device.paired {
            None
        } else if device.connected {
            Some(tr.get("bluetooth-connected"))
        } else {
            Some(tr.get("bluetooth-not-connected"))
        };
        let (row, accessories) =
            widgets::rich_row(Some(circle.upcast_ref()), &device.name, subtitle.as_deref());
        if self.busy.borrow().contains(&device.path) {
            accessories.append(&gtk::Spinner::builder().spinning(true).build());
            return row;
        }
        let (label, connect) = if device.connected {
            ("bluetooth-disconnect", false)
        } else {
            ("bluetooth-connect", true)
        };
        let button = gtk::Button::with_label(&tr.get(label));
        let page = self.clone();
        let path = device.path.clone();
        button.connect_clicked(move |_| {
            if connect {
                page.connect_device(path.clone());
            } else {
                page.disconnect_device(path.clone());
            }
        });
        accessories.append(&button);
        if device.paired {
            let forget = gtk::Button::builder()
                .icon_name("list-remove-symbolic")
                .tooltip_text(tr.get("bluetooth-forget"))
                .css_classes(["circular-button"])
                .build();
            let page = self.clone();
            let path = device.path.clone();
            forget.connect_clicked(move |_| page.forget(path.clone()));
            accessories.append(&forget);
        }
        row
    }

    fn set_powered(self: &Rc<Self>, powered: bool) {
        let Some(path) = self.adapter.borrow().as_ref().map(|a| a.path.clone()) else {
            return;
        };
        let Some(adapter) = self.remote(&path) else {
            return;
        };
        let page = self.clone();
        glib::spawn_future_local(async move {
            if let Err(err) = adapter.set(ADAPTER, "Powered", powered.to_variant()).await {
                page.context
                    .notify_error("bluetooth-power-error", dbus::message(&err));
                page.schedule_refresh();
            }
        });
    }

    /// 주변 기기 찾기를 켜고 끈다. 이미 찾는 중이거나 꺼져 있어 거절되면 무시한다
    fn discover(self: &Rc<Self>, on: bool) {
        let Some(adapter) = self.adapter.borrow().clone() else {
            return;
        };
        if !adapter.powered {
            return;
        }
        let Some(remote) = self.remote(&adapter.path) else {
            return;
        };
        glib::spawn_future_local(async move {
            let method = if on {
                "StartDiscovery"
            } else {
                "StopDiscovery"
            };
            let _ = remote.call(ADAPTER, method, None).await;
        });
    }

    fn set_busy(self: &Rc<Self>, path: &str, busy: bool) {
        if busy {
            self.busy.borrow_mut().insert(path.to_owned());
        } else {
            self.busy.borrow_mut().remove(path);
        }
        self.render();
    }

    /// 페어링이 안 된 기기는 페어링하고 믿는 기기로 둔 뒤 연결한다
    fn connect_device(self: &Rc<Self>, path: String) {
        let Some(remote) = self.remote(&path) else {
            return;
        };
        let paired = self
            .devices
            .borrow()
            .iter()
            .any(|device| device.path == path && device.paired);
        let page = self.clone();
        self.set_busy(&path, true);
        glib::spawn_future_local(async move {
            let result: Result<(), glib::Error> = async {
                if !paired {
                    remote
                        .call_with_timeout(DEVICE, "Pair", None, PAIR_TIMEOUT_MS)
                        .await?;
                    remote.set(DEVICE, "Trusted", true.to_variant()).await?;
                }
                remote
                    .call_with_timeout(DEVICE, "Connect", None, PAIR_TIMEOUT_MS)
                    .await
                    .map(|_| ())
            }
            .await;
            page.set_busy(&path, false);
            // 페어링 창에서 사람이 취소하거나 거부한 것은 알리지 않는다
            if let Err(err) = result
                && !canceled_by_user(&err)
            {
                page.context
                    .notify_error("bluetooth-connect-error", dbus::message(&err));
            }
        });
    }

    fn disconnect_device(self: &Rc<Self>, path: String) {
        let Some(remote) = self.remote(&path) else {
            return;
        };
        let page = self.clone();
        glib::spawn_future_local(async move {
            if let Err(err) = remote.call(DEVICE, "Disconnect", None).await {
                page.context
                    .notify_error("bluetooth-connect-error", dbus::message(&err));
            }
        });
    }

    fn forget(self: &Rc<Self>, path: String) {
        let Some(adapter) = self.adapter.borrow().clone() else {
            return;
        };
        let Some(remote) = self.remote(&adapter.path) else {
            return;
        };
        let page = self.clone();
        glib::spawn_future_local(async move {
            let args = glib::Variant::tuple_from_iter([dbus::object_path(&path)]);
            if let Err(err) = remote.call(ADAPTER, "RemoveDevice", Some(args)).await {
                page.context
                    .notify_error("bluetooth-forget-error", dbus::message(&err));
            }
        });
    }
}

/// 페어링 창(셸의 페어링 에이전트)에서 사람이 취소하거나 거부해 BlueZ가 돌려준 오류
fn canceled_by_user(error: &glib::Error) -> bool {
    gio::DBusError::remote_error(error).is_some_and(|name| {
        matches!(
            name.as_str(),
            "org.bluez.Error.AuthenticationCanceled" | "org.bluez.Error.AuthenticationRejected"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_canceled_in_the_dialog_is_not_an_error() {
        let error = |name| gio::DBusError::new_for_dbus_error(name, "");
        assert!(canceled_by_user(&error(
            "org.bluez.Error.AuthenticationCanceled"
        )));
        assert!(canceled_by_user(&error(
            "org.bluez.Error.AuthenticationRejected"
        )));
        assert!(!canceled_by_user(&error(
            "org.bluez.Error.AuthenticationTimeout"
        )));
        assert!(!canceled_by_user(&error(
            "org.bluez.Error.ConnectionAttemptFailed"
        )));
    }

    #[test]
    fn device_icons_fall_back_to_bluetooth() {
        assert_eq!(device_icon("audio-headset"), "audio-headset-symbolic");
        assert_eq!(device_icon("unknown-thing"), ICON);
        assert_eq!(device_icon(""), ICON);
    }
}
