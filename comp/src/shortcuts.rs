// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 키보드 단축키 표와 해석. 컴포지터가 직접 하는 일(앱 전환, 스크린샷, 모든 앱 보기 …)과, macOS식 ⌘
//! 단축키를 포커스를 가진 앱이 아는 리눅스 단축키로 옮겨 보내는 일(⌘C → Ctrl+C, 터미널에는
//! Ctrl+Shift+C)을 한 표에 둔다. Ctrl 단축키는 앱이 원래 받으므로 그대로 둔다(리눅스 방식도 된다).
//! 키 조합은 GTK 가속기 문자열("<Shift><Super>3")로 적고, 시스템 설정에서 바꾼 값(prefs의
//! [shortcuts])이 기본값보다 앞선다.

use std::collections::BTreeMap;

use smithay::input::keyboard::{Keysym, ModifiersState, xkb};

use crate::prefs::ShortcutPref;

/// 단축키에 쓰는 수정 키 (Caps Lock, Num Lock은 보지 않는다)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// ⌘ (Super, PC 키보드의 Windows 키)
    pub logo: bool,
}

impl Mods {
    pub fn of(state: &ModifiersState) -> Self {
        Self {
            ctrl: state.ctrl,
            alt: state.alt,
            shift: state.shift,
            logo: state.logo,
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            ctrl: self.ctrl || other.ctrl,
            alt: self.alt || other.alt,
            shift: self.shift || other.shift,
            logo: self.logo || other.logo,
        }
    }

    pub fn count(self) -> usize {
        [self.ctrl, self.alt, self.shift, self.logo]
            .into_iter()
            .filter(|held| *held)
            .count()
    }
}

const NONE: Mods = Mods {
    ctrl: false,
    alt: false,
    shift: false,
    logo: false,
};
const CTRL: Mods = Mods { ctrl: true, ..NONE };
const CTRL_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    ..NONE
};
const ALT: Mods = Mods { alt: true, ..NONE };
const SHIFT: Mods = Mods {
    shift: true,
    ..NONE
};

/// 키 조합: 수정 키와 수정하지 않은 키 기호 (글자는 소문자). 키가 NoSymbol이면 수정 키만으로 된
/// 조합이다(⌃⇧, Windows의 입력 언어 전환처럼 다른 키 없이 눌렀다 뗄 때 쓴다)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Accel {
    pub mods: Mods,
    pub key: Keysym,
}

impl Accel {
    /// GTK 가속기 문자열을 읽는다. 수정 키 이름은 대소문자를 가리지 않고 Ctrl·Primary, Mod1,
    /// Meta·Mod4·Hyper도 받는다. 키 이름은 xkb 기호 이름이다. 키 없이 수정 키만 둘 이상
    /// 적으면("<Control><Shift>") 수정 키만으로 된 조합이다
    pub fn parse(text: &str) -> Option<Self> {
        let mut mods = NONE;
        let mut rest = text.trim();
        while let Some(stripped) = rest.strip_prefix('<') {
            let (name, after) = stripped.split_once('>')?;
            match name.to_ascii_lowercase().as_str() {
                "control" | "ctrl" | "primary" => mods.ctrl = true,
                "alt" | "mod1" => mods.alt = true,
                "shift" => mods.shift = true,
                "super" | "meta" | "mod4" | "hyper" => mods.logo = true,
                _ => return None,
            }
            rest = after;
        }
        if rest.is_empty() {
            // 수정 키 하나만으로는 칠 때마다 걸리므로 받지 않는다
            return (mods.count() >= 2).then_some(Self {
                mods,
                key: Keysym::NoSymbol,
            });
        }
        let mut key = xkb::keysym_from_name(rest, xkb::KEYSYM_NO_FLAGS);
        if key == Keysym::NoSymbol {
            key = xkb::keysym_from_name(rest, xkb::KEYSYM_CASE_INSENSITIVE);
        }
        if key == Keysym::NoSymbol || key.is_modifier_key() {
            return None;
        }
        Some(Self {
            mods,
            key: normalize(key),
        })
    }

    /// 정해진 순서(Control, Alt, Shift, Super)로 쓴 문자열
    pub fn name(&self) -> String {
        let mut text = String::new();
        for (on, name) in [
            (self.mods.ctrl, "<Control>"),
            (self.mods.alt, "<Alt>"),
            (self.mods.shift, "<Shift>"),
            (self.mods.logo, "<Super>"),
        ] {
            if on {
                text.push_str(name);
            }
        }
        if !self.is_modifiers_only() {
            text.push_str(&xkb::keysym_get_name(self.key));
        }
        text
    }

    /// 수정 키만으로 된 조합(⌃⇧)
    pub fn is_modifiers_only(&self) -> bool {
        self.key == Keysym::NoSymbol
    }
}

/// 영문 대문자를 소문자로 바꾼다. 단축키는 수정하지 않은 첫 단계의 기호로 맞춘다
pub fn normalize(key: Keysym) -> Keysym {
    match key.raw() {
        raw @ 0x41..=0x5a => Keysym::new(raw + 0x20),
        _ => key,
    }
}

/// 앱에 보내는 키 하나 (수정 키와 키 기호)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub mods: Mods,
    pub sym: Keysym,
}

/// 표 안에서 쓰는 키. 상수만으로 이루어져 `&[key!(..)]`가 정적 수명을 얻는다
macro_rules! key {
    ($mods:expr, $sym:ident) => {
        Key {
            mods: $mods,
            sym: Keysym::$sym,
        }
    };
}

/// 컴포지터가 직접 하는 일
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Builtin {
    /// 모든 앱 패널을 연다(검색 칸에 포커스). 셸이 한다
    ShowLauncher,
    /// Dock 자동 가리기를 켜고 끈다. 셸이 한다
    ToggleDockAutohide,
    NextApp,
    PreviousApp,
    /// 같은 앱의 다음 윈도우
    NextWindow,
    PreviousWindow,
    /// 앱의 윈도우를 모두 닫는다
    QuitApp,
    Minimize,
    /// 앱의 윈도우를 모두 가린다
    HideApp,
    /// 다른 앱의 윈도우를 모두 가린다
    HideOthers,
    ToggleFullscreen,
    CloseWindow,
    OpenTerminal,
    SwitchInputSource,
    Screenshot {
        area: bool,
        clipboard: bool,
    },
    /// ⇧⌘5 스크린샷 도구 막대를 연다. 셸이 한다
    ScreenshotToolbar,
    /// Esc: 스크린샷 영역 고르기를 그만둔다 (고르는 동안만, 표에 없는 고정 키)
    CancelScreenshot,
    /// 스페이스: 스크린샷 영역 고르기와 창 고르기를 바꾼다 (고르는 동안만, 표에 없는 고정 키)
    ScreenshotWindowMode,
    /// ⌘⇥ 앱 전환 창이 떠 있는 동안 ⌘와 함께 누른 키 (표에 없는 고정 키)
    Switcher(SwitcherKey),
    /// Ctrl+Alt+Backspace: 세션 끝내기 (표에 없는 고정 키)
    Quit,
    /// Ctrl+Alt+F1..F12: 가상 터미널 전환 (표에 없는 고정 키)
    SwitchVt(i32),
    /// 화면 잠금 (session_lock.rs)
    LockScreen,
    /// 로그아웃. `confirm`이면 셸이 확인 창을 띄우고, 아니면(⌥⇧⌘Q) 묻지 않고 한다 (session.rs)
    LogOut {
        confirm: bool,
    },
    /// 응용 프로그램 강제 종료 창. 셸이 띄운다
    ForceQuit,
    /// 음량 키 (표에 없는 고정 키). 셸이 음량을 바꾸고 표시를 띄운다
    Volume(VolumeKey),
}

/// 기기와 키보드의 음량 키
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VolumeKey {
    Up,
    Down,
    Mute,
}

impl VolumeKey {
    pub fn of(key: Keysym) -> Option<Self> {
        match key {
            Keysym::XF86_AudioRaiseVolume => Some(Self::Up),
            Keysym::XF86_AudioLowerVolume => Some(Self::Down),
            Keysym::XF86_AudioMute => Some(Self::Mute),
            _ => None,
        }
    }

    /// 셸에 알리는 이름 (ShortcutActivated)
    pub fn id(self) -> &'static str {
        match self {
            Self::Up => "sound.volume-up",
            Self::Down => "sound.volume-down",
            Self::Mute => "sound.mute",
        }
    }

    /// 누르고 있으면 되풀이하는지
    pub fn repeats(self) -> bool {
        self != Self::Mute
    }
}

/// 앱 전환 창의 키 (macOS와 같다)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitcherKey {
    /// Esc: 바꾸지 않고 닫는다
    Cancel,
    /// Q: 고른 앱을 끝낸다
    Quit,
    /// H: 고른 앱을 가린다
    Hide,
    /// ←, →, `: 앞뒤 앱을 고른다
    Step(isize),
}

impl SwitcherKey {
    /// ⌘와 함께 누른 키(배열과 상관없는 라틴 기호)가 앱 전환 창의 키인지
    pub fn of(key: Keysym) -> Option<Self> {
        match key {
            Keysym::Escape => Some(Self::Cancel),
            Keysym::q => Some(Self::Quit),
            Keysym::h => Some(Self::Hide),
            Keysym::Left | Keysym::grave => Some(Self::Step(-1)),
            Keysym::Right => Some(Self::Step(1)),
            _ => None,
        }
    }
}

/// 터미널에서 할 일. 터미널은 Ctrl+글자를 제어 문자로 쓰므로 앱과 다르게 보낸다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InTerminal {
    /// 터미널(foot)의 단축키를 보낸다
    Keys(&'static [Key]),
    /// 컴포지터가 한다
    Run(Builtin),
    /// 누른 키를 그대로 보낸다 (⌥⌫를 받은 셸이 단어를 지운다)
    Forward,
    /// 아무것도 하지 않는다
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Run(Builtin),
    /// 포커스를 가진 앱에 키를 보낸다
    App {
        keys: &'static [Key],
        terminal: InTerminal,
    },
    /// 글을 입력하는 중에만 앱에 키를 보낸다. `select`면 ⇧를 함께 눌렀을 때 보낼 키에도 ⇧를 더해
    /// 선택 영역을 넓힌다
    Text {
        keys: &'static [Key],
        terminal: InTerminal,
        select: bool,
    },
    /// 켜고 끄기만 한다: 표에 없는 ⌘ 단축키를 앱의 Ctrl 단축키로 보낸다 (⌘B → Ctrl+B)
    OtherCommandKeys,
}

/// 시스템 설정의 키보드 단축키 시트가 나누어 보이는 묶음
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Launcher,
    Windows,
    Input,
    Screenshots,
    Text,
    App,
    /// 화면 잠금과 로그아웃
    Session,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Launcher,
        Category::Windows,
        Category::Input,
        Category::Screenshots,
        Category::Text,
        Category::App,
        Category::Session,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Category::Launcher => "launcher",
            Category::Windows => "windows",
            Category::Input => "input",
            Category::Screenshots => "screenshots",
            Category::Text => "text",
            Category::App => "app",
            Category::Session => "session",
        }
    }
}

/// 표의 한 줄
#[derive(Clone, Copy, Debug)]
pub struct Def {
    pub id: &'static str,
    pub category: Category,
    /// 기본 키 (GTK 가속기). 켜고 끄기만 하는 항목은 빈 문자열
    pub default: &'static str,
    pub action: Action,
    /// 누르고 있으면 되풀이한다 (키 반복 설정을 따른다)
    pub repeat: bool,
}

const fn run(id: &'static str, category: Category, default: &'static str, builtin: Builtin) -> Def {
    Def {
        id,
        category,
        default,
        action: Action::Run(builtin),
        repeat: false,
    }
}

const fn app(
    id: &'static str,
    default: &'static str,
    keys: &'static [Key],
    terminal: InTerminal,
    repeat: bool,
) -> Def {
    Def {
        id,
        category: Category::App,
        default,
        action: Action::App { keys, terminal },
        repeat,
    }
}

const fn text(
    id: &'static str,
    default: &'static str,
    keys: &'static [Key],
    terminal: InTerminal,
    select: bool,
) -> Def {
    Def {
        id,
        category: Category::Text,
        default,
        action: Action::Text {
            keys,
            terminal,
            select,
        },
        repeat: true,
    }
}

/// 단축키 표. 기본값은 macOS 26을 따른다 (Mac 키보드 단축키, support.apple.com/102650).
/// 시스템 설정은 이 순서대로 보인다
pub const DEFS: &[Def] = &[
    run(
        "launcher.show",
        Category::Launcher,
        "<Super>space",
        Builtin::ShowLauncher,
    ),
    run(
        "launcher.dock-autohide",
        Category::Launcher,
        "<Alt><Super>d",
        Builtin::ToggleDockAutohide,
    ),
    run(
        "windows.next-app",
        Category::Windows,
        "<Super>Tab",
        Builtin::NextApp,
    ),
    run(
        "windows.previous-app",
        Category::Windows,
        "<Shift><Super>Tab",
        Builtin::PreviousApp,
    ),
    run(
        "windows.next-window",
        Category::Windows,
        "<Super>grave",
        Builtin::NextWindow,
    ),
    run(
        "windows.previous-window",
        Category::Windows,
        "<Shift><Super>grave",
        Builtin::PreviousWindow,
    ),
    run(
        "windows.quit-app",
        Category::Windows,
        "<Super>q",
        Builtin::QuitApp,
    ),
    run(
        "windows.minimize",
        Category::Windows,
        "<Super>m",
        Builtin::Minimize,
    ),
    run(
        "windows.hide-app",
        Category::Windows,
        "<Super>h",
        Builtin::HideApp,
    ),
    run(
        "windows.hide-others",
        Category::Windows,
        "<Alt><Super>h",
        Builtin::HideOthers,
    ),
    run(
        "windows.fullscreen",
        Category::Windows,
        "<Control><Super>f",
        Builtin::ToggleFullscreen,
    ),
    // 리눅스에서 익숙한 키
    run(
        "windows.close",
        Category::Windows,
        "<Alt>F4",
        Builtin::CloseWindow,
    ),
    run(
        "windows.terminal",
        Category::Windows,
        "<Alt>Return",
        Builtin::OpenTerminal,
    ),
    // macOS의 "이전 입력 소스 선택". 한/영 키는 표에 없는 고정 키다
    run(
        "input.switch-source",
        Category::Input,
        "<Control>space",
        Builtin::SwitchInputSource,
    ),
    run(
        "screenshot.screen",
        Category::Screenshots,
        "<Shift><Super>3",
        Builtin::Screenshot {
            area: false,
            clipboard: false,
        },
    ),
    run(
        "screenshot.screen-clipboard",
        Category::Screenshots,
        "<Control><Shift><Super>3",
        Builtin::Screenshot {
            area: false,
            clipboard: true,
        },
    ),
    run(
        "screenshot.area",
        Category::Screenshots,
        "<Shift><Super>4",
        Builtin::Screenshot {
            area: true,
            clipboard: false,
        },
    ),
    run(
        "screenshot.area-clipboard",
        Category::Screenshots,
        "<Control><Shift><Super>4",
        Builtin::Screenshot {
            area: true,
            clipboard: true,
        },
    ),
    run(
        "screenshot.toolbar",
        Category::Screenshots,
        "<Shift><Super>5",
        Builtin::ScreenshotToolbar,
    ),
    // 글 입력 칸의 커서 움직임과 지우기 (macOS의 텍스트 편집 단축키)
    text(
        "text.line-start",
        "<Super>Left",
        &[key!(NONE, Home)],
        InTerminal::Keys(&[key!(NONE, Home)]),
        true,
    ),
    text(
        "text.line-end",
        "<Super>Right",
        &[key!(NONE, End)],
        InTerminal::Keys(&[key!(NONE, End)]),
        true,
    ),
    text(
        "text.document-start",
        "<Super>Up",
        &[key!(CTRL, Home)],
        InTerminal::Nothing,
        true,
    ),
    text(
        "text.document-end",
        "<Super>Down",
        &[key!(CTRL, End)],
        InTerminal::Nothing,
        true,
    ),
    text(
        "text.word-left",
        "<Alt>Left",
        &[key!(CTRL, Left)],
        InTerminal::Keys(&[key!(CTRL, Left)]),
        true,
    ),
    text(
        "text.word-right",
        "<Alt>Right",
        &[key!(CTRL, Right)],
        InTerminal::Keys(&[key!(CTRL, Right)]),
        true,
    ),
    text(
        "text.paragraph-up",
        "<Alt>Up",
        &[key!(CTRL, Up)],
        InTerminal::Forward,
        true,
    ),
    text(
        "text.paragraph-down",
        "<Alt>Down",
        &[key!(CTRL, Down)],
        InTerminal::Forward,
        true,
    ),
    text(
        "text.delete-word",
        "<Alt>BackSpace",
        &[key!(CTRL, BackSpace)],
        InTerminal::Forward,
        false,
    ),
    text(
        "text.delete-word-forward",
        "<Alt>Delete",
        &[key!(CTRL, Delete)],
        InTerminal::Forward,
        false,
    ),
    // GTK 입력 칸에는 줄 앞까지 지우는 키가 없어 줄 앞까지 고른 뒤 지운다. 셸은 Ctrl+U로 지운다
    text(
        "text.delete-line",
        "<Super>BackSpace",
        &[key!(SHIFT, Home), key!(NONE, BackSpace)],
        InTerminal::Keys(&[key!(CTRL, u)]),
        false,
    ),
    // 앱 공통 명령: 앱의 Ctrl 단축키로 보낸다. 터미널에는 foot의 기본 단축키를 보낸다
    app(
        "app.cut",
        "<Super>x",
        &[key!(CTRL, x)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.copy",
        "<Super>c",
        &[key!(CTRL, c)],
        InTerminal::Keys(&[key!(CTRL_SHIFT, c)]),
        false,
    ),
    app(
        "app.paste",
        "<Super>v",
        &[key!(CTRL, v)],
        InTerminal::Keys(&[key!(CTRL_SHIFT, v)]),
        false,
    ),
    app(
        "app.undo",
        "<Super>z",
        &[key!(CTRL, z)],
        InTerminal::Nothing,
        true,
    ),
    app(
        "app.redo",
        "<Shift><Super>z",
        &[key!(CTRL_SHIFT, z)],
        InTerminal::Nothing,
        true,
    ),
    app(
        "app.select-all",
        "<Super>a",
        &[key!(CTRL, a)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.find",
        "<Super>f",
        &[key!(CTRL, f)],
        InTerminal::Keys(&[key!(CTRL_SHIFT, r)]),
        false,
    ),
    app(
        "app.find-next",
        "<Super>g",
        &[key!(CTRL, g)],
        InTerminal::Nothing,
        true,
    ),
    app(
        "app.find-previous",
        "<Shift><Super>g",
        &[key!(CTRL_SHIFT, g)],
        InTerminal::Nothing,
        true,
    ),
    app(
        "app.new",
        "<Super>n",
        &[key!(CTRL, n)],
        InTerminal::Keys(&[key!(CTRL_SHIFT, n)]),
        false,
    ),
    app(
        "app.new-tab",
        "<Super>t",
        &[key!(CTRL, t)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.open",
        "<Super>o",
        &[key!(CTRL, o)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.save",
        "<Super>s",
        &[key!(CTRL, s)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.save-as",
        "<Shift><Super>s",
        &[key!(CTRL_SHIFT, s)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.print",
        "<Super>p",
        &[key!(CTRL, p)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.close",
        "<Super>w",
        &[key!(CTRL, w)],
        InTerminal::Run(Builtin::CloseWindow),
        false,
    ),
    app(
        "app.settings",
        "<Super>comma",
        &[key!(CTRL, comma)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.back",
        "<Super>bracketleft",
        &[key!(ALT, Left)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.forward",
        "<Super>bracketright",
        &[key!(ALT, Right)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.reload",
        "<Super>r",
        &[key!(CTRL, r)],
        InTerminal::Nothing,
        false,
    ),
    app(
        "app.zoom-in",
        "<Super>equal",
        &[key!(CTRL, equal)],
        InTerminal::Keys(&[key!(CTRL, equal)]),
        true,
    ),
    app(
        "app.zoom-out",
        "<Super>minus",
        &[key!(CTRL, minus)],
        InTerminal::Keys(&[key!(CTRL, minus)]),
        true,
    ),
    app(
        "app.zoom-reset",
        "<Super>0",
        &[key!(CTRL, _0)],
        InTerminal::Keys(&[key!(CTRL, _0)]),
        false,
    ),
    Def {
        id: "app.other-command-keys",
        category: Category::App,
        default: "",
        action: Action::OtherCommandKeys,
        repeat: false,
    },
    // macOS의 로고 메뉴 단축키: 강제 종료…, 화면 잠금, 로그아웃…
    run(
        "windows.force-quit",
        Category::Windows,
        "<Alt><Super>Escape",
        Builtin::ForceQuit,
    ),
    run(
        "session.lock-screen",
        Category::Session,
        "<Control><Super>q",
        Builtin::LockScreen,
    ),
    run(
        "session.log-out",
        Category::Session,
        "<Shift><Super>q",
        Builtin::LogOut { confirm: true },
    ),
    // macOS처럼 ⌥를 더하면 묻지 않고 로그아웃한다
    run(
        "session.log-out-now",
        Category::Session,
        "<Alt><Shift><Super>q",
        Builtin::LogOut { confirm: false },
    ),
];

/// 터미널 앱의 앱 ID. 이 앱에는 ⌘ 단축키를 터미널 방식으로 보낸다
const TERMINALS: &[&str] = &[
    "foot",
    "footclient",
    "org.codeberg.dnkl.foot",
    "kitty",
    "Alacritty",
    "org.gnome.Console",
    "org.gnome.Ptyxis",
    "org.gnome.Terminal",
    "org.kde.konsole",
    "com.mitchellh.ghostty",
    "org.wezfurlong.wezterm",
    "xterm",
];

pub fn is_terminal(app_id: &str) -> bool {
    TERMINALS.contains(&app_id)
}

/// 키를 누른 곳의 형편
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Place {
    /// 포커스를 가진 앱이 글 입력(text-input-v3)을 켜 두었다
    pub typing: bool,
    /// 포커스를 가진 창이 터미널이다
    pub terminal: bool,
}

/// 누른 키로 할 일
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Run(Builtin),
    /// 앱에 이 키들을 차례로 보낸다. `repeat`면 누르고 있는 동안 되풀이한다
    Send {
        keys: Vec<Key>,
        repeat: bool,
    },
    /// 받은 키를 아무 데도 보내지 않는다
    Swallow,
}

/// 시스템 설정에 보이는 단축키 하나 (GetShortcuts)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutInfo {
    pub id: String,
    pub category: String,
    /// "key": 키를 정하는 항목, "switch": 켜고 끄기만 하는 항목
    pub kind: String,
    /// 지금 키 (GTK 가속기). 비었으면 키가 없다
    pub accelerator: String,
    pub default: String,
    pub enabled: bool,
    pub default_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry {
    accel: Option<Accel>,
    enabled: bool,
}

/// 지금 쓰는 단축키 (표 순서)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcuts {
    entries: Vec<Entry>,
}

impl Shortcuts {
    pub fn new(prefs: &BTreeMap<String, ShortcutPref>) -> Self {
        let entries = DEFS
            .iter()
            .map(|def| match prefs.get(def.id) {
                Some(pref) => Entry {
                    accel: Accel::parse(&pref.key),
                    enabled: pref.enabled,
                },
                None => Entry {
                    accel: Accel::parse(def.default),
                    enabled: true,
                },
            })
            .collect();
        Self { entries }
    }

    pub fn info(&self) -> Vec<ShortcutInfo> {
        DEFS.iter()
            .zip(&self.entries)
            .map(|(def, entry)| ShortcutInfo {
                id: def.id.into(),
                category: def.category.id().into(),
                kind: match def.action {
                    Action::OtherCommandKeys => "switch",
                    _ => "key",
                }
                .into(),
                accelerator: entry.accel.map(|accel| accel.name()).unwrap_or_default(),
                default: Accel::parse(def.default)
                    .map(|accel| accel.name())
                    .unwrap_or_default(),
                enabled: entry.enabled,
                default_enabled: true,
            })
            .collect()
    }

    /// 다른 키 없이 수정 키만 눌렀다 뗐을 때 할 일 (입력 소스 전환을 ⌃⇧로 정했을 때)
    pub fn modifiers_only(&self, mods: Mods) -> Option<Builtin> {
        DEFS.iter()
            .zip(&self.entries)
            .filter(|(_, entry)| entry.enabled)
            .find(|(_, entry)| {
                entry
                    .accel
                    .is_some_and(|accel| accel.is_modifiers_only() && accel.mods == mods)
            })
            .and_then(|(def, _)| match def.action {
                Action::Run(builtin) => Some(builtin),
                _ => None,
            })
    }

    fn other_command_keys(&self) -> bool {
        DEFS.iter()
            .zip(&self.entries)
            .any(|(def, entry)| def.action == Action::OtherCommandKeys && entry.enabled)
    }

    /// 누른 키로 할 일. 없으면 키를 그대로 앱에 보낸다
    pub fn lookup(&self, pressed: Accel, place: Place) -> Option<Hit> {
        let bindings = || {
            DEFS.iter()
                .zip(&self.entries)
                .filter(|(_, entry)| entry.enabled)
                .filter_map(|(def, entry)| entry.accel.map(|accel| (def, accel)))
        };
        // 정확히 같은 키가 먼저다. 그다음 ⇧를 더해 선택을 넓히는 텍스트 편집 키
        for (def, _) in bindings().filter(|(_, accel)| *accel == pressed) {
            if let Some(hit) = hit(def, place, false) {
                return Some(hit);
            }
        }
        if pressed.mods.shift {
            let unshifted = Accel {
                mods: Mods {
                    shift: false,
                    ..pressed.mods
                },
                ..pressed
            };
            for (def, _) in bindings().filter(|(_, accel)| *accel == unshifted) {
                if let Action::Text { select: true, .. } = def.action
                    && let Some(hit) = hit(def, place, true)
                {
                    return Some(hit);
                }
            }
        }
        // 표에 없는 ⌘ 단축키 (글자, 숫자, 문장 부호, Return)
        if pressed.mods.logo && !pressed.mods.ctrl && is_command_key(pressed.key) {
            if place.terminal {
                return Some(Hit::Swallow);
            }
            if self.other_command_keys() {
                return Some(Hit::Send {
                    keys: vec![Key {
                        mods: Mods {
                            ctrl: true,
                            logo: false,
                            ..pressed.mods
                        },
                        sym: pressed.key,
                    }],
                    repeat: false,
                });
            }
        }
        None
    }
}

/// 표의 한 줄이 그곳에서 할 일. 글 입력 중이 아닌 곳의 텍스트 편집 키처럼 할 일이 없으면 None
fn hit(def: &Def, place: Place, shifted: bool) -> Option<Hit> {
    let send = |keys: &[Key]| Hit::Send {
        keys: keys
            .iter()
            .map(|key| Key {
                mods: Mods {
                    shift: key.mods.shift || shifted,
                    ..key.mods
                },
                ..*key
            })
            .collect(),
        repeat: def.repeat,
    };
    let in_terminal = |terminal: InTerminal| match terminal {
        InTerminal::Keys(keys) => Some(Hit::Send {
            keys: keys.to_vec(),
            repeat: def.repeat,
        }),
        InTerminal::Run(builtin) => Some(Hit::Run(builtin)),
        InTerminal::Forward => None,
        InTerminal::Nothing => Some(Hit::Swallow),
    };
    match def.action {
        Action::Run(builtin) => Some(Hit::Run(builtin)),
        Action::App { terminal, .. } if place.terminal => in_terminal(terminal),
        Action::App { keys, .. } => Some(send(keys)),
        Action::Text { terminal, .. } if place.terminal => in_terminal(terminal),
        Action::Text { keys, .. } if place.typing => Some(send(keys)),
        Action::Text { .. } | Action::OtherCommandKeys => None,
    }
}

/// 표에 없을 때 앱의 Ctrl 단축키로 옮겨 보내는 ⌘ 키: 인쇄되는 ASCII(공백 빼고)와 Return.
/// 화살표, Tab, 스페이스 같은 키는 앱마다 뜻이 달라 옮기지 않는다
fn is_command_key(key: Keysym) -> bool {
    matches!(key.raw(), 0x21..=0x7e) || key == Keysym::Return
}

/// 시스템 설정이 보낸 값을 확인하고 바르게 쓴 가속기를 돌려준다 (SetShortcut). 켠 다른 항목이 같은
/// 키를 쓰면 받지 않는다
pub fn check(
    infos: &[ShortcutInfo],
    id: &str,
    accelerator: &str,
    enabled: bool,
) -> Result<String, String> {
    let info = infos
        .iter()
        .find(|info| info.id == id)
        .ok_or_else(|| format!("unknown shortcut {id:?}"))?;
    if info.kind == "switch" || accelerator.trim().is_empty() {
        return Ok(String::new());
    }
    let accel =
        Accel::parse(accelerator).ok_or_else(|| format!("invalid accelerator {accelerator:?}"))?;
    // 수정 키만으로 된 조합은 입력 소스 전환에만 쓴다 (Windows의 Ctrl+Shift처럼)
    if accel.is_modifiers_only() && info.category != Category::Input.id() {
        return Err(format!(
            "{id} needs a key; modifier keys alone only switch input sources"
        ));
    }
    if enabled
        && let Some(other) = infos.iter().find(|other| {
            other.id != id
                && other.enabled
                && Accel::parse(&other.accelerator).is_some_and(|other| other == accel)
        })
    {
        return Err(format!("{} is already used by {}", accel.name(), other.id));
    }
    Ok(accel.name())
}

/// 바꾼 값을 사용자 설정에 적는다. 기본값과 같아지면 지운다
pub fn remember(
    prefs: &mut BTreeMap<String, ShortcutPref>,
    id: &str,
    accelerator: String,
    enabled: bool,
) {
    let Some(def) = DEFS.iter().find(|def| def.id == id) else {
        return;
    };
    let default = Accel::parse(def.default)
        .map(|accel| accel.name())
        .unwrap_or_default();
    if accelerator == default && enabled {
        prefs.remove(id);
    } else {
        prefs.insert(
            id.to_string(),
            ShortcutPref {
                key: accelerator,
                enabled,
            },
        );
    }
}

/// 한 묶음(빈 문자열이면 전부)을 기본값으로 되돌린다
pub fn reset(prefs: &mut BTreeMap<String, ShortcutPref>, category: &str) {
    prefs.retain(|id, _| {
        let def = DEFS.iter().find(|def| def.id == id);
        !(category.is_empty() || def.is_some_and(|def| def.category.id() == category))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accel(text: &str) -> Accel {
        Accel::parse(text).unwrap_or_else(|| panic!("{text:?} does not parse"))
    }

    fn send(keys: &[&str]) -> Option<Hit> {
        Some(Hit::Send {
            keys: keys
                .iter()
                .map(|text| {
                    let accel = accel(text);
                    Key {
                        mods: accel.mods,
                        sym: accel.key,
                    }
                })
                .collect(),
            repeat: false,
        })
    }

    fn without_repeat(hit: Option<Hit>) -> Option<Hit> {
        hit.map(|hit| match hit {
            Hit::Send { keys, .. } => Hit::Send {
                keys,
                repeat: false,
            },
            other => other,
        })
    }

    const APP: Place = Place {
        typing: false,
        terminal: false,
    };
    const TYPING: Place = Place {
        typing: true,
        terminal: false,
    };
    const TERMINAL: Place = Place {
        typing: true,
        terminal: true,
    };

    #[test]
    fn volume_keys_are_known_by_their_symbols() {
        assert_eq!(
            VolumeKey::of(Keysym::XF86_AudioRaiseVolume),
            Some(VolumeKey::Up)
        );
        assert_eq!(
            VolumeKey::of(Keysym::XF86_AudioLowerVolume),
            Some(VolumeKey::Down)
        );
        assert_eq!(VolumeKey::of(Keysym::XF86_AudioMute), Some(VolumeKey::Mute));
        assert_eq!(VolumeKey::of(Keysym::XF86_PowerOff), None);
        assert!(VolumeKey::Up.repeats() && VolumeKey::Down.repeats());
        assert!(!VolumeKey::Mute.repeats());
    }

    #[test]
    fn accelerators_parse_in_any_order_and_print_in_one() {
        assert_eq!(accel("<Shift><Super>3").name(), "<Shift><Super>3");
        assert_eq!(accel("<super><shift>3").name(), "<Shift><Super>3");
        assert_eq!(accel("<Super>C").name(), "<Super>c");
        assert_eq!(accel("<Ctrl><Mod1>Delete").name(), "<Control><Alt>Delete");
        assert_eq!(accel("<Primary>tab").name(), "<Control>Tab");
        assert_eq!(accel("<Meta>bracketleft").name(), "<Super>bracketleft");
        for bad in [
            "",
            "<Super>",
            "<Foo>a",
            "<Super>nokey",
            "<Super",
            "<Control>Shift_L",
        ] {
            assert_eq!(Accel::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_default_parses_and_no_two_share_a_key() {
        let mut seen = Vec::new();
        for def in DEFS {
            if def.action == Action::OtherCommandKeys {
                assert!(def.default.is_empty());
                continue;
            }
            let accel = accel(def.default);
            assert_eq!(
                accel.name(),
                def.default,
                "write {} in canonical form",
                def.id
            );
            assert!(!seen.contains(&accel), "{} reuses {}", def.id, def.default);
            seen.push(accel);
        }
        for (index, def) in DEFS.iter().enumerate() {
            assert!(
                DEFS[index + 1..].iter().all(|other| other.id != def.id),
                "{} is listed twice",
                def.id
            );
            let prefix = def.id.split('.').next().unwrap_or_default();
            assert!(
                def.category.id().starts_with(prefix),
                "{} is not named after its category",
                def.id
            );
        }
    }

    #[test]
    fn command_keys_become_the_apps_ctrl_keys() {
        let shortcuts = Shortcuts::new(&BTreeMap::new());
        let lookup = |text: &str, place| without_repeat(shortcuts.lookup(accel(text), place));
        assert_eq!(lookup("<Super>c", APP), send(&["<Control>c"]));
        assert_eq!(lookup("<Shift><Super>z", APP), send(&["<Control><Shift>z"]));
        assert_eq!(lookup("<Super>bracketleft", APP), send(&["<Alt>Left"]));
        // 표에 없는 ⌘ 키는 같은 키의 Ctrl 단축키로 (⇧·⌥는 그대로)
        assert_eq!(lookup("<Super>b", APP), send(&["<Control>b"]));
        assert_eq!(lookup("<Alt><Super>i", APP), send(&["<Control><Alt>i"]));
        // Ctrl 단축키와 ⌘가 아닌 키는 앱이 원래대로 받는다
        assert_eq!(lookup("<Control>c", APP), None);
        assert_eq!(lookup("<Super>Left", APP), None);
        assert_eq!(lookup("<Super>F5", APP), None);
    }

    #[test]
    fn terminals_get_their_own_keys() {
        let shortcuts = Shortcuts::new(&BTreeMap::new());
        let lookup = |text: &str| without_repeat(shortcuts.lookup(accel(text), TERMINAL));
        assert_eq!(lookup("<Super>c"), send(&["<Control><Shift>c"]));
        assert_eq!(lookup("<Super>v"), send(&["<Control><Shift>v"]));
        assert_eq!(lookup("<Super>w"), Some(Hit::Run(Builtin::CloseWindow)));
        assert_eq!(lookup("<Super>z"), Some(Hit::Swallow));
        assert_eq!(lookup("<Super>b"), Some(Hit::Swallow));
        assert_eq!(lookup("<Super>BackSpace"), send(&["<Control>u"]));
        // ⌥⌫는 그대로 보내 셸이 단어를 지운다
        assert_eq!(lookup("<Alt>BackSpace"), None);
        assert_eq!(lookup("<Control>c"), None);
    }

    #[test]
    fn text_keys_work_while_typing_and_shift_extends_the_selection() {
        let shortcuts = Shortcuts::new(&BTreeMap::new());
        let lookup = |text: &str, place| without_repeat(shortcuts.lookup(accel(text), place));
        assert_eq!(lookup("<Super>Left", TYPING), send(&["Home"]));
        assert_eq!(lookup("<Shift><Super>Left", TYPING), send(&["<Shift>Home"]));
        assert_eq!(
            lookup("<Shift><Alt>Right", TYPING),
            send(&["<Control><Shift>Right"])
        );
        assert_eq!(
            lookup("<Super>BackSpace", TYPING),
            send(&["<Shift>Home", "BackSpace"])
        );
        // 지우기 키에는 ⇧를 더하지 않는다
        assert_eq!(lookup("<Shift><Alt>BackSpace", TYPING), None);
        // 글을 입력하는 중이 아니면 ⌥←는 앱이 받는다 (시스템 설정의 뒤로)
        assert_eq!(lookup("<Alt>Left", APP), None);
        assert!(matches!(
            shortcuts.lookup(accel("<Alt>Left"), TYPING),
            Some(Hit::Send { repeat: true, .. })
        ));
    }

    #[test]
    fn system_shortcuts_run_in_the_compositor() {
        let shortcuts = Shortcuts::new(&BTreeMap::new());
        let lookup = |text: &str| shortcuts.lookup(accel(text), TYPING);
        assert_eq!(
            lookup("<Super>space"),
            Some(Hit::Run(Builtin::ShowLauncher))
        );
        assert_eq!(lookup("<Super>Tab"), Some(Hit::Run(Builtin::NextApp)));
        assert_eq!(
            lookup("<Shift><Super>Tab"),
            Some(Hit::Run(Builtin::PreviousApp))
        );
        assert_eq!(
            lookup("<Control><Shift><Super>4"),
            Some(Hit::Run(Builtin::Screenshot {
                area: true,
                clipboard: true
            }))
        );
        assert_eq!(
            lookup("<Control>space"),
            Some(Hit::Run(Builtin::SwitchInputSource))
        );
    }

    #[test]
    fn logo_menu_shortcuts_lock_log_out_and_force_quit() {
        let shortcuts = Shortcuts::new(&BTreeMap::new());
        let lookup = |text: &str| shortcuts.lookup(accel(text), APP);
        assert_eq!(
            lookup("<Control><Super>q"),
            Some(Hit::Run(Builtin::LockScreen))
        );
        assert_eq!(
            lookup("<Shift><Super>q"),
            Some(Hit::Run(Builtin::LogOut { confirm: true }))
        );
        assert_eq!(
            lookup("<Alt><Shift><Super>q"),
            Some(Hit::Run(Builtin::LogOut { confirm: false }))
        );
        assert_eq!(
            lookup("<Alt><Super>Escape"),
            Some(Hit::Run(Builtin::ForceQuit))
        );
        // ⌘Q는 그대로 앱 종료다. ⇧⌘Q는 전에는 앱에 Ctrl+Shift+Q로 갔다(Firefox의 종료)
        assert_eq!(lookup("<Super>q"), Some(Hit::Run(Builtin::QuitApp)));
        let info = shortcuts.info();
        let lock = info
            .iter()
            .find(|info| info.id == "session.lock-screen")
            .unwrap();
        assert_eq!(lock.category, "session");
        assert!(
            Category::ALL
                .iter()
                .any(|category| category.id() == "session")
        );
    }

    #[test]
    fn changed_keys_and_switches_come_from_the_prefs() {
        let mut prefs = BTreeMap::new();
        remember(&mut prefs, "app.copy", "<Alt>c".into(), true);
        remember(&mut prefs, "app.paste", "<Super>v".into(), false);
        remember(&mut prefs, "app.other-command-keys", String::new(), false);
        // 기본값으로 돌린 항목은 적지 않는다
        remember(&mut prefs, "app.cut", "<Super>x".into(), true);
        assert_eq!(prefs.len(), 3);
        let shortcuts = Shortcuts::new(&prefs);
        let lookup = |text: &str| without_repeat(shortcuts.lookup(accel(text), APP));
        assert_eq!(lookup("<Alt>c"), send(&["<Control>c"]));
        assert_eq!(lookup("<Super>c"), None);
        assert_eq!(lookup("<Super>v"), None);
        assert_eq!(lookup("<Super>b"), None);
        let info = shortcuts.info();
        let copy = info.iter().find(|info| info.id == "app.copy").unwrap();
        assert_eq!(
            (copy.accelerator.as_str(), copy.default.as_str()),
            ("<Alt>c", "<Super>c")
        );
        let other = info
            .iter()
            .find(|info| info.id == "app.other-command-keys")
            .unwrap();
        assert_eq!((other.kind.as_str(), other.enabled), ("switch", false));

        reset(&mut prefs, "launcher");
        assert_eq!(prefs.len(), 3);
        reset(&mut prefs, "app");
        assert!(prefs.is_empty());
    }

    #[test]
    fn input_sources_can_switch_on_modifier_keys_alone() {
        let ctrl_shift = accel("<Shift><Ctrl>");
        assert!(ctrl_shift.is_modifiers_only());
        assert_eq!(ctrl_shift.name(), "<Control><Shift>");
        // 수정 키 하나는 칠 때마다 걸리므로 받지 않는다
        assert_eq!(Accel::parse("<Control>"), None);

        let info = Shortcuts::new(&BTreeMap::new()).info();
        assert_eq!(
            check(&info, "input.switch-source", "<Control><Shift>", true),
            Ok("<Control><Shift>".into())
        );
        assert!(check(&info, "app.copy", "<Control><Shift>", true).is_err());

        let mut prefs = BTreeMap::new();
        remember(
            &mut prefs,
            "input.switch-source",
            "<Control><Shift>".into(),
            true,
        );
        let shortcuts = Shortcuts::new(&prefs);
        let mods = |text: &str| accel(&format!("{text}a")).mods;
        assert_eq!(
            shortcuts.modifiers_only(mods("<Control><Shift>")),
            Some(Builtin::SwitchInputSource)
        );
        assert_eq!(shortcuts.modifiers_only(mods("<Alt><Shift>")), None);
        assert_eq!(
            Shortcuts::new(&BTreeMap::new()).modifiers_only(mods("<Control><Shift>")),
            None
        );
        // 끈 단축키는 걸리지 않는다
        remember(
            &mut prefs,
            "input.switch-source",
            "<Control><Shift>".into(),
            false,
        );
        assert_eq!(
            Shortcuts::new(&prefs).modifiers_only(mods("<Control><Shift>")),
            None
        );
    }

    #[test]
    fn a_key_in_use_is_refused() {
        let info = Shortcuts::new(&BTreeMap::new()).info();
        assert_eq!(
            check(&info, "app.copy", "<Super>v", true),
            Err("<Super>v is already used by app.paste".into())
        );
        // 끈 채로는 받는다. 같은 항목의 같은 키도
        assert_eq!(
            check(&info, "app.copy", "<super>V", false),
            Ok("<Super>v".into())
        );
        assert_eq!(
            check(&info, "app.copy", "<Super>c", true),
            Ok("<Super>c".into())
        );
        assert_eq!(check(&info, "app.copy", "", true), Ok(String::new()));
        assert!(check(&info, "app.nothing", "<Super>c", true).is_err());
        assert!(check(&info, "app.copy", "<Super>", true).is_err());
        assert_eq!(
            check(&info, "app.other-command-keys", "<Super>b", true),
            Ok(String::new())
        );
    }
}
