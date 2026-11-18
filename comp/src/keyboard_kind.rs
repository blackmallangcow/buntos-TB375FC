// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 키보드의 종류에 따라 수정 키의 역할을 정한다.
//!
//! buntOS의 단축키는 macOS처럼 ⌘를 쓴다. 맥 배열의 Command 키와 PC 배열의 Windows 키는 같은 코드
//! (`KEY_LEFTMETA`)를 내므로 그대로 ⌘다. 안드로이드 태블릿용 키보드에는 그 키가 없고, Ctrl과 Alt 사이에
//! 지구본(입력 언어) 키가 있다. 그런 키보드에서는 ⌘가 든 단축키를 하나도 누를 수 없으므로 지구본 키를 ⌘로
//! 쓴다. Alt는 그대로 ⌥다. 입력 소스는 Caps Lock 키와 ⌃Space로 바꾼다.
//!
//! - 종류는 udev 속성 [`PROPERTY`]로 받는다. 기기의 BSP가 udev 규칙으로 단다.
//! - 장치가 알리는 키 목록으로는 가릴 수 없다. 드라이버는 실제로 없는 키도 목록에 넣는다(TB375FC의 포고
//!   키보드는 `KEY_LEFTMETA`를 알리지만 그 키가 없다).
//! - 키 코드는 키보드 상태(xkb)에 넣기 전에 바꾼다. 그래서 단축키, 앱, 입력기가 모두 ⌘로 본다.
//!
//! 이 파일은 그 규칙만 가진다. 장치가 붙고 떨어지는 것은 udev.rs가, 키를 바꾸는 것은 input.rs가 부른다.

/// 키보드의 종류를 알리는 udev 속성. 값은 [`KeyboardKind::from_property`]가 읽는다
pub const PROPERTY: &str = "BUNTOS_KEYBOARD_KIND";

/// 키보드 상태(xkb)의 키 코드는 evdev 키 코드에 이만큼을 더한 값이다
pub const EVDEV_OFFSET: u32 = 8;

/// evdev 키 코드 (linux/input-event-codes.h)
const KEY_LEFTMETA: u32 = 125;
const KEY_KBD_LAYOUT_NEXT: u32 = 584;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyboardKind {
    /// ⌘ 자리의 키가 `KEY_LEFTMETA`를 낸다: 맥 배열(Command)과 PC 배열(Windows 키)
    #[default]
    Standard,
    /// ⌘ 자리의 키가 없고 지구본 키가 있다: 안드로이드 태블릿용 키보드
    Globe,
}

impl KeyboardKind {
    /// udev 속성의 값. 모르는 값과 없는 값은 보통 키보드다
    pub fn from_property(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("globe") => Self::Globe,
            _ => Self::Standard,
        }
    }

    /// 이 키보드에서 누른 키(evdev 코드)를 컴포지터가 받을 키로 바꾼다
    pub fn remap(self, code: u32) -> u32 {
        match (self, code) {
            (Self::Globe, KEY_KBD_LAYOUT_NEXT) => KEY_LEFTMETA,
            _ => code,
        }
    }
}

/// 붙어 있는 키보드 가운데 보통 키보드가 아닌 것. 장치는 libinput의 이름(`event7`)으로 가린다
#[derive(Debug, Default)]
pub struct KeyboardKinds {
    known: Vec<(String, KeyboardKind)>,
}

impl KeyboardKinds {
    /// 키보드가 붙었다. 같은 이름으로 다시 붙으면 새 종류를 따른다
    pub fn added(&mut self, id: String, kind: KeyboardKind) {
        self.removed(&id);
        if kind != KeyboardKind::Standard {
            self.known.push((id, kind));
        }
    }

    pub fn removed(&mut self, id: &str) {
        self.known.retain(|(known, _)| known != id);
    }

    /// 장치 `id`에서 누른 키를 바꾼다. 보통 키보드뿐일 때는 장치 이름을 묻지 않는다
    pub fn remap(&self, id: impl FnOnce() -> String, code: u32) -> u32 {
        if self.known.is_empty() {
            return code;
        }
        let id = id();
        self.known
            .iter()
            .find(|(known, _)| *known == id)
            .map_or(code, |(_, kind)| kind.remap(code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_globe_value_makes_a_globe_keyboard() {
        assert_eq!(
            KeyboardKind::from_property(Some("globe")),
            KeyboardKind::Globe
        );
        assert_eq!(
            KeyboardKind::from_property(Some(" globe\n")),
            KeyboardKind::Globe
        );
        assert_eq!(KeyboardKind::from_property(None), KeyboardKind::Standard);
        assert_eq!(
            KeyboardKind::from_property(Some("")),
            KeyboardKind::Standard
        );
        assert_eq!(
            KeyboardKind::from_property(Some("Globe")),
            KeyboardKind::Standard
        );
    }

    #[test]
    fn the_globe_key_is_command_on_a_globe_keyboard_only() {
        assert_eq!(KeyboardKind::Globe.remap(KEY_KBD_LAYOUT_NEXT), KEY_LEFTMETA);
        assert_eq!(
            KeyboardKind::Standard.remap(KEY_KBD_LAYOUT_NEXT),
            KEY_KBD_LAYOUT_NEXT
        );
        // Alt(56)와 Ctrl(29)은 그대로다
        assert_eq!(KeyboardKind::Globe.remap(56), 56);
        assert_eq!(KeyboardKind::Globe.remap(29), 29);
        assert_eq!(KeyboardKind::Globe.remap(KEY_LEFTMETA), KEY_LEFTMETA);
    }

    #[test]
    fn keys_are_remapped_for_the_keyboard_they_come_from() {
        let mut kinds = KeyboardKinds::default();
        // 보통 키보드뿐이면 장치 이름을 묻지 않는다
        assert_eq!(
            kinds.remap(|| unreachable!(), KEY_KBD_LAYOUT_NEXT),
            KEY_KBD_LAYOUT_NEXT
        );
        kinds.added("event3".to_owned(), KeyboardKind::Standard);
        assert_eq!(
            kinds.remap(|| unreachable!(), KEY_KBD_LAYOUT_NEXT),
            KEY_KBD_LAYOUT_NEXT
        );

        kinds.added("event7".to_owned(), KeyboardKind::Globe);
        assert_eq!(
            kinds.remap(|| "event7".to_owned(), KEY_KBD_LAYOUT_NEXT),
            KEY_LEFTMETA
        );
        assert_eq!(
            kinds.remap(|| "event3".to_owned(), KEY_KBD_LAYOUT_NEXT),
            KEY_KBD_LAYOUT_NEXT
        );

        // 같은 이름으로 다시 붙은 보통 키보드는 바꾸지 않는다
        kinds.added("event7".to_owned(), KeyboardKind::Standard);
        assert_eq!(
            kinds.remap(|| unreachable!(), KEY_KBD_LAYOUT_NEXT),
            KEY_KBD_LAYOUT_NEXT
        );

        kinds.added("event7".to_owned(), KeyboardKind::Globe);
        kinds.removed("event7");
        assert_eq!(
            kinds.remap(|| unreachable!(), KEY_KBD_LAYOUT_NEXT),
            KEY_KBD_LAYOUT_NEXT
        );
    }
}
