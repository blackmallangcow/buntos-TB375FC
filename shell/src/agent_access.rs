// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AI 접근 요청 창 (ARCHITECTURE 6.5). 허용하지 않은 AI가 에이전트 데몬에 붙으면 사용자에게 묻는다.
//! macOS가 손쉬운 사용 권한이 없는 앱에 "이 컴퓨터를 제어하려고 합니다"라고 묻고 [시스템 설정 열기]로
//! 바로 보내는 경고를 따른다.
//!
//! - 데몬은 거절한 AI를 요청 기록에 남긴다. 셸은 그 파일을 지켜보다가, 방금 붙었고 아직 묻지 않았고
//!   정책 목록에 없는 AI가 있으면 한 번 묻는다 ([`bunt_access::request_to_ask`]).
//! - [시스템 설정 열기]는 AI 에이전트 페이지의 추가 시트를 그 AI를 고른 채로 연다
//!   (`bunt-settings agent --add-client <실행 파일> [--script <스크립트>]`). 사용자가 [추가]를 누르면
//!   인증 창이 관리자 암호를 묻는다. [거부]와 Esc는 창만 닫는다. 어느 쪽이든 같은 프로그램은 다시 묻지
//!   않는다.
//! - 창은 인증 창과 같은 레이어 이름을 써서 에이전트가 건드리지 못한다. AI가 스스로 답할 수 없다.
//! - AI가 띄우는 창이라 키보드는 창을 눌렀을 때만 받는다. 다른 앱에 글을 치던 사용자가 Return으로
//!   잘못 고르지 않게 한다.

use std::{
    cell::{Cell, RefCell},
    ffi::OsStr,
    fs,
    rc::Rc,
    time::Duration,
};

use bunt_access::{Policy, Request};
use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

use crate::polkit_agent;

/// 시스템 설정 AI 에이전트 페이지의 아이콘. 데스크톱 앱이 아닌 AI(CLI)는 이것을 분홍 타일에 그린다
const AGENT_ICON: &str = "org.buntos.Agent-symbolic";
const SETTINGS: &str = "bunt-settings";
/// 데몬은 임시 파일에 쓰고 이름을 바꾸므로, 바뀌었다는 알림을 모아서 한 번 읽는다
const SETTLE: Duration = Duration::from_millis(200);
/// 제목과 설명의 줄 길이 (글자 수)
const TEXT_WIDTH: i32 = 38;

pub fn start(app: &gtk::Application, tr: &Rc<Translations>) {
    let Some(path) = bunt_access::requests_path() else {
        return;
    };
    // 디렉터리가 있어야 파일이 처음 생기는 것도 바로 본다
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let access = Rc::new(AgentAccess {
        app: app.clone(),
        tr: tr.clone(),
        window: RefCell::default(),
        scheduled: Cell::new(false),
        monitor: RefCell::default(),
    });
    match gio::File::for_path(&path)
        .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
    {
        Ok(monitor) => {
            // 처리기가 강한 참조를 들고 monitor가 처리기를 들어 셸이 끝날 때까지 산다
            let watcher = access.clone();
            monitor.connect_changed(move |_, _, _, _| watcher.schedule());
            access.monitor.replace(Some(monitor));
        }
        Err(err) => eprintln!("bunt-shell: cannot watch AI agent requests: {err}"),
    }
    // 셸이 뜨기 바로 전에 거절된 AI도 묻는다
    access.schedule();
}

struct AgentAccess {
    app: gtk::Application,
    tr: Rc<Translations>,
    /// 떠 있는 요청 창. 한 번에 하나만 띄운다
    window: RefCell<Option<gtk::ApplicationWindow>>,
    scheduled: Cell<bool>,
    monitor: RefCell<Option<gio::FileMonitor>>,
}

impl AgentAccess {
    fn schedule(self: &Rc<Self>) {
        if self.scheduled.replace(true) {
            return;
        }
        let access = self.clone();
        glib::timeout_add_local_once(SETTLE, move || {
            access.scheduled.set(false);
            access.check();
        });
    }

    fn check(self: &Rc<Self>) {
        if self.window.borrow().is_some() {
            return;
        }
        let Some(request) = bunt_access::request_to_ask(&Policy::load()) else {
            return;
        };
        // 창을 띄우기 전에 남겨, 그사이 파일이 다시 바뀌어도 두 번 묻지 않는다
        bunt_access::mark_asked(&request.exe, request.script.as_deref());
        self.show(request);
    }

    fn show(self: &Rc<Self>, request: Request) {
        let tr = &self.tr;
        let window = crate::layer_window(&self.app, polkit_agent::NAMESPACE);
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        if crate::interface_settings()
            .is_some_and(|settings| settings.string("color-scheme") == "prefer-dark")
        {
            window.add_css_class("dark");
        }

        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["auth-panel"])
            .build();
        panel.append(&icon(&request));
        let mut args = FluentArgs::new();
        args.set("name", request.name.clone());
        // 인증 창보다 조금 넓혀, 흔한 AI 이름이면 제목이 한 줄에 들어가게 한다
        panel.append(&label(
            &tr.get_with("agent-access-title", &args),
            "auth-title",
            TEXT_WIDTH,
        ));
        panel.append(&label(
            &tr.get("agent-access-message"),
            "auth-message",
            TEXT_WIDTH,
        ));
        let buttons = gtk::Box::builder()
            .spacing(8)
            .homogeneous(true)
            .css_classes(["auth-buttons"])
            .build();
        let deny = gtk::Button::with_label(&tr.get("agent-access-deny"));
        let open = gtk::Button::builder()
            .label(tr.get("agent-access-open"))
            .css_classes(["suggested-action"])
            .build();
        buttons.append(&deny);
        buttons.append(&open);
        panel.append(&buttons);
        window.set_child(Some(&panel));
        window.set_default_widget(Some(&open));

        let access = Rc::downgrade(self);
        deny.connect_clicked(move |_| {
            if let Some(access) = access.upgrade() {
                access.close();
            }
        });
        let access = Rc::downgrade(self);
        open.connect_clicked(move |_| {
            open_settings(&request);
            if let Some(access) = access.upgrade() {
                access.close();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let access = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if let Some(access) = access.upgrade() {
                access.close();
            }
            glib::Propagation::Stop
        });
        window.add_controller(keys);
        window.present();
        self.window.replace(Some(window));
    }

    fn close(self: &Rc<Self>) {
        if let Some(window) = self.window.take() {
            window.destroy();
        }
        // 그사이 거절된 다른 AI를 묻는다
        self.schedule();
    }
}

/// AI가 데스크톱 앱이면 그 앱 아이콘에 자물쇠를 붙이고, 아니면 분홍 타일에 AI 에이전트 아이콘을 그린다
fn icon(request: &Request) -> gtk::Widget {
    let app = bunt_access::desktop_app(&request.exe, request.script.as_deref())
        .and_then(|app| gio_unix::DesktopAppInfo::new(&format!("{}.desktop", app.id)))
        .and_then(|info| info.icon())
        .map(|icon| (request.name.clone(), Some(icon)));
    if app.is_some() {
        return polkit_agent::requester_icon(app.as_ref(), "");
    }
    let image = gtk::Image::from_icon_name(AGENT_ICON);
    image.set_pixel_size(34);
    image.set_hexpand(true);
    image.set_vexpand(true);
    let tile = gtk::Box::builder()
        .width_request(64)
        .height_request(64)
        .css_classes(["auth-lock-tile", "agent-access-tile"])
        .build();
    tile.append(&image);
    let frame = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .css_classes(["auth-icon"])
        .build();
    frame.append(&tile);
    frame.upcast()
}

fn label(text: &str, class: &str, width: i32) -> gtk::Label {
    gtk::Label::builder()
        .label(keep_words(text))
        .use_markup(true)
        .justify(gtk::Justification::Center)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(width)
        .css_classes([class])
        .build()
}

/// 띄어 쓴 낱말마다 줄 바꿈을 막은 Pango 마크업 (CSS의 word-break: keep-all). Pango는 한글 음절
/// 사이마다 줄을 바꿀 수 있다고 보아 "입력|할"처럼 낱말 가운데에서 줄을 바꾸기 때문이다
fn keep_words(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            format!(
                "<span allow_breaks=\"false\">{}</span>",
                glib::markup_escape_text(word)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// AI 에이전트 페이지의 추가 시트를 이 AI를 고른 채로 연다. 시스템 설정이 이미 떠 있으면 그 창이
/// 인자를 받는다
fn open_settings(request: &Request) {
    let mut argv = vec![
        OsStr::new(SETTINGS),
        OsStr::new("agent"),
        OsStr::new("--add-client"),
        request.exe.as_os_str(),
    ];
    if let Some(script) = &request.script {
        argv.push(OsStr::new("--script"));
        argv.push(script.as_os_str());
    }
    if let Err(err) = gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
        eprintln!("bunt-shell: cannot open System Settings: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_each_word_together_and_escapes_markup() {
        assert_eq!(
            keep_words("글을 <입력>할"),
            "<span allow_breaks=\"false\">글을</span> \
             <span allow_breaks=\"false\">&lt;입력&gt;할</span>"
        );
    }
}
