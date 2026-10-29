// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 처음 켤 때의 계정 만들기 (macOS 설정 지원의 "컴퓨터 계정 생성"): 별칭, 계정 이름(별칭에서
//! 제안하고 고칠 수 있다), 암호, 확인. [계속]을 누르면 buntd-setup이 관리자 계정을 만들고,
//! 같은 암호로 바로 로그인한다

use std::{cell::Cell, rc::Rc, time::Duration};

use bunt_greeter::{
    account::{self, Invalid},
    greetd::{self, LoginError},
    setup,
};
use bunt_i18n::FluentArgs;
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::Shared;

/// 계정 만들기는 홈 폴더를 만들므로 D-Bus 기본 시간 제한(25초)보다 길게 기다린다
const CREATE_TIMEOUT_MS: i32 = 60_000;

struct Form {
    shared: Rc<Shared>,
    full_name: gtk::Entry,
    account_name: gtk::Entry,
    password: gtk::PasswordEntry,
    verify: gtk::PasswordEntry,
    error: gtk::Label,
    status: gtk::Label,
    spinner: gtk::Spinner,
    proceed: gtk::Button,
    /// 사용자가 계정 이름을 직접 고쳤으면 더는 제안하지 않는다
    name_edited: Cell<bool>,
    /// 제안한 이름을 넣는 중 (changed 처리기가 직접 고친 것으로 보지 않게)
    suggesting: Cell<bool>,
}

pub fn build(shared: &Rc<Shared>) -> gtk::Widget {
    let tr = &shared.tr;
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .width_request(520)
        .css_classes(["setup-card"])
        .build();

    let badge = gtk::Box::builder()
        .width_request(64)
        .height_request(64)
        .halign(gtk::Align::Center)
        .css_classes(["setup-badge"])
        .build();
    let icon = gtk::Image::from_icon_name("avatar-default-symbolic");
    icon.set_pixel_size(38);
    icon.set_hexpand(true);
    badge.append(&icon);
    card.append(&badge);
    card.append(
        &gtk::Label::builder()
            .label(tr.get("setup-title"))
            .css_classes(["setup-title"])
            .build(),
    );
    card.append(
        &gtk::Label::builder()
            .label(tr.get("setup-description"))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .max_width_chars(56)
            .css_classes(["setup-description"])
            .build(),
    );

    let full_name = gtk::Entry::builder().hexpand(true).build();
    let account_name = gtk::Entry::builder()
        .hexpand(true)
        .max_length(account::ACCOUNT_NAME_MAX as i32)
        .input_purpose(gtk::InputPurpose::FreeForm)
        .input_hints(gtk::InputHints::NO_SPELLCHECK | gtk::InputHints::LOWERCASE)
        .build();
    let password = gtk::PasswordEntry::builder()
        .placeholder_text(tr.get("setup-required"))
        .show_peek_icon(true)
        .hexpand(true)
        .build();
    let verify = gtk::PasswordEntry::builder()
        .placeholder_text(tr.get("setup-verify-placeholder"))
        .show_peek_icon(true)
        .hexpand(true)
        .build();

    let form = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(12)
        .css_classes(["setup-form"])
        .build();
    let mut row = 0;
    let mut add = |label: &str, field: &gtk::Widget, hint: Option<String>| {
        field.update_property(&[gtk::accessible::Property::Label(label)]);
        form.attach(
            &gtk::Label::builder()
                .label(label)
                .xalign(1.0)
                .css_classes(["setup-label"])
                .build(),
            0,
            row,
            1,
            1,
        );
        form.attach(field, 1, row, 1, 1);
        row += 1;
        if let Some(hint) = hint {
            form.attach(
                &gtk::Label::builder()
                    .label(hint)
                    .xalign(0.0)
                    .wrap(true)
                    .css_classes(["setup-hint"])
                    .build(),
                1,
                row,
                1,
                1,
            );
            row += 1;
        }
    };
    add(&tr.get("setup-full-name"), full_name.upcast_ref(), None);
    add(
        &tr.get("setup-account-name"),
        account_name.upcast_ref(),
        Some(tr.get("setup-account-name-hint")),
    );
    let mut args = FluentArgs::new();
    args.set("min", account::PASSWORD_MIN);
    add(
        &tr.get("setup-password"),
        password.upcast_ref(),
        Some(tr.get_with("setup-password-hint", &args)),
    );
    add(&tr.get("setup-verify"), verify.upcast_ref(), None);
    card.append(&form);

    let error = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .css_classes(["setup-error"])
        .build();
    card.append(&error);

    let spinner = gtk::Spinner::builder().visible(false).build();
    let status = gtk::Label::builder()
        .visible(false)
        .css_classes(["setup-status"])
        .build();
    let proceed = gtk::Button::builder()
        .label(tr.get("setup-continue"))
        .css_classes(["suggested-action"])
        .build();
    let buttons = gtk::Box::builder()
        .spacing(10)
        .halign(gtk::Align::End)
        .build();
    buttons.append(&spinner);
    buttons.append(&status);
    buttons.append(&proceed);
    card.append(&buttons);

    let form = Rc::new(Form {
        shared: shared.clone(),
        full_name,
        account_name,
        password,
        verify,
        error,
        status,
        spinner,
        proceed,
        name_edited: Cell::new(false),
        suggesting: Cell::new(false),
    });
    form.connect();
    let weak = Rc::downgrade(&form);
    card.connect_map(move |_| {
        if let Some(form) = weak.upgrade() {
            form.full_name.grab_focus();
        }
    });
    card.connect_destroy(move |_| {
        let _ = &form;
    });

    // 카드가 화면보다 크면(작은 화면) 움직여 볼 수 있게 한다
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&card)
        .build()
        .upcast()
}

impl Form {
    fn connect(self: &Rc<Self>) {
        // 계정 이름은 사용자가 고치기 전까지 별칭을 따라간다
        let weak = Rc::downgrade(self);
        self.full_name.connect_changed(move |entry| {
            let Some(form) = weak.upgrade() else {
                return;
            };
            if !form.name_edited.get() {
                form.suggesting.set(true);
                form.account_name
                    .set_text(&account::suggest_account_name(&entry.text()));
                form.suggesting.set(false);
            }
        });
        let weak = Rc::downgrade(self);
        self.account_name.connect_changed(move |entry| {
            let Some(form) = weak.upgrade() else {
                return;
            };
            if !form.suggesting.get() {
                // 지우면 다시 제안한다
                form.name_edited.set(!entry.text().is_empty());
            }
        });

        // Enter는 다음 칸으로, 마지막 칸에서는 [계속]
        let next = |from: &gtk::Widget, to: gtk::Widget| {
            let to = to.downgrade();
            let focus = move || {
                if let Some(to) = to.upgrade() {
                    to.grab_focus();
                }
            };
            if let Some(entry) = from.downcast_ref::<gtk::Entry>() {
                entry.connect_activate(move |_| focus());
            } else if let Some(entry) = from.downcast_ref::<gtk::PasswordEntry>() {
                entry.connect_activate(move |_| focus());
            }
        };
        next(
            self.full_name.upcast_ref(),
            self.account_name.clone().upcast(),
        );
        next(
            self.account_name.upcast_ref(),
            self.password.clone().upcast(),
        );
        next(self.password.upcast_ref(), self.verify.clone().upcast());
        let weak = Rc::downgrade(self);
        self.verify.connect_activate(move |_| {
            if let Some(form) = weak.upgrade() {
                form.submit();
            }
        });
        let weak = Rc::downgrade(self);
        self.proceed.connect_clicked(move |_| {
            if let Some(form) = weak.upgrade() {
                form.submit();
            }
        });
    }

    fn tr(&self, id: &str) -> String {
        self.shared.tr.get(id)
    }

    fn fail(&self, message: &str, focus: Option<&gtk::Widget>) {
        self.error.set_text(message);
        self.error.set_visible(true);
        if let Some(focus) = focus {
            focus.grab_focus();
        }
    }

    fn invalid_message(&self, invalid: Invalid) -> String {
        match invalid {
            Invalid::FullName => self.tr("setup-error-full-name"),
            Invalid::AccountName => self.tr("setup-error-account-name"),
            Invalid::AccountNameTaken => self.tr("setup-error-account-taken"),
            Invalid::PasswordTooShort => {
                let mut args = FluentArgs::new();
                args.set("min", account::PASSWORD_MIN);
                self.shared.tr.get_with("setup-error-password-short", &args)
            }
            Invalid::Password => self.tr("setup-error-password-invalid"),
        }
    }

    fn invalid_field(&self, invalid: Invalid) -> gtk::Widget {
        match invalid {
            Invalid::FullName => self.full_name.clone().upcast(),
            Invalid::AccountName | Invalid::AccountNameTaken => self.account_name.clone().upcast(),
            Invalid::PasswordTooShort | Invalid::Password => self.password.clone().upcast(),
        }
    }

    /// 입력을 검사한다. 같은 검사를 buntd-setup이 한 번 더 한다
    fn check(&self) -> Result<(String, String, String), Invalid> {
        let full_name = account::check_full_name(&self.full_name.text())?;
        let account_name = self.account_name.text().to_string();
        account::check_account_name(&account_name)?;
        let passwd = std::fs::read_to_string(account::PASSWD).unwrap_or_default();
        let group = std::fs::read_to_string(account::GROUP).unwrap_or_default();
        if account::is_name_taken(&account_name, &passwd, &group) {
            return Err(Invalid::AccountNameTaken);
        }
        let password = self.password.text().to_string();
        account::check_password(&password)?;
        Ok((full_name, account_name, password))
    }

    fn submit(self: &Rc<Self>) {
        if self.spinner.is_visible() {
            return;
        }
        self.error.set_visible(false);
        let (full_name, account_name, password) = match self.check() {
            Ok(values) => values,
            Err(invalid) => {
                self.fail(
                    &self.invalid_message(invalid),
                    Some(&self.invalid_field(invalid)),
                );
                return;
            }
        };
        if self.verify.text() != password {
            self.verify.set_text("");
            self.fail(
                &self.tr("setup-error-password-mismatch"),
                Some(self.verify.upcast_ref()),
            );
            return;
        }
        self.set_busy(Some(&self.tr("setup-creating")));
        let weak = Rc::downgrade(self);
        if self.shared.preview {
            glib::timeout_add_local_once(Duration::from_millis(800), move || {
                if let Some(form) = weak.upgrade() {
                    form.set_busy(None);
                    form.fail(&form.tr("setup-preview"), None);
                }
            });
            return;
        }
        glib::spawn_future_local(async move {
            let created = create_account(&full_name, &account_name, &password).await;
            let Some(form) = weak.upgrade() else {
                return;
            };
            match created {
                Ok(uid) => {
                    eprintln!("bunt-greeter: created {account_name} (uid {uid}), logging in");
                    form.set_busy(Some(&form.tr("setup-signing-in")));
                    form.login(account_name, password).await;
                }
                Err(err) => form.create_failed(err),
            }
        });
    }

    async fn login(self: &Rc<Self>, account_name: String, password: String) {
        let (cmd, env) = crate::session();
        let result = gio::spawn_blocking(move || {
            let mut client = greetd::Client::from_env()?;
            greetd::login(&mut client, &account_name, &password, &cmd, &env)
        })
        .await
        .unwrap_or_else(|_| Err(LoginError::Failed("login thread panicked".into())));
        match result {
            Ok(()) => self.shared.finish(),
            Err(err) => {
                // 계정은 만들었으므로 로그인 화면에서 다시 시도한다
                eprintln!("bunt-greeter: login after setup failed: {err}");
                self.shared.show_login();
            }
        }
    }

    fn create_failed(&self, mut err: glib::Error) {
        self.set_busy(None);
        let name = gio::DBusError::remote_error(&err)
            .and_then(|name| {
                name.strip_prefix(setup::ERROR_PREFIX)
                    .and_then(|rest| rest.strip_prefix('.'))
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        gio::DBusError::strip_remote_error(&mut err);
        eprintln!(
            "bunt-greeter: CreateAccount failed: {name} {}",
            err.message()
        );
        if let Some(invalid) = Invalid::from_error_name(&name) {
            self.fail(
                &self.invalid_message(invalid),
                Some(&self.invalid_field(invalid)),
            );
        } else if name == setup::ERROR_ACCOUNT_EXISTS {
            // 그 사이에 다른 곳에서 계정이 생겼다
            self.shared.show_login();
        } else {
            let mut args = FluentArgs::new();
            args.set("error", err.message().to_owned());
            self.fail(&self.shared.tr.get_with("setup-error-failed", &args), None);
        }
    }

    /// 진행 중이면 입력을 막고 상태를 보인다
    fn set_busy(&self, status: Option<&str>) {
        let busy = status.is_some();
        for widget in [
            self.full_name.upcast_ref::<gtk::Widget>(),
            self.account_name.upcast_ref(),
            self.password.upcast_ref(),
            self.verify.upcast_ref(),
            self.proceed.upcast_ref(),
        ] {
            widget.set_sensitive(!busy);
        }
        self.spinner.set_visible(busy);
        self.spinner.set_spinning(busy);
        self.status.set_visible(busy);
        self.status.set_text(status.unwrap_or_default());
    }
}

async fn create_account(
    full_name: &str,
    account_name: &str,
    password: &str,
) -> Result<u32, glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::System).await?;
    let reply = bus
        .call_future(
            Some(setup::BUS_NAME),
            setup::OBJECT_PATH,
            setup::INTERFACE,
            "CreateAccount",
            Some(&(full_name, account_name, password).to_variant()),
            Some(glib::VariantTy::new("(u)").expect("valid D-Bus type")),
            gio::DBusCallFlags::NONE,
            CREATE_TIMEOUT_MS,
        )
        .await?;
    Ok(reply.child_value(0).get::<u32>().unwrap_or_default())
}
