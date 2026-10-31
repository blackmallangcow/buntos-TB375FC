// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드 창 (bunt-osk, 설계안 5.1·5.2절). iPadOS 키보드를 화면 맨 아래에 띄운다. 키보드는 공유 크레이트
//! (keyboard/)가 그리고, 이 모듈은 창과 뜨고 숨는 규칙을 맡는다.
//!
//! - 창: overlay 레이어로 아래·왼쪽·오른쪽에 붙고, 배타 영역 없이(-1) Dock 위를 덮는다. 키보드 포커스를 받지 않는다
//!   (포커스를 가진 글 칸이 포커스를 지킨다). 판 위에는 글자 미리 보기가 뜰 투명한 띠가 있다. 창의 입력 영역은 판뿐이라
//!   띠를 누르면 아래 창이 받는다. 컴포지터는 입력 영역 위까지만 창 자리로 준다(comp/src/osk.rs).
//! - 뜬다: 입력 모드가 터치이고, 컴포지터가 키보드 포커스를 가진 앱이 글 입력을 켰다고 알릴 때(TextInputChanged(true),
//!   켠 칸을 다시 눌렀을 때도 온다).
//! - 숨는다: 글 입력이 꺼지면 0.25초 뒤(칸을 옮기면 꺼졌다 바로 켜진다), 포인터 모드가 되거나 키보드 가리기 키를 누르면
//!   바로. 가리기 키로 숨기면 칸을 다시 눌러야 뜬다.
//! - 키: 컴포지터의 셸 전용 메서드로 실제 키보드처럼 넣어 입력기(fcitx5)가 한글을 조합한다.
//! - 배열은 메뉴 막대의 입력 소스를 따른다(한글이면 2벌식, 그 밖은 QWERTY). 지구본 키는 다음 입력 소스다.
//! - 셸의 다른 창(관리자 인증 창, Wi-Fi 암호 창, 모든 앱)은 키보드 위로 비킨다([`avoid_keyboard`]).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use bunt_keyboard::{
    CompositorKeys, Keyboard, Room,
    fcitx::script_of,
    mode,
    rules::{Decision, Rules},
};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

use crate::input_source::InputSources;

/// 레이어 이름이자 창의 접근성 이름 (comp/src/osk.rs의 KEYBOARD_NAMESPACE, agent/src/atspi.rs의 KEYBOARD_WINDOW)
const NAMESPACE: &str = "bunt-osk";
const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
/// 글 입력이 꺼진 뒤 숨기까지 기다리는 시간. 다른 칸으로 옮기면 꺼졌다가 바로 켜진다
const HIDE_DELAY: Duration = Duration::from_millis(250);
/// 키보드 위로 비킨 창과 키보드 사이의 틈
const DIALOG_GAP: i32 = 16;

struct Inner {
    window: gtk::ApplicationWindow,
    keyboard: Keyboard,
    sources: InputSources,
    rules: Cell<Rules>,
    hide_timer: RefCell<Option<glib::SourceId>>,
}

#[derive(Clone)]
pub struct Osk(Rc<Inner>);

impl Osk {
    pub fn start(app: &gtk::Application, sources: &InputSources) -> Self {
        let keyboard = Keyboard::new(Rc::new(CompositorKeys::new()), Room::Window);
        // layer_window가 키보드를 받지 않게(KeyboardMode::None) 둔다
        let window = crate::layer_window(app, NAMESPACE);
        window.set_layer(Layer::Overlay);
        for edge in [Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        window.set_exclusive_zone(-1);
        window.set_resizable(false);
        window.set_child(Some(keyboard.widget()));
        let osk = Self(Rc::new(Inner {
            window,
            keyboard,
            sources: sources.clone(),
            rules: Cell::default(),
            hide_timer: RefCell::default(),
        }));
        osk.follow_sources();
        osk.connect_keyboard();
        osk.clip_input();
        // 입력 모드 구독도 화면 키보드를 붙든다(셸이 끝날 때까지 받는다)
        let on_mode = osk.clone();
        mode::watch(move |mode| on_mode.decide(|rules| rules.mode(mode)));
        osk.watch_text_input();
        osk
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    /// 배열은 지금 입력 소스를, 지구본 키는 입력 소스가 둘 이상인지를 따른다
    fn follow_sources(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.sources.subscribe(move |sources| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if let Some(current) = sources.current() {
                inner.keyboard.set_script(script_of(&current.id));
            }
            inner.keyboard.set_globe(sources.sources().len() > 1);
        });
    }

    fn connect_keyboard(&self) {
        let sources = self.0.sources.clone();
        self.0.keyboard.connect_next_source(move || {
            if let Some(next) = next_source(&sources) {
                sources.select(&next);
            }
        });
        let weak = Rc::downgrade(&self.0);
        self.0.keyboard.connect_hide(move || {
            if let Some(inner) = weak.upgrade() {
                Osk(inner).decide(Rules::hide_key);
            }
        });
        // 다 들어간 뒤에 창을 내린다. 그사이 다시 뜨게 됐으면 내리지 않는다
        let weak = Rc::downgrade(&self.0);
        self.0.keyboard.connect_concealed(move || {
            if let Some(inner) = weak.upgrade()
                && !inner.keyboard.is_revealed()
            {
                inner.window.set_visible(false);
            }
        });
    }

    /// 창의 입력 영역을 판으로 줄인다. 키보드가 들어가는 동안은 비워, 컴포지터가 창 자리를 바로 돌려준다
    fn clip_input(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.window.connect_realize(move |window| {
            let Some(surface) = window.surface() else {
                return;
            };
            let weak = weak.clone();
            // GTK가 크기를 바꾸면 입력 영역을 다시 정한다
            surface.connect_layout(move |_, _, _| {
                if let Some(inner) = weak.upgrade() {
                    Osk(inner).apply_input_region();
                }
            });
        });
    }

    fn apply_input_region(&self) {
        let inner = &self.0;
        let Some(surface) = inner.window.surface() else {
            return;
        };
        let region = if inner.keyboard.is_revealed() {
            let (room, panel) = inner.keyboard.heights();
            let (dx, dy) = inner.window.surface_transform();
            gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(
                dx.floor() as i32,
                dy.floor() as i32 + room,
                inner.window.width(),
                panel,
            ))
        } else {
            gtk::cairo::Region::create()
        };
        surface.set_input_region(Some(&region));
    }

    /// 컴포지터가 알리는 글 입력 상태를 받는다. 신호는 WatchTextInput을 부른 셸에만 오므로, 컴포지터의 버스 이름이
    /// (다시) 생길 때마다 부른다
    fn watch_text_input(&self) {
        // 이 처리기와 아래의 신호 처리기가 화면 키보드를 셸이 끝날 때까지 붙든다(main은 Osk를 들고 있지 않는다)
        let osk = self.clone();
        gio::DBusProxy::for_bus(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES
                | gio::DBusProxyFlags::DO_NOT_CONNECT_SIGNALS,
            None,
            NAME,
            PATH,
            INTERFACE,
            gio::Cancellable::NONE,
            move |result| {
                let proxy = match result {
                    Ok(proxy) => proxy,
                    Err(err) => {
                        eprintln!("bunt-shell: {NAME}: {err}");
                        return;
                    }
                };
                let on_signal = osk.clone();
                let subscription = proxy.connection().subscribe_to_signal(
                    Some(NAME),
                    Some(INTERFACE),
                    Some("TextInputChanged"),
                    Some(PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let Some((active,)) = signal.parameters.get::<(bool,)>() {
                            on_signal.decide(|rules| rules.text_input(active));
                        }
                    },
                );
                // 셸이 끝날 때까지 받는다
                std::mem::forget(subscription);
                let weak = Rc::downgrade(&osk.0);
                let on_owner = weak.clone();
                proxy.connect_notify_local(Some("g-name-owner"), move |proxy, _| {
                    watch(proxy, on_owner.clone());
                });
                watch(&proxy, weak);
            },
        );
    }

    /// 규칙에 따라 띄우거나 숨긴다
    fn decide(&self, rule: impl FnOnce(&mut Rules) -> Decision) {
        let mut rules = self.0.rules.get();
        let decision = rule(&mut rules);
        self.0.rules.set(rules);
        match decision {
            Decision::Show => self.show(),
            Decision::HideSoon => self.hide_soon(),
            Decision::Hide => self.hide(),
            Decision::Stay => {}
        }
    }

    fn show(&self) {
        let inner = &self.0;
        cancel(&inner.hide_timer);
        if let Some(height) = screen_height(&inner.window) {
            inner.keyboard.set_screen_height(height);
        }
        // 암호 칸에서는 fcitx5가 영문 배열을 쓴다. 메뉴 막대가 다음에 읽을 때(0.4초)를 기다리지 않는다
        inner.sources.refresh();
        if !inner.window.is_visible() {
            inner.window.present();
        }
        inner.keyboard.reveal();
        self.apply_input_region();
        set_inset(inner.keyboard.heights().1);
    }

    fn hide_soon(&self) {
        let inner = &self.0;
        if inner.hide_timer.borrow().is_some() || !inner.keyboard.is_revealed() {
            return;
        }
        let weak = Rc::downgrade(inner);
        let timer = glib::timeout_add_local_once(HIDE_DELAY, move || {
            if let Some(inner) = weak.upgrade() {
                inner.hide_timer.take();
                Osk(inner).hide();
            }
        });
        inner.hide_timer.replace(Some(timer));
    }

    fn hide(&self) {
        let inner = &self.0;
        cancel(&inner.hide_timer);
        if !inner.keyboard.is_revealed() {
            return;
        }
        inner.keyboard.conceal();
        self.apply_input_region();
        set_inset(0);
    }
}

/// WatchTextInput: 지금 상태를 받고 신호를 받기 시작한다. 셸 전용이라 컴포지터가 띄운 셸만 부를 수 있다
fn watch(proxy: &gio::DBusProxy, osk: std::rc::Weak<Inner>) {
    if !crate::dbus::is_running(proxy) {
        return;
    }
    proxy.call(
        "WatchTextInput",
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(reply) => {
                if let (Some(inner), Some(active)) =
                    (osk.upgrade(), reply.child_value(0).get::<bool>())
                {
                    Osk(inner).decide(|rules| rules.text_input(active));
                }
            }
            // 화면 키보드 이전의 컴포지터
            Err(err) => eprintln!("bunt-shell: {INTERFACE}.WatchTextInput: {err}"),
        },
    );
}

fn cancel(timer: &RefCell<Option<glib::SourceId>>) {
    if let Some(timer) = timer.take() {
        timer.remove();
    }
}

/// 지금 입력 소스 다음의 입력 소스 (지구본 키)
fn next_source(sources: &InputSources) -> Option<String> {
    let list = sources.sources();
    let current = sources.current()?;
    let index = list.iter().position(|source| source.id == current.id)?;
    list.get((index + 1) % list.len())
        .map(|source| source.id.clone())
}

/// 키보드가 뜰 화면의 높이 (논리 픽셀)
fn screen_height(window: &gtk::ApplicationWindow) -> Option<i32> {
    let display = WidgetExt::display(window);
    window
        .surface()
        .and_then(|surface| display.monitor_at_surface(&surface))
        .or_else(|| display.monitors().item(0).and_downcast::<gdk::Monitor>())
        .map(|monitor| monitor.geometry().height())
}

/// 셸의 창이 키보드를 피하는 법
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Avoid {
    /// 화면 가운데의 창(관리자 인증 창, Wi-Fi 암호 창): 키보드가 뜨면 키보드 바로 위로 올라간다
    Centered,
    /// 화면을 채우는 창(모든 앱): 키보드 위까지만 채운다
    Fill,
}

/// 키보드가 차지한 높이와 키보드를 피하는 셸 창
#[derive(Default)]
struct Inset {
    height: i32,
    windows: Vec<(glib::WeakRef<gtk::ApplicationWindow>, Avoid)>,
}

thread_local! {
    static INSET: RefCell<Inset> = RefCell::default();
}

/// 이 창이 키보드를 피하게 한다. 컴포지터는 앱 창의 자리만 줄이고, 셸의 레이어 창은 셸이 옮긴다. 가운데의 창은
/// 키보드를 덮지도 키보드에 가리지도 않아야 사람이 암호를 칠 수 있다
pub fn avoid_keyboard(window: &gtk::ApplicationWindow, how: Avoid) {
    let height = INSET.with(|inset| {
        let mut inset = inset.borrow_mut();
        inset
            .windows
            .retain(|(window, _)| window.upgrade().is_some());
        inset.windows.push((window.downgrade(), how));
        inset.height
    });
    place(window, how, height);
}

fn set_inset(height: i32) {
    let windows: Vec<(gtk::ApplicationWindow, Avoid)> = INSET.with(|inset| {
        let mut inset = inset.borrow_mut();
        if inset.height == height {
            return Vec::new();
        }
        inset.height = height;
        inset
            .windows
            .retain(|(window, _)| window.upgrade().is_some());
        inset
            .windows
            .iter()
            .filter_map(|(window, how)| window.upgrade().map(|window| (window, *how)))
            .collect()
    });
    for (window, how) in windows {
        place(&window, how, height);
    }
}

/// 레이어 창의 자리. 키보드가 뜨면 가운데의 창은 아래에 붙여 키보드 위로 올리고(배타 영역을 보지 않아야 Dock 자리를
/// 덮은 키보드 바로 위다), 들어가면 다시 가운데에 둔다
fn place(window: &gtk::ApplicationWindow, how: Avoid, height: i32) {
    match placement(how, height) {
        Placement::Fill { bottom } => window.set_margin(Edge::Bottom, bottom),
        Placement::Raised { bottom } => {
            window.set_anchor(Edge::Bottom, true);
            window.set_exclusive_zone(-1);
            window.set_margin(Edge::Bottom, bottom);
        }
        Placement::Centered => {
            window.set_anchor(Edge::Bottom, false);
            window.set_exclusive_zone(0);
            window.set_margin(Edge::Bottom, 0);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Placement {
    Fill { bottom: i32 },
    Raised { bottom: i32 },
    Centered,
}

fn placement(how: Avoid, keyboard: i32) -> Placement {
    match how {
        Avoid::Fill => Placement::Fill { bottom: keyboard },
        Avoid::Centered if keyboard > 0 => Placement::Raised {
            bottom: keyboard + DIALOG_GAP,
        },
        Avoid::Centered => Placement::Centered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_windows_move_above_the_keyboard_and_back() {
        // 키보드(판 280px)가 뜨면 가운데의 창은 키보드 위 16px에, 모든 앱은 키보드 위까지
        assert_eq!(
            placement(Avoid::Centered, 280),
            Placement::Raised { bottom: 296 }
        );
        assert_eq!(placement(Avoid::Fill, 280), Placement::Fill { bottom: 280 });
        // 들어가면 다시 가운데, 화면 끝까지
        assert_eq!(placement(Avoid::Centered, 0), Placement::Centered);
        assert_eq!(placement(Avoid::Fill, 0), Placement::Fill { bottom: 0 });
    }
}
