// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 로그인 화면 (macOS 26 로그인 창): 위에 날짜와 큰 시각, 아래에 사진, 이름, 암호 칸, 맨 아래에
//! 다시 시작·시스템 종료. 계정이 여럿이면 사진을 늘어놓고 고른다. 암호가 틀리면 칸이 흔들린다

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::{Rc, Weak},
    time::Duration,
};

use bunt_greeter::{
    account::User,
    greetd::{self, LoginError},
};
use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{Shared, avatar};

const AVATAR_SIZE: i32 = 96;
/// 계정이 여럿일 때 늘어놓는 사진
const TILE_AVATAR_SIZE: i32 = 72;
/// 흔들기 애니메이션 길이 (data/style.css의 @keyframes shake와 같다)
const SHAKE: Duration = Duration::from_millis(450);
const ACCOUNTS: &str = "org.freedesktop.Accounts";
const ACCOUNTS_PATH: &str = "/org/freedesktop/Accounts";
const ACCOUNTS_USER: &str = "org.freedesktop.Accounts.User";

struct Login {
    shared: Rc<Shared>,
    users: Vec<User>,
    selected: Cell<usize>,
    /// 사용자마다 (사진 틀, 이름 표, 고르는 단추)
    tiles: RefCell<Vec<(gtk::Box, gtk::Label, Option<gtk::Button>)>>,
    /// AccountsService에서 읽은 사용자마다의 (별칭, 사진)
    details: RefCell<Vec<(String, Option<PathBuf>)>>,
    entry: gtk::PasswordEntry,
    field: gtk::Box,
    go: gtk::Button,
    spinner: gtk::Spinner,
    message: gtk::Label,
}

pub fn build(shared: &Rc<Shared>, users: Vec<User>) -> gtk::Widget {
    let tr = &shared.tr;
    let view = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["greeter-login"])
        .build();
    view.append(&clock(tr));
    view.append(&gtk::Box::builder().vexpand(true).build());

    let entry = gtk::PasswordEntry::builder()
        .placeholder_text(tr.get("login-password-placeholder"))
        .show_peek_icon(false)
        .hexpand(true)
        .css_classes(["greeter-password"])
        .build();
    let go = gtk::Button::builder()
        .icon_name("go-next-symbolic")
        .tooltip_text(tr.get("login-submit"))
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .visible(false)
        .css_classes(["greeter-go"])
        .build();
    let spinner = gtk::Spinner::builder()
        .halign(gtk::Align::End)
        .valign(gtk::Align::Center)
        .visible(false)
        .css_classes(["greeter-spinner"])
        .build();
    let field_overlay = gtk::Overlay::builder().child(&entry).build();
    field_overlay.add_overlay(&go);
    field_overlay.add_overlay(&spinner);
    let field = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .width_request(220)
        .css_classes(["greeter-field"])
        .build();
    field.append(&field_overlay);
    // 안내가 나타나도 사진과 암호 칸이 움직이지 않게 빈 글로 자리를 잡아 둔다 (macOS)
    let message = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .max_width_chars(48)
        .css_classes(["greeter-message"])
        .build();

    let count = users.len();
    let login = Rc::new(Login {
        shared: shared.clone(),
        details: RefCell::new(
            users
                .iter()
                .map(|user| (user.display_name().to_owned(), None))
                .collect(),
        ),
        users,
        selected: Cell::new(0),
        tiles: RefCell::default(),
        entry,
        field,
        go,
        spinner,
        message,
    });

    let account = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .css_classes(["greeter-account"])
        .build();
    if count == 0 {
        login.show_message(&tr.get("login-no-accounts"));
        login.field.set_visible(false);
    } else {
        account.append(&login.user_tiles());
    }
    account.append(&login.field);
    account.append(&login.message);
    view.append(&account);
    view.append(&power_buttons(shared));

    login.connect();
    login.load_details();
    let weak = Rc::downgrade(&login);
    view.connect_map(move |_| {
        if let Some(login) = weak.upgrade() {
            login.entry.grab_focus();
        }
    });
    // 화면이 남아 있는 동안 상태를 붙든다
    view.connect_destroy(move |_| {
        let _ = &login;
    });
    view.upcast()
}

impl Login {
    /// 사진과 이름. 계정이 여럿이면 누를 수 있게 늘어놓는다
    fn user_tiles(self: &Rc<Self>) -> gtk::Box {
        let row = gtk::Box::builder()
            .spacing(28)
            .halign(gtk::Align::Center)
            .css_classes(["greeter-users"])
            .build();
        let several = self.users.len() > 1;
        let size = if several {
            TILE_AVATAR_SIZE
        } else {
            AVATAR_SIZE
        };
        for (index, user) in self.users.iter().enumerate() {
            let picture = avatar::avatar(user.display_name(), None, size);
            let name = gtk::Label::builder()
                .label(user.display_name())
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(18)
                .css_classes(["greeter-user-name"])
                .build();
            let tile = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(10)
                .build();
            tile.append(&picture);
            tile.append(&name);
            let button = several.then(|| {
                let button = gtk::Button::builder()
                    .child(&tile)
                    .css_classes(["greeter-user"])
                    .build();
                button.update_property(&[gtk::accessible::Property::Label(user.display_name())]);
                let weak = Rc::downgrade(self);
                button.connect_clicked(move |_| {
                    if let Some(login) = weak.upgrade() {
                        login.select(index);
                    }
                });
                row.append(&button);
                button
            });
            if button.is_none() {
                row.append(&tile);
            }
            self.tiles.borrow_mut().push((picture, name, button));
        }
        self.select(0);
        row
    }

    fn select(&self, index: usize) {
        self.selected.set(index);
        for (position, (_, _, button)) in self.tiles.borrow().iter().enumerate() {
            if let Some(button) = button {
                if position == index {
                    button.add_css_class("selected");
                } else {
                    button.remove_css_class("selected");
                }
            }
        }
        if let Some((name, _)) = self.details.borrow().get(index) {
            let mut args = FluentArgs::new();
            args.set("name", name.clone());
            self.entry
                .update_property(&[gtk::accessible::Property::Label(
                    &self.shared.tr.get_with("login-password-for", &args),
                )]);
        }
        self.entry.set_text("");
        self.hide_message();
        self.entry.grab_focus();
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.entry.connect_activate(move |_| {
            if let Some(login) = weak.upgrade() {
                login.submit();
            }
        });
        let weak = Rc::downgrade(self);
        self.go.connect_clicked(move |_| {
            if let Some(login) = weak.upgrade() {
                login.submit();
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_changed(move |entry| {
            if let Some(login) = weak.upgrade() {
                login
                    .go
                    .set_visible(!entry.text().is_empty() && !login.spinner.is_visible());
            }
        });
        // Esc는 친 암호를 지운다
        let escape = gtk::ShortcutController::new();
        let weak = Rc::downgrade(self);
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(login) = weak.upgrade() {
                    login.entry.set_text("");
                    login.hide_message();
                }
                glib::Propagation::Stop
            })),
        ));
        self.entry.add_controller(escape);
    }

    fn submit(self: &Rc<Self>) {
        let password = self.entry.text().to_string();
        let Some(user) = self.users.get(self.selected.get()) else {
            return;
        };
        if password.is_empty() || self.spinner.is_visible() {
            return;
        }
        self.set_busy(true);
        self.hide_message();
        let user = user.name.clone();
        let weak = Rc::downgrade(self);
        if self.shared.preview {
            // greetd 없이 화면만 볼 때는 늘 틀린 암호로 본다
            glib::timeout_add_local_once(Duration::from_millis(600), move || {
                if let Some(login) = weak.upgrade() {
                    login.failed(LoginError::Auth);
                }
            });
            return;
        }
        let (cmd, env) = crate::session();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let mut client = greetd::Client::from_env()?;
                greetd::login(&mut client, &user, &password, &cmd, &env)
            })
            .await
            .unwrap_or_else(|_| Err(LoginError::Failed("login thread panicked".into())));
            if let Some(login) = weak.upgrade() {
                match result {
                    Ok(()) => login.shared.finish(),
                    Err(err) => login.failed(err),
                }
            }
        });
    }

    fn failed(&self, err: LoginError) {
        self.set_busy(false);
        self.entry.set_text("");
        self.entry.grab_focus();
        match err {
            LoginError::Auth => {
                eprintln!("bunt-greeter: authentication failed");
                self.shake();
                self.show_message(&self.shared.tr.get("login-wrong-password"));
            }
            LoginError::Failed(description) => {
                eprintln!("bunt-greeter: login failed: {description}");
                let mut args = FluentArgs::new();
                args.set("error", description);
                self.show_message(&self.shared.tr.get_with("login-failed", &args));
            }
        }
    }

    fn set_busy(&self, busy: bool) {
        self.entry.set_sensitive(!busy);
        self.spinner.set_visible(busy);
        self.spinner.set_spinning(busy);
        self.go.set_visible(!busy && !self.entry.text().is_empty());
        for (_, _, button) in self.tiles.borrow().iter() {
            if let Some(button) = button {
                button.set_sensitive(!busy);
            }
        }
    }

    fn shake(&self) {
        self.field.remove_css_class("shake");
        self.field.add_css_class("shake");
        let field = self.field.downgrade();
        glib::timeout_add_local_once(SHAKE, move || {
            if let Some(field) = field.upgrade() {
                field.remove_css_class("shake");
            }
        });
    }

    fn show_message(&self, text: &str) {
        self.message.set_text(text);
    }

    fn hide_message(&self) {
        self.message.set_text("");
    }

    /// 별칭과 사진을 AccountsService에서 읽어 바꾼다. 서비스가 없으면 /etc/passwd의 이름을 둔다
    fn load_details(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let names: Vec<String> = self.users.iter().map(|user| user.name.clone()).collect();
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::System).await else {
                return;
            };
            for (index, name) in names.iter().enumerate() {
                let Some((real_name, icon)) = account_details(&bus, name).await else {
                    continue;
                };
                let Some(login) = weak.upgrade() else {
                    return;
                };
                login.update_user(index, real_name, icon);
            }
        });
    }

    fn update_user(&self, index: usize, real_name: String, icon: Option<PathBuf>) {
        let size = if self.users.len() > 1 {
            TILE_AVATAR_SIZE
        } else {
            AVATAR_SIZE
        };
        let fallback = self.users[index].display_name().to_owned();
        let name = if real_name.trim().is_empty() {
            fallback
        } else {
            real_name
        };
        if let Some((picture, label, button)) = self.tiles.borrow().get(index) {
            avatar::set_avatar(picture, &name, icon.as_deref(), size);
            label.set_text(&name);
            if let Some(button) = button {
                button.update_property(&[gtk::accessible::Property::Label(&name)]);
            }
        }
        if let Some(details) = self.details.borrow_mut().get_mut(index) {
            *details = (name, icon);
        }
        if index == self.selected.get() {
            let selected = self.selected.get();
            if let Some((name, _)) = self.details.borrow().get(selected) {
                let mut args = FluentArgs::new();
                args.set("name", name.clone());
                self.entry
                    .update_property(&[gtk::accessible::Property::Label(
                        &self.shared.tr.get_with("login-password-for", &args),
                    )]);
            }
        }
    }
}

/// AccountsService의 (RealName, IconFile)
async fn account_details(
    bus: &gio::DBusConnection,
    name: &str,
) -> Option<(String, Option<PathBuf>)> {
    let reply = bus
        .call_future(
            Some(ACCOUNTS),
            ACCOUNTS_PATH,
            ACCOUNTS,
            "FindUserByName",
            Some(&(name,).to_variant()),
            Some(glib::VariantTy::new("(o)").ok()?),
            gio::DBusCallFlags::NONE,
            5000,
        )
        .await
        .ok()?;
    let path = reply.child_value(0).str()?.to_owned();
    let property = async |property: &str| {
        let reply = bus
            .call_future(
                Some(ACCOUNTS),
                &path,
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&(ACCOUNTS_USER, property).to_variant()),
                Some(glib::VariantTy::new("(v)").ok()?),
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await
            .ok()?;
        reply.child_value(0).as_variant()?.str().map(str::to_owned)
    };
    let real_name = property("RealName").await.unwrap_or_default();
    let icon = property("IconFile")
        .await
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    Some((real_name, icon))
}

/// 날짜와 큰 시각. 분이 바뀔 때마다 고친다
fn clock(tr: &Rc<Translations>) -> gtk::Box {
    let date = gtk::Label::builder().css_classes(["greeter-date"]).build();
    let time = gtk::Label::builder().css_classes(["greeter-time"]).build();
    let clock = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .css_classes(["greeter-clock"])
        .build();
    clock.append(&date);
    clock.append(&time);
    let twelve_hour = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup("org.gnome.desktop.interface", true))
        .is_none_or(|_| {
            gio::Settings::new("org.gnome.desktop.interface").string("clock-format") != "24h"
        });
    let formats = Rc::new((
        tr.get("clock-date-format"),
        tr.get(if twelve_hour {
            "clock-time-format-12h"
        } else {
            "clock-time-format-24h"
        }),
    ));
    tick(date.downgrade(), time.downgrade(), formats);
    clock
}

fn tick(
    date: glib::WeakRef<gtk::Label>,
    time: glib::WeakRef<gtk::Label>,
    formats: Rc<(String, String)>,
) {
    let (Some(date_label), Some(time_label)) = (date.upgrade(), time.upgrade()) else {
        return;
    };
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };
    if let Ok(text) = now.format(&formats.0) {
        date_label.set_text(&text);
    }
    if let Ok(text) = now.format(&formats.1) {
        time_label.set_text(&text);
    }
    let delay = 60u32.saturating_sub(now.second() as u32).max(1);
    glib::timeout_add_seconds_local_once(delay, move || tick(date, time, formats));
}

/// 맨 아래의 다시 시작·시스템 종료 (logind. 로그인 화면은 활성 세션이라 polkit이 허락한다)
fn power_buttons(shared: &Rc<Shared>) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(40)
        .halign(gtk::Align::Center)
        .css_classes(["greeter-power-row"])
        .build();
    for (icon, label, method) in [
        ("system-reboot-symbolic", "power-restart", "Reboot"),
        ("system-shutdown-symbolic", "power-off", "PowerOff"),
    ] {
        let text = shared.tr.get(label);
        let button = gtk::Button::builder()
            .icon_name(icon)
            .halign(gtk::Align::Center)
            .css_classes(["greeter-power"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(&text)]);
        let weak: Weak<Shared> = Rc::downgrade(shared);
        button.connect_clicked(move |_| {
            let Some(shared) = weak.upgrade() else {
                return;
            };
            if shared.preview {
                eprintln!("bunt-greeter: preview: {method}");
                return;
            }
            glib::spawn_future_local(async move {
                let result = async {
                    let bus = gio::bus_get_future(gio::BusType::System).await?;
                    bus.call_future(
                        Some("org.freedesktop.login1"),
                        "/org/freedesktop/login1",
                        "org.freedesktop.login1.Manager",
                        method,
                        Some(&(false,).to_variant()),
                        None,
                        gio::DBusCallFlags::NONE,
                        -1,
                    )
                    .await
                }
                .await;
                if let Err(err) = result {
                    eprintln!("bunt-greeter: {method}: {}", err.message());
                }
            });
        });
        let item = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        item.append(&button);
        item.append(
            &gtk::Label::builder()
                .label(text)
                .css_classes(["greeter-power-label"])
                .build(),
        );
        row.append(&item);
    }
    row
}
