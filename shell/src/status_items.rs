// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 앱이 넣는 메뉴 막대 항목 (macOS의 menu bar extra, 리눅스의 StatusNotifierItem).
//! 셸이 StatusNotifierWatcher와 호스트를 맡아, 앱(메신저, 동기화 도구 등)이 등록한 항목을
//! 시스템 항목 왼쪽에 아이콘으로 보인다. 누르면 앱이 com.canonical.dbusmenu로 내보낸 메뉴가
//! 열리고, 메뉴가 없는 항목은 앱에 Activate를 보낸다. 사용자는 시스템 설정의 메뉴 막대
//! 페이지에서 앱 항목을 모두 숨길 수 있다(show-status-items).

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

use crate::extras::Visibility;

const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const DEFAULT_ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_INTERFACE: &str = "com.canonical.dbusmenu";
/// 셸이 따로 그리는 항목 (fcitx5 입력기는 입력 소스 항목이 맡는다)
const HIDDEN_IDS: &[&str] = &["Fcitx"];
const ICON_SIZE: i32 = 16;

const WATCHER_XML: &str = r#"
<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem">
      <arg name="service" type="s" direction="in"/>
    </method>
    <method name="RegisterStatusNotifierHost">
      <arg name="service" type="s" direction="in"/>
    </method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
    <signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
    <signal name="StatusNotifierHostRegistered"/>
    <signal name="StatusNotifierHostUnregistered"/>
  </interface>
</node>
"#;

/// 등록된 항목 하나: 앱의 버스 이름과 객체 경로
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Address {
    bus: String,
    path: String,
}

impl Address {
    /// RegisterStatusNotifierItem의 인자는 버스 이름이거나(경로는 기본값) 객체 경로다(버스는 보낸 쪽)
    fn parse(service: &str, sender: &str) -> Self {
        if service.starts_with('/') {
            Self {
                bus: sender.to_owned(),
                path: service.to_owned(),
            }
        } else {
            Self {
                bus: service.to_owned(),
                path: DEFAULT_ITEM_PATH.to_owned(),
            }
        }
    }

    fn id(&self) -> String {
        format!("{}{}", self.bus, self.path)
    }
}

struct Inner {
    container: gtk::Box,
    items: RefCell<HashMap<Address, gtk::Widget>>,
    connection: RefCell<Option<gio::DBusConnection>>,
}

/// 메뉴 막대에 넣는 항목 상자를 만들고 Watcher를 연다
pub fn build() -> gtk::Box {
    let container = gtk::Box::builder().spacing(2).build();
    let visibility = Visibility::new(&container, "show-status-items");
    visibility.set_available(true);
    let inner = Rc::new(Inner {
        container: container.clone(),
        items: RefCell::default(),
        connection: RefCell::default(),
    });
    let (acquired, keep) = (inner.clone(), visibility);
    gio::bus_own_name(
        gio::BusType::Session,
        WATCHER_NAME,
        gio::BusNameOwnerFlags::NONE,
        move |connection, _| {
            let _ = &keep;
            register_watcher(&acquired, &connection);
        },
        |_, _| {},
        |_, name| eprintln!("bunt-shell: could not own {name}; app menu bar items are off"),
    );
    container
}

fn register_watcher(inner: &Rc<Inner>, connection: &gio::DBusConnection) {
    inner.connection.replace(Some(connection.clone()));
    let info = gio::DBusNodeInfo::for_xml(WATCHER_XML)
        .ok()
        .and_then(|node| node.lookup_interface(WATCHER_NAME));
    let Some(info) = info else {
        return;
    };
    let (calls, props) = (Rc::downgrade(inner), Rc::downgrade(inner));
    let registered = connection
        .register_object(WATCHER_PATH, &info)
        .method_call(
            move |connection, sender, _, _, method, params, invocation| {
                let Some(inner) = calls.upgrade() else {
                    return;
                };
                match method {
                    "RegisterStatusNotifierItem" => {
                        let service: String = params.child_value(0).get().unwrap_or_default();
                        let address = Address::parse(&service, sender.unwrap_or_default());
                        invocation.return_value(None);
                        add_item(&inner, &connection, address);
                    }
                    "RegisterStatusNotifierHost" => invocation.return_value(None),
                    _ => invocation.return_dbus_error(
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        "unknown method",
                    ),
                }
            },
        )
        .property(move |_, _, _, _, name| match name {
            "RegisteredStatusNotifierItems" => {
                let ids: Vec<String> = props
                    .upgrade()
                    .map(|inner| inner.items.borrow().keys().map(Address::id).collect())
                    .unwrap_or_default();
                ids.to_variant()
            }
            "ProtocolVersion" => 0i32.to_variant(),
            // IsStatusNotifierHostRegistered: 셸이 호스트다
            _ => true.to_variant(),
        })
        .build();
    if let Err(err) = registered {
        eprintln!("bunt-shell: StatusNotifierWatcher: {err}");
        return;
    }
    // 셸이 호스트이기도 하다고 알린다 (앱은 호스트가 생기면 다시 등록한다)
    emit(connection, "StatusNotifierHostRegistered", None);
}

fn emit(connection: &gio::DBusConnection, signal: &str, args: Option<&glib::Variant>) {
    if let Err(err) = connection.emit_signal(None, WATCHER_PATH, WATCHER_NAME, signal, args) {
        eprintln!("bunt-shell: {signal}: {err}");
    }
}

fn add_item(inner: &Rc<Inner>, connection: &gio::DBusConnection, address: Address) {
    if inner.items.borrow().contains_key(&address) {
        return;
    }
    let weak = Rc::downgrade(inner);
    let connection = connection.clone();
    glib::spawn_future_local(async move {
        let Ok(proxy) = gio::DBusProxy::new_future(
            &connection,
            gio::DBusProxyFlags::NONE,
            None,
            Some(&address.bus),
            &address.path,
            ITEM_INTERFACE,
        )
        .await
        else {
            return;
        };
        let Some(inner) = weak.upgrade() else {
            return;
        };
        let id: String = proxy
            .cached_property("Id")
            .and_then(|value| value.get())
            .unwrap_or_default();
        if HIDDEN_IDS.contains(&id.as_str()) {
            return;
        }
        let widget = item_widget(&proxy, &connection, &address);
        inner.container.append(&widget);
        inner.items.borrow_mut().insert(address.clone(), widget);
        emit(
            &connection,
            "StatusNotifierItemRegistered",
            Some(&(address.id(),).to_variant()),
        );

        // 앱이 끝나면 항목을 치운다
        let weak = Rc::downgrade(&inner);
        let gone = address.clone();
        let watcher = gio::bus_watch_name_on_connection(
            &connection,
            &address.bus,
            gio::BusNameWatcherFlags::NONE,
            |_, _, _| {},
            move |connection, _| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if let Some(widget) = inner.items.borrow_mut().remove(&gone) {
                    inner.container.remove(&widget);
                    emit(
                        &connection,
                        "StatusNotifierItemUnregistered",
                        Some(&(gone.id(),).to_variant()),
                    );
                }
            },
        );
        // 이름 감시는 항목이 사라질 때 함께 멈춘다
        let stop = std::cell::Cell::new(Some(watcher));
        if let Some(widget) = inner.items.borrow().get(&address) {
            widget.connect_destroy(move |_| {
                if let Some(watcher) = stop.take() {
                    gio::bus_unwatch_name(watcher);
                }
            });
        }
    });
}

/// 아이콘: 테마 이름이 있으면 그것(기호 아이콘이 있으면 흰 메뉴 막대에 맞게 그것), 없으면 픽스맵
fn update_icon(image: &gtk::Image, proxy: &gio::DBusProxy) {
    let text = |name: &str| -> String {
        proxy
            .cached_property(name)
            .and_then(|value| value.get())
            .unwrap_or_default()
    };
    let status = text("Status");
    let name = if status == "NeedsAttention" && !text("AttentionIconName").is_empty() {
        text("AttentionIconName")
    } else {
        text("IconName")
    };
    if !name.is_empty() {
        if let Some(display) = gdk::Display::default() {
            let theme = gtk::IconTheme::for_display(&display);
            let path = text("IconThemePath");
            if !path.is_empty()
                && !theme
                    .search_path()
                    .iter()
                    .any(|p| p.to_str() == Some(&path))
            {
                theme.add_search_path(&path);
            }
            let symbolic = format!("{name}-symbolic");
            if theme.has_icon(&symbolic) {
                image.set_icon_name(Some(&symbolic));
                return;
            }
        }
        image.set_icon_name(Some(&name));
        return;
    }
    if let Some(texture) = pixmap(proxy) {
        image.set_paintable(Some(&texture));
    }
}

/// IconPixmap: a(iiay), ARGB32 (네트워크 바이트 순서). 메뉴 막대 크기에 가장 가까운 것을 고른다
fn pixmap(proxy: &gio::DBusProxy) -> Option<gdk::Texture> {
    let pixmaps: Vec<(i32, i32, Vec<u8>)> = proxy.cached_property("IconPixmap")?.get()?;
    let (width, height, argb) = pixmaps
        .into_iter()
        .filter(|(w, h, data)| *w > 0 && *h > 0 && data.len() == (*w * *h * 4) as usize)
        .min_by_key(|(w, _, _)| (w - ICON_SIZE * 2).abs())?;
    let rgba: Vec<u8> = argb
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[a, r, g, b]| [*r, *g, *b, *a])
        .collect();
    Some(
        gdk::MemoryTexture::new(
            width,
            height,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(rgba),
            (width * 4) as usize,
        )
        .upcast(),
    )
}

fn item_widget(
    proxy: &gio::DBusProxy,
    connection: &gio::DBusConnection,
    address: &Address,
) -> gtk::Widget {
    let image = gtk::Image::new();
    image.set_pixel_size(ICON_SIZE);
    update_icon(&image, proxy);
    let title: String = proxy
        .cached_property("Title")
        .and_then(|value| value.get())
        .unwrap_or_default();
    let menu_path: Option<glib::variant::ObjectPath> = proxy
        .cached_property("Menu")
        .and_then(|value| value.get())
        .filter(|path: &glib::variant::ObjectPath| path.as_str() != "/");

    let widget: gtk::Widget = match menu_path {
        Some(menu_path) => {
            let button = gtk::MenuButton::builder()
                .child(&image)
                .tooltip_text(&title)
                .css_classes(["status-item"])
                .build();
            let menu = DbusMenu::new(connection, &address.bus, menu_path.as_str());
            button.insert_action_group("sni", Some(&menu.actions));
            let popover = gtk::PopoverMenu::from_model(Some(&menu.model));
            popover.set_has_arrow(false);
            button.set_popover(Some(&popover));
            // 열기 직전에 앱에 알린다 (앱이 메뉴를 고칠 수 있다)
            let opening = menu.clone();
            popover.connect_show(move |_| opening.about_to_show());
            // 메뉴 객체가 단추와 함께 산다
            let keep = menu;
            button.connect_destroy(move |_| {
                let _ = &keep;
            });
            button.upcast()
        }
        None => {
            let button = gtk::Button::builder()
                .child(&image)
                .tooltip_text(&title)
                .css_classes(["status-item"])
                .build();
            let target = proxy.clone();
            button.connect_clicked(move |_| {
                crate::dbus::call(&target, "Activate", Some(&(0i32, 0i32).to_variant()));
            });
            button.upcast()
        }
    };

    // 아이콘·상태가 바뀌면 다시 그린다. 신호를 받으면 캐시된 속성을 새로 읽는다
    let (shown, target) = (widget.clone(), proxy.clone());
    proxy.connect_local("g-signal", false, move |values| {
        let signal: String = values.get(2)?.get().ok()?;
        let target = target.clone();
        let (shown, image) = (shown.clone(), image.clone());
        if signal.starts_with("New") {
            glib::spawn_future_local(async move {
                for name in [
                    "IconName",
                    "IconPixmap",
                    "AttentionIconName",
                    "Status",
                    "Title",
                ] {
                    if let Ok(reply) = target
                        .connection()
                        .call_future(
                            target.name().as_deref(),
                            &target.object_path(),
                            "org.freedesktop.DBus.Properties",
                            "Get",
                            Some(&(ITEM_INTERFACE, name).to_variant()),
                            None,
                            gio::DBusCallFlags::NONE,
                            -1,
                        )
                        .await
                        && let Some(value) = reply.child_value(0).as_variant()
                    {
                        target.set_cached_property(name, Some(&value));
                    }
                }
                update_icon(&image, &target);
                update_status(&shown, &target);
            });
        }
        None
    });
    update_status(&widget, proxy);
    widget
}

/// Passive 항목은 숨긴다 (명세: 지금 알릴 것이 없는 항목)
fn update_status(widget: &gtk::Widget, proxy: &gio::DBusProxy) {
    let status: String = proxy
        .cached_property("Status")
        .and_then(|value| value.get())
        .unwrap_or_default();
    widget.set_visible(status != "Passive");
}

/// 앱이 com.canonical.dbusmenu로 내보낸 메뉴를 GMenu로 옮긴 것
#[derive(Clone)]
struct DbusMenu(Rc<MenuInner>);

struct MenuInner {
    connection: gio::DBusConnection,
    bus: String,
    path: String,
    model: gio::Menu,
    actions: gio::SimpleActionGroup,
    /// 메뉴가 바뀌었다는 신호 구독. 메뉴와 함께 사라진다
    subscription: RefCell<Option<gio::SignalSubscription>>,
}

impl std::ops::Deref for DbusMenu {
    type Target = MenuInner;
    fn deref(&self) -> &MenuInner {
        &self.0
    }
}

/// dbusmenu 레이아웃의 한 항목: (id, 속성, 자식들)
type Layout = (i32, HashMap<String, glib::Variant>, Vec<glib::Variant>);

/// dbusmenu 항목의 동작 이름 (동작 묶음 "sni" 안)
fn action_name(id: i32) -> String {
    format!("item-{}", id.unsigned_abs())
}

/// GetLayout의 자식 항목들을 model에 넣고, 누를 수 있는 항목마다 (dbusmenu id, 켜짐)을 `items`에
/// 더한다. 구분선이 나오면 새 묶음(section)을 열고, 하위 메뉴는 그 안으로 들어간다
fn fill_model(model: &gio::Menu, children: &[glib::Variant], items: &mut Vec<(i32, bool)>) {
    let mut section = gio::Menu::new();
    for child in children {
        // 자식(av)은 glib-rs가 Vec<Variant>로 읽을 때 상자(v)를 이미 벗겼다(FromVariant for Variant). 한 번 더
        // 벗기면(as_variant) GLib이 항목마다 g_variant_get_variant 경고를 내고 None을 돌려줘, 메뉴가 대체
        // 길로만 채워졌다. 그대로 읽는다
        let Some((id, props, grandchildren)) = child.get::<Layout>() else {
            continue;
        };
        let text = |name: &str| props.get(name).and_then(|value| value.get::<String>());
        let flag = |name: &str, default: bool| {
            props
                .get(name)
                .and_then(|value| value.get::<bool>())
                .unwrap_or(default)
        };
        if !flag("visible", true) {
            continue;
        }
        if text("type").as_deref() == Some("separator") {
            if section.n_items() > 0 {
                model.append_section(None, &section);
                section = gio::Menu::new();
            }
            continue;
        }
        let mut label = plain_label(&text("label").unwrap_or_default());
        let toggled = props
            .get("toggle-state")
            .and_then(|value| value.get::<i32>())
            == Some(1);
        if toggled {
            label = format!("✓ {label}");
        }
        if text("children-display").as_deref() == Some("submenu") {
            let submenu = gio::Menu::new();
            fill_model(&submenu, &grandchildren, items);
            section.append_submenu(Some(&label), &submenu);
            continue;
        }
        items.push((id, flag("enabled", true)));
        section.append(Some(&label), Some(&format!("sni.{}", action_name(id))));
    }
    if section.n_items() > 0 {
        model.append_section(None, &section);
    }
}

/// 메뉴 글자의 니모닉 밑줄을 뺀다 ("_Open" → "Open", "__" → "_")
fn plain_label(label: &str) -> String {
    let mut out = String::new();
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                out.push('_');
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    out
}

impl DbusMenu {
    fn new(connection: &gio::DBusConnection, bus: &str, path: &str) -> Self {
        let menu = Self(Rc::new(MenuInner {
            connection: connection.clone(),
            bus: bus.to_owned(),
            path: path.to_owned(),
            model: gio::Menu::new(),
            actions: gio::SimpleActionGroup::new(),
            subscription: RefCell::default(),
        }));
        menu.reload();
        // 앱이 메뉴를 바꾸면 다시 읽는다
        let weak = Rc::downgrade(&menu.0);
        let subscription = connection.subscribe_to_signal(
            Some(bus),
            Some(MENU_INTERFACE),
            None,
            Some(path),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let changed = matches!(
                    signal.signal_name,
                    "LayoutUpdated" | "ItemsPropertiesUpdated"
                );
                if changed && let Some(inner) = weak.upgrade() {
                    DbusMenu(inner).reload();
                }
            },
        );
        menu.subscription.replace(Some(subscription));
        menu
    }

    fn about_to_show(&self) {
        let menu = self.clone();
        glib::spawn_future_local(async move {
            let reply = menu
                .connection
                .call_future(
                    Some(&menu.bus),
                    &menu.path,
                    MENU_INTERFACE,
                    "AboutToShow",
                    Some(&(0i32,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            let changed = reply
                .ok()
                .and_then(|reply| reply.child_value(0).get::<bool>())
                .unwrap_or(false);
            if changed {
                menu.reload();
            }
        });
    }

    fn reload(&self) {
        let menu = self.clone();
        glib::spawn_future_local(async move {
            let properties: Vec<String> = Vec::new();
            let Ok(reply) = menu
                .connection
                .call_future(
                    Some(&menu.bus),
                    &menu.path,
                    MENU_INTERFACE,
                    "GetLayout",
                    Some(&(0i32, -1i32, properties).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await
            else {
                return;
            };
            let Some(root) = reply.child_value(1).get::<Layout>() else {
                return;
            };
            menu.model.remove_all();
            for action in menu.actions.list_actions() {
                menu.actions.remove_action(&action);
            }
            menu.fill(&root.2);
        });
    }

    /// 자식 항목들을 메뉴 모델에 넣고, 누를 수 있는 항목마다 동작을 단다
    fn fill(&self, children: &[glib::Variant]) {
        let mut items = Vec::new();
        fill_model(&self.model, children, &mut items);
        for (id, enabled) in items {
            let action = gio::SimpleAction::new(&action_name(id), None);
            action.set_enabled(enabled);
            let menu = self.clone();
            action.connect_activate(move |_, _| menu.clicked(id));
            self.actions.add_action(&action);
        }
    }

    fn clicked(&self, id: i32) {
        let timestamp = (glib::monotonic_time() / 1000) as u32;
        let data = glib::Variant::from_variant(&0i32.to_variant());
        self.connection.call(
            Some(&self.bus),
            &self.path,
            MENU_INTERFACE,
            "Event",
            Some(&(id, "clicked", data, timestamp).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            |result| {
                if let Err(err) = result {
                    eprintln!("bunt-shell: menu bar item: {err}");
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_register_by_bus_name_or_path() {
        assert_eq!(
            Address::parse("org.kde.StatusNotifierItem-42-1", ":1.7"),
            Address {
                bus: "org.kde.StatusNotifierItem-42-1".into(),
                path: "/StatusNotifierItem".into()
            }
        );
        assert_eq!(
            Address::parse("/org/ayatana/NotificationItem/app", ":1.9"),
            Address {
                bus: ":1.9".into(),
                path: "/org/ayatana/NotificationItem/app".into()
            }
        );
    }

    #[test]
    fn mnemonics_are_removed() {
        assert_eq!(plain_label("_Open"), "Open");
        assert_eq!(plain_label("Save__As"), "Save_As");
        assert_eq!(plain_label("Quit"), "Quit");
    }

    /// dbusmenu 항목 하나 (ia{sv}av). 속성 값과 자식은 앱이 보내듯 상자(v)에 넣는다
    fn entry(
        id: i32,
        props: &[(&str, glib::Variant)],
        children: &[glib::Variant],
    ) -> glib::Variant {
        let props: HashMap<String, glib::Variant> = props
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect();
        (id, props, children.to_vec()).to_variant()
    }

    /// 메뉴 모델의 묶음마다 항목 (레이블, 동작). 하위 메뉴는 동작 대신 "submenu: <항목 레이블들>"
    fn sections(menu: &gio::Menu) -> Vec<Vec<(String, String)>> {
        let label = |model: &gio::MenuModel, item| {
            model
                .item_attribute_value(item, gio::MENU_ATTRIBUTE_LABEL, None)
                .and_then(|value| value.get::<String>())
                .unwrap_or_default()
        };
        let action = |model: &gio::MenuModel, item| {
            if let Some(submenu) = model.item_link(item, gio::MENU_LINK_SUBMENU) {
                let labels: Vec<String> = (0..submenu.n_items())
                    .filter_map(|section| submenu.item_link(section, gio::MENU_LINK_SECTION))
                    .flat_map(|section| {
                        (0..section.n_items())
                            .map(|item| label(&section, item))
                            .collect::<Vec<_>>()
                    })
                    .collect();
                return format!("submenu: {}", labels.join(", "));
            }
            model
                .item_attribute_value(item, gio::MENU_ATTRIBUTE_ACTION, None)
                .and_then(|value| value.get::<String>())
                .unwrap_or_default()
        };
        (0..menu.n_items())
            .filter_map(|index| menu.item_link(index, gio::MENU_LINK_SECTION))
            .map(|section| {
                (0..section.n_items())
                    .map(|item| (label(&section, item), action(&section, item)))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn app_menus_are_read_without_unboxing_twice() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        // 이미 벗긴 값을 다시 벗기면 GLib이 g_variant_get_variant 경고(critical)를 낸다
        let warnings = Arc::new(AtomicUsize::new(0));
        let counter = warnings.clone();
        let handler = glib::log_set_handler(
            Some("GLib"),
            glib::LogLevels::LEVEL_CRITICAL | glib::LogLevels::LEVEL_WARNING,
            false,
            false,
            move |_, _, message| {
                if message.contains("g_variant_get_variant") {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
        let text = |value: &str| value.to_variant();
        let root = entry(
            0,
            &[("children-display", text("submenu"))],
            &[
                entry(1, &[("label", text("_Open"))], &[]),
                entry(
                    2,
                    &[("label", text("Hidden")), ("visible", false.to_variant())],
                    &[],
                ),
                entry(3, &[("type", text("separator"))], &[]),
                entry(
                    4,
                    &[
                        ("label", text("Sync")),
                        ("toggle-type", text("checkmark")),
                        ("toggle-state", 1i32.to_variant()),
                        ("enabled", false.to_variant()),
                    ],
                    &[],
                ),
                entry(
                    5,
                    &[
                        ("label", text("More")),
                        ("children-display", text("submenu")),
                    ],
                    &[entry(6, &[("label", text("About"))], &[])],
                ),
                entry(7, &[("type", text("separator"))], &[]),
            ],
        );
        // GetLayout의 답 (u(ia{sv}av))을 reload와 같은 길로 읽는다
        let reply = glib::Variant::tuple_from_iter([7u32.to_variant(), root]);
        let layout = reply.child_value(1).get::<Layout>().unwrap();
        let model = gio::Menu::new();
        let mut items = Vec::new();
        fill_model(&model, &layout.2, &mut items);
        glib::log_remove_handler(Some("GLib"), handler);

        assert_eq!(warnings.load(Ordering::SeqCst), 0);
        let owned = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(label, action)| ((*label).to_owned(), (*action).to_owned()))
                .collect()
        };
        // 숨긴 항목은 빠지고, 구분선이 묶음을 나누고, 끝의 구분선은 빈 묶음을 만들지 않는다
        assert_eq!(
            sections(&model),
            [
                owned(&[("Open", "sni.item-1")]),
                owned(&[("✓ Sync", "sni.item-4"), ("More", "submenu: About")]),
            ]
        );
        // 누를 수 있는 항목마다 동작이 하나다. 꺼진 항목은 꺼진 동작이다
        assert_eq!(items, [(1, true), (4, false), (6, true)]);
    }
}
