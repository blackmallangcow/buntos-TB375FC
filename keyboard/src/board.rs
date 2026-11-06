// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드의 상태: 글자 배열, 쪽, Shift (iPadOS 키보드의 규칙). GTK에 기대지 않는다.
//!
//! - Shift: 한 번 누르면 다음 글자 하나만 대문자(한글은 ㅃ ㅉ ㄸ ㄲ ㅆ ㅒ ㅖ)이고, 그 글자를 치면 풀린다. 두 번
//!   빨리([`DOUBLE_TAP`] 안에) 누르면 Caps Lock이고, 한 번 더 누르면 풀린다.
//! - 숫자·기호 쪽에서 스페이스를 치면 글자 쪽으로 돌아간다(iPadOS). 쪽을 바꾸면 한 번 누른 Shift는 풀린다.
//! - 키보드가 다시 뜨면 글자 쪽, Shift 끔으로 시작한다. 앞 글을 모르므로 문장 첫 글자의 자동 대문자는 없다.

use std::time::{Duration, Instant};

use crate::layout::{self, Action, Command, Page, Script, Shift, Slot};

/// Shift를 이 안에 두 번 누르면 Caps Lock이다
pub const DOUBLE_TAP: Duration = Duration::from_millis(350);

/// 누른 키의 결과: 보낼 것과 배열이 바뀌었는지
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub output: Option<Output>,
    /// 보이는 키가 바뀌어 다시 그려야 한다
    pub redraw: bool,
}

/// 키보드가 보내는 것
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// 글자 하나 (스페이스 포함. 지금 키 배열의 글자)
    Char(char),
    Delete,
    Return,
    /// 단축 줄의 명령
    Command(Command),
}

impl Output {
    /// 컴포지터의 화면 키보드 프로토콜(bunt_osk_v1의 press)이 받는 이름 (comp/src/osk.rs의 parse_key, EditCommand::parse)
    pub fn key_name(self) -> String {
        match self {
            Output::Char(character) => character.to_string(),
            Output::Delete => "BackSpace".into(),
            Output::Return => "Return".into(),
            Output::Command(Command::Cut) => "Cut".into(),
            Output::Command(Command::Copy) => "Copy".into(),
            Output::Command(Command::Paste) => "Paste".into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Board {
    script: Script,
    page: Page,
    shift: Shift,
    /// Shift를 한 번 누른 때 (두 번 누르기를 알아본다)
    shift_tapped: Option<Instant>,
}

impl Board {
    pub fn script(&self) -> Script {
        self.script
    }

    pub fn page(&self) -> Page {
        self.page
    }

    pub fn shift(&self) -> Shift {
        self.shift
    }

    /// 지금 보일 네 줄
    pub fn rows(&self, globe: bool) -> [Vec<Slot>; 4] {
        layout::rows(self.script, self.page, self.shift, globe)
    }

    /// 입력 소스가 바뀌었다. 바뀌었으면 true
    pub fn set_script(&mut self, script: Script) -> bool {
        std::mem::replace(&mut self.script, script) != script
    }

    /// 키보드가 다시 뜬다: 글자 쪽, Shift 끔
    pub fn reset(&mut self) -> bool {
        let changed = self.page != Page::Letters || self.shift != Shift::Off;
        self.page = Page::Letters;
        self.shift = Shift::Off;
        self.shift_tapped = None;
        changed
    }

    /// 키를 눌렀다. 지우기와 Shift는 누를 때, 나머지는 뗄 때 부른다(view.rs). 지구본과 키보드 가리기는 보낼 것이
    /// 없고 부른 쪽이 따로 한다
    pub fn press(&mut self, action: Action, now: Instant) -> Outcome {
        match action {
            Action::Char(character) => {
                let release = self.shift == Shift::Once;
                if release {
                    self.shift = Shift::Off;
                }
                Outcome {
                    output: Some(Output::Char(character)),
                    redraw: release,
                }
            }
            Action::Space => {
                let back = self.page != Page::Letters;
                if back {
                    self.page = Page::Letters;
                }
                Outcome {
                    output: Some(Output::Char(' ')),
                    redraw: back,
                }
            }
            Action::Delete => Outcome {
                output: Some(Output::Delete),
                redraw: false,
            },
            Action::Return => Outcome {
                output: Some(Output::Return),
                redraw: false,
            },
            // 글자를 친 것이 아니므로 한 번 누른 Shift와 쪽은 그대로다
            Action::Command(command) => Outcome {
                output: Some(Output::Command(command)),
                redraw: false,
            },
            Action::Shift => {
                self.tap_shift(now);
                Outcome {
                    output: None,
                    redraw: true,
                }
            }
            Action::Turn(page) => {
                self.page = page;
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                }
                Outcome {
                    output: None,
                    redraw: true,
                }
            }
            Action::NextSource | Action::Hide => Outcome {
                output: None,
                redraw: false,
            },
        }
    }

    fn tap_shift(&mut self, now: Instant) {
        let double = self
            .shift_tapped
            .is_some_and(|tapped| now.saturating_duration_since(tapped) < DOUBLE_TAP);
        (self.shift, self.shift_tapped) = match self.shift {
            Shift::Off => (Shift::Once, Some(now)),
            Shift::Once if double => (Shift::Locked, None),
            Shift::Once | Shift::Locked => (Shift::Off, None),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(board: &mut Board, action: Action, now: Instant) -> Option<Output> {
        board.press(action, now).output
    }

    #[test]
    fn one_shift_is_for_one_letter() {
        let now = Instant::now();
        let mut board = Board::default();
        board.press(Action::Shift, now);
        assert_eq!(board.shift(), Shift::Once);
        // 다음 글자 하나를 치면 풀리고 배열을 다시 그린다
        let outcome = board.press(Action::Char('Q'), now + Duration::from_secs(1));
        assert_eq!(outcome.output, Some(Output::Char('Q')));
        assert!(outcome.redraw);
        assert_eq!(board.shift(), Shift::Off);
        assert!(
            !board
                .press(Action::Char('q'), now + Duration::from_secs(2))
                .redraw
        );
    }

    #[test]
    fn two_quick_shifts_lock_the_capitals() {
        let now = Instant::now();
        let mut board = Board::default();
        board.press(Action::Shift, now);
        board.press(Action::Shift, now + Duration::from_millis(200));
        assert_eq!(board.shift(), Shift::Locked);
        // Caps Lock은 글자를 쳐도 남는다
        typed(&mut board, Action::Char('A'), now + Duration::from_secs(1));
        typed(&mut board, Action::Char('B'), now + Duration::from_secs(1));
        assert_eq!(board.shift(), Shift::Locked);
        // 한 번 더 누르면 풀린다. 그 뒤 빨리 누른 것은 다시 한 번 누른 것이다
        let later = now + Duration::from_secs(2);
        board.press(Action::Shift, later);
        assert_eq!(board.shift(), Shift::Off);
        board.press(Action::Shift, later + Duration::from_millis(100));
        assert_eq!(board.shift(), Shift::Once);
    }

    #[test]
    fn slow_shifts_toggle() {
        let now = Instant::now();
        let mut board = Board::default();
        board.press(Action::Shift, now);
        board.press(Action::Shift, now + DOUBLE_TAP);
        assert_eq!(board.shift(), Shift::Off);
    }

    #[test]
    fn space_on_the_number_page_goes_back_to_letters() {
        let now = Instant::now();
        let mut board = Board::default();
        board.press(Action::Turn(Page::Numbers), now);
        assert_eq!(board.page(), Page::Numbers);
        typed(&mut board, Action::Char('1'), now);
        assert_eq!(board.page(), Page::Numbers);
        let outcome = board.press(Action::Space, now);
        assert_eq!(outcome.output, Some(Output::Char(' ')));
        assert!(outcome.redraw);
        assert_eq!(board.page(), Page::Letters);
        // 글자 쪽의 스페이스는 다시 그리지 않는다
        assert!(!board.press(Action::Space, now).redraw);
    }

    #[test]
    fn turning_the_page_drops_a_single_shift_but_keeps_caps_lock() {
        let now = Instant::now();
        let mut board = Board::default();
        board.press(Action::Shift, now);
        board.press(Action::Turn(Page::Numbers), now);
        assert_eq!(board.shift(), Shift::Off);
        board.press(Action::Turn(Page::Letters), now);
        board.press(Action::Shift, now);
        board.press(Action::Shift, now);
        board.press(Action::Turn(Page::Symbols), now);
        assert_eq!(board.shift(), Shift::Locked);
        // 다시 뜨면 글자 쪽, Shift 끔
        assert!(board.reset());
        assert_eq!((board.page(), board.shift()), (Page::Letters, Shift::Off));
        assert!(!board.reset());
    }

    /// 단축 줄의 명령은 글자를 친 것이 아니다: 한 번 누른 Shift와 쪽이 그대로이고 배열을 다시 그리지 않는다
    #[test]
    fn commands_leave_the_shift_and_the_page_alone() {
        let now = Instant::now();
        let mut board = Board::default();
        board.press(Action::Turn(Page::Numbers), now);
        board.press(Action::Shift, now);
        for command in [Command::Cut, Command::Copy, Command::Paste] {
            let outcome = board.press(Action::Command(command), now);
            assert_eq!(outcome.output, Some(Output::Command(command)));
            assert!(!outcome.redraw);
        }
        assert_eq!((board.page(), board.shift()), (Page::Numbers, Shift::Once));
    }

    #[test]
    fn keys_name_what_the_compositor_presses() {
        assert_eq!(Output::Char('q').key_name(), "q");
        assert_eq!(Output::Char(' ').key_name(), " ");
        assert_eq!(Output::Delete.key_name(), "BackSpace");
        assert_eq!(Output::Return.key_name(), "Return");
        assert_eq!(Output::Command(Command::Cut).key_name(), "Cut");
        assert_eq!(Output::Command(Command::Copy).key_name(), "Copy");
        assert_eq!(Output::Command(Command::Paste).key_name(), "Paste");
        // 지우기와 Return은 누를 때 보낸다(되풀이는 view.rs). 지구본·가리기는 보낼 것이 없다
        let mut board = Board::default();
        let now = Instant::now();
        assert_eq!(typed(&mut board, Action::Delete, now), Some(Output::Delete));
        assert_eq!(typed(&mut board, Action::Return, now), Some(Output::Return));
        assert_eq!(typed(&mut board, Action::NextSource, now), None);
        assert_eq!(typed(&mut board, Action::Hide, now), None);
        assert!(board.set_script(Script::Hangul));
        assert!(!board.set_script(Script::Hangul));
    }
}
