// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-greeter: 로그인 화면과 처음 켤 때의 계정 만들기. greetd가 로그인 화면 사용자(_greetd)로
//! 띄운 컴포지터(bunt-comp, /etc/buntos/greeter.toml)의 셸로 뜬다. 정규 계정이 없으면 macOS 설정
//! 지원처럼 계정을 만들고(buntd-setup) 바로 그 계정으로 로그인한다. 계정이 있으면 macOS 로그인
//! 창처럼 흐린 배경화면 위에 시각, 사진, 이름, 암호 칸을 보인다. 로그인은 greetd IPC로 한다.
//!
//! 개발할 때는 `bunt-greeter --preview login|create|signing-in`로 greetd 없이 화면만 볼 수 있다

mod accounts;
mod avatar;
mod background;
mod clock;
mod create;
mod login;
mod startup_sound;

use std::{cell::RefCell, rc::Rc, time::Duration};

use bunt_greeter::account;
use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

const APP_ID: &str = "org.buntos.Greeter";
/// 공통 디자인 토큰(theme/tokens.css) 다음에 이 앱의 CSS
const STYLE: &str = concat!(
    include_str!("../../theme/tokens.css"),
    include_str!("../data/style.css")
);
const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../i18n/ko/bunt-greeter.ftl")),
    ("en", include_str!("../../i18n/en/bunt-greeter.ftl")),
];
/// 로그인한 뒤 greetd가 띄우는 사용자 세션
const SESSION_COMMAND: &str = "/usr/libexec/buntos/session";
/// 사용자 세션에 넘기는 환경. pam_systemd가 세션 종류를 logind에 알린다.
/// XDG_CURRENT_DESKTOP은 앞이 우선인 목록이다. buntOS가 먼저라 포털은 buntos-portals.conf를 쓴다. 뒤의
/// GNOME은 Chromium·Electron 앱(Claude 데스크톱 앱 등)이 비밀 저장소로 GNOME 키링(libsecret)을 고르게
/// 한다. 그 앱들은 이 목록에 아는 데스크톱이 없으면 키링이 있어도 평문(basic_text)에 저장한다. 우분투의
/// "ubuntu:GNOME"과 같은 방식이다
const SESSION_ENV: [&str; 3] = [
    "XDG_SESSION_TYPE=wayland",
    "XDG_SESSION_DESKTOP=buntos",
    "XDG_CURRENT_DESKTOP=buntOS:GNOME",
];

/// 보일 화면
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Login,
    Create,
    /// 로그인에 성공한 뒤의 로딩 막대 (--preview로만 바로 본다)
    SigningIn,
}

/// 로그인에 성공한 뒤 로딩 막대를 채우며 기다리는 시간. 사용자 세션은 로그인 화면이 끝나야 뜬다(greetd)
const LOADING_TIME: Duration = Duration::from_millis(450);
/// 로그인 화면이 끝날 때 로딩 막대가 찬 비율. 마지막 화면은 세션이 뜰 때까지 그대로 남는다
const LOADING_FILL: f64 = 0.6;

/// 로딩 막대가 `elapsed` 뒤에 찬 비율. 처음에 빠르고 끝에서 느려진다
fn loading_fraction(elapsed: Duration) -> f64 {
    let progress = (elapsed.as_secs_f64() / LOADING_TIME.as_secs_f64()).min(1.0);
    LOADING_FILL * (1.0 - (1.0 - progress).powi(2))
}

/// 로그인 화면이 함께 쓰는 것
pub struct Shared {
    pub tr: Rc<Translations>,
    /// greetd 없이 화면만 보는 중 (--preview)
    pub preview: bool,
    stack: gtk::Stack,
    window: gtk::ApplicationWindow,
    /// 계정을 만든 뒤 로그인 화면을 다시 만들 때 쓴다
    login: RefCell<Option<gtk::Widget>>,
    /// 화면 키보드. 로그인에 성공하면 내린다
    keyboard: bunt_keyboard::Keyboard,
}

impl Shared {
    /// 정규 계정을 다시 읽어 로그인 화면으로 간다 (예: 계정이 이미 있다는 답을 받았을 때)
    pub fn show_login(self: &Rc<Self>) {
        if let Some(old) = self.login.take() {
            self.stack.remove(&old);
        }
        let users = account::read_regular_users().unwrap_or_default();
        let view = login::build(self, users);
        self.stack.add_named(&view, Some("login"));
        self.stack.set_visible_child(&view);
        self.login.replace(Some(view));
    }

    /// 다른 화면으로 바꾼다
    pub fn show_view(&self, view: &impl IsA<gtk::Widget>) {
        self.stack.add_child(view);
        self.stack.set_visible_child(view);
    }

    /// 로그인에 성공하면 화면 키보드를 내리고 로딩 막대를 잠깐 채운 뒤 끝난다. 셸이 끝나면 로그인 화면의
    /// 컴포지터도 끝나고(greeter.toml의 exit-with-shell), greetd가 예약한 사용자 세션을 연다. 세션이 첫
    /// 화면을 낼 때까지 이 마지막 화면이 남는다
    pub fn finish(&self, loading: &gtk::ProgressBar) {
        self.keyboard.conceal();
        let start = std::cell::Cell::new(None);
        loading.add_tick_callback(move |bar, clock| {
            let now = clock.frame_time();
            let begun = start.get().unwrap_or(now);
            start.set(Some(begun));
            let elapsed = Duration::from_micros((now - begun).max(0) as u64);
            bar.set_fraction(loading_fraction(elapsed));
            if elapsed < LOADING_TIME {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        if self.preview {
            return;
        }
        let app = self.window.application();
        glib::timeout_add_local_once(LOADING_TIME, move || {
            eprintln!("bunt-greeter: session scheduled, exiting");
            if let Some(app) = app {
                app.quit();
            }
        });
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.window
    }
}

/// 사용자 세션 명령과 환경 (greetd start_session)
pub fn session() -> (Vec<String>, Vec<String>) {
    (
        vec![SESSION_COMMAND.to_owned()],
        SESSION_ENV
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
    )
}

fn main() -> glib::ExitCode {
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "bunt-greeter".into());
    let preview = match (args.next().as_deref(), args.next().as_deref()) {
        (None, _) => None,
        (Some("--preview"), Some("login")) => Some(View::Login),
        (Some("--preview"), Some("create")) => Some(View::Create),
        (Some("--preview"), Some("signing-in")) => Some(View::SigningIn),
        _ => {
            eprintln!("usage: bunt-greeter [--preview login|create|signing-in]");
            return glib::ExitCode::from(2);
        }
    };

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|_| load_style());
    let tr = Rc::new(Translations::load(TRANSLATIONS));
    app.connect_activate(move |app| {
        // 처음 한 번만 창을 만든다
        if app.active_window().is_some() {
            return;
        }
        build(app, &tr, preview);
    });
    // 인자는 위에서 읽었으므로 GApplication에는 넘기지 않는다
    app.run_with_args(&[program])
}

fn build(app: &gtk::Application, tr: &Rc<Translations>, preview: Option<View>) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(tr.get("app-title"))
        .css_classes(["greeter"])
        .build();
    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    titlebar.set_visible(false);
    window.set_titlebar(Some(&titlebar));
    // 로그인 화면은 닫히지 않는다 (Alt+F4)
    window.connect_close_request(|_| glib::Propagation::Stop);

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(200)
        .hexpand(true)
        .vexpand(true)
        .build();
    let overlay = gtk::Overlay::builder()
        .child(&background::picture())
        .build();
    overlay.add_overlay(
        &gtk::Box::builder()
            .css_classes(["greeter-dim"])
            .can_target(false)
            .build(),
    );
    // 터치 모드에서는 화면 키보드가 아래에서 나오고 내용이 위로 올라간다. 키는 로그인 화면의 컴포지터에 실제 키보드처럼
    // 넣어 입력기가 별칭의 한글을 조합한다(bunt-greeter가 그 컴포지터의 셸이라 셸 전용 프로토콜을 본다). 그 프로토콜이
    // 없으면(미리 보기 창처럼 다른 컴포지터 위) 칸에 바로 넣는다
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.append(&stack);
    overlay.add_overlay(&column);
    window.set_child(Some(&overlay));
    let keys: Rc<dyn bunt_keyboard::KeySink> =
        match bunt_keyboard::CompositorKeyboard::connect(&WidgetExt::display(&window)) {
            Some(compositor) => Rc::new(compositor),
            None => Rc::new(bunt_keyboard::EditableKeys::new(&window)),
        };
    let keyboard = bunt_keyboard::embed(&window, &overlay, &column, keys);
    // 배열은 fcitx5의 지금 입력 소스를 따른다(암호 칸에서는 fcitx5가 영문 배열을 쓴다). 지구본은 다음 입력 소스다
    let weak = keyboard.downgrade();
    let fcitx = bunt_keyboard::fcitx::Fcitx::start(move |script, globe| {
        if let Some(keyboard) = weak.upgrade() {
            keyboard.set_script(script);
            keyboard.set_globe(globe);
        }
    });
    keyboard.connect_next_source(move || fcitx.next());

    let shared = Rc::new(Shared {
        tr: tr.clone(),
        preview: preview.is_some(),
        stack: stack.clone(),
        window: window.clone(),
        login: RefCell::default(),
        keyboard: keyboard.clone(),
    });
    let view = match preview {
        Some(view) => view,
        None => match account::read_regular_users() {
            Ok(users) if users.is_empty() => View::Create,
            Ok(_) => View::Login,
            Err(err) => {
                eprintln!("bunt-greeter: {}: {err}", account::PASSWD);
                View::Login
            }
        },
    };
    match view {
        View::Login => shared.show_login(),
        View::Create => {
            let view = create::build(&shared);
            stack.add_named(&view, Some("create"));
            stack.set_visible_child(&view);
        }
        View::SigningIn => login::show_signing_in(&shared, "buntOS"),
    }

    if preview.is_none() {
        window.fullscreen();
    } else {
        window.set_default_size(1280, 800);
    }
    // 시험(tests/e2e, tools/image/vmctl.sh)이 이 줄을 보고 암호를 친다. 컴포지터가 창에 키보드를 준
    // 뒤(활성 창) 입력 칸이 포커스를 받을 틈을 두고 한 번 알린다. 활성으로 알려 오지 않아도 5초 뒤에는 알린다.
    // 이때 화면이 다 떴으므로 부팅 뒤 처음이면 시동음도 낸다
    let announced = Rc::new(std::cell::Cell::new(false));
    let announce = move || {
        if announced.replace(true) {
            return;
        }
        if preview.is_none() {
            startup_sound::play_once();
        }
        eprintln!(
            "bunt-greeter: ready ({})",
            match view {
                View::Login => "login",
                View::Create => "create",
                View::SigningIn => "signing-in",
            }
        );
    };
    let on_active = announce.clone();
    window.connect_is_active_notify(move |window| {
        if window.is_active() {
            let announce = on_active.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(300), announce);
        }
    });
    glib::timeout_add_local_once(std::time::Duration::from_secs(5), announce);
    window.present();
}

fn load_style() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_loading_bar_fills_quickly_and_stops_at_its_end() {
        assert_eq!(loading_fraction(Duration::ZERO), 0.0);
        let half = loading_fraction(LOADING_TIME / 2);
        assert!(half > LOADING_FILL / 2.0 && half < LOADING_FILL);
        assert_eq!(loading_fraction(LOADING_TIME), LOADING_FILL);
        assert_eq!(loading_fraction(LOADING_TIME * 3), LOADING_FILL);
    }

    #[test]
    fn every_language_has_the_same_messages() {
        bunt_i18n::check_same_messages(TRANSLATIONS).unwrap();
    }
}
