// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 단축키 글을 읽고 쓰고 macOS처럼 기호로 보인다. 글은 GTK 가속기 문법이다: 조합 키
//! `<Control><Alt><Shift><Super>` 다음에 xkb keysym 이름(소문자 글자, "3", "space", "F4",
//! "bracketleft"). 컴포지터(GetShortcuts)는 조합 키를 이 순서로 돌려주고, 시트도 이 순서로 보낸다.
//! 기호는 macOS 순서(⌃⌥⇧⌘ 다음 키)이고 ⌘가 Super 키다

use std::{borrow::Cow, fmt};

/// 조합 키. ⌥는 Alt, ⌘는 Super 키다
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub command: bool,
}

impl Modifiers {
    /// ⌃⌥⇧⌘ 순서의 기호
    pub fn symbols(self) -> String {
        [
            (self.control, '⌃'),
            (self.alt, '⌥'),
            (self.shift, '⇧'),
            (self.command, '⌘'),
        ]
        .into_iter()
        .filter_map(|(held, symbol)| held.then_some(symbol))
        .collect()
    }

    /// ⌃, ⌥, ⌘ 가운데 하나가 있다. ⇧만으로는 단축키가 되지 않는다
    pub fn has_non_shift(self) -> bool {
        self.control || self.alt || self.command
    }

    pub fn count(self) -> usize {
        [self.control, self.alt, self.shift, self.command]
            .into_iter()
            .filter(|held| *held)
            .count()
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            control: self.control || other.control,
            alt: self.alt || other.alt,
            shift: self.shift || other.shift,
            command: self.command || other.command,
        }
    }

    pub fn without(self, other: Self) -> Self {
        Self {
            control: self.control && !other.control,
            alt: self.alt && !other.alt,
            shift: self.shift && !other.shift,
            command: self.command && !other.command,
        }
    }
}

/// 번역하는 키 이름
pub struct KeyNames {
    /// 스페이스 바 ("스페이스", "Space")
    pub space: String,
    /// 한/영 키
    pub hangul: String,
}

/// 단축키 하나: 조합 키와 키(xkb keysym 이름)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accelerator {
    pub modifiers: Modifiers,
    pub key: String,
}

impl Accelerator {
    /// GTK 가속기 글을 읽는다. 조합 키 이름은 컴포지터처럼 대소문자를 가리지 않고 Ctrl·Primary,
    /// Mod1, Meta·Mod4·Hyper도 받는다. 모르는 조합 키가 있으면 None이다. 키 없이 조합 키만 둘
    /// 이상이면("<Control><Shift>") 조합 키만 눌렀다 떼는 단축키다(입력 소스 전환)
    pub fn parse(text: &str) -> Option<Self> {
        let mut modifiers = Modifiers::default();
        let mut rest = text.trim();
        while let Some(tail) = rest.strip_prefix('<') {
            let (name, after) = tail.split_once('>')?;
            match name.to_ascii_lowercase().as_str() {
                "control" | "ctrl" | "primary" => modifiers.control = true,
                "alt" | "mod1" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "super" | "meta" | "mod4" | "hyper" => modifiers.command = true,
                _ => return None,
            }
            rest = after;
        }
        let key_or_modifiers_only = !rest.is_empty() || modifiers.count() >= 2;
        (key_or_modifiers_only && !rest.contains(['<', '>'])).then(|| Self {
            modifiers,
            key: rest.to_owned(),
        })
    }

    /// macOS 기호: "⇧⌘3", "⌃⌘F", "⌘스페이스", "⌥F4"
    pub fn symbols(&self, names: &KeyNames) -> String {
        format!(
            "{}{}",
            self.modifiers.symbols(),
            key_symbol(&self.key, names)
        )
    }
}

/// 컴포지터에 보내는 글. 조합 키는 Control, Alt, Shift, Super 순서다
impl fmt::Display for Accelerator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Modifiers {
            control,
            alt,
            shift,
            command,
        } = self.modifiers;
        for (held, name) in [
            (control, "<Control>"),
            (alt, "<Alt>"),
            (shift, "<Shift>"),
            (command, "<Super>"),
        ] {
            if held {
                f.write_str(name)?;
            }
        }
        f.write_str(&self.key)
    }
}

/// 단축키 글을 기호로 보인다. 빈 글(단축키 없음)은 빈 글이고, 읽지 못하는 글은 그대로 보인다
pub fn display(text: &str, names: &KeyNames) -> String {
    match Accelerator::parse(text) {
        Some(accelerator) => accelerator.symbols(names),
        None => text.trim().to_owned(),
    }
}

/// 키 기호. 글자는 대문자로, 숫자와 기능 키(F1)는 그대로, 모르는 키는 keysym 이름으로 보인다
fn key_symbol<'a>(key: &'a str, names: &'a KeyNames) -> Cow<'a, str> {
    let symbol = match key {
        "Tab" | "ISO_Left_Tab" => "⇥",
        "Return" => "↩",
        "KP_Enter" => "⌤",
        "Escape" => "⎋",
        "BackSpace" => "⌫",
        "Delete" | "KP_Delete" => "⌦",
        "Left" => "←",
        "Right" => "→",
        "Up" => "↑",
        "Down" => "↓",
        "Home" => "↖",
        "End" => "↘",
        "Page_Up" | "Prior" => "⇞",
        "Page_Down" | "Next" => "⇟",
        "space" => names.space.as_str(),
        "Hangul" => names.hangul.as_str(),
        "grave" => "`",
        "bracketleft" => "[",
        "bracketright" => "]",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        "backslash" => "\\",
        "semicolon" => ";",
        "apostrophe" => "'",
        "equal" => "=",
        "minus" => "-",
        "plus" => "+",
        _ if key.chars().count() == 1 => return Cow::Owned(key.to_uppercase()),
        _ => key,
    };
    Cow::Borrowed(symbol)
}

/// GDK keysym 이름을 xkb 이름으로 맞춘다. 이름 없는 유니코드 키를 GDK는 "U+20AC"로, xkb는
/// "U20AC"로 쓴다
pub fn keysym_name(gdk_name: &str) -> Cow<'_, str> {
    match gdk_name.strip_prefix("U+") {
        Some(code) => Cow::Owned(format!("U{code}")),
        None => Cow::Borrowed(gdk_name),
    }
}

/// 기능 키(F1~F35). ⌃⌥⌘ 없이 혼자 단축키가 될 수 있다
fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=35).contains(&number))
}

/// 단축키를 기록할 때 누른 조합을 어떻게 할지
#[derive(Debug, PartialEq, Eq)]
pub enum Recorded {
    /// Esc만 눌렀다: 기록을 그만두고 그대로 둔다
    Cancel,
    /// ⌫만 눌렀다: 단축키를 지운다
    Clear,
    Set(Accelerator),
    /// ⌃, ⌥, ⌘ 없이 눌렀다 (기능 키는 혼자 써도 된다)
    NeedsModifier,
    /// 이 분류(텍스트 편집)에는 ⇧를 쓰지 않는다. ⇧를 더하면 선택 영역을 넓히기 때문이다
    ShiftNotAllowed,
    /// 조합 키만 눌렀다 뗐다. 이 단축키는 다른 키가 있어야 한다
    NeedsKey,
}

/// 누른 조합 키와 키(조합 키 없이 읽은 keysym 이름)로 할 일을 정한다. `input`이면 입력 소스
/// 전환이라 ⇧만 함께 눌러도 된다(⇧스페이스, 한국어 Windows의 한/영 전환)
pub fn record(modifiers: Modifiers, key: &str, allow_shift: bool, input: bool) -> Recorded {
    let bare = modifiers == Modifiers::default();
    let shift_alone = input && modifiers.shift;
    if bare && key == "Escape" {
        Recorded::Cancel
    } else if bare && key == "BackSpace" {
        Recorded::Clear
    } else if !modifiers.has_non_shift() && !is_function_key(key) && !shift_alone {
        Recorded::NeedsModifier
    } else if modifiers.shift && !allow_shift {
        Recorded::ShiftNotAllowed
    } else {
        Recorded::Set(Accelerator {
            modifiers,
            key: key.to_owned(),
        })
    }
}

/// 다른 키 없이 조합 키만 눌렀다 모두 뗐을 때 할 일. `peak`는 그동안 함께 누른 조합 키다.
/// 입력 소스 전환은 둘 이상이면 그 조합을 단축키로 하고(Windows의 Ctrl+Shift처럼), 다른 단축키는
/// 다른 키가 있어야 한다고 알린다. 조합 키 하나를 눌렀다 뗀 것은 넘긴다
pub fn record_modifiers(peak: Modifiers, input: bool) -> Option<Recorded> {
    if peak.count() < 2 {
        None
    } else if input {
        Some(Recorded::Set(Accelerator {
            modifiers: peak,
            key: String::new(),
        }))
    } else {
        Some(Recorded::NeedsKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> KeyNames {
        KeyNames {
            space: "스페이스".into(),
            hangul: "한/영".into(),
        }
    }

    fn symbols(text: &str) -> String {
        display(text, &names())
    }

    const COMMAND: Modifiers = Modifiers {
        control: false,
        alt: false,
        shift: false,
        command: true,
    };
    const SHIFT: Modifiers = Modifiers {
        control: false,
        alt: false,
        shift: true,
        command: false,
    };
    const OPTION: Modifiers = Modifiers {
        control: false,
        alt: true,
        shift: false,
        command: false,
    };

    #[test]
    fn parses_compositor_accelerators() {
        let accelerator = Accelerator::parse("<Shift><Super>3").unwrap();
        assert_eq!(accelerator.modifiers, SHIFT.union(COMMAND));
        assert_eq!(accelerator.key, "3");
        let accelerator = Accelerator::parse("<Control><Alt><Shift><Super>Tab").unwrap();
        assert!(accelerator.modifiers.control && accelerator.modifiers.alt);
        assert_eq!(accelerator.key, "Tab");
        assert_eq!(
            Accelerator::parse("Hangul").unwrap().modifiers,
            Modifiers::default()
        );
        // 조합 키 이름은 대소문자를 가리지 않고, 다른 이름도 읽는다
        assert_eq!(
            Accelerator::parse("<ctrl><SUPER>q").unwrap().to_string(),
            "<Control><Super>q"
        );
        assert_eq!(
            Accelerator::parse("<Primary><Mod1><Meta>w")
                .unwrap()
                .to_string(),
            "<Control><Alt><Super>w"
        );
    }

    #[test]
    fn unicode_keysyms_use_xkb_names() {
        assert_eq!(keysym_name("U+20AC"), "U20AC");
        assert_eq!(keysym_name("bracketleft"), "bracketleft");
        assert_eq!(keysym_name("U"), "U");
    }

    #[test]
    fn rejects_broken_accelerators() {
        for text in [
            "",
            "  ",
            "<Super>",
            "<Bogus>a",
            "<Super",
            "<Super>a>b",
            "a<Shift>",
        ] {
            assert_eq!(Accelerator::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn writes_modifiers_in_canonical_order() {
        let accelerator = Accelerator::parse("<Super><Shift><Alt><Control>F1").unwrap();
        assert_eq!(accelerator.to_string(), "<Control><Alt><Shift><Super>F1");
        assert_eq!(
            Accelerator::parse("<Super>space").unwrap().to_string(),
            "<Super>space"
        );
    }

    #[test]
    fn shows_macos_symbols() {
        for (text, expected) in [
            ("<Shift><Super>3", "⇧⌘3"),
            ("<Control><Shift><Super>4", "⌃⇧⌘4"),
            ("<Control><Super>f", "⌃⌘F"),
            ("<Super>space", "⌘스페이스"),
            ("<Control>space", "⌃스페이스"),
            ("<Alt>F4", "⌥F4"),
            ("<Alt><Super>d", "⌥⌘D"),
            ("<Super>bracketleft", "⌘["),
            ("<Super>bracketright", "⌘]"),
            ("<Alt>BackSpace", "⌥⌫"),
            ("<Alt>Delete", "⌥⌦"),
            ("<Super>BackSpace", "⌘⌫"),
            ("<Super>Left", "⌘←"),
            ("<Super>Right", "⌘→"),
            ("<Alt>Up", "⌥↑"),
            ("<Alt>Down", "⌥↓"),
            ("<Super>Tab", "⌘⇥"),
            ("<Shift><Super>Tab", "⇧⌘⇥"),
            ("<Super>grave", "⌘`"),
            ("<Alt>Return", "⌥↩"),
            ("<Super>Escape", "⌘⎋"),
            ("<Super>Home", "⌘↖"),
            ("<Super>End", "⌘↘"),
            ("<Super>Page_Up", "⌘⇞"),
            ("<Super>Page_Down", "⌘⇟"),
            ("<Super>comma", "⌘,"),
            ("<Super>period", "⌘."),
            ("<Super>slash", "⌘/"),
            ("<Super>equal", "⌘="),
            ("<Super>minus", "⌘-"),
            ("<Super>0", "⌘0"),
            ("F12", "F12"),
            ("Hangul", "한/영"),
            ("<Super>XF86AudioMute", "⌘XF86AudioMute"),
        ] {
            assert_eq!(symbols(text), expected, "{text}");
        }
    }

    #[test]
    fn shows_unreadable_text_as_is() {
        assert_eq!(symbols(""), "");
        assert_eq!(symbols("<Bogus>x"), "<Bogus>x");
        assert_eq!(symbols(" <Super> "), "<Super>");
    }

    #[test]
    fn modifier_sets() {
        assert_eq!(Modifiers::default().symbols(), "");
        let all = Modifiers {
            control: true,
            alt: true,
            shift: true,
            command: true,
        };
        assert_eq!(all.symbols(), "⌃⌥⇧⌘");
        assert_eq!(all.without(SHIFT).symbols(), "⌃⌥⌘");
        assert_eq!(SHIFT.union(COMMAND).symbols(), "⇧⌘");
        assert!(!SHIFT.has_non_shift());
        assert!(OPTION.has_non_shift());
    }

    #[test]
    fn recording_escape_and_backspace() {
        assert_eq!(
            record(Modifiers::default(), "Escape", true, false),
            Recorded::Cancel
        );
        assert_eq!(
            record(Modifiers::default(), "BackSpace", true, false),
            Recorded::Clear
        );
        // 조합 키와 함께면 단축키다 (⌥⌫: 왼쪽 단어 삭제)
        assert_eq!(
            record(OPTION, "BackSpace", false, false),
            Recorded::Set(Accelerator::parse("<Alt>BackSpace").unwrap())
        );
        assert_eq!(
            record(COMMAND, "Escape", true, false),
            Recorded::Set(Accelerator::parse("<Super>Escape").unwrap())
        );
    }

    #[test]
    fn recording_needs_a_modifier_except_function_keys() {
        assert_eq!(
            record(Modifiers::default(), "a", true, false),
            Recorded::NeedsModifier
        );
        assert_eq!(record(SHIFT, "a", true, false), Recorded::NeedsModifier);
        assert_eq!(
            record(Modifiers::default(), "Tab", true, false),
            Recorded::NeedsModifier
        );
        assert_eq!(
            record(Modifiers::default(), "Find", true, false),
            Recorded::NeedsModifier
        );
        assert_eq!(
            record(Modifiers::default(), "F5", true, false),
            Recorded::Set(Accelerator::parse("F5").unwrap())
        );
        assert_eq!(
            record(SHIFT, "F11", true, false),
            Recorded::Set(Accelerator::parse("<Shift>F11").unwrap())
        );
        assert_eq!(
            record(SHIFT.union(COMMAND), "3", true, false),
            Recorded::Set(Accelerator::parse("<Shift><Super>3").unwrap())
        );
    }

    #[test]
    fn text_shortcuts_reject_shift() {
        assert_eq!(
            record(SHIFT.union(OPTION), "Left", false, false),
            Recorded::ShiftNotAllowed
        );
        assert_eq!(
            record(OPTION, "Left", false, false),
            Recorded::Set(Accelerator::parse("<Alt>Left").unwrap())
        );
    }

    #[test]
    fn input_sources_take_modifier_keys_alone_and_shift_space() {
        let control_shift = Modifiers {
            control: true,
            shift: true,
            ..Modifiers::default()
        };
        // 조합 키만으로 된 단축키 (Windows의 Ctrl+Shift)
        let alone = Accelerator::parse("<Control><Shift>").unwrap();
        assert_eq!(alone.key, "");
        assert_eq!(alone.to_string(), "<Control><Shift>");
        assert_eq!(alone.symbols(&names()), "⌃⇧");
        assert_eq!(
            record_modifiers(control_shift, true),
            Some(Recorded::Set(alone))
        );
        // 다른 단축키는 다른 키가 있어야 한다고 알리고, 조합 키 하나는 넘긴다
        assert_eq!(
            record_modifiers(control_shift, false),
            Some(Recorded::NeedsKey)
        );
        assert_eq!(record_modifiers(SHIFT, true), None);
        // 입력 소스 전환은 ⇧스페이스도 된다
        assert_eq!(
            record(SHIFT, "space", true, true),
            Recorded::Set(Accelerator::parse("<Shift>space").unwrap())
        );
        assert_eq!(record(SHIFT, "space", true, false), Recorded::NeedsModifier);
    }
}
