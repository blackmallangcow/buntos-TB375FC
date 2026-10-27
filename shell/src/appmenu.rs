// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴바의 앱 메뉴: 앞에 있는 앱의 이름과 파일·창·도움말 메뉴.
//! GNOME 앱 같은 GApplication은 창 안의 메뉴를 밖으로 내보내지 않지만, 동작(정보, 설정, 새 창,
//! 종료, 도움말)은 D-Bus로 내보낸다. 그 동작으로 메뉴를 만들고, 창 조작은 창 목록 프로토콜로 한다.

use std::{cell::RefCell, rc::Rc};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, prelude::*};

use crate::toplevels::{Toplevel, Toplevels};

/// 메뉴 모델이 앱의 원격 동작과 셸의 창 동작을 가리킬 때 쓰는 이름 앞부분
const APP_ACTIONS: &str = "focused";
const WINDOW_ACTIONS: &str = "toplevel";

struct AppMenuState {
    tr: Rc<Translations>,
    toplevels: Toplevels,
    menubar: gtk::PopoverMenuBar,
    app_id: RefCell<Option<String>>,
    remote: RefCell<Option<gio::DBusActionGroup>>,
}

pub fn build(tr: &Rc<Translations>, toplevels: &Toplevels) -> gtk::PopoverMenuBar {
    let menubar = gtk::PopoverMenuBar::from_model(None::<&gio::MenuModel>);
    menubar.add_css_class("app-menus");
    menubar.insert_action_group(WINDOW_ACTIONS, Some(&window_actions(toplevels)));

    let state = Rc::new(AppMenuState {
        tr: tr.clone(),
        toplevels: toplevels.clone(),
        menubar: menubar.clone(),
        app_id: RefCell::new(None),
        remote: RefCell::new(None),
    });
    toplevels.subscribe(move |toplevels| update(&state, toplevels.active()));
    menubar
}

/// 앞에 있는 창을 다루는 동작
fn window_actions(toplevels: &Toplevels) -> gio::SimpleActionGroup {
    let group = gio::SimpleActionGroup::new();
    let add = |name: &str, run: fn(&Toplevels, &Toplevel)| {
        let action = gio::SimpleAction::new(name, None);
        let toplevels = toplevels.clone();
        action.connect_activate(move |_, _| {
            if let Some(active) = toplevels.active() {
                run(&toplevels, &active);
            }
        });
        group.add_action(&action);
    };
    add("close", |toplevels, active| toplevels.close(active));
    add("quit-app", |toplevels, active| {
        toplevels.close_app(&active.app_id)
    });
    add("zoom", |toplevels, active| {
        toplevels.set_maximized(active, !active.maximized)
    });
    add("fullscreen", |toplevels, active| {
        toplevels.set_fullscreen(active, !active.fullscreen)
    });
    group
}

fn update(state: &Rc<AppMenuState>, active: Option<Toplevel>) {
    let app_id = active.as_ref().map(|toplevel| toplevel.app_id.clone());
    if *state.app_id.borrow() != app_id {
        let remote = app_id.as_deref().and_then(remote_actions);
        state
            .menubar
            .insert_action_group(APP_ACTIONS, remote.as_ref());
        if let Some(remote) = &remote {
            // 원격 동작 목록은 나중에 도착하므로 도착할 때마다 메뉴를 다시 만든다
            let weak = Rc::downgrade(state);
            let rebuild = move || {
                if let Some(state) = weak.upgrade() {
                    rebuild(&state, state.toplevels.active());
                }
            };
            let added = rebuild.clone();
            remote.connect_action_added(None, move |_, _| added());
            remote.connect_action_removed(None, move |_, _| rebuild());
            remote.list_actions();
        }
        *state.app_id.borrow_mut() = app_id;
        *state.remote.borrow_mut() = remote;
    }
    rebuild(state, active);
}

/// GApplication은 앱 ID를 버스 이름으로 쓰고, 동작을 앱 ID에서 만든 경로에 내보낸다
fn remote_actions(app_id: &str) -> Option<gio::DBusActionGroup> {
    if !gio::Application::id_is_valid(app_id) {
        return None;
    }
    let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok()?;
    Some(gio::DBusActionGroup::get(
        &connection,
        Some(app_id),
        &object_path(app_id),
    ))
}

/// g_application_get_dbus_object_path와 같은 변환: 점은 /로, -는 _로
fn object_path(app_id: &str) -> String {
    format!("/{}", app_id.replace('.', "/").replace('-', "_"))
}

/// .desktop 파일의 (지역화된) 이름. 없으면 앱 ID를 쓴다
pub fn app_name(app_id: &str) -> String {
    gio_unix::DesktopAppInfo::new(&format!("{app_id}.desktop"))
        .map(|info| info.name().to_string())
        .unwrap_or_else(|| app_id.to_owned())
}

fn rebuild(state: &AppMenuState, active: Option<Toplevel>) {
    let model = active.map(|active| {
        let remote = state.remote.borrow();
        let has = |action: &str| remote.as_ref().is_some_and(|r| r.has_action(action));
        menu_model(&state.tr, &active, &app_name(&active.app_id), &has)
    });
    state.menubar.set_menu_model(model.as_ref());
}

fn menu_model(
    tr: &Translations,
    active: &Toplevel,
    name: &str,
    has: &dyn Fn(&str) -> bool,
) -> gio::Menu {
    let mut args = FluentArgs::new();
    args.set("app", name);
    let item = |label: String, action: &str| gio::MenuItem::new(Some(&label), Some(action));
    let section = |items: &[gio::MenuItem]| {
        let section = gio::Menu::new();
        items.iter().for_each(|item| section.append_item(item));
        section
    };
    let menubar = gio::Menu::new();

    let app = gio::Menu::new();
    if has("about") {
        app.append_section(
            None,
            &section(&[item(tr.get_with("app-menu-about", &args), "focused.about")]),
        );
    }
    if has("preferences") {
        app.append_section(
            None,
            &section(&[item(tr.get("app-menu-preferences"), "focused.preferences")]),
        );
    }
    // 종료 동작을 내보내지 않는 앱은 창을 모두 닫아 끝낸다
    let quit = if has("quit") {
        "focused.quit"
    } else {
        "toplevel.quit-app"
    };
    app.append_section(
        None,
        &section(&[item(tr.get_with("app-menu-quit", &args), quit)]),
    );
    menubar.append_submenu(Some(name), &app);

    let file = gio::Menu::new();
    if has("new-window") {
        file.append_item(&item(tr.get("app-menu-new-window"), "focused.new-window"));
    }
    file.append_item(&item(tr.get("app-menu-close"), "toplevel.close"));
    menubar.append_submenu(Some(&tr.get("app-menu-file")), &file);

    let window = gio::Menu::new();
    window.append_item(&item(tr.get("app-menu-zoom"), "toplevel.zoom"));
    let fullscreen = if active.fullscreen {
        "app-menu-fullscreen-leave"
    } else {
        "app-menu-fullscreen-enter"
    };
    window.append_item(&item(tr.get(fullscreen), "toplevel.fullscreen"));
    menubar.append_submenu(Some(&tr.get("app-menu-window")), &window);

    if has("help") {
        let help = gio::Menu::new();
        help.append_item(&item(
            tr.get_with("app-menu-help-app", &args),
            "focused.help",
        ));
        menubar.append_submenu(Some(&tr.get("app-menu-help")), &help);
    }
    menubar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_path_follows_gapplication_rules() {
        assert_eq!(object_path("org.gnome.TextEditor"), "/org/gnome/TextEditor");
        assert_eq!(object_path("org.example.my-app"), "/org/example/my_app");
    }

    #[test]
    fn apps_without_an_application_id_have_no_remote_actions() {
        // foot처럼 GApplication이 아닌 앱의 app_id는 D-Bus 이름이 될 수 없다
        assert!(remote_actions("foot").is_none());
    }
}
