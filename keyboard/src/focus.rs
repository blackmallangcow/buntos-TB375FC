// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창 안의 글 칸 포커스를 따라 화면 키보드를 띄우고 숨긴다 (잠금·로그인 화면). 셸은 컴포지터가 알리는 앱의 글
//! 입력 상태를 따른다(shell/src/osk.rs). 이 창들은 제 창 안의 글 칸만 쓰므로 GTK의 포커스로 안다.
//!
//! - 글 칸에 포커스가 오면 `focused(true)`, 글 칸이 아닌 위젯(단추)으로 가면 `focused(false)`. 포커스가 없어지면
//!   (칸을 막아 GTK가 포커스를 거뒀다) 부르지 않는다
//! - 포커스를 가진 글 칸을 다시 누르면 `focused(true)`를 다시 부른다(가린 키보드를 다시 띄운다, iPadOS)

use std::rc::Rc;

use gtk4::{self as gtk, prelude::*};

/// 글 칸(GtkText. 글자 칸과 암호 칸이 그 안에 둔다)인지
fn is_text(widget: Option<&gtk::Widget>) -> bool {
    widget.is_some_and(|widget| widget.is::<gtk::Editable>())
}

/// 누른 위젯이 포커스를 가진 글 칸(이나 그 칸을 감싼 글자 칸·암호 칸) 안인지
fn inside_focus(picked: &gtk::Widget, focus: &gtk::Widget) -> bool {
    let entry = focus.parent();
    picked == focus
        || picked.is_ancestor(focus)
        || entry
            .as_ref()
            .is_some_and(|entry| picked == entry || picked.is_ancestor(entry))
}

pub fn follow_text_focus(window: &impl IsA<gtk::Window>, focused: impl Fn(bool) + 'static) {
    let window = window.upcast_ref::<gtk::Window>();
    let focused = Rc::new(focused);
    let on_focus = focused.clone();
    window.connect_focus_widget_notify(move |window| {
        if let Some(focus) = GtkWindowExt::focus(window) {
            on_focus(is_text(Some(&focus)));
        }
    });
    // 누르기는 칸에 그대로 간다 (잡지 않는다)
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    click.connect_pressed(move |_, _, x, y| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some(focus) = GtkWindowExt::focus(&window).filter(|focus| is_text(Some(focus))) else {
            return;
        };
        if window
            .pick(x, y, gtk::PickFlags::DEFAULT)
            .is_some_and(|picked| inside_focus(&picked, &focus))
        {
            focused(true);
        }
    });
    window.add_controller(click);
}
