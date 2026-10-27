// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 네트워크: 연결 방식(서비스)마다 상태를 보이고, 유선(이더넷)은 주소들과 연결·끊기를 보인다.
//! Wi-Fi 행을 누르면 Wi-Fi 페이지로 간다

use std::{cell::RefCell, rc::Rc, time::Duration};

use bunt_i18n::FluentArgs;
use gtk4::{self as gtk, gio, glib, prelude::*};

use super::{Context, Page, wifi::address_rows};
use crate::{
    dbus::{self, Debounce},
    network_manager::{self as nm, Device, Link, Manager},
    widgets::{self, BadgeSize, Status, Tint, badge},
};

const REFRESH_DELAY: Duration = Duration::from_millis(500);
const ICON: &str = "network-workgroup-symbolic";

struct NetworkPage {
    context: Rc<Context>,
    manager: RefCell<Option<Manager>>,
    /// 장치마다 새로 그리는 묶음들
    content: gtk::Box,
    debounce: Debounce,
    subscription: RefCell<Option<gio::SignalSubscription>>,
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let (scroller, column) = widgets::page();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .build();
    column.append(&content);
    let page = Rc::new(NetworkPage {
        context: context.clone(),
        manager: RefCell::new(None),
        content,
        debounce: Debounce::default(),
        subscription: RefCell::new(None),
    });
    glib::spawn_future_local(page.clone().start());
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

fn link_status(tr: &bunt_i18n::Translations, link: Link, wifi: bool) -> (Status, String) {
    match link {
        Link::Connected => (Status::Good, tr.get("net-status-connected")),
        Link::Connecting => (Status::Busy, tr.get("net-status-connecting")),
        Link::Disconnected => (Status::Off, tr.get("net-status-disconnected")),
        Link::Unavailable if wifi => (Status::Off, tr.get("net-status-off")),
        Link::Unavailable => (Status::Off, tr.get("net-status-cable")),
    }
}

impl NetworkPage {
    async fn start(self: Rc<Self>) {
        match Manager::connect().await {
            Ok(manager) => {
                let weak = Rc::downgrade(&self);
                let subscription = manager.subscribe(move || {
                    if let Some(page) = weak.upgrade() {
                        page.schedule_refresh();
                    }
                });
                self.subscription.replace(Some(subscription));
                self.manager.replace(Some(manager));
                self.refresh().await;
            }
            Err(_) => self.show_unavailable(),
        }
    }

    fn schedule_refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.debounce.run(REFRESH_DELAY, move || {
            if let Some(page) = weak.upgrade() {
                glib::spawn_future_local(async move { page.refresh().await });
            }
        });
    }

    fn show_unavailable(&self) {
        let tr = &self.context.tr;
        clear_box(&self.content);
        self.content.append(&widgets::placeholder(
            ICON,
            Tint::Gray,
            &tr.get("network-unavailable-title"),
            &tr.get("network-unavailable-description"),
        ));
    }

    async fn refresh(self: &Rc<Self>) {
        let Some(manager) = self.manager.borrow().clone() else {
            return;
        };
        let Ok(devices) = manager.devices().await else {
            self.show_unavailable();
            return;
        };
        // 주소는 미리 읽어 두고 한 번에 그린다
        let mut details = Vec::new();
        for device in devices
            .iter()
            .filter(|device| device.kind == nm::TYPE_ETHERNET)
        {
            let ip4 = match &device.ip4 {
                Some(path) if device.link() == Link::Connected => {
                    manager.ip4(path).await.unwrap_or_default()
                }
                _ => nm::Ip4::default(),
            };
            details.push((device.clone(), ip4));
        }
        self.render(&devices, &details);
    }

    fn render(self: &Rc<Self>, devices: &[Device], details: &[(Device, nm::Ip4)]) {
        let tr = &self.context.tr;
        clear_box(&self.content);
        if devices.is_empty() {
            self.content.append(&widgets::placeholder(
                ICON,
                Tint::Gray,
                &tr.get("net-no-device-title"),
                &tr.get("net-no-device-description"),
            ));
            return;
        }

        // 서비스 목록 (macOS 네트워크 첫 화면)
        let services = widgets::group(&self.content, None, None);
        for device in devices {
            // 핫스팟으로 쓰는 Wi-Fi 장치는 Wi-Fi 페이지로 가지 않는다
            let wifi = device.kind == nm::TYPE_WIFI && !device.hotspot;
            let (status, text) = link_status(tr, device.link(), wifi);
            let line = widgets::status_line(status, &text);
            let (icon, mut title) = if device.hotspot {
                ("network-wireless-hotspot-symbolic", tr.get("net-hotspot"))
            } else if wifi {
                ("network-wireless-symbolic", tr.get("page-wifi"))
            } else {
                ("network-wired-symbolic", tr.get("net-ethernet"))
            };
            // 같은 종류 장치가 여럿이면 장치 이름을 붙여 가린다
            if devices
                .iter()
                .filter(|other| other.kind == device.kind)
                .count()
                > 1
            {
                title = format!("{title} ({})", device.interface);
            }
            let tint = if device.link() == Link::Connected {
                Tint::Blue
            } else {
                Tint::Gray
            };
            let leading = badge(icon, tint, BadgeSize::Medium);
            let (row, accessories) =
                widgets::row_parts(Some(leading.upcast_ref()), &title, Some(line.upcast_ref()));
            if wifi {
                accessories.append(&widgets::symbol("go-next-symbolic"));
            }
            row.set_activatable(wifi);
            if wifi {
                row.set_widget_name("wifi");
            }
            services.append(&row);
        }
        let context = self.context.clone();
        services.connect_row_activated(move |_, row| {
            if row.widget_name() == "wifi" {
                context.open(Page::Wifi);
            }
        });

        // 유선 장치마다 자세한 묶음
        for (device, ip4) in details {
            let mut args = FluentArgs::new();
            args.set("interface", device.interface.clone());
            let list = widgets::group(
                &self.content,
                Some(&tr.get_with("net-ethernet-named", &args)),
                None,
            );
            let (status, text) = link_status(tr, device.link(), false);
            let line = widgets::status_line(status, &text);
            let (row, accessories) =
                widgets::row_parts(None, &tr.get("net-status"), Some(line.upcast_ref()));
            match device.link() {
                Link::Connected | Link::Connecting => {
                    let button = gtk::Button::with_label(&tr.get("net-disconnect"));
                    let page = self.clone();
                    let path = device.path.clone();
                    button.connect_clicked(move |_| page.disconnect(path.clone()));
                    accessories.append(&button);
                }
                Link::Disconnected => {
                    let button = gtk::Button::with_label(&tr.get("net-connect"));
                    let page = self.clone();
                    let path = device.path.clone();
                    button.connect_clicked(move |_| page.connect(path.clone()));
                    accessories.append(&button);
                }
                Link::Unavailable => {}
            }
            list.append(&row);
            if device.link() == Link::Connected {
                for row in address_rows(tr, ip4) {
                    list.append(&row);
                }
            }
            list.append(&widgets::info_row(
                &tr.get("net-hw-address"),
                &device.hw_address,
            ));
            if device.speed > 0 {
                let mut args = FluentArgs::new();
                args.set("speed", device.speed);
                list.append(&widgets::info_row(
                    &tr.get("net-speed"),
                    &tr.get_with("net-speed-value", &args),
                ));
            }
        }
    }

    fn connect(self: &Rc<Self>, device: String) {
        let page = self.clone();
        glib::spawn_future_local(async move {
            let Some(manager) = page.manager.borrow().clone() else {
                return;
            };
            if let Err(err) = manager.activate(None, &device, None).await {
                page.context
                    .notify_error("net-connect-error", dbus::message(&err));
            }
        });
    }

    fn disconnect(self: &Rc<Self>, device: String) {
        let page = self.clone();
        glib::spawn_future_local(async move {
            let Some(manager) = page.manager.borrow().clone() else {
                return;
            };
            if let Err(err) = manager.disconnect(&device).await {
                page.context
                    .notify_error("net-disconnect-error", dbus::message(&err));
            }
        });
    }
}

fn clear_box(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
