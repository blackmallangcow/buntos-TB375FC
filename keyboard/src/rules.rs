// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드가 뜨고 숨는 규칙 (설계안 5.1절). 셸(shell/src/osk.rs)과 잠금·로그인 화면(embed.rs)이 같이 쓴다. GTK에
//! 기대지 않는다.
//!
//! - 뜬다: 입력 모드가 터치이고 사람이 글 칸을 눌렀을 때(셸은 컴포지터가 알리는 앱의 눌린 칸, 잠금·로그인 화면은 제 창의
//!   글 칸 누르기). 켜진 칸을 다시 눌러도 뜬다. 칸이 저절로 포커스를 받은 것(창이 열리며, 앱이 스스로)만으로는 뜨지 않는다.
//! - 그대로 있다: 떠 있으면 다른 칸으로 옮기거나(칸이 꺼졌다 바로 켜진다) 암호가 틀려 칸이 다시 포커스를 받아도 그대로다.
//! - 숨는다: 글 칸이 꺼지면 조금 뒤, 포인터 모드가 되거나 키보드 가리기 키를 누르면 바로. 숨은 뒤에는 칸을 다시 눌러야
//!   뜬다.

use crate::mode::InputMode;

/// 뜨고 숨는 규칙에 쓰는 상태
#[derive(Clone, Copy, Debug, Default)]
pub struct Rules {
    mode: InputMode,
    /// 키보드가 떠 있다 (칸이 꺼져 숨기를 기다리는 동안도)
    up: bool,
}

/// 규칙이 정한 일
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Show,
    /// 조금 기다렸다 숨는다. 다 기다리면 [`Rules::hide_delay_passed`]를 부른다. 그사이 [`Decision::Show`]가 나오면
    /// 그만둔다
    HideSoon,
    Hide,
    Stay,
}

impl Rules {
    /// 사람이 글 칸을 눌렀다
    pub fn tapped(&mut self) -> Decision {
        match self.mode {
            InputMode::Touch => {
                self.up = true;
                Decision::Show
            }
            InputMode::Pointer => Decision::Stay,
        }
    }

    /// 글 칸이 켜졌거나 꺼졌다 (포커스가 옮겨 갔다). 이것만으로는 뜨지 않는다
    pub fn text_input(&mut self, active: bool) -> Decision {
        match (self.up, active) {
            (false, _) => Decision::Stay,
            // 숨으려던 것을 그만둔다
            (true, true) => Decision::Show,
            (true, false) => Decision::HideSoon,
        }
    }

    /// 꺼진 칸이 다시 켜지지 않은 채 기다리는 시간이 지났다
    pub fn hide_delay_passed(&mut self) -> Decision {
        self.up = false;
        Decision::Hide
    }

    /// 입력 모드가 바뀌었다 (키보드를 떼거나 붙였다). 터치 모드가 되어도 칸을 눌러야 뜬다
    pub fn mode(&mut self, mode: InputMode) -> Decision {
        self.mode = mode;
        match mode {
            InputMode::Pointer => {
                self.up = false;
                Decision::Hide
            }
            InputMode::Touch => Decision::Stay,
        }
    }

    /// 키보드 가리기 키를 눌렀다
    pub fn hide_key(&mut self) -> Decision {
        self.up = false;
        Decision::Hide
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch() -> Rules {
        let mut rules = Rules::default();
        rules.mode(InputMode::Touch);
        rules
    }

    #[test]
    fn a_field_focused_by_itself_does_not_show_the_keyboard() {
        let mut rules = touch();
        assert_eq!(rules.text_input(true), Decision::Stay);
        // 칸을 옮겨 다녀도 사람이 누르지 않았으면 뜨지 않는다
        assert_eq!(rules.text_input(false), Decision::Stay);
        assert_eq!(rules.text_input(true), Decision::Stay);
        // 누르면 뜬다
        assert_eq!(rules.tapped(), Decision::Show);
    }

    /// 로그인 화면은 입력 모드를 읽기 전에 암호 칸에 포커스를 준다
    #[test]
    fn learning_touch_mode_with_a_focused_field_does_not_show_the_keyboard() {
        let mut rules = Rules::default();
        assert_eq!(rules.text_input(true), Decision::Stay);
        assert_eq!(rules.mode(InputMode::Touch), Decision::Stay);
        assert_eq!(rules.tapped(), Decision::Show);
    }

    #[test]
    fn the_pointer_mode_never_shows_the_keyboard() {
        let mut rules = Rules::default();
        assert_eq!(rules.text_input(true), Decision::Stay);
        assert_eq!(rules.tapped(), Decision::Stay);
        // 포인터 모드에서 누른 것은 터치 모드가 되어도 뜨지 않는다
        assert_eq!(rules.mode(InputMode::Touch), Decision::Stay);
        assert_eq!(rules.tapped(), Decision::Show);
        // 포인터 모드가 되면 바로 숨고, 터치 모드로 돌아와도 칸을 다시 눌러야 뜬다
        assert_eq!(rules.mode(InputMode::Pointer), Decision::Hide);
        assert_eq!(rules.text_input(true), Decision::Stay);
        assert_eq!(rules.mode(InputMode::Touch), Decision::Stay);
        assert_eq!(rules.text_input(true), Decision::Stay);
    }

    #[test]
    fn the_keyboard_stays_while_moving_between_fields() {
        let mut rules = touch();
        rules.text_input(true);
        assert_eq!(rules.tapped(), Decision::Show);
        // 다른 칸을 누르거나 Enter로 다음 칸에 간다: 꺼졌다 바로 켜지면 숨으려던 것을 그만둔다
        assert_eq!(rules.text_input(false), Decision::HideSoon);
        assert_eq!(rules.text_input(true), Decision::Show);
        // 암호가 틀려 칸이 다시 포커스를 받는다
        assert_eq!(rules.text_input(true), Decision::Show);
    }

    #[test]
    fn the_keyboard_hides_after_its_field_turns_off() {
        let mut rules = touch();
        rules.text_input(true);
        rules.tapped();
        assert_eq!(rules.text_input(false), Decision::HideSoon);
        assert_eq!(rules.hide_delay_passed(), Decision::Hide);
        // 숨은 뒤에 칸이 저절로 켜져도 뜨지 않는다. 눌러야 뜬다
        assert_eq!(rules.text_input(true), Decision::Stay);
        assert_eq!(rules.tapped(), Decision::Show);
    }

    #[test]
    fn a_hidden_keyboard_comes_back_when_the_field_is_tapped_again() {
        let mut rules = touch();
        rules.text_input(true);
        assert_eq!(rules.tapped(), Decision::Show);
        assert_eq!(rules.hide_key(), Decision::Hide);
        // 칸이 켜진 채다. 포커스가 다시 와도 뜨지 않고, 켜진 칸을 다시 누르면 뜬다
        assert_eq!(rules.text_input(true), Decision::Stay);
        assert_eq!(rules.tapped(), Decision::Show);
        // 가린 동안 모드가 바뀌었다 돌아와도 다시 뜨지 않는다
        assert_eq!(rules.hide_key(), Decision::Hide);
        rules.mode(InputMode::Pointer);
        assert_eq!(rules.mode(InputMode::Touch), Decision::Stay);
    }
}
