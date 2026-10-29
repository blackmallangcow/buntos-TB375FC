// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 사용자 및 그룹: 이 기기의 계정. macOS의 사용자 및 그룹과 계정 상세처럼 맨 위에 큰 사진(편집)과
//! 이름, 그 아래 별칭, 계정 이름, 계정 종류, 홈 폴더, 로그인 암호, 다른 사용자를 보인다.
//! 이름과 사진은 AccountsService로 바꾼다. 암호는 passwd를 가상 터미널(script)에서 돌려 바꾸므로
//! 시스템 설정은 root 권한을 받지 않는다 (passwd가 지금 암호를 확인한다)

use std::{cell::RefCell, ffi::OsStr, rc::Rc, time::Duration};

use bunt_i18n::FluentArgs;
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

use super::Context;
use crate::{
    widgets::{
        BadgeSize, Sheet, Tint, badge, group, info_row, keep_alive, page, rich_row, row,
        set_group_visible,
    },
    window::account::{self, Account, DEFAULT_PICTURES},
};

const HERO_AVATAR_SIZE: i32 = 96;
const PICKER_PREVIEW_SIZE: i32 = 112;
const PICKER_OPTION_SIZE: i32 = 52;
const PICKER_COLUMNS: i32 = 5;
const OTHER_AVATAR_SIZE: i32 = 28;
/// passwd를 가상 터미널에서 돌리는 util-linux script (bsdutils, Debian 필수 패키지)
const SCRIPT: &str = "script";
/// passwd가 묻는 줄을 쓰고 입력을 받을 준비를 하기까지(터미널 에코를 끄기까지) 기다린다.
/// 너무 일찍 쓰면 passwd가 입력 버퍼를 비우며 답을 버린다
const ANSWER_DELAY: Duration = Duration::from_millis(150);
/// passwd가 이만큼 아무것도 쓰지 않으면 멈춘 것으로 본다 (틀린 암호에는 PAM이 2초쯤 늦게 답한다)
const PASSWD_TIMEOUT: Duration = Duration::from_secs(15);

struct UsersPage {
    context: Rc<Context>,
    account: RefCell<Option<Account>>,
    avatar: gtk::Box,
    avatar_button: gtk::Button,
    name: gtk::Label,
    kind: gtk::Label,
    full_name: gtk::Entry,
    account_name: gtk::Label,
    account_type: gtk::Label,
    home: gtk::Label,
    others: gtk::ListBox,
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = page();

    // 맨 위: 큰 사진(누르면 편집), 이름, 계정 종류
    let fallback = glib::user_name().to_string_lossy().into_owned();
    let avatar = account::avatar(&fallback, None, HERO_AVATAR_SIZE);
    let edit = gtk::Label::builder()
        .label(tr.get("users-edit-picture"))
        .valign(gtk::Align::End)
        .halign(gtk::Align::Fill)
        .css_classes(["avatar-edit"])
        .build();
    let avatar_overlay = gtk::Overlay::builder().child(&avatar).build();
    avatar_overlay.add_overlay(&edit);
    avatar_overlay.set_overflow(gtk::Overflow::Hidden);
    avatar_overlay.add_css_class("avatar-overlay");
    let avatar_button = gtk::Button::builder()
        .child(&avatar_overlay)
        .halign(gtk::Align::Center)
        .tooltip_text(tr.get("users-edit-picture"))
        .sensitive(false)
        .css_classes(["avatar-button"])
        .build();
    avatar_button.update_property(&[gtk::accessible::Property::Label(
        &tr.get("users-edit-picture"),
    )]);
    let name = gtk::Label::builder()
        .label(&fallback)
        .css_classes(["users-name"])
        .build();
    let kind = gtk::Label::builder().css_classes(["users-kind"]).build();
    let hero = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Center)
        .css_classes(["users-hero"])
        .build();
    hero.append(&avatar_button);
    hero.append(&name);
    hero.append(&kind);
    column.append(&hero);

    // 계정
    let details = group(&column, None, None);
    let full_name = gtk::Entry::builder()
        .width_chars(24)
        .sensitive(false)
        .build();
    details.append(&row(&tr.get("users-full-name"), &full_name));
    let (account_row, account_name) = value_row(&tr.get("users-account-name"), &fallback);
    details.append(&account_row);
    let (type_row, account_type) = value_row(&tr.get("users-account-type"), "");
    details.append(&type_row);
    let (home_row, home) = value_row(&tr.get("users-home"), &glib::home_dir().to_string_lossy());
    details.append(&home_row);

    // 로그인 암호
    let password = group(&column, None, None);
    let (password_row, accessories) = rich_row(
        None,
        &tr.get("users-password-row"),
        Some(&tr.get("users-password-row-description")),
    );
    let change = gtk::Button::with_label(&tr.get("users-change-password"));
    accessories.append(&change);
    password.append(&password_row);

    let others = group(&column, Some(&tr.get("users-others")), None);
    set_group_visible(&others, false);

    let page = Rc::new(UsersPage {
        context: context.clone(),
        account: RefCell::default(),
        avatar,
        avatar_button,
        name,
        kind,
        full_name,
        account_name,
        account_type,
        home,
        others,
    });
    page.connect();
    let weak = Rc::downgrade(&page);
    change.connect_clicked(move |_| {
        if let Some(page) = weak.upgrade() {
            page.open_password_sheet();
        }
    });
    page.load();
    page.load_others();
    keep_alive(&scroller, page);
    scroller.upcast()
}

/// 읽기 전용 값 행. 값 글을 돌려주어 나중에 바꿀 수 있게 한다
fn value_row(title: &str, value: &str) -> (gtk::ListBoxRow, gtk::Label) {
    let row = info_row(title, value);
    let label = row
        .child()
        .and_then(|content| content.last_child())
        .and_then(|widget| widget.downcast::<gtk::Label>().ok())
        .expect("info_row ends with its value label");
    (row, label)
}

impl UsersPage {
    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.avatar_button.connect_clicked(move |_| {
            if let Some(page) = weak.upgrade() {
                page.open_picture_sheet();
            }
        });
        // 별칭은 Enter를 누르거나 칸을 떠날 때 바꾼다 (macOS)
        let weak = Rc::downgrade(self);
        self.full_name.connect_activate(move |_| {
            if let Some(page) = weak.upgrade() {
                page.commit_name();
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(page) = weak.upgrade() {
                page.commit_name();
            }
        });
        self.full_name.add_controller(focus);
    }

    /// AccountsService에서 계정을 읽고, 바뀌면 다시 읽는다
    fn load(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let account = Account::current().await;
            let Some(page) = weak.upgrade() else {
                return;
            };
            match account {
                Ok(account) => {
                    page.show(&account);
                    let weak = Rc::downgrade(&page);
                    let subscription = account.watch(move || {
                        let Some(page) = weak.upgrade() else {
                            return;
                        };
                        page.reload();
                    });
                    keep_alive(&page.name, subscription);
                }
                Err(err) => {
                    eprintln!("bunt-settings: account: {}", err.message());
                    page.kind
                        .set_text(&page.context.tr.get("users-unavailable"));
                }
            }
        });
    }

    fn reload(self: &Rc<Self>) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            if let (Ok(account), Some(page)) = (account.reload().await, weak.upgrade()) {
                page.show(&account);
            }
        });
    }

    fn show(&self, account: &Account) {
        let tr = &self.context.tr;
        let texture = account
            .icon_file
            .as_ref()
            .and_then(|path| gdk::Texture::from_filename(path).ok());
        account::set_avatar(
            &self.avatar,
            account.display_name(),
            texture.as_ref(),
            HERO_AVATAR_SIZE,
        );
        self.name.set_text(account.display_name());
        let kind = tr.get(if account.administrator {
            "users-type-admin"
        } else {
            "users-type-standard"
        });
        self.kind.set_text(&kind);
        self.account_type.set_text(&kind);
        self.account_name.set_text(&account.user_name);
        self.home.set_text(&account.home);
        // 고치는 중인 이름은 덮어쓰지 않는다
        let editing = self
            .full_name
            .state_flags()
            .contains(gtk::StateFlags::FOCUS_WITHIN);
        if !editing || self.account.borrow().is_none() {
            self.full_name.set_text(&account.real_name);
        }
        self.full_name.set_sensitive(true);
        self.avatar_button.set_sensitive(true);
        self.account.replace(Some(account.clone()));
    }

    fn commit_name(self: &Rc<Self>) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        let text = self.full_name.text();
        let name = text.trim();
        if name == account.real_name.trim() {
            return;
        }
        let bad = |c: char| c == ':' || c == ',' || c.is_control();
        if name.is_empty() || name.contains(bad) {
            self.context
                .notify(&self.context.tr.get("users-name-invalid"));
            self.full_name.set_text(&account.real_name);
            return;
        }
        let name = name.to_owned();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = account.set_real_name(&name).await;
            let Some(page) = weak.upgrade() else {
                return;
            };
            match result {
                // 바뀐 이름은 AccountsService의 Changed 신호로 다시 읽는다
                Ok(()) => page.reload(),
                Err(err) => {
                    page.context
                        .notify_error("users-name-error", crate::dbus::message(&err));
                    page.full_name.set_text(&account.real_name);
                }
            }
        });
    }

    /// 다른 정규 계정 (이 사용자 말고). 고치지는 않고 보이기만 한다
    fn load_others(self: &Rc<Self>) {
        let me = glib::user_name().to_string_lossy().into_owned();
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        let others: Vec<String> = passwd
            .lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split(':').collect();
                let [name, _, uid, _, _, _, shell] = fields[..] else {
                    return None;
                };
                let uid: u32 = uid.parse().ok()?;
                let login = !shell.ends_with("nologin") && !shell.ends_with("false");
                ((1000..60000).contains(&uid) && login && name != me).then(|| name.to_owned())
            })
            .collect();
        if others.is_empty() {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            for name in others {
                let account = Account::find(&name).await.ok();
                let Some(page) = weak.upgrade() else {
                    return;
                };
                let tr = &page.context.tr;
                let (display, icon, kind) = match &account {
                    Some(account) => (
                        account.display_name().to_owned(),
                        account.icon_file.clone(),
                        tr.get(if account.administrator {
                            "users-type-admin"
                        } else {
                            "users-type-standard"
                        }),
                    ),
                    None => (name.clone(), None, String::new()),
                };
                let picture = account::avatar(&display, icon.as_deref(), OTHER_AVATAR_SIZE);
                let (row, _) = rich_row(
                    Some(picture.upcast_ref()),
                    &display,
                    (!kind.is_empty()).then_some(kind.as_str()),
                );
                page.others.append(&row);
                set_group_visible(&page.others, true);
            }
        });
    }

    /// 사진 고르기 (macOS의 사진 편집): 미리 보기, 모노그램과 기본 사진, 사진 파일 고르기
    fn open_picture_sheet(self: &Rc<Self>) {
        let Some(account) = self.account.borrow().clone() else {
            return;
        };
        let tr = &self.context.tr;
        let window = self.context.window();
        let sheet = Sheet::new(
            window.as_ref(),
            None,
            &tr.get("users-picture-title"),
            Some(&tr.get("users-picture-description")),
            Some(&tr.get("cancel")),
            &tr.get("users-picture-save"),
        );
        let preview = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .css_classes(["picture-preview"])
            .build();
        sheet.content.append(&preview);
        let choice: Rc<RefCell<Choice>> = Rc::new(RefCell::new(match &account.icon_file {
            Some(path) => gdk::Texture::from_filename(path)
                .map(Choice::Photo)
                .unwrap_or(Choice::Monogram),
            None => Choice::Monogram,
        }));
        let name = account.display_name().to_owned();

        let grid = gtk::Grid::builder()
            .row_spacing(10)
            .column_spacing(10)
            .halign(gtk::Align::Center)
            .css_classes(["picture-options"])
            .build();
        let options: Rc<RefCell<Vec<(gtk::Button, Choice)>>> = Rc::default();
        let add_option = |content: gtk::Box, accessible: String, value: Choice| {
            let index = options.borrow().len() as i32;
            let button = gtk::Button::builder()
                .child(&content)
                .tooltip_text(&accessible)
                .css_classes(["picture-option"])
                .build();
            button.update_property(&[gtk::accessible::Property::Label(&accessible)]);
            grid.attach(
                &button,
                index % PICKER_COLUMNS,
                index / PICKER_COLUMNS,
                1,
                1,
            );
            options.borrow_mut().push((button, value));
        };
        add_option(
            account::avatar(&name, None, PICKER_OPTION_SIZE),
            tr.get("users-picture-monogram"),
            Choice::Monogram,
        );
        for (index, picture) in DEFAULT_PICTURES.iter().enumerate() {
            add_option(
                account::default_picture(picture, PICKER_OPTION_SIZE),
                tr.get(picture.name_id),
                Choice::Default(index),
            );
        }
        sheet.content.append(&grid);

        let choose = gtk::Button::builder()
            .label(tr.get("users-picture-choose"))
            .halign(gtk::Align::Center)
            .build();
        sheet.content.append(&choose);

        // 고른 것을 미리 보기와 선택 표시에 반영한다
        let refresh = {
            let (preview, choice, options, name) = (
                preview.clone(),
                choice.clone(),
                options.clone(),
                name.clone(),
            );
            Rc::new(move || {
                while let Some(child) = preview.first_child() {
                    preview.remove(&child);
                }
                let current = choice.borrow();
                preview.append(&current.widget(&name, PICKER_PREVIEW_SIZE));
                for (button, value) in options.borrow().iter() {
                    if value.same(&current) {
                        button.add_css_class("selected");
                    } else {
                        button.remove_css_class("selected");
                    }
                }
            })
        };
        refresh();
        for (button, value) in options.borrow().iter() {
            let (choice, refresh, value) = (choice.clone(), refresh.clone(), value.clone());
            button.connect_clicked(move |_| {
                choice.replace(value.clone());
                refresh();
            });
        }
        let weak_sheet = sheet.window.downgrade();
        let (choice_for_file, refresh_for_file) = (choice.clone(), refresh.clone());
        let context = self.context.clone();
        choose.connect_clicked(move |_| {
            let Some(parent) = weak_sheet.upgrade() else {
                return;
            };
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&context.tr.get("users-picture-filter")));
            filter.add_mime_type("image/*");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder()
                .title(context.tr.get("users-picture-choose-title"))
                .modal(true)
                .filters(&filters)
                .default_filter(&filter)
                .build();
            let (choice, refresh, context) = (
                choice_for_file.clone(),
                refresh_for_file.clone(),
                context.clone(),
            );
            glib::spawn_future_local(async move {
                let Ok(file) = dialog.open_future(Some(&parent)).await else {
                    return;
                };
                match gdk::Texture::from_file(&file) {
                    Ok(texture) => {
                        // 미리 보기도 저장할 모양(가운데 정사각형)으로 보인다
                        let cropped = account::render_photo(&parent, &texture).unwrap_or(texture);
                        choice.replace(Choice::Photo(cropped));
                        refresh();
                    }
                    Err(err) => context.notify_error("users-picture-load-error", err.message()),
                }
            });
        });

        let weak = Rc::downgrade(self);
        let window = sheet.window.clone();
        sheet.confirm.connect_clicked(move |confirm| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            confirm.set_sensitive(false);
            let chosen = choice.borrow().clone();
            let (account, window) = (account.clone(), window.clone());
            glib::spawn_future_local(async move {
                let result = page.apply_picture(&account, &chosen).await;
                window.close();
                match result {
                    Ok(()) => page.reload(),
                    Err(message) => page.context.notify_error("users-picture-error", message),
                }
            });
        });
        sheet.present();
    }

    async fn apply_picture(&self, account: &Account, choice: &Choice) -> Result<(), String> {
        let texture = match choice {
            Choice::Monogram => None,
            Choice::Default(index) => Some(
                account::render_default_picture(&self.avatar, &DEFAULT_PICTURES[*index])
                    .ok_or("rendering failed")?,
            ),
            Choice::Photo(texture) => {
                Some(account::render_photo(&self.avatar, texture).ok_or("rendering failed")?)
            }
        };
        let path = match texture {
            Some(texture) => Some(account::save_picture(&texture).map_err(|err| err.to_string())?),
            None => None,
        };
        account
            .set_icon_file(path.as_deref())
            .await
            .map_err(|err| crate::dbus::message(&err))
    }

    /// 암호 변경 시트 (macOS): 이전 암호, 새로운 암호, 확인
    fn open_password_sheet(self: &Rc<Self>) {
        let tr = &self.context.tr;
        let window = self.context.window();
        let sheet = Sheet::new(
            window.as_ref(),
            Some(&badge(
                "system-lock-screen-symbolic",
                Tint::Gray,
                BadgeSize::Medium,
            )),
            &tr.get("users-password-title"),
            Some(&tr.get("users-password-description")),
            Some(&tr.get("cancel")),
            &tr.get("users-password-confirm"),
        );
        let old = sheet.add_entry(&tr.get("users-old-password"), true);
        let new = sheet.add_entry(&tr.get("users-new-password"), true);
        let verify = sheet.add_entry(&tr.get("users-verify-password"), true);
        for (entry, label) in [
            (&old, "users-old-password"),
            (&new, "users-new-password"),
            (&verify, "users-verify-password"),
        ] {
            entry.update_property(&[gtk::accessible::Property::Label(&tr.get(label))]);
        }
        let error = gtk::Label::builder()
            .wrap(true)
            .xalign(0.0)
            .visible(false)
            .css_classes(["password-error"])
            .build();
        sheet.content.append(&error);
        let spinner = gtk::Spinner::builder()
            .halign(gtk::Align::Start)
            .visible(false)
            .build();
        sheet.content.append(&spinner);

        let weak = Rc::downgrade(self);
        let window = sheet.window.clone();
        sheet.confirm.connect_clicked(move |confirm| {
            let Some(page) = weak.upgrade() else {
                return;
            };
            let tr = page.context.tr.clone();
            let fail = |message: &str, focus: &gtk::Entry| {
                error.set_text(message);
                error.set_visible(true);
                focus.grab_focus();
            };
            let (old_text, new_text) = (old.text().to_string(), new.text().to_string());
            if new_text.is_empty() {
                fail(&tr.get("users-password-empty"), &new);
                return;
            }
            if new_text != verify.text() {
                verify.set_text("");
                fail(&tr.get("users-password-mismatch"), &verify);
                return;
            }
            error.set_visible(false);
            confirm.set_sensitive(false);
            spinner.set_visible(true);
            spinner.set_spinning(true);
            let (confirm, window) = (confirm.clone(), window.clone());
            let (old, new, verify, error, spinner) = (
                old.clone(),
                new.clone(),
                verify.clone(),
                error.clone(),
                spinner.clone(),
            );
            glib::spawn_future_local(async move {
                let result = change_password(&old_text, &new_text).await;
                confirm.set_sensitive(true);
                spinner.set_visible(false);
                spinner.set_spinning(false);
                let (message, focus) = match result {
                    Ok(()) => {
                        window.close();
                        page.context.notify(&tr.get("users-password-changed"));
                        return;
                    }
                    Err(PasswdError::WrongPassword) => {
                        old.set_text("");
                        (tr.get("users-password-wrong"), &old)
                    }
                    Err(PasswdError::Rejected(reason)) => {
                        new.set_text("");
                        verify.set_text("");
                        (rejection_message(&tr, &reason), &new)
                    }
                    Err(PasswdError::Failed(reason)) => {
                        let mut args = FluentArgs::new();
                        args.set("error", reason);
                        (tr.get_with("users-password-failed", &args), &old)
                    }
                };
                error.set_text(&message);
                error.set_visible(true);
                focus.grab_focus();
            });
        });
        sheet.present();
    }
}

/// 사진 고르기에서 고른 것
#[derive(Clone)]
enum Choice {
    Monogram,
    Default(usize),
    Photo(gdk::Texture),
}

impl Choice {
    fn same(&self, other: &Choice) -> bool {
        match (self, other) {
            (Self::Monogram, Self::Monogram) => true,
            (Self::Default(a), Self::Default(b)) => a == b,
            _ => false,
        }
    }

    fn widget(&self, name: &str, size: i32) -> gtk::Box {
        match self {
            Self::Monogram => account::avatar(name, None, size),
            Self::Default(index) => account::default_picture(&DEFAULT_PICTURES[*index], size),
            Self::Photo(texture) => {
                let frame = account::avatar(name, None, size);
                account::set_avatar(&frame, name, Some(texture), size);
                frame
            }
        }
    }
}

/// passwd가 묻는 것
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prompt {
    Current,
    New,
    Retype,
}

/// 출력의 마지막 줄이 암호를 묻는 줄이면 무엇을 묻는지 (LC_ALL=C의 passwd와 PAM 문구)
fn prompt(output: &str) -> Option<Prompt> {
    let line = output.trim_end().lines().last()?.trim().to_lowercase();
    if !line.ends_with(':') || !line.contains("password") {
        return None;
    }
    Some(
        if line.contains("retype") || line.contains("re-enter") || line.contains("again") {
            Prompt::Retype
        } else if line.contains("new") {
            Prompt::New
        } else {
            Prompt::Current
        },
    )
}

#[derive(Debug, PartialEq, Eq)]
enum PasswdError {
    /// 이전 암호가 틀렸다
    WrongPassword,
    /// 새 암호를 받지 않았다 (PAM이 준 까닭)
    Rejected(String),
    Failed(String),
}

/// passwd 출력에서 묻는 줄과 머리말을 뺀 안내. 예: "BAD PASSWORD: is too simple"
fn explanation(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && prompt(line).is_none()
                && !line.starts_with("Changing password")
                && *line != "passwd: password unchanged"
        })
        .map(|line| line.strip_prefix("passwd: ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join(" ")
}

/// passwd가 끝난 뒤의 실패. 새 암호를 묻기 전에 끝났으면 이전 암호가 틀린 것이다
fn failure(output: &str, asked_new: bool) -> PasswdError {
    let text = explanation(output);
    let lower = text.to_lowercase();
    if !asked_new
        && (lower.contains("authentication token manipulation error")
            || lower.contains("authentication failure"))
    {
        PasswdError::WrongPassword
    } else if asked_new && !text.is_empty() {
        PasswdError::Rejected(text)
    } else {
        PasswdError::Failed(if text.is_empty() {
            "passwd failed".into()
        } else {
            text
        })
    }
}

/// 알려진 PAM 문구는 우리말로 바꾼다
fn rejection_message(tr: &bunt_i18n::Translations, reason: &str) -> String {
    let lower = reason.to_lowercase();
    let id = if lower.contains("too simple") || lower.contains("simplistic") {
        Some("users-password-too-simple")
    } else if lower.contains("longer password") || lower.contains("too short") {
        Some("users-password-too-short")
    } else if lower.contains("similar")
        || lower.contains("palindrome")
        || lower.contains("case changes")
        || lower.contains("rotated")
    {
        Some("users-password-too-similar")
    } else if lower.contains("not been changed") || lower.contains("same as the old") {
        Some("users-password-unchanged")
    } else {
        None
    };
    match id {
        Some(id) => tr.get(id),
        None => {
            let mut args = FluentArgs::new();
            args.set("error", reason.to_owned());
            tr.get_with("users-password-rejected", &args)
        }
    }
}

/// 암호를 바꾼다. passwd(setuid)는 터미널에서만 암호를 받으므로 script가 만든 가상 터미널에서 돌리고,
/// 묻는 줄을 보고 답한다 (GNOME 설정의 방법). 영어 문구를 읽도록 LC_ALL=C로 띄운다
async fn change_password(old: &str, new: &str) -> Result<(), PasswdError> {
    let failed = |err: glib::Error| PasswdError::Failed(err.message().to_owned());
    let launcher = gio::SubprocessLauncher::new(
        gio::SubprocessFlags::STDIN_PIPE
            | gio::SubprocessFlags::STDOUT_PIPE
            | gio::SubprocessFlags::STDERR_MERGE,
    );
    launcher.setenv("LC_ALL", "C", true);
    let process = launcher
        .spawn(&[
            OsStr::new(SCRIPT),
            OsStr::new("--quiet"),
            OsStr::new("--return"),
            OsStr::new("--command"),
            OsStr::new("passwd"),
            OsStr::new("/dev/null"),
        ])
        .map_err(failed)?;
    let (Some(stdin), Some(stdout)) = (process.stdin_pipe(), process.stdout_pipe()) else {
        process.force_exit();
        return Err(PasswdError::Failed("no pipes to passwd".into()));
    };
    // 마지막으로 답한 뒤의 출력
    let mut output = String::new();
    let mut asked = Vec::new();
    loop {
        let read = glib::future_with_timeout(
            PASSWD_TIMEOUT,
            stdout.read_bytes_future(4096, glib::Priority::DEFAULT),
        )
        .await;
        let bytes = match read {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(err)) => {
                process.force_exit();
                return Err(failed(err));
            }
            Err(_) => {
                process.force_exit();
                return Err(PasswdError::Failed("passwd did not answer".into()));
            }
        };
        if bytes.is_empty() {
            break;
        }
        output.push_str(&String::from_utf8_lossy(&bytes));
        let Some(question) = prompt(&output) else {
            continue;
        };
        // 같은 것을 또 물으면 앞의 답을 받지 않은 것이다 (예: 새 암호가 너무 단순함)
        if asked.contains(&question) {
            process.force_exit();
            let mut before = output.trim_end().lines().collect::<Vec<_>>();
            before.pop();
            return Err(failure(&before.join("\n"), asked.contains(&Prompt::New)));
        }
        asked.push(question);
        let answer = if question == Prompt::Current {
            old
        } else {
            new
        };
        glib::timeout_future(ANSWER_DELAY).await;
        stdin
            .write_all_future(format!("{answer}\n").into_bytes(), glib::Priority::DEFAULT)
            .await
            .map_err(|(_, err)| failed(err))?;
        output.clear();
    }
    match process.wait_check_future().await {
        Ok(()) => Ok(()),
        Err(_) => Err(failure(&output, asked.contains(&Prompt::New))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwd_prompts() {
        assert_eq!(
            prompt("Changing password for buntos.\r\nCurrent password: "),
            Some(Prompt::Current)
        );
        assert_eq!(prompt("\r\nNew password: "), Some(Prompt::New));
        assert_eq!(prompt("\r\nRetype new password: "), Some(Prompt::Retype));
        assert_eq!(prompt("Changing password for buntos.\r\n"), None);
        assert_eq!(prompt("passwd: password updated successfully\r\n"), None);
    }

    #[test]
    fn wrong_current_password() {
        let output =
            "\r\npasswd: Authentication token manipulation error\r\npasswd: password unchanged\r\n";
        assert_eq!(failure(output, false), PasswdError::WrongPassword);
    }

    #[test]
    fn rejected_new_password_keeps_the_reason() {
        let output = "\r\nBAD PASSWORD: is too simple";
        assert_eq!(
            failure(output, true),
            PasswdError::Rejected("BAD PASSWORD: is too simple".into())
        );
        assert_eq!(
            explanation(
                "Changing password for a.\nNew password: \nYou must choose a longer password.\n"
            ),
            "You must choose a longer password."
        );
    }

    #[test]
    fn other_failures() {
        assert_eq!(
            failure(
                "passwd: You must wait longer to change your password\n",
                false
            ),
            PasswdError::Failed("You must wait longer to change your password".into())
        );
        assert_eq!(
            failure("", false),
            PasswdError::Failed("passwd failed".into())
        );
    }
}
