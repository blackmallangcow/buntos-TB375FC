// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-lock: 잠금 화면 (macOS 26의 화면 잠금, ARCHITECTURE 5.2). 컴포지터가 화면을 잠근 뒤 띄우고
//! (comp.toml의 [session] lock), 화면마다 ext-session-lock-v1 잠금 표면을 낸다. 로그인 화면(bunt-greeter)과 같은
//! 모양이다. 사람이 친 암호를 PAM(서비스 bunt-lock)으로 확인하고, 맞으면 잠금을 풀고 끝난다. 잠금을 푸는 길은
//! 이것뿐이다.
//!
//! - 잠금 프로토콜은 컴포지터가 띄운 이 프로세스의 연결에만 보인다. 직접 실행하면 잠그지 못하고 끝난다.
//! - 이 프로세스가 죽어도 화면은 잠긴 채로 남고, 컴포지터가 다시 띄운다.
//! - 창의 제목(접근성 이름)은 bunt-lock이다. 에이전트 데몬은 이 창을 읽지도 조작하지도 않는다.
//!
//! 개발할 때는 `bunt-lock --preview`로 잠그지 않고 보통 창으로 화면만 볼 수 있다(암호 확인은 진짜로 한다)

#[path = "../accounts.rs"]
mod accounts;
#[path = "../avatar.rs"]
mod avatar;
#[path = "../background.rs"]
mod background;
#[path = "../clock.rs"]
mod clock;
mod pam;
mod screen;
mod session_lock;

use std::rc::Rc;

use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{screen::LockScreen, session_lock::SessionLock};

const APP_ID: &str = "org.buntos.Lock";
/// 창 제목이자 접근성 이름 (agent/src/atspi.rs의 LOCK_WINDOW)
const WINDOW_TITLE: &str = "bunt-lock";
/// 로그인 화면과 같은 모양: 공통 디자인 토큰(theme/tokens.css) 다음에 로그인 화면의 CSS
const STYLE: &str = concat!(
    include_str!("../../../theme/tokens.css"),
    include_str!("../../data/style.css")
);
/// 로그인 화면과 문구(시각 형식, 암호 칸)를 함께 쓴다
const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../../i18n/ko/bunt-greeter.ftl")),
    ("en", include_str!("../../../i18n/en/bunt-greeter.ftl")),
];

fn main() -> glib::ExitCode {
    deny_ptrace();
    without_portals();
    let mut args = std::env::args();
    let program = args.next().unwrap_or_else(|| "bunt-lock".into());
    let preview = match args.next().as_deref() {
        None => false,
        Some("--preview") => true,
        _ => {
            eprintln!("usage: bunt-lock [--preview]");
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
        // 처음 한 번만 만든다
        if app.active_window().is_some() {
            return;
        }
        if preview {
            LockScreen::new(app, tr.clone(), None).add_screen(None);
        } else {
            lock(app, &tr);
        }
    });
    // 인자는 위에서 읽었으므로 GApplication에는 넘기지 않는다
    app.run_with_args(&[program])
}

/// 화면을 잠그고 화면마다 창을 낸다. 잠금이 풀리면 끝난다
fn lock(app: &gtk::Application, tr: &Rc<Translations>) {
    let lock = match SessionLock::new() {
        Ok(lock) => Rc::new(lock),
        Err(err) => {
            eprintln!("bunt-lock: {err}");
            std::process::exit(1);
        }
    };
    // 창이 생기기 전에도, 잠금이 풀려 창이 닫힌 뒤에도 끝내는 것은 아래의 unlocked다
    std::mem::forget(app.hold());
    let screen = LockScreen::new(app, tr.clone(), Some(lock.clone()));
    let on_monitor = screen.clone();
    lock.connect_monitor(move |monitor| on_monitor.add_screen(Some(monitor)));
    let on_locked = screen.clone();
    lock.connect_locked(move || on_locked.locked());
    lock.connect_failed(|| {
        // 컴포지터가 받지 않았다(다른 잠금 화면이 화면을 쥐고 있다). 화면은 컴포지터가 잠근 채로 둔다
        eprintln!("bunt-lock: the compositor refused the lock");
        std::process::exit(1);
    });
    let app = app.clone();
    lock.connect_unlocked(move || {
        // 잠금을 푸는 요청이 컴포지터에 닿은 뒤 끝난다
        if let Some(display) = gtk::gdk::Display::default() {
            display.sync();
        }
        eprintln!("bunt-lock: unlocked");
        app.quit();
    });
    if !lock.lock() {
        eprintln!("bunt-lock: could not start locking the screen");
        std::process::exit(1);
    }
}

/// 같은 사용자로 도는 다른 프로그램(AI 도구 포함)이 이 프로세스에 붙거나(ptrace) 메모리(/proc/<pid>/mem)를 읽지
/// 못하게 한다. 붙을 수 있으면 친 암호를 읽거나, 암호 없이 잠금을 풀게 만들 수 있다. ssh-agent, gpg-agent와 같은
/// 방법이다(프로세스는 자기 /proc을 그대로 읽는다). 코어 덤프도 남기지 않는다
fn deny_ptrace() {
    // 안전: 정수 인자만 넘기는 시스템 호출이다
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        eprintln!(
            "bunt-lock: cannot protect the process from ptrace: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// 잠금 화면은 데스크톱 포털(xdg-desktop-portal)을 쓰지 않는다. GTK는 뜨면서 포털을 부르고 답을 25초씩 세 번까지
/// 기다리는데, 포털은 사용자 세션의 D-Bus 서비스라 늦거나 멈출 수 있고 같은 사용자의 다른 프로그램이 그 이름을
/// 차지할 수도 있다. 잠금 화면은 그것과 상관없이 떠야 한다(2026-11-17 TB375FC: 포털이 멈춘 동안 잠금 화면이
/// 91초 동안 뜨지 못했다). 포털에서 받던 것은 GTK 설정(글꼴, 밝게·어둡게, 커서 크기)뿐인데, 이 화면은 로그인
/// 화면처럼 모양을 CSS로 정하고 포인터는 컴포지터가 그리므로 화면이 달라지지 않는다. 로그인 화면도 같은 값으로
/// 뜬다(greeter-session). GTK가 뜨기 전에 불러야 한다
fn without_portals() {
    let flags = gdk_debug_without_portals(std::env::var("GDK_DEBUG").ok().as_deref());
    // 안전: main의 첫머리라 환경을 읽거나 쓰는 다른 스레드가 아직 없다
    unsafe { std::env::set_var("GDK_DEBUG", flags) };
}

/// 지금의 GDK_DEBUG 값에 포털 끄기를 더한 값. 개발할 때 준 다른 값은 그대로 둔다
fn gdk_debug_without_portals(current: Option<&str>) -> String {
    const NO_PORTALS: &str = "no-portals";
    match current.map(str::trim) {
        None | Some("") => NO_PORTALS.to_owned(),
        Some(flags)
            if flags
                .split([',', ':', ';', ' ', '\t'])
                .any(|flag| flag == NO_PORTALS) =>
        {
            flags.to_owned()
        }
        Some(flags) => format!("{flags},{NO_PORTALS}"),
    }
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
    fn every_language_has_the_same_messages() {
        bunt_i18n::check_same_messages(TRANSLATIONS).unwrap();
    }

    #[test]
    fn portals_are_turned_off_and_other_debug_flags_stay() {
        assert_eq!(gdk_debug_without_portals(None), "no-portals");
        assert_eq!(gdk_debug_without_portals(Some("")), "no-portals");
        assert_eq!(
            gdk_debug_without_portals(Some("frames")),
            "frames,no-portals"
        );
        assert_eq!(
            gdk_debug_without_portals(Some("frames:no-portals")),
            "frames:no-portals"
        );
        // 포털을 억지로 쓰게 하는 값(portals)은 다른 값이다
        assert_eq!(
            gdk_debug_without_portals(Some("portals")),
            "portals,no-portals"
        );
    }

    #[test]
    fn other_programs_cannot_attach() {
        deny_ptrace();
        // 안전: 인자 없는 조회다
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) }, 0);
    }

    #[test]
    fn lock_messages_exist_in_every_language() {
        for (language, source) in TRANSLATIONS {
            let tr = Translations::from_source(language, source);
            for id in ["lock-retry-seconds", "lock-retry-minutes", "lock-failed"] {
                assert_ne!(tr.get(id), id, "{language} has no {id}");
            }
        }
    }
}
