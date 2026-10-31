// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 다른 창 안의 화면 키보드 (잠금 화면, 로그인 화면). 셸이 그려지지 않을 때 뜨는 창이라 셸의 bunt-osk 창을 쓸 수
//! 없다(잠긴 동안 컴포지터는 잠금 표면만 그린다). 키보드를 그 창의 아래쪽에 넣고, 그 창의 글 칸에 포커스가 있고
//! 입력 모드가 터치일 때 띄운다(rules.rs). 키보드가 나오면 그 창의 내용이 위로 올라간다.
//!
//! - 키보드는 배경화면 위라 늘 어두운 모양이다.
//! - 글 칸에서 다른 위젯(단추)으로 포커스가 가면 숨고, 포커스가 없어지면(칸을 막았다) 그대로 둔다. 잠금 화면은 암호를
//!   확인하는 동안 칸을 막는다.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use gtk4::{self as gtk, gdk, glib, prelude::*};

use crate::{
    KeySink, Keyboard, Room, focus, mode,
    rules::{Decision, Rules},
};

/// 글 칸이 꺼진 뒤 숨기까지 기다리는 시간 (셸과 같다)
const HIDE_DELAY: Duration = Duration::from_millis(250);

struct State {
    keyboard: Keyboard,
    window: glib::WeakRef<gtk::Window>,
    rules: Cell<Rules>,
    hide_timer: RefCell<Option<glib::SourceId>>,
}

/// `window`의 `column`(창 내용을 담은 세로 상자) 아래에 키보드를 넣는다. 미리 보기 풍선은 창 전체를 덮는 `overlay`에
/// 띄운다(`column`보다 뒤에 단다). 키는 `sink`로 보낸다. 돌려준 키보드에 입력 소스(배열, 지구본)를 알린다
pub fn embed(
    window: &impl IsA<gtk::Window>,
    overlay: &gtk::Overlay,
    column: &gtk::Box,
    sink: Rc<dyn KeySink>,
) -> Keyboard {
    let window = window.upcast_ref::<gtk::Window>();
    let keyboard = Keyboard::new(sink, Room::Embedded(overlay.clone()));
    keyboard.set_dark(true);
    column.append(keyboard.widget());
    let state = Rc::new(State {
        keyboard: keyboard.clone(),
        window: window.downgrade(),
        rules: Cell::default(),
        hide_timer: RefCell::default(),
    });
    // 창의 포커스 처리기가 창이 있는 동안 상태를 붙든다. 입력 모드 구독은 프로그램이 끝날 때까지 남으므로 붙들지
    // 않는다(잠금 화면은 화면이 빠지면 다른 창에 키보드를 다시 만든다)
    let weak = Rc::downgrade(&state);
    mode::watch(move |mode| {
        if let Some(state) = weak.upgrade() {
            state.decide(|rules| rules.mode(mode));
        }
    });
    let on_focus = state.clone();
    focus::follow_text_focus(window, move |focused| {
        on_focus.decide(|rules| rules.text_input(focused))
    });
    let weak = Rc::downgrade(&state);
    keyboard.connect_hide(move || {
        if let Some(state) = weak.upgrade() {
            state.decide(Rules::hide_key);
        }
    });
    keyboard
}

impl State {
    fn decide(self: &Rc<Self>, rule: impl FnOnce(&mut Rules) -> Decision) {
        let mut rules = self.rules.get();
        let decision = rule(&mut rules);
        self.rules.set(rules);
        match decision {
            Decision::Show => {
                self.cancel_hide();
                if let Some(height) = self
                    .window
                    .upgrade()
                    .and_then(|window| screen_height(&window))
                {
                    self.keyboard.set_screen_height(height);
                }
                self.keyboard.reveal();
            }
            Decision::HideSoon => {
                if self.hide_timer.borrow().is_some() || !self.keyboard.is_revealed() {
                    return;
                }
                let weak = Rc::downgrade(self);
                let timer = glib::timeout_add_local_once(HIDE_DELAY, move || {
                    if let Some(state) = weak.upgrade() {
                        state.hide_timer.take();
                        state.keyboard.conceal();
                    }
                });
                self.hide_timer.replace(Some(timer));
            }
            Decision::Hide => {
                self.cancel_hide();
                self.keyboard.conceal();
            }
            Decision::Stay => {}
        }
    }

    fn cancel_hide(&self) {
        if let Some(timer) = self.hide_timer.take() {
            timer.remove();
        }
    }
}

/// 창이 놓인 화면의 높이 (논리 픽셀). 아직 화면이 없으면(미리 보기 창) 창의 높이다
fn screen_height(window: &gtk::Window) -> Option<i32> {
    let display = WidgetExt::display(window);
    window
        .surface()
        .and_then(|surface| display.monitor_at_surface(&surface))
        .map(|monitor: gdk::Monitor| monitor.geometry().height())
        .or_else(|| (window.height() > 0).then(|| window.height()))
}
