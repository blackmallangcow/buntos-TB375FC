// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 잠금 화면의 모양 (macOS 26의 화면 잠금. 로그인 화면과 같은 모양이다): 흐린 배경화면, 위에 날짜와 큰 시각,
//! 아래에 사진, 별칭, 암호 칸. 로그인 화면과 달리 다시 시작·시스템 종료가 없다(macOS와 같다).
//!
//! - 암호 칸은 첫 화면에만 두고 다른 화면에는 배경화면만 보인다. 그 화면이 빠지면 남은 화면으로 옮긴다.
//! - 암호가 틀리면 칸이 흔들리고, 다섯 번째부터는 잠시 기다려야 다시 칠 수 있다(macOS처럼 기다리는 시간이 늘어난다).
//! - Esc는 친 암호를 지운다. 창은 닫히지 않는다(Alt+F4).

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

use crate::{
    WINDOW_TITLE,
    accounts::account_details,
    avatar, background,
    clock::clock,
    pam::{self, Outcome, Secret},
    session_lock::SessionLock,
};

const AVATAR_SIZE: i32 = 96;
/// 흔들기 애니메이션 길이 (greeter/data/style.css의 @keyframes shake와 같다)
const SHAKE: Duration = Duration::from_millis(450);
/// 잠그고 창이 키보드를 받은 뒤 시험(tests/e2e)에 준비됐다고 알리기까지의 틈과, 키보드를 받았다는 알림이 없을 때의 상한
const ANNOUNCE_DELAY: Duration = Duration::from_millis(300);
const ANNOUNCE_FALLBACK: Duration = Duration::from_secs(5);

/// `failures`번 틀린 뒤 다시 칠 수 있기까지 기다릴 시간. 다섯 번째부터 30초, 1분, 2분, 그 뒤로는 5분씩이다.
/// 그 전에도 PAM(pam_unix)이 틀릴 때마다 2초쯤 기다린다
pub fn retry_delay(failures: u32) -> Option<Duration> {
    let seconds = match failures {
        0..=4 => return None,
        5 => 30,
        6 => 60,
        7 => 120,
        _ => 300,
    };
    Some(Duration::from_secs(seconds))
}

/// 남은 시간의 안내: 1분 이상이면 분, 아니면 초 (올림). (메시지 ID, 값)
fn retry_message(remaining: Duration) -> (&'static str, u64) {
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    if seconds >= 60 {
        ("lock-retry-minutes", seconds.div_ceil(60))
    } else {
        ("lock-retry-seconds", seconds.max(1))
    }
}

/// 잠금 화면 전체
pub struct LockScreen {
    app: gtk::Application,
    tr: Rc<Translations>,
    /// None이면 잠그지 않고 화면만 본다 (--preview)
    lock: Option<Rc<SessionLock>>,
    /// 이 프로세스의 사용자 (계정 이름)
    user: String,
    /// 별칭과 사진. AccountsService에서 읽기 전에는 passwd의 이름이다
    details: RefCell<(String, Option<PathBuf>)>,
    screens: RefCell<Vec<Screen>>,
    /// 암호 칸이 있는 화면
    main: RefCell<Option<Main>>,
    failures: Cell<u32>,
    /// 이때까지는 다시 칠 수 없다 (여러 번 틀렸다)
    wait_until: Cell<Option<Instant>>,
    /// 남은 시간을 1초마다 고치는 중이다
    wait_ticking: Cell<bool>,
    /// PAM이 확인하는 중이다
    busy: Cell<bool>,
    /// 컴포지터가 잠겼다고 알렸다 (미리 보기는 처음부터 참)
    locked: Cell<bool>,
    announced: Cell<bool>,
}

/// 화면 하나의 창
struct Screen {
    monitor: Option<gdk::Monitor>,
    window: gtk::ApplicationWindow,
    overlay: gtk::Overlay,
}

/// 암호 칸이 있는 화면의 위젯
struct Main {
    window: gtk::ApplicationWindow,
    avatar: gtk::Box,
    name: gtk::Label,
    entry: gtk::PasswordEntry,
    field: gtk::Box,
    go: gtk::Button,
    spinner: gtk::Spinner,
    message: gtk::Label,
}

impl LockScreen {
    pub fn new(
        app: &gtk::Application,
        tr: Rc<Translations>,
        lock: Option<Rc<SessionLock>>,
    ) -> Rc<Self> {
        let user = glib::user_name().to_string_lossy().into_owned();
        let real = glib::real_name().to_string_lossy().into_owned();
        let name = if real.trim().is_empty() || real == "Unknown" {
            user.clone()
        } else {
            real
        };
        let preview = lock.is_none();
        let screen = Rc::new(Self {
            app: app.clone(),
            tr,
            lock,
            user,
            details: RefCell::new((name, None)),
            screens: RefCell::default(),
            main: RefCell::default(),
            failures: Cell::new(0),
            wait_until: Cell::new(None),
            wait_ticking: Cell::new(false),
            busy: Cell::new(false),
            locked: Cell::new(preview),
            announced: Cell::new(false),
        });
        screen.load_details();
        screen
    }

    /// 화면 하나의 창을 만든다. 잠금 중이면 그 화면의 잠금 표면이 되고, 미리 보기면 보통 창으로 띄운다
    pub fn add_screen(self: &Rc<Self>, monitor: Option<&gdk::Monitor>) {
        let window = gtk::ApplicationWindow::builder()
            .application(&self.app)
            // 접근성 이름. 에이전트 데몬은 이 이름의 창을 읽지도 조작하지도 않는다 (agent/src/atspi.rs)
            .title(WINDOW_TITLE)
            .decorated(false)
            .css_classes(["greeter", "lock-screen"])
            .build();
        // 잠금 화면은 닫히지 않는다 (Alt+F4)
        window.connect_close_request(|_| glib::Propagation::Stop);
        let overlay = gtk::Overlay::builder()
            .child(&background::picture())
            .build();
        overlay.add_overlay(
            &gtk::Box::builder()
                .css_classes(["greeter-dim"])
                .can_target(false)
                .build(),
        );
        window.set_child(Some(&overlay));
        let weak = Rc::downgrade(self);
        window.connect_is_active_notify(move |window| {
            if let Some(screen) = weak.upgrade()
                && window.is_active()
            {
                screen.window_active(window);
            }
        });
        if let Some(monitor) = monitor {
            let weak = Rc::downgrade(self);
            monitor.connect_invalidate(move |monitor| {
                if let Some(screen) = weak.upgrade() {
                    screen.monitor_gone(monitor);
                }
            });
        }
        self.screens.borrow_mut().push(Screen {
            monitor: monitor.cloned(),
            window: window.clone(),
            overlay: overlay.clone(),
        });
        if self.main.borrow().is_none() {
            self.build_main(&window, &overlay);
        }
        match (&self.lock, monitor) {
            // 라이브러리가 창을 그 화면의 잠금 표면으로 띄운다
            (Some(lock), Some(monitor)) => lock.assign_window_to_monitor(&window, monitor),
            _ => {
                window.set_default_size(1280, 800);
                window.present();
            }
        }
    }

    /// 컴포지터가 잠겼다고 알렸다
    pub fn locked(self: &Rc<Self>) {
        self.locked.set(true);
        let active = self
            .main
            .borrow()
            .as_ref()
            .is_some_and(|main| main.window.is_active());
        if active {
            self.announce_soon();
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(ANNOUNCE_FALLBACK, move || {
            if let Some(screen) = weak.upgrade() {
                screen.announce();
            }
        });
    }

    fn window_active(self: &Rc<Self>, window: &gtk::ApplicationWindow) {
        let main_window = self
            .main
            .borrow()
            .as_ref()
            .is_some_and(|main| main.window == *window);
        if main_window && self.locked.get() {
            self.announce_soon();
        }
    }

    fn announce_soon(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(ANNOUNCE_DELAY, move || {
            if let Some(screen) = weak.upgrade() {
                screen.announce();
            }
        });
    }

    /// 시험(tests/e2e)이 이 줄을 보고 암호를 친다. 한 번만 알린다
    fn announce(&self) {
        if !self.announced.replace(true) {
            eprintln!("bunt-lock: ready");
        }
    }

    /// 화면이 빠졌다. 암호 칸이 그 화면에 있었으면 남은 첫 화면에 다시 만든다. 빠진 화면의 창은 라이브러리가 닫는다
    fn monitor_gone(self: &Rc<Self>, monitor: &gdk::Monitor) {
        let gone: Vec<gtk::ApplicationWindow> = self
            .screens
            .borrow()
            .iter()
            .filter(|screen| screen.monitor.as_ref() == Some(monitor))
            .map(|screen| screen.window.clone())
            .collect();
        self.screens
            .borrow_mut()
            .retain(|screen| screen.monitor.as_ref() != Some(monitor));
        let main_gone = self
            .main
            .borrow()
            .as_ref()
            .is_some_and(|main| gone.contains(&main.window));
        if !main_gone {
            return;
        }
        self.main.replace(None);
        let next = self
            .screens
            .borrow()
            .first()
            .map(|screen| (screen.window.clone(), screen.overlay.clone()));
        if let Some((window, overlay)) = next {
            self.build_main(&window, &overlay);
        }
    }

    /// 시각, 사진, 별칭, 암호 칸을 그 화면에 만든다
    fn build_main(self: &Rc<Self>, window: &gtk::ApplicationWindow, overlay: &gtk::Overlay) {
        let tr = &self.tr;
        let view = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["greeter-login"])
            .build();
        view.append(&clock(tr));
        view.append(&gtk::Box::builder().vexpand(true).build());

        let (name, icon) = self.details.borrow().clone();
        let avatar = avatar::avatar(&name, icon.as_deref(), AVATAR_SIZE);
        let name_label = gtk::Label::builder()
            .label(&name)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(18)
            .css_classes(["greeter-user-name"])
            .build();
        let tile = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .halign(gtk::Align::Center)
            .build();
        tile.append(&avatar);
        tile.append(&name_label);

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
        let account = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .halign(gtk::Align::Center)
            .css_classes(["greeter-account"])
            .build();
        account.append(&tile);
        account.append(&field);
        account.append(&message);
        view.append(&account);
        // 터치 모드에서는 화면 키보드가 아래에서 나오고 사진과 암호 칸이 위로 올라간다. 키보드 없는 태블릿에서도
        // 암호를 친다. 잠긴 동안 컴포지터는 잠금 표면만 그리므로 셸의 키보드가 아니라 이 창 안의 키보드이고, 키는 암호
        // 칸에 바로 넣는다(다른 프로그램이 잠금 화면에 키를 넣는 길을 열지 않는다)
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        view.set_vexpand(true);
        column.append(&view);
        overlay.add_overlay(&column);
        bunt_keyboard::embed(
            window,
            overlay,
            &column,
            Rc::new(bunt_keyboard::EditableKeys::new(window)),
        );

        let weak_entry = entry.downgrade();
        view.connect_map(move |_| {
            if let Some(entry) = weak_entry.upgrade() {
                entry.grab_focus();
            }
        });
        self.main.replace(Some(Main {
            window: window.clone(),
            avatar,
            name: name_label,
            entry,
            field,
            go,
            spinner,
            message,
        }));
        self.label_entry();
        self.connect_entry();
        self.refresh_wait();
    }

    fn connect_entry(self: &Rc<Self>) {
        let main = self.main.borrow();
        let Some(main) = main.as_ref() else {
            return;
        };
        let weak = Rc::downgrade(self);
        main.entry.connect_activate(move |_| {
            if let Some(screen) = weak.upgrade() {
                screen.submit();
            }
        });
        let weak = Rc::downgrade(self);
        main.go.connect_clicked(move |_| {
            if let Some(screen) = weak.upgrade() {
                screen.submit();
            }
        });
        let weak = Rc::downgrade(self);
        main.entry.connect_changed(move |entry| {
            if let Some(screen) = weak.upgrade()
                && let Some(main) = screen.main.borrow().as_ref()
            {
                main.go
                    .set_visible(!entry.text().is_empty() && !screen.busy.get());
            }
        });
        // Esc는 친 암호를 지운다
        let escape = gtk::ShortcutController::new();
        let weak = Rc::downgrade(self);
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(screen) = weak.upgrade()
                    && let Some(main) = screen.main.borrow().as_ref()
                {
                    main.entry.set_text("");
                    if screen.wait_until.get().is_none() {
                        main.message.set_text("");
                    }
                }
                glib::Propagation::Stop
            })),
        ));
        main.entry.add_controller(escape);
    }

    /// 암호 칸의 접근성 이름 ("buntOS의 암호")
    fn label_entry(&self) {
        if let Some(main) = self.main.borrow().as_ref() {
            let mut args = FluentArgs::new();
            args.set("name", self.details.borrow().0.clone());
            main.entry
                .update_property(&[gtk::accessible::Property::Label(
                    &self.tr.get_with("login-password-for", &args),
                )]);
        }
    }

    fn submit(self: &Rc<Self>) {
        if self.busy.get() || self.wait_until.get().is_some() {
            return;
        }
        let password = {
            let main = self.main.borrow();
            let Some(main) = main.as_ref() else {
                return;
            };
            let text = main.entry.text();
            if text.is_empty() {
                return;
            }
            let password = Secret::new(&text);
            main.entry.set_text("");
            main.message.set_text("");
            password
        };
        self.set_busy(true);
        let user = self.user.clone();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let outcome = gio::spawn_blocking(move || pam::authenticate(&user, &password))
                .await
                .unwrap_or_else(|_| Outcome::Failed("the password check stopped".into()));
            if let Some(screen) = weak.upgrade() {
                screen.checked(outcome);
            }
        });
    }

    fn checked(self: &Rc<Self>, outcome: Outcome) {
        self.set_busy(false);
        match outcome {
            Outcome::Accepted => {
                eprintln!("bunt-lock: the password was accepted; unlocking");
                self.failures.set(0);
                match &self.lock {
                    Some(lock) => lock.unlock(),
                    None => self.app.quit(),
                }
                return;
            }
            Outcome::Rejected => {
                let failures = self.failures.get() + 1;
                self.failures.set(failures);
                eprintln!("bunt-lock: wrong password ({failures})");
                self.shake();
                match retry_delay(failures) {
                    Some(delay) => {
                        self.wait_until.set(Some(Instant::now() + delay));
                        self.refresh_wait();
                    }
                    None => self.show_message(&self.tr.get("login-wrong-password")),
                }
            }
            Outcome::Failed(error) => {
                eprintln!("bunt-lock: cannot check the password: {error}");
                let mut args = FluentArgs::new();
                args.set("error", error);
                self.show_message(&self.tr.get_with("lock-failed", &args));
            }
        }
        if let Some(main) = self.main.borrow().as_ref() {
            main.entry.grab_focus();
        }
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        if let Some(main) = self.main.borrow().as_ref() {
            main.entry
                .set_sensitive(!busy && self.wait_until.get().is_none());
            main.spinner.set_visible(busy);
            main.spinner.set_spinning(busy);
            main.go.set_visible(!busy && !main.entry.text().is_empty());
        }
    }

    /// 기다리는 동안 암호 칸을 막고 남은 시간을 1초마다 고친다. 다 기다리면 다시 연다
    fn refresh_wait(self: &Rc<Self>) {
        let remaining = self
            .wait_until
            .get()
            .map(|until| until.saturating_duration_since(Instant::now()))
            .filter(|remaining| !remaining.is_zero());
        let Some(remaining) = remaining else {
            if self.wait_until.take().is_some() {
                self.show_message("");
                self.set_busy(self.busy.get());
                if let Some(main) = self.main.borrow().as_ref() {
                    main.entry.grab_focus();
                }
            }
            return;
        };
        let (id, value) = retry_message(remaining);
        let mut args = FluentArgs::new();
        args.set("count", value);
        self.show_message(&self.tr.get_with(id, &args));
        if let Some(main) = self.main.borrow().as_ref() {
            main.entry.set_sensitive(false);
        }
        if self.wait_ticking.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(1), move || {
            if let Some(screen) = weak.upgrade() {
                screen.wait_ticking.set(false);
                screen.refresh_wait();
            }
        });
    }

    fn shake(&self) {
        let Some(field) = self.main.borrow().as_ref().map(|main| main.field.clone()) else {
            return;
        };
        field.remove_css_class("shake");
        field.add_css_class("shake");
        let field = field.downgrade();
        glib::timeout_add_local_once(SHAKE, move || {
            if let Some(field) = field.upgrade() {
                field.remove_css_class("shake");
            }
        });
    }

    fn show_message(&self, text: &str) {
        if let Some(main) = self.main.borrow().as_ref() {
            main.message.set_text(text);
        }
    }

    /// 별칭과 사진을 AccountsService에서 읽어 바꾼다. 서비스가 없으면 passwd의 이름을 둔다
    fn load_details(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let user = self.user.clone();
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::System).await else {
                return;
            };
            let Some((real_name, icon)) = account_details(&bus, &user).await else {
                return;
            };
            let Some(screen) = weak.upgrade() else {
                return;
            };
            let name = if real_name.trim().is_empty() {
                screen.details.borrow().0.clone()
            } else {
                real_name
            };
            if let Some(main) = screen.main.borrow().as_ref() {
                avatar::set_avatar(&main.avatar, &name, icon.as_deref(), AVATAR_SIZE);
                main.name.set_text(&name);
            }
            screen.details.replace((name, icon));
            screen.label_entry();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waiting_starts_at_the_fifth_wrong_password_and_grows() {
        for failures in 0..5 {
            assert_eq!(retry_delay(failures), None, "{failures}");
        }
        assert_eq!(retry_delay(5), Some(Duration::from_secs(30)));
        assert_eq!(retry_delay(6), Some(Duration::from_secs(60)));
        assert_eq!(retry_delay(7), Some(Duration::from_secs(120)));
        assert_eq!(retry_delay(8), Some(Duration::from_secs(300)));
        assert_eq!(retry_delay(u32::MAX), Some(Duration::from_secs(300)));
    }

    #[test]
    fn remaining_time_reads_in_seconds_then_minutes() {
        assert_eq!(
            retry_message(Duration::from_millis(29_400)),
            ("lock-retry-seconds", 30)
        );
        assert_eq!(
            retry_message(Duration::from_millis(1)),
            ("lock-retry-seconds", 1)
        );
        assert_eq!(
            retry_message(Duration::from_secs(60)),
            ("lock-retry-minutes", 1)
        );
        assert_eq!(
            retry_message(Duration::from_secs(61)),
            ("lock-retry-minutes", 2)
        );
        assert_eq!(
            retry_message(Duration::from_secs(300)),
            ("lock-retry-minutes", 5)
        );
    }
}
