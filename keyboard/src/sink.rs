// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드가 누른 키를 보내는 곳.
//!
//! - [`CompositorKeyboard`](crate::CompositorKeyboard): 컴포지터의 셸 전용 프로토콜로 실제 키보드처럼 넣는다(셸,
//!   로그인 화면. compositor.rs). 입력기(fcitx5)의 키보드 잡기를 거쳐 한글이 조합된다.
//! - [`EditableKeys`]: 그 창의 포커스를 가진 글 칸에 바로 넣는다(잠금 화면, 컴포지터가 그 프로토콜을 주지 않을 때의
//!   로그인 화면). 잠긴 동안 컴포지터는 셸의 키를 받지 않고, 잠금 화면 밖의 프로그램이 잠금 화면에 키를 넣는 길도
//!   열지 않는다. 암호 칸에는 한글을 조합하지 않는다.
//!
//! 바꾼 점: 컴포지터로 보내는 쪽(전에는 세션 버스의 PressOnScreenKeys를 부르던 CompositorKeys)은 compositor.rs로
//! 옮겼다. 세션 버스는 같은 사용자의 프로그램이 엿볼 수 있다.

use gtk4::{self as gtk, glib, prelude::*};

use crate::board::Output;

/// 화면 키보드가 누른 키를 받는 곳
pub trait KeySink {
    fn send(&self, output: Output);
}

/// 창의 포커스를 가진 글 칸에 바로 넣는다 (잠금 화면)
pub struct EditableKeys {
    window: glib::WeakRef<gtk::Window>,
}

impl EditableKeys {
    pub fn new(window: &impl IsA<gtk::Window>) -> Self {
        Self {
            window: window.upcast_ref::<gtk::Window>().downgrade(),
        }
    }
}

impl KeySink for EditableKeys {
    fn send(&self, output: Output) {
        let Some(focus) = self
            .window
            .upgrade()
            .and_then(|window| GtkWindowExt::focus(&window))
        else {
            return;
        };
        // 포커스는 글 칸 안의 GtkText에 있다. 막아 둔 칸(암호를 확인하는 동안, 기다리는 동안)에는 넣지 않는다
        let Ok(editable) = focus.clone().dynamic_cast::<gtk::Editable>() else {
            return;
        };
        if !editable.is_editable() || !focus.is_sensitive() {
            return;
        }
        match output {
            Output::Char(character) => {
                editable.delete_selection();
                let mut position = editable.position();
                editable.insert_text(&character.to_string(), &mut position);
                editable.set_position(position);
            }
            Output::Delete => {
                if editable.selection_bounds().is_some() {
                    editable.delete_selection();
                } else {
                    let position = editable.position();
                    if position > 0 {
                        editable.delete_text(position - 1, position);
                    }
                }
            }
            // 글 칸의 기본 동작 (잠금 화면: 암호 확인). GtkText는 위젯의 기본 동작 신호를 정해 두지 않아
            // gtk_widget_activate가 아무것도 하지 않는다. Return 키가 보내는 그 칸의 activate 신호를 보낸다. 글자 칸과
            // 암호 칸은 이것을 제 activate로 옮긴다
            Output::Return => {
                if focus.is::<gtk::Text>() {
                    focus.emit_by_name::<()>("activate", &[]);
                } else {
                    focus.activate();
                }
            }
        }
    }
}
