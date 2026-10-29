// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 로고 메뉴의 세션 항목(강제 종료…, 잠자기, 화면 잠금, <별칭> 로그아웃…)과 그 창들 (macOS의 Apple 메뉴).
//!
//! - 로그아웃 확인 창(⇧⌘Q, "<별칭> 로그아웃…"): "모든 응용 프로그램을 종료하고 지금 로그아웃하겠습니까?" 60초를
//!   세다가 스스로 로그아웃한다. 로그아웃은 컴포지터가 한다(LogOut): 앱 창에 닫기를 요청하고 다 닫히면 세션을
//!   끝낸다. ⌥⇧⌘Q는 묻지 않고 컴포지터가 바로 한다.
//! - 응용 프로그램 강제 종료 창(⌥⌘⎋, "강제 종료…"): 창을 가진 앱과 응답 여부(GetApps)를 보이고, 고른 앱을 확인한
//!   뒤 끝낸다(ForceQuit, SIGKILL). 열려 있는 동안 목록을 다시 읽어 응답 없는 앱을 빨간 글로 알린다.
//! - 화면 잠금은 컴포지터(Lock), 잠자기는 logind(Suspend)에 맡긴다. 잠자기 직전에는 컴포지터가 화면을 잠근다.
//!
//! 셸 전용 컴포지터 메서드(LogOut, GetApps, ForceQuit)는 컴포지터가 띄운 이 셸 프로세스만 부를 수 있다.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

use crate::agent_access::keep_words;

const COMPOSITOR: &str = "org.buntos.Compositor";
const COMPOSITOR_PATH: &str = "/org/buntos/Compositor";
const COMPOSITOR_INTERFACE: &str = "org.buntos.Compositor1";
/// 로그아웃 확인 창이 스스로 로그아웃하기까지의 시간 (macOS와 같다)
const LOGOUT_COUNTDOWN: u32 = 60;
/// 강제 종료 창이 열려 있는 동안 앱 목록을 다시 읽는 간격. 컴포지터는 3초 넘게 핑에 답하지 않은 앱을 알린다
const REFRESH: Duration = Duration::from_millis(1500);
/// 강제 종료 창의 목록에서 앱 아이콘 크기
const APP_ICON_SIZE: i32 = 24;
/// 강제 종료를 묻는 쪽의 앱 아이콘 크기 (macOS 경고의 아이콘)
const CONFIRM_ICON_SIZE: i32 = 64;
/// 데스크톱 항목이 없는 앱의 아이콘
const FALLBACK_APP_ICON: &str = "application-x-executable";
/// 로그아웃 확인 창의 그림 (로고 메뉴의 아이콘)
const LOGO_ICON: &str = "org.buntos.Logo-symbolic";
/// 레이어 이름이자 창의 접근성 이름
const LOGOUT_NAMESPACE: &str = "bunt-logout";
const FORCE_QUIT_NAMESPACE: &str = "bunt-force-quit";

/// 로고 메뉴의 세션 항목
#[derive(Clone)]
pub struct Session {
    inner: Rc<Inner>,
}

struct Inner {
    app: gtk::Application,
    tr: Rc<Translations>,
    logout: RefCell<Option<gtk::ApplicationWindow>>,
    force_quit: RefCell<Option<Rc<ForceQuit>>>,
}

impl Session {
    /// 로고 메뉴가 부르는 동작(app.force-quit, app.sleep, app.lock-screen, app.log-out)을 단다
    pub fn new(app: &gtk::Application, tr: &Rc<Translations>) -> Self {
        let session = Self {
            inner: Rc::new(Inner {
                app: app.clone(),
                tr: tr.clone(),
                logout: RefCell::default(),
                force_quit: RefCell::default(),
            }),
        };
        let on_force_quit = session.clone();
        let force_quit = gio::ActionEntry::builder("force-quit")
            .activate(move |_, _, _| on_force_quit.show_force_quit())
            .build();
        let sleep = gio::ActionEntry::builder("sleep")
            .activate(|_, _, _| suspend())
            .build();
        let lock_screen = gio::ActionEntry::builder("lock-screen")
            .activate(|_, _, _| call_compositor("Lock", None, |_| {}))
            .build();
        let on_log_out = session.clone();
        let log_out = gio::ActionEntry::builder("log-out")
            .activate(move |_, _, _| on_log_out.confirm_log_out())
            .build();
        app.add_action_entries([force_quit, sleep, lock_screen, log_out]);
        session
    }

    /// 로그아웃 확인 창을 띄운다. 이미 떠 있으면 그대로 둔다. 레이어가 다른 까닭(화면이 빠짐)으로 닫혔으면
    /// 새로 띄운다
    pub fn confirm_log_out(&self) {
        let shown = self
            .inner
            .logout
            .borrow()
            .as_ref()
            .map(|window| window.is_visible());
        if shown == Some(true) {
            return;
        }
        let window = logout_dialog(&self.inner);
        if let Some(old) = self.inner.logout.replace(Some(window)) {
            old.destroy();
        }
    }

    /// 응용 프로그램 강제 종료 창을 띄운다. 이미 떠 있으면 목록을 다시 읽는다
    pub fn show_force_quit(&self) {
        let shown = self.inner.force_quit.borrow().clone();
        if let Some(force_quit) = shown {
            if force_quit.window.is_visible() {
                force_quit.refresh();
                return;
            }
            force_quit.window.destroy();
        }
        let window = ForceQuit::new(&self.inner);
        self.inner.force_quit.replace(Some(window));
    }
}

/// 로고 메뉴의 로그아웃 항목: "<별칭> 로그아웃…"
pub fn log_out_label(tr: &Translations) -> String {
    let mut args = FluentArgs::new();
    args.set("name", display_name());
    tr.get_with("menu-log-out", &args)
}

/// 이 사용자의 별칭: passwd의 GECOS 첫 칸(시스템 설정의 사용자 및 그룹이 AccountsService로 고친다), 없으면 계정
/// 이름. 메뉴를 열 때마다 읽어 바꾼 별칭이 바로 보인다(GLib의 g_get_real_name은 처음 읽은 값을 둔다)
fn display_name() -> String {
    let user = glib::user_name().to_string_lossy().into_owned();
    std::fs::read_to_string("/etc/passwd")
        .ok()
        .and_then(|passwd| alias_in(&passwd, &user))
        .unwrap_or(user)
}

/// passwd 글에서 그 계정의 GECOS 첫 칸
fn alias_in(passwd: &str, user: &str) -> Option<String> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        if fields.next()? != user {
            return None;
        }
        let gecos = fields.nth(3)?;
        let alias = gecos.split(',').next()?.trim();
        (!alias.is_empty()).then(|| alias.to_owned())
    })
}

/// 컴포지터 메서드를 부르고 답을 `done`에 넘긴다
fn call_compositor(
    method: &'static str,
    args: Option<glib::Variant>,
    done: impl FnOnce(Result<glib::Variant, glib::Error>) + 'static,
) {
    glib::spawn_future_local(async move {
        let result = async {
            let bus = gio::bus_get_future(gio::BusType::Session).await?;
            bus.call_future(
                Some(COMPOSITOR),
                COMPOSITOR_PATH,
                COMPOSITOR_INTERFACE,
                method,
                args.as_ref(),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await
        }
        .await;
        if let Err(err) = &result {
            eprintln!(
                "bunt-shell: {COMPOSITOR_INTERFACE}.{method}: {}",
                err.message()
            );
        }
        done(result);
    });
}

/// logind에 잠자기를 요청한다. 허용 여부는 polkit이 정하고, 잠들기 직전에 컴포지터가 화면을 잠근다
fn suspend() {
    glib::spawn_future_local(async {
        let result = async {
            let bus = gio::bus_get_future(gio::BusType::System).await?;
            bus.call_future(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
                "Suspend",
                // interactive: polkit이 인증을 물을 수 있게 한다
                Some(&(true,).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await
        }
        .await;
        if let Err(err) = result {
            eprintln!("bunt-shell: Suspend failed: {}", err.message());
        }
    });
}

/// 셸의 대화 창: 화면 가운데의 overlay 레이어. 키보드를 독점한다(Esc, Enter, 화살표). 어둡게를 따른다
fn dialog_window(app: &gtk::Application, name: &str) -> gtk::ApplicationWindow {
    let window = crate::layer_window(app, name);
    window.set_layer(Layer::Overlay);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    if crate::interface_settings()
        .is_some_and(|settings| settings.string("color-scheme") == "prefer-dark")
    {
        window.add_css_class("dark");
    }
    window
}

/// 여러 줄로 감싸는 글. 띄어 쓴 낱말 가운데서는 줄을 바꾸지 않는다 (디자인 스펙 8장의 AI 접근 요청 창과 같다).
/// 글을 바꿀 때도 keep_words를 거친 마크업으로 넣는다
fn wrapped_label(
    text: &str,
    width: i32,
    justify: gtk::Justification,
    classes: &[&str],
) -> gtk::Label {
    gtk::Label::builder()
        .label(keep_words(text))
        .use_markup(true)
        .justify(justify)
        .xalign(if justify == gtk::Justification::Left {
            0.0
        } else {
            0.5
        })
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(width)
        .css_classes(classes.to_vec())
        .build()
}

/// Esc를 누르면 `close`를 부른다
fn close_on_escape(window: &gtk::ApplicationWindow, close: impl Fn() + 'static) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(move |_, key, _, _| {
        if key != gdk::Key::Escape {
            return glib::Propagation::Proceed;
        }
        close();
        glib::Propagation::Stop
    });
    window.add_controller(keys);
}

/// 로그아웃 확인 창 (macOS): 그림, 질문, 남은 시간, [취소] [로그아웃]
fn logout_dialog(inner: &Rc<Inner>) -> gtk::ApplicationWindow {
    let tr = &inner.tr;
    let window = dialog_window(&inner.app, LOGOUT_NAMESPACE);
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["auth-panel", "logout-panel"])
        .build();
    let icon = gtk::Image::builder()
        .icon_name(LOGO_ICON)
        .pixel_size(36)
        .width_request(64)
        .height_request(64)
        .halign(gtk::Align::Center)
        .css_classes(["auth-icon", "auth-lock-tile", "logout-tile"])
        .build();
    panel.append(&icon);
    panel.append(&wrapped_label(
        &tr.get("logout-title"),
        30,
        gtk::Justification::Center,
        &["auth-title"],
    ));
    let countdown = wrapped_label("", 34, gtk::Justification::Center, &["auth-message"]);
    panel.append(&countdown);
    let buttons = gtk::Box::builder()
        .spacing(8)
        .homogeneous(true)
        .css_classes(["auth-buttons"])
        .build();
    let cancel = gtk::Button::with_label(&tr.get("dialog-cancel"));
    let log_out = gtk::Button::builder()
        .label(tr.get("logout-confirm"))
        .css_classes(["suggested-action"])
        .build();
    buttons.append(&cancel);
    buttons.append(&log_out);
    panel.append(&buttons);
    window.set_child(Some(&panel));
    window.set_default_widget(Some(&log_out));

    let remaining = Rc::new(Cell::new(LOGOUT_COUNTDOWN));
    let show_remaining = {
        let tr = tr.clone();
        let countdown = countdown.clone();
        move |seconds: u32| {
            let mut args = FluentArgs::new();
            args.set("seconds", seconds);
            countdown.set_markup(&keep_words(&tr.get_with("logout-countdown", &args)));
        }
    };
    show_remaining(LOGOUT_COUNTDOWN);

    let close = {
        let inner = Rc::downgrade(inner);
        move || {
            if let Some(inner) = inner.upgrade()
                && let Some(window) = inner.logout.take()
            {
                window.destroy();
            }
        }
    };
    let confirm = {
        let close = close.clone();
        move || {
            close();
            call_compositor("LogOut", None, |_| {});
        }
    };
    let on_click = confirm.clone();
    log_out.connect_clicked(move |_| on_click());
    let on_cancel = close.clone();
    cancel.connect_clicked(move |_| on_cancel());
    close_on_escape(&window, close);
    // 1초마다 남은 시간을 고치고, 다 세면 스스로 로그아웃한다. 창이 닫히면 멈춘다
    let weak = window.downgrade();
    glib::timeout_add_seconds_local(1, move || {
        if weak.upgrade().is_none_or(|window| !window.is_visible()) {
            return glib::ControlFlow::Break;
        }
        let left = remaining.get().saturating_sub(1);
        remaining.set(left);
        if left == 0 {
            confirm();
            return glib::ControlFlow::Break;
        }
        show_remaining(left);
        glib::ControlFlow::Continue
    });
    // 창을 띄우기 전에 포커스를 정한다: Enter는 로그아웃, Esc는 취소다
    GtkWindowExt::set_focus(&window, Some(&log_out));
    window.present();
    window
}

/// 컴포지터가 알린 앱 하나 (GetApps)
#[derive(Debug, Clone, PartialEq)]
struct App {
    pid: u32,
    app_id: String,
    title: String,
    responding: bool,
}

/// 응용 프로그램 강제 종료 창 (macOS): 제목, 설명, 앱 목록, 단축키 안내, [강제 종료]. 고른 앱을 끝내기 전에 묻는다
struct ForceQuit {
    inner: std::rc::Weak<Inner>,
    tr: Rc<Translations>,
    window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    list: gtk::ListBox,
    error: gtk::Label,
    quit: gtk::Button,
    /// 묻는 쪽의 [강제 종료] (기본 단추)
    really: gtk::Button,
    confirm_icon: gtk::Image,
    confirm_title: gtk::Label,
    /// 목록에 보이는 앱 (줄 순서)
    apps: RefCell<Vec<App>>,
    /// 끝내기 전에 묻는 앱
    pending: Cell<Option<u32>>,
    /// 컴포지터에서 목록을 받지 못했다는 오류를 보이는 중
    unavailable: Cell<bool>,
}

impl ForceQuit {
    fn new(inner: &Rc<Inner>) -> Rc<ForceQuit> {
        let tr = inner.tr.clone();
        let window = dialog_window(&inner.app, FORCE_QUIT_NAMESPACE);
        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["auth-panel", "force-quit-panel"])
            .build();
        let header = gtk::CenterBox::builder()
            .css_classes(["force-quit-header"])
            .build();
        let close = gtk::Button::builder()
            .icon_name("window-close-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["force-quit-close"])
            .build();
        close.update_property(&[gtk::accessible::Property::Label(
            &tr.get("force-quit-close"),
        )]);
        header.set_start_widget(Some(&close));
        header.set_center_widget(Some(
            &gtk::Label::builder()
                .label(tr.get("force-quit-title"))
                .css_classes(["auth-title"])
                .build(),
        ));
        panel.append(&header);

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            // 묻는 쪽으로 넘어가도 창 크기가 그대로다. 묻는 내용은 가운데에 둔다
            .vhomogeneous(true)
            .build();
        // 목록
        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        page.append(&wrapped_label(
            &tr.get("force-quit-message"),
            44,
            gtk::Justification::Left,
            &["auth-message", "force-quit-message"],
        ));
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["force-quit-list"])
            .build();
        list.update_property(&[gtk::accessible::Property::Label(
            &tr.get("force-quit-title"),
        )]);
        list.set_placeholder(Some(
            &gtk::Label::builder()
                .label(tr.get("force-quit-empty"))
                .css_classes(["force-quit-empty"])
                .build(),
        ));
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(200)
            .max_content_height(320)
            .propagate_natural_height(true)
            .child(&list)
            .css_classes(["force-quit-scroll"])
            .build();
        page.append(&scrolled);
        let error = wrapped_label("", 44, gtk::Justification::Left, &["auth-info", "error"]);
        error.set_visible(false);
        page.append(&error);
        let footer = gtk::Box::builder()
            .spacing(8)
            .css_classes(["force-quit-footer"])
            .build();
        let hint = wrapped_label(
            &tr.get("force-quit-hint"),
            30,
            gtk::Justification::Left,
            &["force-quit-hint"],
        );
        hint.set_hexpand(true);
        footer.append(&hint);
        let quit = gtk::Button::builder()
            .label(tr.get("force-quit-button"))
            .sensitive(false)
            .valign(gtk::Align::Center)
            .css_classes(["suggested-action"])
            .build();
        let quit_box = gtk::Box::builder().css_classes(["auth-buttons"]).build();
        quit_box.append(&quit);
        footer.append(&quit_box);
        page.append(&footer);
        stack.add_named(&page, Some("list"));

        // 끝내기 전에 묻는다
        let confirm = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .css_classes(["force-quit-confirm"])
            .build();
        // macOS의 경고처럼 끝낼 앱의 아이콘을 보인다
        let confirm_icon = gtk::Image::builder()
            .pixel_size(CONFIRM_ICON_SIZE)
            .halign(gtk::Align::Center)
            .css_classes(["force-quit-confirm-icon"])
            .build();
        confirm.append(&confirm_icon);
        let confirm_title = wrapped_label("", 36, gtk::Justification::Center, &["auth-title"]);
        confirm.append(&confirm_title);
        confirm.append(&wrapped_label(
            &tr.get("force-quit-confirm-message"),
            36,
            gtk::Justification::Center,
            &["auth-message"],
        ));
        let confirm_buttons = gtk::Box::builder()
            .spacing(8)
            .homogeneous(true)
            .css_classes(["auth-buttons"])
            .build();
        let back = gtk::Button::with_label(&tr.get("dialog-cancel"));
        let really = gtk::Button::builder()
            .label(tr.get("force-quit-button"))
            .css_classes(["suggested-action"])
            .build();
        confirm_buttons.append(&back);
        confirm_buttons.append(&really);
        confirm.append(&confirm_buttons);
        stack.add_named(&confirm, Some("confirm"));
        panel.append(&stack);
        window.set_child(Some(&panel));

        let force_quit = Rc::new(ForceQuit {
            inner: Rc::downgrade(inner),
            tr,
            window: window.clone(),
            stack,
            list,
            error,
            quit,
            really: really.clone(),
            confirm_icon,
            confirm_title,
            apps: RefCell::default(),
            pending: Cell::new(None),
            unavailable: Cell::new(false),
        });
        force_quit.connect(&close, &back, &really);
        GtkWindowExt::set_focus(&window, Some(&force_quit.list));
        force_quit.refresh();
        // 열려 있는 동안 목록을 다시 읽는다. 창이 닫히면 멈춘다
        let weak = Rc::downgrade(&force_quit);
        glib::timeout_add_local(REFRESH, move || match weak.upgrade() {
            Some(force_quit) if force_quit.window.is_visible() => {
                force_quit.refresh();
                glib::ControlFlow::Continue
            }
            _ => glib::ControlFlow::Break,
        });
        window.present();
        force_quit
    }

    fn connect(self: &Rc<Self>, close: &gtk::Button, back: &gtk::Button, really: &gtk::Button) {
        let weak = Rc::downgrade(self);
        close.connect_clicked(move |_| {
            if let Some(force_quit) = weak.upgrade() {
                force_quit.close();
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_selected(move |_, row| {
            if let Some(force_quit) = weak.upgrade() {
                force_quit.quit.set_sensitive(row.is_some());
            }
        });
        // Enter나 두 번 누르기도 [강제 종료]와 같다 (끝내기 전에 묻는다)
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, _| {
            if let Some(force_quit) = weak.upgrade() {
                force_quit.ask();
            }
        });
        let weak = Rc::downgrade(self);
        self.quit.connect_clicked(move |_| {
            if let Some(force_quit) = weak.upgrade() {
                force_quit.ask();
            }
        });
        let weak = Rc::downgrade(self);
        back.connect_clicked(move |_| {
            if let Some(force_quit) = weak.upgrade() {
                force_quit.back_to_list();
            }
        });
        let weak = Rc::downgrade(self);
        really.connect_clicked(move |_| {
            if let Some(force_quit) = weak.upgrade() {
                force_quit.kill_pending();
            }
        });
        let weak = Rc::downgrade(self);
        close_on_escape(&self.window, move || {
            if let Some(force_quit) = weak.upgrade() {
                // 묻는 중이면 묻기만 그만둔다
                if force_quit.pending.get().is_some() {
                    force_quit.back_to_list();
                } else {
                    force_quit.close();
                }
            }
        });
    }

    fn close(&self) {
        self.window.destroy();
        if let Some(inner) = self.inner.upgrade() {
            inner.force_quit.replace(None);
        }
    }

    /// 컴포지터에 앱 목록을 다시 묻는다
    fn refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        call_compositor("GetApps", None, move |result| {
            let Some(force_quit) = weak.upgrade() else {
                return;
            };
            match result {
                Ok(reply) => {
                    let apps = reply
                        .child_value(0)
                        .get::<Vec<(u32, String, String, bool)>>()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(pid, app_id, title, responding)| App {
                            pid,
                            app_id,
                            title,
                            responding,
                        })
                        .collect();
                    if force_quit.unavailable.replace(false) {
                        force_quit.error.set_visible(false);
                    }
                    force_quit.show_apps(apps);
                }
                Err(_) => {
                    force_quit.unavailable.set(true);
                    force_quit.show_error(&force_quit.tr.get("force-quit-unavailable"));
                }
            }
        });
    }

    /// 목록을 고친다. 고른 앱은 그대로 고른 채로 두고, 처음에는 맨 앞에 있던 앱을 고른다. 앱은 이름 차례다
    fn show_apps(&self, apps: Vec<App>) {
        let front = apps.first().map(|app| app.pid);
        let first_fill = self.apps.borrow().is_empty();
        let mut apps: Vec<(App, String, Option<gio::Icon>)> = apps
            .into_iter()
            .map(|app| {
                let (name, icon) = app_name_and_icon(&app);
                (app, name, icon)
            })
            .collect();
        apps.sort_by_key(|(_, name, _)| name.to_lowercase());
        let old_index = self
            .list
            .selected_row()
            .and_then(|row| usize::try_from(row.index()).ok());
        let selected = old_index
            .and_then(|index| self.apps.borrow().get(index).map(|app| app.pid))
            .or(front.filter(|_| first_fill));
        let same = {
            let known = self.apps.borrow();
            known.len() == apps.len()
                && known.iter().zip(&apps).all(|(old, (new, _, _))| old == new)
        };
        if same {
            return;
        }
        // 줄을 다시 만들면 포커스를 잃는다. 목록에 포커스가 있었으면 고른 줄로 옮겨 Enter와 화살표가 계속 듣게 한다.
        // ListBox의 focus-child는 줄에 포커스가 있어도 비어 있을 수 있어 창의 포커스로 본다
        let focused = first_fill
            || GtkWindowExt::focus(&self.window)
                .is_some_and(|widget| widget == self.list || widget.is_ancestor(&self.list));
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        for (app, name, icon) in &apps {
            self.list.append(&self.row(app, name, icon.as_ref()));
        }
        let pids: Vec<u32> = apps.iter().map(|(app, _, _)| app.pid).collect();
        self.apps
            .replace(apps.into_iter().map(|(app, _, _)| app).collect());
        let row = selected
            .and_then(|pid| pids.iter().position(|known| *known == pid))
            // 고른 앱이 사라졌으면(강제 종료) 그 자리에 온 앱을 고른다. 키보드로 이어서 고를 수 있다
            .or_else(|| old_index.map(|index| index.min(pids.len().saturating_sub(1))))
            .and_then(|index| i32::try_from(index).ok())
            .and_then(|index| self.list.row_at_index(index));
        self.list.select_row(row.as_ref());
        if focused && let Some(row) = &row {
            row.grab_focus();
        }
        self.quit.set_sensitive(row.is_some());
        // 앞서 강제 종료하지 못했다는 오류는 목록이 바뀌면 지운다
        if !self.unavailable.get() {
            self.error.set_visible(false);
        }
    }

    /// 앱 한 줄: 아이콘, 이름, 응답 없으면 빨간 "(응답 없음)"
    fn row(&self, app: &App, name: &str, icon: Option<&gio::Icon>) -> gtk::ListBoxRow {
        let content = gtk::Box::builder()
            .spacing(8)
            .css_classes(["force-quit-row"])
            .build();
        let image = match icon {
            Some(icon) => gtk::Image::from_gicon(icon),
            None => gtk::Image::from_icon_name(FALLBACK_APP_ICON),
        };
        image.set_pixel_size(APP_ICON_SIZE);
        content.append(&image);
        content.append(
            &gtk::Label::builder()
                .label(name)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .xalign(0.0)
                .build(),
        );
        let mut label = name.to_owned();
        if !app.responding {
            let not_responding = self.tr.get("force-quit-not-responding");
            content.append(
                &gtk::Label::builder()
                    .label(&not_responding)
                    .css_classes(["force-quit-hung"])
                    .build(),
            );
            label = format!("{label} {not_responding}");
        }
        let row = gtk::ListBoxRow::builder().child(&content).build();
        row.update_property(&[gtk::accessible::Property::Label(&label)]);
        row
    }

    fn show_error(&self, text: &str) {
        self.error.set_markup(&keep_words(text));
        self.error.set_visible(true);
    }

    /// 고른 앱을 끝낼지 묻는다 (macOS도 묻는다)
    fn ask(&self) {
        let Some(app) = self
            .list
            .selected_row()
            .and_then(|row| usize::try_from(row.index()).ok())
            .and_then(|index| self.apps.borrow().get(index).cloned())
        else {
            return;
        };
        let (name, icon) = app_name_and_icon(&app);
        match icon {
            Some(icon) => self.confirm_icon.set_from_gicon(&icon),
            None => self.confirm_icon.set_icon_name(Some(FALLBACK_APP_ICON)),
        }
        let mut args = FluentArgs::new();
        args.set("app", name);
        self.confirm_title.set_markup(&keep_words(
            &self.tr.get_with("force-quit-confirm-title", &args),
        ));
        self.pending.set(Some(app.pid));
        self.stack.set_visible_child_name("confirm");
        // Enter는 강제 종료, Esc는 목록으로 돌아간다 (macOS의 확인 시트)
        self.really.grab_focus();
    }

    fn back_to_list(&self) {
        self.pending.set(None);
        self.stack.set_visible_child_name("list");
        self.focus_list();
    }

    /// 목록에 포커스를 돌려준다: 고른 줄, 없으면 목록
    fn focus_list(&self) {
        if let Some(row) = self.list.selected_row() {
            row.grab_focus();
        } else {
            self.list.grab_focus();
        }
    }

    fn kill_pending(self: &Rc<Self>) {
        let Some(pid) = self.pending.take() else {
            return;
        };
        self.stack.set_visible_child_name("list");
        self.focus_list();
        let weak = Rc::downgrade(self);
        call_compositor("ForceQuit", Some((pid,).to_variant()), move |result| {
            let Some(force_quit) = weak.upgrade() else {
                return;
            };
            if let Err(err) = result {
                let mut args = FluentArgs::new();
                args.set("error", err.message().to_owned());
                force_quit.show_error(&force_quit.tr.get_with("force-quit-failed", &args));
            }
            force_quit.refresh();
        });
    }
}

/// 앱의 이름과 아이콘: 데스크톱 항목(앱 ID.desktop), 없으면 창 제목이나 앱 ID
fn app_name_and_icon(app: &App) -> (String, Option<gio::Icon>) {
    let info = (!app.app_id.is_empty())
        .then(|| gio_unix::DesktopAppInfo::new(&format!("{}.desktop", app.app_id)))
        .flatten();
    match info {
        Some(info) => (info.name().to_string(), info.icon()),
        None if !app.title.is_empty() => (app.title.clone(), None),
        None => (app.app_id.clone(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_come_from_the_gecos_field() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n\
                      buntos:x:1000:1000:버니,,,:/home/buntos:/bin/bash\n\
                      plain:x:1001:1001::/home/plain:/bin/sh\n";
        assert_eq!(alias_in(passwd, "buntos").as_deref(), Some("버니"));
        assert_eq!(alias_in(passwd, "plain"), None);
        assert_eq!(alias_in(passwd, "nobody"), None);
        // 이름이 다른 계정의 앞부분만 같으면 맞지 않는다
        assert_eq!(alias_in(passwd, "bunt"), None);
    }
}
