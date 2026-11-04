// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-keyboard: 화면 키보드 (설계안 5.1·5.2절, iPadOS 키보드). 셸(bunt-shell의 bunt-osk 창), 로그인 화면
//! (bunt-greeter), 잠금 화면(bunt-lock)이 같이 쓴다. 잠금·로그인 화면은 셸이 그려지지 않을 때 뜨므로 키보드를
//! 그 창 안에 넣는다.
//!
//! - [`layout`]: 배열 표(2벌식·QWERTY, .?123·#+=). GTK에 기대지 않는다
//! - [`board`]: Shift(두 번 누르면 Caps Lock)와 쪽 바꾸기의 규칙
//! - [`Keyboard`]: GTK 위젯. 여러 손가락 누르기, 글자 미리 보기, 지우기 되풀이, 키마다 접근성 이름
//! - [`KeySink`]: 누른 키를 보내는 곳. 컴포지터([`CompositorKeyboard`]: 셸 전용 Wayland 프로토콜, 입력기를 거쳐 한글
//!   조합)나 창 안의 글 칸([`EditableKeys`]: 잠금 화면)
//! - [`mode`]: 컴포지터의 입력 모드(포인터·터치). 키보드는 터치 모드에서만 뜬다
//! - [`rules`]: 뜨고 숨는 규칙 (터치 모드이고 글 칸을 눌렀을 때 뜬다)
//! - [`embed`]: 잠금·로그인 화면의 창 안에 키보드를 넣는다. [`focus`], [`fcitx`]로 창 안의 글 칸과 입력 소스를 따라간다

pub mod board;
mod compositor;
mod embed;
pub mod fcitx;
pub mod focus;
pub mod layout;
pub mod mode;
pub mod rules;
mod sink;
mod view;

pub use board::Output;
pub use compositor::{CompositorKeyboard, TextInputEvent};
pub use embed::embed;
pub use layout::Script;
pub use mode::InputMode;
pub use sink::{EditableKeys, KeySink};
pub use view::{Keyboard, Room, WeakKeyboard};

#[cfg(test)]
mod tests {
    #[test]
    fn every_language_has_the_same_messages() {
        bunt_i18n::check_same_messages(&[
            ("ko", include_str!("../../i18n/ko/bunt-keyboard.ftl")),
            ("en", include_str!("../../i18n/en/bunt-keyboard.ftl")),
        ])
        .unwrap();
    }

    /// 기능 키의 이름은 모두 문구 파일에 있다
    #[test]
    fn every_key_name_has_a_message() {
        use crate::layout::Name;
        for (language, source) in [
            ("ko", include_str!("../../i18n/ko/bunt-keyboard.ftl")),
            ("en", include_str!("../../i18n/en/bunt-keyboard.ftl")),
        ] {
            let tr = bunt_i18n::Translations::from_source(language, source);
            for name in [
                Name::Shift,
                Name::CapsLock,
                Name::Delete,
                Name::Return,
                Name::Space,
                Name::NextKeyboard,
                Name::Hide,
                Name::Numbers,
                Name::Symbols,
                Name::Letters,
            ] {
                let id = name.message_id();
                assert_ne!(tr.get(id), id, "{language} has no {id}");
            }
        }
    }
}
