// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 입력 소스 항목 (macOS의 입력 메뉴). 지금 입력 소스를 "A"(영문), "한"(한글)
//! 같은 글자로 보이고, 누르면 입력 소스 목록에서 고른다. 입력기는 fcitx5이고 D-Bus
//! (org.fcitx.Fcitx.Controller1)로 읽고 바꾼다. 전환 단축키(Ctrl+Space, 한/영)는 fcitx5와
//! 컴포지터가 처리하므로, 여기서는 바뀐 것을 짧은 간격으로 읽어 따라간다.
//! fcitx5가 없거나 입력 소스가 하나뿐이면 항목을 숨긴다 (macOS와 같다).

use std::{cell::RefCell, rc::Rc, time::Duration};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    extras::{self, Listeners, Menu, Visibility},
    menubar::open_settings,
};

const FCITX: &str = "org.fcitx.Fcitx5";
const CONTROLLER_PATH: &str = "/controller";
const CONTROLLER: &str = "org.fcitx.Fcitx.Controller1";
/// 단축키로 바꾼 입력 소스를 따라가는 간격
const POLL_INTERVAL: Duration = Duration::from_millis(400);

/// 입력 소스 하나
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// fcitx5의 입력기 이름 (keyboard-us, hangul)
    pub id: String,
    pub name: String,
    /// fcitx5가 주는 짧은 표시 (없을 수 있다)
    pub label: String,
}

/// 메뉴 막대에 보이는 글자
pub fn badge(source: &Source) -> String {
    match source.id.as_str() {
        "hangul" => "한".into(),
        id if id.starts_with("keyboard-") => "A".into(),
        _ if !source.label.is_empty() => source.label.chars().take(2).collect(),
        _ => source.name.chars().take(1).collect(),
    }
}

#[derive(Clone)]
pub struct InputSources(Rc<Inner>);

struct Inner {
    controller: RefCell<Option<gio::DBusProxy>>,
    sources: RefCell<Vec<Source>>,
    current: RefCell<String>,
    listeners: Listeners<InputSources>,
}

impl InputSources {
    pub fn start() -> Self {
        let sources = Self(Rc::new(Inner {
            controller: RefCell::default(),
            sources: RefCell::default(),
            current: RefCell::default(),
            listeners: RefCell::default(),
        }));
        let weak = Rc::downgrade(&sources.0);
        glib::spawn_future_local(async move {
            // 버스 연결을 먼저 얻는다. gio::bus_watch_name은 세션 버스에 붙지 못하면 사라짐 처리기에
            // 연결 대신 NULL을 주고, gio-rs가 그것을 unwrap해 셸이 죽었다
            let bus = match gio::bus_get_future(gio::BusType::Session).await {
                Ok(bus) => bus,
                Err(err) => {
                    eprintln!("bunt-shell: no session bus, input sources are unavailable: {err}");
                    return;
                }
            };
            let (appeared, vanished) = (weak.clone(), weak);
            crate::dbus::watch_name(
                &bus,
                FCITX,
                move |connection| {
                    if let Some(inner) = appeared.upgrade() {
                        InputSources(inner).open_controller(connection);
                    }
                },
                move || {
                    if let Some(inner) = vanished.upgrade() {
                        inner.controller.replace(None);
                        inner.sources.replace(Vec::new());
                        InputSources(inner).notify();
                    }
                },
            );
        });
        let weak = Rc::downgrade(&sources.0);
        glib::timeout_add_local(POLL_INTERVAL, move || match weak.upgrade() {
            Some(inner) => {
                InputSources(inner).poll();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        sources
    }

    /// fcitx5가 (다시) 뜨면 컨트롤러 프록시를 만들고 입력 소스를 읽는다
    fn open_controller(&self, connection: gio::DBusConnection) {
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let Ok(proxy) = gio::DBusProxy::new_future(
                &connection,
                gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
                None,
                Some(FCITX),
                CONTROLLER_PATH,
                CONTROLLER,
            )
            .await
            else {
                return;
            };
            if let Some(inner) = weak.upgrade() {
                inner.controller.replace(Some(proxy));
                InputSources(inner).reload();
            }
        });
    }

    fn notify(&self) {
        for listener in self.0.listeners.borrow().iter() {
            listener(self);
        }
    }

    pub fn subscribe(&self, listener: impl Fn(&InputSources) + 'static) {
        listener(self);
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    pub fn sources(&self) -> Vec<Source> {
        self.0.sources.borrow().clone()
    }

    pub fn current(&self) -> Option<Source> {
        let current = self.0.current.borrow();
        self.0
            .sources
            .borrow()
            .iter()
            .find(|source| source.id == *current)
            .cloned()
    }

    /// 지금 그룹의 입력 소스와 이름을 다시 읽는다 (시스템 설정에서 목록을 바꿨을 때도)
    fn reload(&self) {
        let Some(proxy) = self.0.controller.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let call = |method: &'static str, args: Option<glib::Variant>| {
                proxy.call_future(method, args.as_ref(), gio::DBusCallFlags::NONE, -1)
            };
            let Ok(group) = call("CurrentInputMethodGroup", None).await else {
                return;
            };
            let group: String = group.child_value(0).get().unwrap_or_default();
            let Ok(info) = call("InputMethodGroupInfo", Some((group,).to_variant())).await else {
                return;
            };
            // (레이아웃, [(입력기, 레이아웃)])
            let items: Vec<(String, String)> = info.child_value(1).get().unwrap_or_default();
            let available: Vec<(String, String, String, String, String, String, bool)> =
                match call("AvailableInputMethods", None).await {
                    Ok(reply) => reply.child_value(0).get().unwrap_or_default(),
                    Err(_) => Vec::new(),
                };
            let sources: Vec<Source> = items
                .into_iter()
                .map(|(id, _)| {
                    let found = available.iter().find(|entry| entry.0 == id);
                    Source {
                        name: found
                            .map(|entry| entry.1.clone())
                            .unwrap_or_else(|| id.clone()),
                        label: found.map(|entry| entry.4.clone()).unwrap_or_default(),
                        id,
                    }
                })
                .collect();
            if let Some(inner) = weak.upgrade() {
                inner.sources.replace(sources);
                InputSources(inner).notify();
            }
        });
    }

    fn poll(&self) {
        let Some(proxy) = self.0.controller.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let Ok(reply) = proxy
                .call_future("CurrentInputMethod", None, gio::DBusCallFlags::NONE, 1000)
                .await
            else {
                return;
            };
            let current: String = reply.child_value(0).get().unwrap_or_default();
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if *inner.current.borrow() == current {
                return;
            }
            let known = inner
                .sources
                .borrow()
                .iter()
                .any(|source| source.id == current);
            inner.current.replace(current);
            let sources = InputSources(inner);
            if known {
                sources.notify();
            } else {
                // 목록에 없는 입력기로 바뀌었으면 시스템 설정에서 목록을 바꾼 것이다
                sources.reload();
            }
        });
    }

    pub fn select(&self, id: &str) {
        if let Some(proxy) = self.0.controller.borrow().as_ref() {
            crate::dbus::call(proxy, "SetCurrentIM", Some(&(id,).to_variant()));
        }
        self.0.current.replace(id.to_owned());
        self.notify();
    }
}

/// 입력 소스 이름: 잘 아는 것은 macOS와 같은 이름을 쓴다
fn display_name(tr: &Translations, source: &Source) -> String {
    match source.id.as_str() {
        "keyboard-us" => tr.get("input-source-abc"),
        "hangul" => tr.get("input-source-hangul"),
        _ => source.name.clone(),
    }
}

pub fn extra(tr: &Rc<Translations>, sources: &InputSources) -> gtk::MenuButton {
    // 테두리를 그리는 글자다. 단추가 터치 모드에서 44px로 높아져도 글자 높이 그대로 가운데에 둔다
    let label = gtk::Label::builder()
        .css_classes(["input-badge"])
        .valign(gtk::Align::Center)
        .build();
    let menu = Menu::new();
    let button = extras::button(&label, &menu, &tr.get("input-source-title"));
    let visibility = Visibility::new(&button, "show-input-source");
    let tooltip_tr = tr.clone();
    sources.subscribe(move |sources| {
        visibility.set_available(sources.sources().len() > 1);
        if let Some(current) = sources.current() {
            label.set_text(&badge(&current));
            label.set_tooltip_text(Some(&display_name(&tooltip_tr, &current)));
        }
    });

    let tr = tr.clone();
    let sources = sources.clone();
    menu.on_show(move |menu| {
        let current = sources.current();
        for source in sources.sources() {
            let chosen = sources.clone();
            let id = source.id.clone();
            menu.check(
                &display_name(&tr, &source),
                current
                    .as_ref()
                    .is_some_and(|current| current.id == source.id),
                move || chosen.select(&id),
            );
        }
        menu.separator();
        menu.text(&tr.get("input-source-shortcut"));
        menu.action(&tr.get("input-source-settings"), || {
            open_settings(Some("keyboard"))
        });
    });
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, label: &str) -> Source {
        Source {
            id: id.into(),
            name: id.into(),
            label: label.into(),
        }
    }

    #[test]
    fn badges_follow_macos() {
        assert_eq!(badge(&source("hangul", "한")), "한");
        assert_eq!(badge(&source("keyboard-us", "en")), "A");
        assert_eq!(badge(&source("mozc", "あ")), "あ");
    }
}
