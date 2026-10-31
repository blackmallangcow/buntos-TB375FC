// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드(셸의 `bunt-osk` 레이어, shell/src/osk.rs)를 위한 컴포지터 쪽 (ARCHITECTURE 5.1·5.2).
//!
//! - 글 입력 상태: 키보드 포커스를 가진 앱이 글 입력(text-input-v3)을 켰는지만 셸에 알린다(`TextInputChanged`,
//!   `WatchTextInput`을 부른 셸의 버스 이름으로만 보낸다). 글 내용은 알리지 않는다. 에이전트의 `TextInputActive`와
//!   같은 상태다. 켠 칸을 다시 눌러 앱이 글 입력을 다시 켜도 알린다(가린 키보드를 다시 띄운다).
//! - 키: 셸이 누른 키를 실제 키보드와 같은 길(`KeyboardHandle::input`)로 seat에 넣는다(`PressOnScreenKeys`).
//!   입력기(fcitx5)가 키보드를 잡고 있으면 그 잡기로 가서 한글이 조합된다. 가상 키보드 프로토콜(virtual-keyboard-v1)을
//!   셸에 여는 길은 쓰지 않았다: smithay의 가상 키보드는 키를 포커스를 가진 앱에 바로 보내 입력기 잡기를 거치지 않는다
//!   (입력기가 쓰지 않은 키를 돌려보내는 길이라서다). 셸 전용 메서드는 컴포지터가 띄운 셸의 pid만 부르고, 앱과 AI
//!   에이전트는 부르지 못한다(에이전트에는 정책을 보는 `PressKeys`·`TypeText`가 따로 있다).
//! - 자리: 키보드는 화면 맨 아래의 overlay 레이어이고 배타 영역을 쓰지 않는다(-1). Dock보다 먼저 놓여야 하는데
//!   smithay는 배타 영역을 놓인 차례대로 쌓아 키보드를 Dock 위에 올리기 때문이다. 대신 창 자리(window_area)를
//!   키보드가 키를 받는 영역(입력 영역)의 위쪽 끝까지로 줄인다. 앱(GTK)은 가려진 글 칸이 보이게 스스로 스크롤하지
//!   않으므로 iPadOS처럼 겹치지 않고 창을 줄인다. 미리 보기 풍선을 그리는 위쪽의 투명한 띠는 입력 영역 밖이라 창
//!   자리를 줄이지 않고, 그 자리의 누르기는 아래 창이 받는다.
//! - 에이전트는 키보드를 누르지 못한다: 레이어 목록(`GetLayers`)에 들지 않고, `ClickLayer`와 화면 좌표 `Click`이
//!   키보드 위면 거절한다(agent.rs). 그러지 않으면 막은 앱에 키보드로 글을 칠 수 있다.

use std::{sync::mpsc, time::Duration};

use smithay::{
    backend::input::KeyState,
    backend::renderer::utils::with_renderer_surface_state,
    desktop::layer_map_for_output,
    input::keyboard::{FilterResult, Keycode, Keysym, xkb},
    output::Output,
    reexports::wayland_server::{Resource, protocol::wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::compositor::{RectangleKind, RegionAttributes, SurfaceAttributes, with_states},
};
use zbus::fdo;

use crate::{
    dbus::Command,
    state::{BuntState, ClientState},
};

/// 셸의 화면 키보드 레이어 이름 (shell/src/osk.rs, agent/src/atspi.rs의 KEYBOARD_WINDOW)
pub const KEYBOARD_NAMESPACE: &str = "bunt-osk";
/// 한 번에 받는 키 수. 셸은 앞 요청의 답을 기다리는 동안 누른 키를 모아 보낸다
pub const MAX_KEYS: usize = 64;
/// 이벤트 루프가 키를 다 넣기를 기다리는 시간 (에이전트 요청과 같다, dbus.rs의 AGENT_REPLY)
const REPLY: Duration = Duration::from_secs(2);
/// evdev 키 코드 + 8 (xkb 키 코드)의 범위 (agent.rs와 같다)
const KEYCODES: std::ops::RangeInclusive<u32> = 8..=255;

/// 화면 키보드의 키 하나. 글자는 지금 키 배열(us)에 있는 것만 친다. 한글 자모는 셸이 2벌식 자리의 라틴 글자로
/// 보내고(ㅂ은 q, ㅃ은 Q) 입력기가 조합한다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OskKey {
    /// 글자 하나 (공백 포함). 대문자와 Shift 기호(!, @ …)는 Shift를 함께 누른다
    Char(char),
    BackSpace,
    Return,
}

/// 셸이 보낸 키 이름: 인쇄할 수 있는 ASCII 글자 하나(공백 포함)나 "BackSpace", "Return". 다른 키(Ctrl, 가상 터미널
/// 전환 등)는 받지 않는다: 화면 키보드에 없는 키이고, 이 길로 세션을 끝내는 키 같은 것을 누르지 못하게 한다
pub fn parse_key(text: &str) -> Result<OskKey, String> {
    match text {
        "BackSpace" => Ok(OskKey::BackSpace),
        "Return" => Ok(OskKey::Return),
        _ => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(character), None) if (' '..='~').contains(&character) => {
                    Ok(OskKey::Char(character))
                }
                _ => Err(format!("{text:?} is not a key of the on-screen keyboard")),
            }
        }
    }
}

pub fn parse_keys(keys: &[String]) -> Result<Vec<OskKey>, String> {
    if keys.len() > MAX_KEYS {
        return Err(format!("send at most {MAX_KEYS} keys at a time"));
    }
    keys.iter().map(|key| parse_key(key)).collect()
}

/// 셸의 키를 넣지 않은 까닭
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeysRefused {
    /// 화면이 잠겼다. 잠금 화면은 제 창 안의 키보드를 쓴다 (greeter/src/lock)
    Locked,
    /// 지금 키 배열에 없는 글자
    NotOnLayout(char),
}

/// D-Bus에서 이벤트 루프로 넘기는 화면 키보드 요청 (dbus.rs의 Request::Keyboard)
#[derive(Debug)]
pub enum KeyboardRequest {
    /// 키를 차례로 누른다. 하나라도 칠 수 없으면 아무것도 치지 않는다
    Keys {
        keys: Vec<OskKey>,
        reply: mpsc::Sender<Result<(), KeysRefused>>,
    },
}

/// 이벤트 루프의 답을 기다려 D-Bus 답으로 바꾼다
pub fn answer(receiver: mpsc::Receiver<Result<(), KeysRefused>>) -> fdo::Result<()> {
    match receiver.recv_timeout(REPLY) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(KeysRefused::Locked)) => Err(fdo::Error::AccessDenied(
            "the screen is locked; the lock screen has its own keyboard".into(),
        )),
        Ok(Err(KeysRefused::NotOnLayout(missing))) => Err(fdo::Error::InvalidArgs(format!(
            "{missing:?} is not on the keyboard layout"
        ))),
        Err(_) => Err(fdo::Error::Failed("the compositor did not answer".into())),
    }
}

/// 셸에 알린 글 입력 상태 (BuntState의 osk)
#[derive(Debug, Default)]
pub struct TextInputState {
    /// 마지막으로 알린 값
    published: bool,
    /// 포커스를 가진 앱이 이번 루프에서 글 입력을 (다시) 켰다. 이미 켜진 칸을 누르면 GTK가 다시 켠다
    /// (gtk_im_context_activate_osk). 그때도 알려 가린 키보드를 다시 띄운다
    enabled_again: bool,
}

impl TextInputState {
    /// 포커스를 가진 앱이 글 입력을 켜는 요청(enable)을 보냈다 (ime.rs)
    pub fn enable_requested(&mut self) {
        self.enabled_again = true;
    }

    /// 지금 상태(`active`)를 알릴지. 값이 바뀌었거나, 켜진 채로 다시 켰으면 알릴 값
    fn decide(&mut self, active: bool) -> Option<bool> {
        let again = std::mem::take(&mut self.enabled_again);
        if active == self.published && !(active && again) {
            return None;
        }
        self.published = active;
        Some(active)
    }
}

/// 키 하나를 누르는 법: xkb 키 코드와 Shift를 함께 누를지
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Press {
    code: Keycode,
    shift: bool,
}

/// 키 배열(`keymap`의 `layout`)에서 그 기호를 그 단계(0: 수정 없이, 1: Shift)에 내는 키 코드 (agent.rs의 key_for와 같다)
fn find_key(keymap: &xkb::Keymap, layout: u32, keysym: Keysym, level: u32) -> Option<Keycode> {
    KEYCODES.map(Keycode::new).find(|code| {
        keymap
            .key_get_syms_by_level(*code, layout, level)
            .contains(&keysym)
    })
}

/// 셸의 키를 지금 키 배열의 키로 바꾼다. 글자가 배열에 없으면 그 글자
fn resolve(keymap: &xkb::Keymap, layout: u32, key: OskKey) -> Result<Press, char> {
    let (keysym, character) = match key {
        OskKey::Char(character) => (xkb::utf32_to_keysym(u32::from(character)), character),
        OskKey::BackSpace => (Keysym::BackSpace, '\u{8}'),
        OskKey::Return => (Keysym::Return, '\r'),
    };
    if let Some(code) = find_key(keymap, layout, keysym, 0) {
        return Ok(Press { code, shift: false });
    }
    match key {
        OskKey::Char(_) => find_key(keymap, layout, keysym, 1)
            .map(|code| Press { code, shift: true })
            .ok_or(character),
        _ => Err(character),
    }
}

/// 창이 놓일 영역(`area`, 전역 좌표)의 아래쪽을 화면 키보드의 위쪽 끝(`top`, 전역 y, keyboard_top)까지로 줄인다.
/// 이미 그 위에서 끝나면 그대로 둔다 (state.rs의 window_area)
pub(crate) fn reserve_above(area: &mut Rectangle<i32, Logical>, top: i32) {
    let bottom = area.loc.y + area.size.h;
    if top < bottom {
        area.size.h = (top - area.loc.y).max(0);
    }
}

/// 입력 영역의 위쪽 끝 (표면 좌표). 입력 영역을 정하지 않았으면(표면 전체) 0, 비었으면 None
fn input_top(region: Option<&RegionAttributes>) -> Option<i32> {
    let Some(region) = region else {
        return Some(0);
    };
    region
        .rects
        .iter()
        .filter(|(kind, _)| matches!(kind, RectangleKind::Add))
        .map(|(_, rect)| rect.loc.y)
        .min()
}

impl BuntState {
    pub fn keyboard_request(&mut self, request: KeyboardRequest) {
        match request {
            KeyboardRequest::Keys { keys, reply } => {
                let _ = reply.send(self.on_screen_keys(&keys));
            }
        }
    }

    /// 셸이 누른 키를 실제 키보드처럼 넣는다: 키를 모두 찾은 뒤 Shift가 필요한 키는 Shift를 누른 채 눌렀다 뗀다.
    /// 컴포지터 단축키는 거치지 않는다(에이전트의 PressKeys와 같다). 입력기가 키보드를 잡고 있으면 그 잡기로 간다
    fn on_screen_keys(&mut self, keys: &[OskKey]) -> Result<(), KeysRefused> {
        // 잠긴 동안 키는 잠금 화면으로 간다. 셸의 키보드는 그리지도 않으므로 받지 않는다
        if self.session_lock.is_locked() {
            return Err(KeysRefused::Locked);
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return Ok(());
        };
        let (presses, shift) = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap();
            let layout = xkb.active_layout().0;
            // 안전: 키맵의 참조를 이 닫힘 밖으로 내보내지 않는다
            let keymap = unsafe { xkb.keymap() };
            let presses: Result<Vec<Press>, char> = keys
                .iter()
                .map(|key| resolve(keymap, layout, *key))
                .collect();
            (presses, find_key(keymap, layout, Keysym::Shift_L, 0))
        });
        let presses = presses.map_err(KeysRefused::NotOnLayout)?;
        // 앱에 옮겨 보내며 되풀이하던 단축키는 다른 키를 누르면 멈춘다 (input.rs의 on_keyboard와 같다)
        self.stop_key_repeat();
        let time = self.start_time.elapsed().as_millis() as u32;
        let send = |state: &mut BuntState, code: Keycode, key_state: KeyState| {
            keyboard.input::<(), _>(
                state,
                code,
                key_state,
                SERIAL_COUNTER.next_serial(),
                time,
                |_, _, _| FilterResult::Forward,
            );
        };
        for press in presses {
            let shift = shift.filter(|_| press.shift);
            if let Some(code) = shift {
                send(self, code, KeyState::Pressed);
            }
            send(self, press.code, KeyState::Pressed);
            send(self, press.code, KeyState::Released);
            if let Some(code) = shift {
                send(self, code, KeyState::Released);
            }
        }
        Ok(())
    }

    /// 이벤트 루프가 한 바퀴 돌 때마다 부른다(udev.rs): 글 입력 상태가 바뀌었거나 다시 켜졌으면 셸에 알린다.
    /// 잠긴 동안에는 끈 것으로 알린다. 잠금 화면의 암호 칸에 셸의 키보드를 띄우지 않는다
    pub fn publish_text_input(&mut self) {
        let active = !self.session_lock.is_locked() && self.text_input_active();
        if let Some(active) = self.osk.decide(active) {
            tracing::debug!("text input {}", if active { "on" } else { "off" });
            self.dbus.update(|snapshot| snapshot.text_input = active);
            self.dbus.send(Command::TextInputChanged(active));
        }
    }

    /// 화면 키보드가 보이면 그 키가 놓인 영역의 위쪽 끝 (전역 y). 미끄러져 나오는 동안은 보이는 만큼이다.
    /// 레이어 맵을 잠그므로 맵을 잠근 채 부르지 않는다
    pub(crate) fn keyboard_top(&self, output: &Output) -> Option<i32> {
        let output_geo = self.space.output_geometry(output)?;
        let map = layer_map_for_output(output);
        let layer = map
            .layers()
            .find(|layer| self.is_keyboard(layer.namespace(), layer.wl_surface()))?;
        let geo = map.layer_geometry(layer)?;
        let top = with_states(layer.wl_surface(), |states| {
            input_top(
                states
                    .cached_state
                    .get::<SurfaceAttributes>()
                    .current()
                    .input_region
                    .as_ref(),
            )
        })?;
        Some(output_geo.loc.y + geo.loc.y + top.clamp(0, geo.size.h))
    }

    /// 그 점에서 포인터가 화면 키보드에 가는지. 에이전트의 화면 좌표 누르기를 거절할 때 쓴다 (agent.rs)
    pub(crate) fn over_keyboard(&self, pos: Point<f64, Logical>) -> bool {
        let Some((surface, _)) = self.surface_under(pos) else {
            return false;
        };
        let root = self.root_surface(&surface);
        self.space.outputs().any(|output| {
            layer_map_for_output(output)
                .layers()
                .any(|layer| layer.wl_surface() == &root && layer.namespace() == KEYBOARD_NAMESPACE)
        })
    }

    /// 컴포지터가 띄운 셸이 그린, 보이는 화면 키보드 레이어인지. 다른 프로그램이 같은 이름의 레이어를 내도
    /// 창 자리를 줄이지 못한다
    fn is_keyboard(&self, namespace: &str, surface: &WlSurface) -> bool {
        if namespace != KEYBOARD_NAMESPACE {
            return false;
        }
        // state.rs의 is_privileged, has_buffer와 같은 확인이다 (그 함수들은 state.rs 밖에 열려 있지 않다)
        let from_shell = self
            .display_handle
            .get_client(surface.id())
            .ok()
            .and_then(|client| client.get_data::<ClientState>().map(|data| data.privileged))
            .unwrap_or(false);
        from_shell
            && with_renderer_surface_state(surface, |state| state.buffer().is_some())
                .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// us 키 배열 (xkeyboard-config). 없는 기계에서는 None
    fn us_keymap() -> Option<xkb::Keymap> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "us",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
    }

    /// evdev 키 코드 (linux/input-event-codes.h)
    const KEY_1: u32 = 2;
    const KEY_2: u32 = 3;
    const KEY_0: u32 = 11;
    const KEY_BACKSPACE: u32 = 14;
    const KEY_Q: u32 = 16;
    const KEY_P: u32 = 25;
    const KEY_ENTER: u32 = 28;
    const KEY_A: u32 = 30;
    const KEY_APOSTROPHE: u32 = 40;
    const KEY_GRAVE: u32 = 41;
    const KEY_BACKSLASH: u32 = 43;
    const KEY_M: u32 = 50;
    const KEY_COMMA: u32 = 51;
    const KEY_DOT: u32 = 52;
    const KEY_SLASH: u32 = 53;
    const KEY_SPACE: u32 = 57;

    fn press(code: u32, shift: bool) -> Press {
        Press {
            code: Keycode::new(code + 8),
            shift,
        }
    }

    #[test]
    fn only_printable_ascii_backspace_and_return_are_keys() {
        assert_eq!(parse_key("q"), Ok(OskKey::Char('q')));
        assert_eq!(parse_key("Q"), Ok(OskKey::Char('Q')));
        assert_eq!(parse_key(" "), Ok(OskKey::Char(' ')));
        assert_eq!(parse_key("~"), Ok(OskKey::Char('~')));
        assert_eq!(parse_key("BackSpace"), Ok(OskKey::BackSpace));
        assert_eq!(parse_key("Return"), Ok(OskKey::Return));
        // 한글은 셸이 2벌식 자리의 라틴 글자로 보낸다. 조합은 입력기가 한다
        for refused in [
            "ㅂ",
            "€",
            "",
            "ab",
            "\t",
            "\n",
            "\u{7f}",
            "ctrl+c",
            "Escape",
            "XF86Switch_VT_2",
        ] {
            assert!(parse_key(refused).is_err(), "{refused:?}");
        }
        let many = vec!["a".to_owned(); MAX_KEYS + 1];
        assert!(parse_keys(&many).is_err());
        assert_eq!(
            parse_keys(&many[..MAX_KEYS]).map(|keys| keys.len()),
            Ok(MAX_KEYS)
        );
    }

    #[test]
    fn keys_map_to_the_us_layout_with_shift_when_needed() {
        let Some(keymap) = us_keymap() else {
            eprintln!("xkeyboard-config is not installed; skipping the key map test");
            return;
        };
        let cases = [
            (OskKey::Char('q'), press(KEY_Q, false)),
            (OskKey::Char('Q'), press(KEY_Q, true)),
            (OskKey::Char('p'), press(KEY_P, false)),
            (OskKey::Char('P'), press(KEY_P, true)),
            (OskKey::Char('a'), press(KEY_A, false)),
            (OskKey::Char('m'), press(KEY_M, false)),
            (OskKey::Char('1'), press(KEY_1, false)),
            (OskKey::Char('0'), press(KEY_0, false)),
            (OskKey::Char('!'), press(KEY_1, true)),
            (OskKey::Char('@'), press(KEY_2, true)),
            (OskKey::Char(','), press(KEY_COMMA, false)),
            (OskKey::Char('.'), press(KEY_DOT, false)),
            (OskKey::Char('?'), press(KEY_SLASH, true)),
            (OskKey::Char('\''), press(KEY_APOSTROPHE, false)),
            (OskKey::Char('"'), press(KEY_APOSTROPHE, true)),
            (OskKey::Char('`'), press(KEY_GRAVE, false)),
            (OskKey::Char('~'), press(KEY_GRAVE, true)),
            (OskKey::Char('\\'), press(KEY_BACKSLASH, false)),
            (OskKey::Char('|'), press(KEY_BACKSLASH, true)),
            (OskKey::Char(' '), press(KEY_SPACE, false)),
            (OskKey::BackSpace, press(KEY_BACKSPACE, false)),
            (OskKey::Return, press(KEY_ENTER, false)),
        ];
        for (key, expected) in cases {
            assert_eq!(resolve(&keymap, 0, key), Ok(expected), "{key:?}");
        }
        // 인쇄할 수 있는 ASCII는 모두 us 배열에 있다
        for character in ' '..='~' {
            assert!(
                resolve(&keymap, 0, OskKey::Char(character)).is_ok(),
                "{character:?}"
            );
        }
        // 배열에 없는 글자는 그 글자로 알린다
        assert_eq!(resolve(&keymap, 0, OskKey::Char('€')), Err('€'));
        assert!(find_key(&keymap, 0, Keysym::Shift_L, 0).is_some());
    }

    #[test]
    fn text_input_is_published_on_change_and_on_enabling_again() {
        let mut state = TextInputState::default();
        // 처음에는 꺼져 있어 알릴 것이 없다
        assert_eq!(state.decide(false), None);
        assert_eq!(state.decide(true), Some(true));
        assert_eq!(state.decide(true), None);
        // 이미 켜진 칸을 다시 누르면 앱이 다시 켜고, 가린 키보드를 다시 띄우도록 다시 알린다
        state.enable_requested();
        assert_eq!(state.decide(true), Some(true));
        assert_eq!(state.decide(true), None);
        assert_eq!(state.decide(false), Some(false));
        // 꺼진 채로 켜는 요청만 왔으면(아직 commit 전) 알리지 않는다. 켜지면 바뀐 값으로 알린다
        state.enable_requested();
        assert_eq!(state.decide(false), None);
        assert_eq!(state.decide(true), Some(true));
    }

    #[test]
    fn windows_end_above_the_keyboard() {
        let mut area = Rectangle::new((0, 28).into(), (1280, 692).into());
        reserve_above(&mut area, 516);
        assert_eq!(area, Rectangle::new((0, 28).into(), (1280, 488).into()));
        // 이미 키보드 위에서 끝나는 영역(Dock이 더 높은 경우 등)은 그대로
        let mut area = Rectangle::new((0, 28).into(), (1280, 400).into());
        reserve_above(&mut area, 516);
        assert_eq!(area.size.h, 400);
        // 키보드가 영역 위쪽까지 덮어도 높이는 음수가 되지 않는다
        let mut area = Rectangle::new((0, 28).into(), (1280, 692).into());
        reserve_above(&mut area, 10);
        assert_eq!(area.size.h, 0);
    }

    #[test]
    fn the_keyboard_area_is_its_input_region() {
        // 입력 영역을 정하지 않은 표면은 전체가 키보드다
        assert_eq!(input_top(None), Some(0));
        // 미리 보기 풍선의 띠(위 70px)를 뺀 입력 영역
        let region = RegionAttributes {
            rects: vec![(
                RectangleKind::Add,
                Rectangle::new((0, 70).into(), (1280, 284).into()),
            )],
        };
        assert_eq!(input_top(Some(&region)), Some(70));
        // 비운 입력 영역(키보드가 다 들어갔다)은 자리를 차지하지 않는다
        assert_eq!(input_top(Some(&RegionAttributes::default())), None);
        let subtract_only = RegionAttributes {
            rects: vec![(
                RectangleKind::Subtract,
                Rectangle::new((0, 0).into(), (10, 10).into()),
            )],
        };
        assert_eq!(input_top(Some(&subtract_only)), None);
    }

    #[test]
    fn refusals_name_the_reason() {
        let refusal = |why: KeysRefused| {
            let (reply, receiver) = mpsc::channel();
            reply.send(Err(why)).unwrap();
            answer(receiver).unwrap_err()
        };
        assert!(matches!(
            refusal(KeysRefused::Locked),
            fdo::Error::AccessDenied(_)
        ));
        match refusal(KeysRefused::NotOnLayout('€')) {
            fdo::Error::InvalidArgs(message) => assert!(message.contains('€'), "{message}"),
            other => panic!("{other:?}"),
        }
    }
}
