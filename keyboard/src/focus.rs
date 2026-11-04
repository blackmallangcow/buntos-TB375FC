// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창 안의 글 칸을 지켜봐 화면 키보드 규칙(rules.rs)에 알린다 (잠금·로그인 화면). 셸은 컴포지터가 알리는 앱의 글 입력
//! 상태를 따른다(shell/src/osk.rs). 이 창들은 제 창 안의 글 칸만 쓰므로 GTK로 안다.
//!
//! - 글 칸을 누르면 `tapped`. 포커스를 가졌든 아니든 같다(가린 키보드를 다시 띄운다).
//! - 글 칸에 포커스가 오면 `focused(true)`, 글 칸이 아닌 위젯(단추)으로 가면 `focused(false)`. 포커스가 없어지면
//!   (칸을 막아 GTK가 포커스를 거뒀다) 부르지 않는다

use gtk4::{self as gtk, prelude::*};

/// 글 칸(GtkText. 글자 칸과 암호 칸이 그 안에 둔다)인지
fn is_text(widget: Option<&gtk::Widget>) -> bool {
    widget.is_some_and(|widget| widget.is::<gtk::Editable>())
}

/// 창의 그 자리에 칠 수 있는 글 칸이 있는지. 글 칸(글자 칸, 암호 칸)의 안쪽(GtkText, 눈 아이콘)도 칸이다. 막아 둔 칸은
/// 고르지 않는다(`PickFlags::DEFAULT`)
fn field_at(window: &gtk::Window, x: f64, y: f64) -> bool {
    let mut widget = window.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(current) = widget {
        if let Some(editable) = current.dynamic_cast_ref::<gtk::Editable>() {
            return editable.is_editable();
        }
        widget = current.parent();
    }
    false
}

pub fn follow_text_fields(
    window: &impl IsA<gtk::Window>,
    focused: impl Fn(bool) + 'static,
    tapped: impl Fn() + 'static,
) {
    let window = window.upcast_ref::<gtk::Window>();
    window.connect_focus_widget_notify(move |window| {
        if let Some(focus) = GtkWindowExt::focus(window) {
            focused(is_text(Some(&focus)));
        }
    });
    // 누르기는 칸에 그대로 간다 (잡지 않는다). 뗄 때는 알 수 없다: 칸의 제스처가 누르기를 잡으면 이 제스처는 released를
    // 받지 못한다. 칸이 제 포커스를 받기 전이라 포커스가 아니라 자리로 본다
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = window.downgrade();
    click.connect_pressed(move |_, _, x, y| {
        if weak.upgrade().is_some_and(|window| field_at(&window, x, y)) {
            tapped();
        }
    });
    window.add_controller(click);
}
