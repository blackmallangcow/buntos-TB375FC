// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Caps Lock 키로 입력 소스를 바꾼다 (macOS의 "Caps Lock 키로 ABC 입력 소스 전환").
//!
//! - 짧게 눌렀다 떼면 입력 소스를 바꾼다(한/영). Caps Lock은 켜지지 않는다.
//! - [`HOLD`]보다 길게 누르고 있으면 Caps Lock이 켜진다(키보드의 불도 켜진다, udev.rs).
//! - Caps Lock이 켜져 있을 때는 누르는 순간 꺼진다. 입력 소스는 바꾸지 않는다.
//!
//! 짧게 누른 것인지는 뗄 때에야 알 수 있으므로, 사람이 누른 Caps Lock 키는 키보드 상태(xkb)와 앱에 보내지
//! 않고 여기서 받는다. Caps Lock을 켜고 끌 때에만 컴포지터가 그 키를 눌렀다 뗀 것으로 넣는다(input.rs).
//! 이 파일은 그 규칙만 가진다.

use std::time::Duration;

/// 이보다 길게 누르고 있으면 Caps Lock을 켠다. 빠르게 치는 사람의 누름은 0.2초를 넘지 않고, 이보다 길면
/// Caps Lock을 켜려 할 때 굼뜨게 느껴진다
pub const HOLD: Duration = Duration::from_millis(400);

/// Caps Lock 키를 누르고 있는 동안의 상태
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// 아직 짧게 누른 것인지 길게 누른 것인지 모른다 (타이머가 돈다)
    Waiting,
    /// 이미 Caps Lock을 켜거나 껐다. 떼어도 아무 일도 하지 않는다
    Done,
}

/// 키 하나에 대해 컴포지터가 할 일
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Nothing,
    /// [`HOLD`] 뒤에 [`CapsKey::held_long`]을 부른다
    Wait,
    /// Caps Lock을 켜거나 끈다
    Lock,
    /// 입력 소스를 바꾼다
    Switch,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CapsKey {
    held: Option<Held>,
}

impl CapsKey {
    /// 받은 누름의 뗌을 기다리는 중인지. 그동안에는 설정이 바뀌어도 뗌을 여기서 받는다
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }

    /// Caps Lock 키를 눌렀다. `locked`는 지금 Caps Lock이 켜져 있는지다. libinput은 키 반복을 알리지
    /// 않으므로, 떼지 않고 다시 눌린 것은 뗌을 놓친 것(가상 터미널 전환)이고 새 누름으로 본다
    pub fn pressed(&mut self, locked: bool) -> Step {
        if locked {
            self.held = Some(Held::Done);
            Step::Lock
        } else {
            self.held = Some(Held::Waiting);
            Step::Wait
        }
    }

    /// 누른 지 [`HOLD`]가 지났다
    pub fn held_long(&mut self) -> Step {
        match self.held {
            Some(Held::Waiting) => {
                self.held = Some(Held::Done);
                Step::Lock
            }
            _ => Step::Nothing,
        }
    }

    /// Caps Lock 키를 뗐다
    pub fn released(&mut self) -> Step {
        match self.held.take() {
            Some(Held::Waiting) => Step::Switch,
            _ => Step::Nothing,
        }
    }

    /// 누르고 있던 것을 잊는다 (다른 가상 터미널로 가면 뗌이 오지 않는다)
    pub fn forget(&mut self) {
        self.held = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_press_switches_the_input_source_and_does_not_lock() {
        let mut key = CapsKey::default();
        assert_eq!(key.pressed(false), Step::Wait);
        assert!(key.is_held());
        assert_eq!(key.released(), Step::Switch);
        assert!(!key.is_held());
        // 뗀 뒤에 늦게 온 타이머는 아무 일도 하지 않는다
        assert_eq!(key.held_long(), Step::Nothing);
    }

    #[test]
    fn a_long_press_locks_once_and_the_release_does_nothing() {
        let mut key = CapsKey::default();
        assert_eq!(key.pressed(false), Step::Wait);
        assert_eq!(key.held_long(), Step::Lock);
        assert_eq!(key.held_long(), Step::Nothing);
        assert!(key.is_held());
        assert_eq!(key.released(), Step::Nothing);
        assert!(!key.is_held());
    }

    #[test]
    fn a_press_while_locked_unlocks_at_once_and_does_not_switch() {
        let mut key = CapsKey::default();
        assert_eq!(key.pressed(true), Step::Lock);
        assert_eq!(key.held_long(), Step::Nothing);
        assert_eq!(key.released(), Step::Nothing);
    }

    #[test]
    fn a_press_without_the_release_of_the_last_one_starts_over() {
        let mut key = CapsKey::default();
        assert_eq!(key.pressed(false), Step::Wait);
        assert_eq!(key.held_long(), Step::Lock);
        // 뗌을 놓쳤다. 이제 Caps Lock이 켜져 있으므로 다음 누름은 끈다
        assert_eq!(key.pressed(true), Step::Lock);
        assert_eq!(key.released(), Step::Nothing);
        key.pressed(false);
        key.forget();
        assert_eq!(key.released(), Step::Nothing);
    }
}
