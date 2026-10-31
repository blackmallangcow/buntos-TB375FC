// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드가 뜨고 숨는 규칙 (설계안 5.1절, iPadOS). 셸(shell/src/osk.rs)과 잠금·로그인 화면(embed.rs)이 같이
//! 쓴다. GTK에 기대지 않는다.
//!
//! - 뜬다: 입력 모드가 터치이고 글 칸이 켜졌을 때(셸은 컴포지터가 알리는 앱의 글 입력, 잠금·로그인 화면은 제 창의
//!   글 칸 포커스). 켠 칸을 다시 눌러도 뜬다.
//! - 숨는다: 글 칸이 꺼지면 조금 뒤(다른 칸으로 옮기면 꺼졌다 바로 켜진다), 포인터 모드가 되거나 키보드 가리기 키를
//!   누르면 바로. 가리기 키로 숨기면 칸을 다시 눌러야 뜬다.

use crate::mode::InputMode;

/// 뜨고 숨는 규칙에 쓰는 상태
#[derive(Clone, Copy, Debug, Default)]
pub struct Rules {
    mode: InputMode,
    /// 글 칸이 켜졌다
    text_input: bool,
    /// 키보드 가리기 키로 숨겼다. 칸을 다시 누르면(다시 켜지면) 풀린다
    dismissed: bool,
}

/// 규칙이 정한 일
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Show,
    /// 조금 기다렸다 숨는다. 그사이 다시 켜지면 그대로 둔다
    HideSoon,
    Hide,
    Stay,
}

impl Rules {
    /// 글 칸이 켜졌거나(켠 칸을 다시 누른 것 포함) 꺼졌다
    pub fn text_input(&mut self, active: bool) -> Decision {
        self.text_input = active;
        if !active {
            return Decision::HideSoon;
        }
        self.dismissed = false;
        match self.mode {
            InputMode::Touch => Decision::Show,
            InputMode::Pointer => Decision::Stay,
        }
    }

    /// 입력 모드가 바뀌었다 (키보드를 떼거나 붙였다)
    pub fn mode(&mut self, mode: InputMode) -> Decision {
        self.mode = mode;
        match mode {
            InputMode::Pointer => Decision::Hide,
            InputMode::Touch if self.text_input && !self.dismissed => Decision::Show,
            InputMode::Touch => Decision::Stay,
        }
    }

    /// 키보드 가리기 키를 눌렀다
    pub fn hide_key(&mut self) -> Decision {
        self.dismissed = true;
        Decision::Hide
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_shows_for_text_input_in_touch_mode_only() {
        let mut rules = Rules::default();
        // 포인터 모드(키보드와 마우스가 있다)에서는 글 칸에 포커스가 가도 뜨지 않는다
        assert_eq!(rules.text_input(true), Decision::Stay);
        // 키보드를 떼어 터치 모드가 되면 켜진 칸이 있으니 뜬다
        assert_eq!(rules.mode(InputMode::Touch), Decision::Show);
        // 칸에서 나가면 조금 기다렸다 숨고(칸을 옮기면 바로 다시 켜진다), 다시 켜지면 뜬다
        assert_eq!(rules.text_input(false), Decision::HideSoon);
        assert_eq!(rules.text_input(true), Decision::Show);
        // 포인터 모드가 되면 바로 숨는다
        assert_eq!(rules.mode(InputMode::Pointer), Decision::Hide);
        // 글 칸이 꺼진 채 터치 모드가 되면 그대로다
        rules.text_input(false);
        assert_eq!(rules.mode(InputMode::Touch), Decision::Stay);
    }

    #[test]
    fn a_hidden_keyboard_comes_back_when_the_field_is_tapped_again() {
        let mut rules = Rules::default();
        rules.mode(InputMode::Touch);
        assert_eq!(rules.text_input(true), Decision::Show);
        assert_eq!(rules.hide_key(), Decision::Hide);
        // 가린 동안 모드가 바뀌었다 돌아와도 다시 뜨지 않는다
        rules.mode(InputMode::Pointer);
        assert_eq!(rules.mode(InputMode::Touch), Decision::Stay);
        // 켠 칸을 다시 누르면 다시 켜지고(셸은 컴포지터의 TextInputChanged(true)) 다시 뜬다
        assert_eq!(rules.text_input(true), Decision::Show);
    }
}
