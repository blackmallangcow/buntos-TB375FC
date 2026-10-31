// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드의 배열 (iPadOS 키보드, 설계안 5.2절). GTK에 기대지 않는 표다.
//!
//! - 한 줄은 1/4 키 단위로 [`COLUMNS`]칸이다. 글자 키는 4칸(1키)이고, 줄마다 기능 키의 너비를 맞춰 46칸을 채운다.
//! - 글자 키가 보내는 것은 지금 키 배열(us)의 글자다. 한글(2벌식)은 자모 자리의 라틴 글자를 보내고 입력기(fcitx5)가
//!   조합한다(ㅂ은 q, Shift의 ㅃ은 Q). Shift를 눌러도 같은 자모인 키는 소문자를 보낸다.
//! - 지금 키 배열에 없는 글자(€ £ ¥ • ₩)는 키로 칠 수 없어 넣지 않았다. iPadOS의 #+= 둘째 줄과 다르다.

/// 한 줄의 칸 수 (1/4 키 단위)
pub const COLUMNS: u8 = 46;

/// 글자 배열. 지금 입력 소스를 따른다(한글이면 2벌식, 나머지는 QWERTY)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Script {
    Hangul,
    #[default]
    Latin,
}

/// 키보드의 쪽 (iPadOS: 글자, .?123, #+=)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Letters,
    Numbers,
    Symbols,
}

/// Shift 상태. 한 번 누르면 다음 글자 하나만, 두 번 누르면 Caps Lock
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shift {
    #[default]
    Off,
    Once,
    Locked,
}

/// 키가 하는 일
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 그 글자를 보낸다 (지금 키 배열의 글자)
    Char(char),
    Shift,
    /// 지우기. 누르고 있으면 되풀이한다
    Delete,
    Return,
    Space,
    /// 다음 입력 소스 (지구본)
    NextSource,
    /// 키보드 가리기
    Hide,
    /// 다른 쪽으로 간다 (.?123, #+=, 123, ABC·한글)
    Turn(Page),
}

/// 기능 키의 접근성 이름 (i18n/<언어>/bunt-keyboard.ftl의 메시지 ID). 글자 키는 그 글자가 이름이다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Name {
    Shift,
    CapsLock,
    Delete,
    Return,
    Space,
    NextKeyboard,
    Hide,
    Numbers,
    Symbols,
    Letters,
}

impl Name {
    pub fn message_id(self) -> &'static str {
        match self {
            Name::Shift => "keyboard-shift",
            Name::CapsLock => "keyboard-caps-lock",
            Name::Delete => "keyboard-delete",
            Name::Return => "keyboard-return",
            Name::Space => "keyboard-space",
            Name::NextKeyboard => "keyboard-next-keyboard",
            Name::Hide => "keyboard-hide",
            Name::Numbers => "keyboard-numbers",
            Name::Symbols => "keyboard-symbols",
            Name::Letters => "keyboard-letters",
        }
    }
}

/// 기능 키의 그림 (shell/data/icons의 기호 아이콘). 아이콘이 없으면 글자로 보인다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Shift,
    ShiftOn,
    CapsLock,
    Delete,
    Return,
    Globe,
    Hide,
}

impl Icon {
    pub fn icon_name(self) -> &'static str {
        match self {
            Icon::Shift => "org.buntos.KeyShift-symbolic",
            Icon::ShiftOn => "org.buntos.KeyShiftOn-symbolic",
            Icon::CapsLock => "org.buntos.KeyCapsLock-symbolic",
            Icon::Delete => "org.buntos.KeyDelete-symbolic",
            Icon::Return => "org.buntos.KeyReturn-symbolic",
            Icon::Globe => "org.buntos.KeyGlobe-symbolic",
            Icon::Hide => "org.buntos.KeyboardHide-symbolic",
        }
    }

    /// 아이콘을 찾지 못할 때 보이는 글자
    pub fn fallback(self) -> &'static str {
        match self {
            Icon::Shift => "⇧",
            Icon::ShiftOn => "⬆",
            Icon::CapsLock => "⇪",
            Icon::Delete => "⌫",
            Icon::Return => "⏎",
            Icon::Globe => "🌐",
            Icon::Hide => "⌨",
        }
    }
}

/// 키의 얼굴: 글자나 그림
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Face {
    /// 보이는 글자와, 그 위에 작게 보이는 Shift 글자(",!" 키의 "!")
    Text(String, Option<char>),
    Icon(Icon),
    /// 스페이스 막대 (iPadOS처럼 글자가 없다)
    Blank,
}

/// 키 하나
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub action: Action,
    pub face: Face,
    /// 기능 키의 접근성 이름. None이면 보이는 글자가 이름이다
    pub name: Option<Name>,
    /// 기능 키(어두운 바탕)인지
    pub function: bool,
    /// 켜진 기능 키(Shift)는 글자 키처럼 밝다
    pub on: bool,
    /// 칸 수 (1/4 키 단위)
    pub width: u8,
}

/// 줄의 한 자리: 키나 빈 칸(둘째 줄의 들여쓰기)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Key(Key),
    Gap(u8),
}

impl Slot {
    pub fn width(&self) -> u8 {
        match self {
            Slot::Key(key) => key.width,
            Slot::Gap(width) => *width,
        }
    }
}

/// 2벌식(KS X 5002): 라틴 글자 자리와 자모. Shift로 바뀌는 자모(ㅃ ㅉ ㄸ ㄲ ㅆ ㅒ ㅖ)는 대문자 자리에 있다.
/// fcitx5-hangul(libhangul)의 2벌식과 같다
const DUBEOLSIK: [(char, char); 33] = [
    ('q', 'ㅂ'),
    ('w', 'ㅈ'),
    ('e', 'ㄷ'),
    ('r', 'ㄱ'),
    ('t', 'ㅅ'),
    ('y', 'ㅛ'),
    ('u', 'ㅕ'),
    ('i', 'ㅑ'),
    ('o', 'ㅐ'),
    ('p', 'ㅔ'),
    ('a', 'ㅁ'),
    ('s', 'ㄴ'),
    ('d', 'ㅇ'),
    ('f', 'ㄹ'),
    ('g', 'ㅎ'),
    ('h', 'ㅗ'),
    ('j', 'ㅓ'),
    ('k', 'ㅏ'),
    ('l', 'ㅣ'),
    ('z', 'ㅋ'),
    ('x', 'ㅌ'),
    ('c', 'ㅊ'),
    ('v', 'ㅍ'),
    ('b', 'ㅠ'),
    ('n', 'ㅜ'),
    ('m', 'ㅡ'),
    ('Q', 'ㅃ'),
    ('W', 'ㅉ'),
    ('E', 'ㄸ'),
    ('R', 'ㄲ'),
    ('T', 'ㅆ'),
    ('O', 'ㅒ'),
    ('P', 'ㅖ'),
];

/// 그 라틴 글자 자리의 2벌식 자모
pub fn jamo(latin: char) -> Option<char> {
    DUBEOLSIK
        .iter()
        .find(|(key, _)| *key == latin)
        .map(|(_, jamo)| *jamo)
}

/// 글자 쪽의 세 줄 (QWERTY 자리)
const LETTER_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
/// 숫자 쪽(.?123)의 세 줄
const NUMBER_ROWS: [&str; 3] = ["1234567890", "@#$&*()'\"", "%-+=/;:,."];
/// 기호 쪽(#+=)의 세 줄. iPadOS 둘째 줄의 € £ ¥는 키 배열에 없어 빼고 `를 넣었다
const SYMBOL_ROWS: [&str; 3] = ["[]{}#%^*+=", "_\\|~<>`", ".,?!'\""];

/// 글자 키 하나. Shift를 누르면 대문자(한글은 바뀌는 자모만)를 보낸다
fn letter(script: Script, latin: char, shifted: bool) -> Key {
    let upper = latin.to_ascii_uppercase();
    let send = match script {
        Script::Latin if shifted => upper,
        Script::Hangul if shifted && jamo(upper).is_some() => upper,
        _ => latin,
    };
    let label = match script {
        Script::Latin => send,
        // 2벌식 표에 없는 자리는 없다 (LETTER_ROWS는 모두 표에 있다)
        Script::Hangul => jamo(send).unwrap_or(send),
    };
    char_key(send, label.to_string(), None, 4)
}

fn char_key(send: char, label: String, hint: Option<char>, width: u8) -> Key {
    Key {
        action: Action::Char(send),
        face: Face::Text(label, hint),
        name: None,
        function: false,
        on: false,
        width,
    }
}

fn symbols(text: &str, width: u8) -> impl Iterator<Item = Slot> + '_ {
    text.chars()
        .map(move |character| Slot::Key(char_key(character, character.to_string(), None, width)))
}

fn function(action: Action, face: Face, name: Name, width: u8) -> Slot {
    Slot::Key(Key {
        action,
        face,
        name: Some(name),
        function: true,
        on: false,
        width,
    })
}

fn turn(page: Page, label: &str, name: Name, width: u8) -> Slot {
    function(
        Action::Turn(page),
        Face::Text(label.into(), None),
        name,
        width,
    )
}

/// 맨 아래 줄: 지구본(입력 소스가 둘 이상일 때), 쪽 바꾸기, 스페이스, 쪽 바꾸기, 키보드 가리기
fn bottom_row(script: Script, page: Page, globe: bool) -> Vec<Slot> {
    let (to, label, name) = match page {
        Page::Letters => (Page::Numbers, ".?123", Name::Numbers),
        Page::Numbers | Page::Symbols => (
            Page::Letters,
            match script {
                Script::Hangul => "한글",
                Script::Latin => "ABC",
            },
            Name::Letters,
        ),
    };
    let mut row = Vec::new();
    // 지구본이 없으면 그 자리만큼 스페이스가 넓다
    let space = if globe {
        row.push(function(
            Action::NextSource,
            Face::Icon(Icon::Globe),
            Name::NextKeyboard,
            4,
        ));
        28
    } else {
        32
    };
    row.push(turn(to, label, name, 5));
    row.push(Slot::Key(Key {
        action: Action::Space,
        face: Face::Blank,
        name: Some(Name::Space),
        function: false,
        on: false,
        width: space,
    }));
    row.push(turn(to, label, name, 5));
    row.push(function(
        Action::Hide,
        Face::Icon(Icon::Hide),
        Name::Hide,
        4,
    ));
    row
}

fn delete() -> Slot {
    function(Action::Delete, Face::Icon(Icon::Delete), Name::Delete, 6)
}

fn enter() -> Slot {
    function(Action::Return, Face::Icon(Icon::Return), Name::Return, 8)
}

fn shift_key(shift: Shift) -> Slot {
    let (icon, name) = match shift {
        Shift::Off => (Icon::Shift, Name::Shift),
        Shift::Once => (Icon::ShiftOn, Name::Shift),
        Shift::Locked => (Icon::CapsLock, Name::CapsLock),
    };
    let Slot::Key(mut key) = function(Action::Shift, Face::Icon(icon), name, 5) else {
        unreachable!()
    };
    key.on = shift != Shift::Off;
    Slot::Key(key)
}

/// 그 쪽의 네 줄. `globe`는 입력 소스를 바꾸는 지구본 키를 둘지(입력 소스가 둘 이상일 때)
pub fn rows(script: Script, page: Page, shift: Shift, globe: bool) -> [Vec<Slot>; 4] {
    let shifted = shift != Shift::Off;
    match page {
        Page::Letters => {
            let letters = |row: &str| {
                row.chars()
                    .map(|latin| Slot::Key(letter(script, latin, shifted)))
                    .collect::<Vec<_>>()
            };
            let mut first = letters(LETTER_ROWS[0]);
            first.push(delete());
            let mut second = vec![Slot::Gap(2)];
            second.extend(letters(LETTER_ROWS[1]));
            second.push(enter());
            let mut third = vec![shift_key(shift)];
            third.extend(letters(LETTER_ROWS[2]));
            // 쉼표·마침표 키는 Shift를 누르면 느낌표·물음표다. 누르지 않았을 때는 그 글자를 위에 작게 보인다
            for (plain, upper) in [(',', '!'), ('.', '?')] {
                third.push(Slot::Key(if shifted {
                    char_key(upper, upper.to_string(), None, 4)
                } else {
                    char_key(plain, plain.to_string(), Some(upper), 4)
                }));
            }
            third.push(shift_key(shift));
            [first, second, third, bottom_row(script, page, globe)]
        }
        Page::Numbers => {
            let mut first: Vec<Slot> = symbols(NUMBER_ROWS[0], 4).collect();
            first.push(delete());
            let mut second = vec![Slot::Gap(2)];
            second.extend(symbols(NUMBER_ROWS[1], 4));
            second.push(enter());
            let mut third = vec![turn(Page::Symbols, "#+=", Name::Symbols, 5)];
            third.extend(symbols(NUMBER_ROWS[2], 4));
            third.push(turn(Page::Symbols, "#+=", Name::Symbols, 5));
            [first, second, third, bottom_row(script, page, globe)]
        }
        Page::Symbols => {
            let mut first: Vec<Slot> = symbols(SYMBOL_ROWS[0], 4).collect();
            first.push(delete());
            let mut second = vec![Slot::Gap(3)];
            second.extend(symbols(SYMBOL_ROWS[1], 5));
            second.push(enter());
            let mut third = vec![turn(Page::Numbers, "123", Name::Numbers, 5)];
            third.extend(symbols(SYMBOL_ROWS[2], 6));
            third.push(turn(Page::Numbers, "123", Name::Numbers, 5));
            [first, second, third, bottom_row(script, page, globe)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 줄의 글자 키가 보내는 것
    fn sent(row: &[Slot]) -> String {
        row.iter()
            .filter_map(|slot| match slot {
                Slot::Key(Key {
                    action: Action::Char(character),
                    ..
                }) => Some(*character),
                _ => None,
            })
            .collect()
    }

    /// 줄의 글자 키에 보이는 것
    fn labels(row: &[Slot]) -> String {
        row.iter()
            .filter_map(|slot| match slot {
                Slot::Key(Key {
                    action: Action::Char(_),
                    face: Face::Text(label, _),
                    ..
                }) => Some(label.clone()),
                _ => None,
            })
            .collect()
    }

    /// us 배열에서 그 글자를 내는 evdev 키 코드와 Shift (linux/input-event-codes.h). 컴포지터가 같은 표를
    /// xkb 키 배열로 찾는다(comp/src/osk.rs의 시험)
    fn us_key(character: char) -> (u32, bool) {
        let rows: [(&str, &str, u32); 4] = [
            ("1234567890-=", "!@#$%^&*()_+", 2),
            ("qwertyuiop[]", "QWERTYUIOP{}", 16),
            ("asdfghjkl;'`", "ASDFGHJKL:\"~", 30),
            ("\\zxcvbnm,./", "|ZXCVBNM<>?", 43),
        ];
        if character == ' ' {
            return (57, false);
        }
        for (plain, shifted, first) in rows {
            if let Some(index) = plain.chars().position(|c| c == character) {
                return (first + index as u32, false);
            }
            if let Some(index) = shifted.chars().position(|c| c == character) {
                return (first + index as u32, true);
            }
        }
        panic!("{character:?} is not on the us layout");
    }

    #[test]
    fn every_row_fills_the_keyboard_width() {
        for script in [Script::Hangul, Script::Latin] {
            for page in [Page::Letters, Page::Numbers, Page::Symbols] {
                for shift in [Shift::Off, Shift::Once, Shift::Locked] {
                    for globe in [true, false] {
                        for (index, row) in rows(script, page, shift, globe).iter().enumerate() {
                            let width: u32 = row.iter().map(|slot| u32::from(slot.width())).sum();
                            assert_eq!(
                                width,
                                u32::from(COLUMNS),
                                "{script:?} {page:?} {shift:?} globe={globe} row {index}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn dubeolsik_keys_send_the_latin_key_of_each_jamo() {
        let [first, second, third, _] = rows(Script::Hangul, Page::Letters, Shift::Off, true);
        assert_eq!(labels(&first), "ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ");
        assert_eq!(sent(&first), "qwertyuiop");
        assert_eq!(labels(&second), "ㅁㄴㅇㄹㅎㅗㅓㅏㅣ");
        assert_eq!(sent(&second), "asdfghjkl");
        assert_eq!(labels(&third), "ㅋㅌㅊㅍㅠㅜㅡ,.");
        assert_eq!(sent(&third), "zxcvbnm,.");
        // Shift: 바뀌는 자모만 대문자로 보낸다
        let [first, second, third, _] = rows(Script::Hangul, Page::Letters, Shift::Once, true);
        assert_eq!(labels(&first), "ㅃㅉㄸㄲㅆㅛㅕㅑㅒㅖ");
        assert_eq!(sent(&first), "QWERTyuiOP");
        assert_eq!(sent(&second), "asdfghjkl");
        assert_eq!(labels(&third), "ㅋㅌㅊㅍㅠㅜㅡ!?");
        assert_eq!(sent(&third), "zxcvbnm!?");
    }

    #[test]
    fn qwerty_keys_send_what_they_show() {
        let [first, second, third, _] = rows(Script::Latin, Page::Letters, Shift::Off, false);
        assert_eq!(sent(&first), "qwertyuiop");
        assert_eq!(sent(&second), "asdfghjkl");
        assert_eq!(sent(&third), "zxcvbnm,.");
        let [first, _, third, _] = rows(Script::Latin, Page::Letters, Shift::Locked, false);
        assert_eq!(labels(&first), "QWERTYUIOP");
        assert_eq!(sent(&first), "QWERTYUIOP");
        assert_eq!(sent(&third), "ZXCVBNM!?");
    }

    /// 모든 쪽의 글자 키가 us 배열의 맞는 키(키 코드와 Shift)를 낸다. 한글 자모는 2벌식 자리의 키다
    #[test]
    fn every_key_is_on_the_us_layout() {
        // 2벌식 자모 → 키 코드: ㅂ은 Q 키(16), ㅃ은 Shift+Q, ㅁ은 A 키(30), ㅡ는 M 키(50)
        let key_of = |jamo_char: char| {
            DUBEOLSIK
                .iter()
                .find(|(_, jamo)| *jamo == jamo_char)
                .map(|(latin, _)| us_key(*latin))
                .unwrap()
        };
        assert_eq!(key_of('ㅂ'), (16, false));
        assert_eq!(key_of('ㅃ'), (16, true));
        assert_eq!(key_of('ㅔ'), (25, false));
        assert_eq!(key_of('ㅖ'), (25, true));
        assert_eq!(key_of('ㅁ'), (30, false));
        assert_eq!(key_of('ㅣ'), (38, false));
        assert_eq!(key_of('ㅋ'), (44, false));
        assert_eq!(key_of('ㅡ'), (50, false));
        let mut seen = std::collections::BTreeSet::new();
        for script in [Script::Hangul, Script::Latin] {
            for page in [Page::Letters, Page::Numbers, Page::Symbols] {
                for shift in [Shift::Off, Shift::Once] {
                    for row in rows(script, page, shift, true) {
                        for character in sent(&row).chars() {
                            us_key(character);
                            seen.insert(character);
                        }
                    }
                }
            }
        }
        // 인쇄할 수 있는 ASCII 글자는 모두 어느 쪽에든 있다 (스페이스는 막대)
        let missing: String = ('!'..='~').filter(|c| !seen.contains(c)).collect();
        assert_eq!(missing, "");
    }

    #[test]
    fn number_and_symbol_pages_follow_ipados() {
        let [first, second, third, bottom] = rows(Script::Hangul, Page::Numbers, Shift::Off, true);
        assert_eq!(sent(&first), "1234567890");
        assert_eq!(sent(&second), "@#$&*()'\"");
        assert_eq!(sent(&third), "%-+=/;:,.");
        // 숫자 쪽의 셋째 줄 양 끝은 #+=, 맨 아래 줄의 쪽 바꾸기는 글자 쪽(한글)이다
        assert!(matches!(&third[0], Slot::Key(key) if key.action == Action::Turn(Page::Symbols)));
        assert!(bottom.iter().any(|slot| matches!(slot,
            Slot::Key(Key { action: Action::Turn(Page::Letters), face: Face::Text(label, _), .. })
                if label == "한글")));
        let [first, second, third, bottom] = rows(Script::Latin, Page::Symbols, Shift::Off, true);
        assert_eq!(sent(&first), "[]{}#%^*+=");
        assert_eq!(sent(&second), "_\\|~<>`");
        assert_eq!(sent(&third), ".,?!'\"");
        assert!(bottom.iter().any(|slot| matches!(slot,
            Slot::Key(Key { action: Action::Turn(Page::Letters), face: Face::Text(label, _), .. })
                if label == "ABC")));
    }

    #[test]
    fn function_keys_have_names_and_the_globe_is_optional() {
        for page in [Page::Letters, Page::Numbers, Page::Symbols] {
            for row in rows(Script::Hangul, page, Shift::Off, true) {
                for slot in row {
                    if let Slot::Key(key) = slot
                        && !matches!(key.action, Action::Char(_))
                    {
                        assert!(key.name.is_some(), "{key:?}");
                    }
                }
            }
        }
        let has_globe = |globe| {
            rows(Script::Latin, Page::Letters, Shift::Off, globe)[3]
                .iter()
                .any(|slot| matches!(slot, Slot::Key(key) if key.action == Action::NextSource))
        };
        assert!(has_globe(true));
        assert!(!has_globe(false));
        // Caps Lock은 이름과 그림이 따로다
        let [_, _, third, _] = rows(Script::Latin, Page::Letters, Shift::Locked, false);
        assert!(matches!(&third[0], Slot::Key(key)
            if key.name == Some(Name::CapsLock) && key.face == Face::Icon(Icon::CapsLock) && key.on));
    }
}
