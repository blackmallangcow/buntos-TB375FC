// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드(셸의 `bunt-osk` 레이어, shell/src/osk.rs)를 위한 컴포지터 쪽 (ARCHITECTURE 5.1·5.2).
//!
//! - 통로: 셸 전용 Wayland 프로토콜 `bunt_osk_v1`(protocols/bunt-osk-v1.xml, 클라이언트 쪽은
//!   keyboard/src/compositor.rs). 컴포지터가 셸로 띄운 특권 클라이언트만 이 전역을 본다(창 목록 프로토콜과 같다).
//!   앱과 AI 에이전트는 쓰지 못한다(에이전트에는 정책을 보는 `PressKeys`·`TypeText`가 따로 있다). 키와 글 입력
//!   상태가 셸의 Wayland 연결로만 오가므로 다른 프로그램이 엿보지 못한다.
//!   - 바꾼 점: 처음에는 세션 버스의 셸 전용 메서드(`PressOnScreenKeys`, 셸에만 보내는 신호 `TextInputChanged`)였다.
//!     부르는 쪽은 셸의 pid로 막았지만, 세션 버스는 같은 사용자의 프로그램이 모두 엿볼 수 있어(BecomeMonitor,
//!     dbus-monitor) 화면 키보드로 친 암호(Wi-Fi 암호 창, 관리자 인증 창, 앱의 암호 칸)가 그대로 보였다
//! - 글 입력 상태: 키보드 포커스를 가진 앱이 글 입력(text-input-v3)을 켰는지만 알린다(`text_input`, `watch_text_input`을
//!   보낸 객체에만). 글 내용은 알리지 않는다. 에이전트의 `TextInputActive`와 같은 상태다.
//! - 눌린 칸: 키보드는 사람이 글 칸을 눌렀을 때 뜬다(`tapped`). 칸이 저절로 포커스를 받아도 글 입력은 켜지므로 그것만으로는
//!   모른다. text-input-v3는 칸의 자리를 알리지 않아, 컴포지터가 누르기와 앱의 글 입력 요청을 함께 봐서 안다([`TapWatch`]).
//! - 키: 셸이 누른 키(`press`)를 실제 키보드와 같은 길(`KeyboardHandle::input`)로 seat에 넣는다. 입력기(fcitx5)가
//!   키보드를 잡고 있으면 그 잡기로 가서 한글이 조합된다. 가상 키보드 프로토콜(virtual-keyboard-v1)을 셸에 여는 길은
//!   쓰지 않았다: smithay의 가상 키보드는 키를 포커스를 가진 앱에 바로 보내 입력기 잡기를 거치지 않는다(입력기가 쓰지
//!   않은 키를 돌려보내는 길이라서다).
//! - 자리: 키보드는 화면 맨 아래의 overlay 레이어이고 배타 영역을 쓰지 않는다(-1). Dock보다 먼저 놓여야 하는데
//!   smithay는 배타 영역을 놓인 차례대로 쌓아 키보드를 Dock 위에 올리기 때문이다. 대신 창 자리(window_area)를
//!   키보드가 키를 받는 영역(입력 영역)의 위쪽 끝까지로 줄인다. 앱(GTK)은 가려진 글 칸이 보이게 스스로 스크롤하지
//!   않으므로 iPadOS처럼 겹치지 않고 창을 줄인다. 미리 보기 풍선을 그리는 위쪽의 투명한 띠는 입력 영역 밖이라 창
//!   자리를 줄이지 않고, 그 자리의 누르기는 아래 창이 받는다.
//! - 에이전트는 키보드를 누르지 못한다: 레이어 목록(`GetLayers`)에 들지 않고, `ClickLayer`와 화면 좌표 `Click`이
//!   키보드 위면 거절한다(agent.rs). 그러지 않으면 막은 앱에 키보드로 글을 칠 수 있다.

use std::time::{Duration, Instant};

use smithay::{
    backend::input::KeyState,
    backend::renderer::utils::with_renderer_surface_state,
    desktop::layer_map_for_output,
    input::keyboard::{FilterResult, Keycode, Keysym, xkb},
    output::Output,
    reexports::{
        wayland_protocols::wp::text_input::zv3::server::zwp_text_input_v3::{
            ContentPurpose, ZwpTextInputV3,
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
            backend::{ClientId, ObjectId},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::{RectangleKind, RegionAttributes, SurfaceAttributes, get_parent, with_states},
        text_input::TextInputSeat,
    },
};

use crate::state::{BuntState, ClientState};

use protocol::bunt_osk_v1::{self, BuntOskV1};

/// 셸 전용 프로토콜 bunt_osk_v1의 서버 쪽 (wayland-protocols처럼 XML에서 만든다)
mod protocol {
    #![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
    #![allow(non_upper_case_globals, non_snake_case, unused_imports)]
    #![allow(missing_docs, clippy::all)]

    use smithay::reexports::wayland_server;
    use smithay::reexports::wayland_server::protocol::*;

    pub mod __interfaces {
        // 만든 코드가 wayland_backend::protocol을 부른다. wayland-server가 그 모듈을 다시 내보낸다
        use smithay::reexports::wayland_server::backend as wayland_backend;
        use smithay::reexports::wayland_server::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/bunt-osk-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_server_code!("protocols/bunt-osk-v1.xml");

    // XML을 고치면 다시 빌드한다. proc macro가 읽은 파일은 cargo가 모른다
    const _: &str = include_str!("../protocols/bunt-osk-v1.xml");
}

/// 셸의 화면 키보드 레이어 이름 (shell/src/osk.rs, agent/src/atspi.rs의 KEYBOARD_WINDOW)
pub const KEYBOARD_NAMESPACE: &str = "bunt-osk";
/// bunt_osk_v1의 판
const VERSION: u32 = 1;
/// evdev 키 코드 + 8 (xkb 키 코드)의 범위 (agent.rs와 같다)
const KEYCODES: std::ops::RangeInclusive<u32> = 8..=255;
/// 손가락이나 포인터가 이보다 덜 움직였으면 누르기다. 더 움직였으면 끌기나 스크롤이다 (touch.rs의 SLOP과 같은 값)
const TAP_SLOP: f64 = 10.0;
/// 앱이 글 입력을 켜는 것을 뗀 뒤 얼마까지 그 누르기의 결과로 볼지. 앱은 바쁘면 늦는다: 메모리가 빠듯한 시험 VM에서 GTK가
/// 3초, Firefox가 4초 걸렸다. 뗀 뒤 곧 켜는 것만으로 보지 않으므로(누른 칸의 글 커서를 본다) 넉넉히 둔다
const TAP_WINDOW: Duration = Duration::from_secs(10);
/// 칸이 켜질 때 글 커서가 누른 자리의 위아래로 이만큼(논리 픽셀) 안에 있으면 그 칸을 누른 것으로 본다. 글 커서는 줄 하나의
/// 높이이고 칸은 더 크다. 웹의 댓글 칸처럼 여러 줄 칸의 빈 곳을 눌러도 글 커서는 윗줄에 있다
const CURSOR_REACH: i32 = 100;

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

impl OskKey {
    /// 칠 수 없다고 알릴 때의 글자
    fn character(self) -> char {
        match self {
            OskKey::Char(character) => character,
            OskKey::BackSpace => '\u{8}',
            OskKey::Return => '\r',
        }
    }
}

/// 셸의 키를 넣지 않은 까닭. 셸에 답하지 않고 기록만 남긴다(사람이 누른 키라 다시 보낼 것이 없다)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeysRefused {
    /// 화면이 잠겼다. 잠금 화면은 제 창 안의 키보드를 쓴다 (greeter/src/lock)
    Locked,
    /// 지금 키 배열에 없는 글자
    NotOnLayout(char),
}

/// 화면 키보드 프로토콜의 상태 (BuntState의 osk)
pub struct OskState {
    /// watch_text_input을 보낸 셸의 객체. 글 입력 상태를 이 객체에만 보낸다
    watchers: Vec<BuntOskV1>,
    /// 마지막으로 알린 글 입력 상태
    published: bool,
    tap: TapWatch,
    /// 마지막 누르기가 닿은 표면
    pressed: Option<WlSurface>,
    /// 터미널이라고 알린 글 입력 객체 (set_content_type)
    terminal: Option<ObjectId>,
}

impl OskState {
    /// bunt_osk_v1 전역을 만든다. 컴포지터가 셸로 띄운 클라이언트만 본다 ([`may_use`])
    pub fn new(display: &DisplayHandle) -> Self {
        display.create_global::<BuntState, BuntOskV1, ()>(VERSION, ());
        Self {
            watchers: Vec::new(),
            published: false,
            tap: TapWatch::default(),
            pressed: None,
            terminal: None,
        }
    }

    /// 글 입력 객체가 내용의 종류를 알렸다 (ime.rs)
    pub fn content_purpose(&mut self, text_input: &ZwpTextInputV3, purpose: WEnum<ContentPurpose>) {
        if matches!(purpose, WEnum::Value(ContentPurpose::Terminal)) {
            self.terminal = Some(text_input.id());
        } else if self.terminal.as_ref() == Some(&text_input.id()) {
            self.terminal = None;
        }
    }
}

/// 진행 중이거나 막 끝난 누르기 하나
#[derive(Debug)]
struct Pressing {
    /// 누른 자리 (전역 좌표)
    start: Point<f64, Logical>,
    /// 뗀 시각. 없으면 아직 누르는 중이다
    released: Option<Instant>,
    /// 누른 표면이 글 입력을 켰다. 글 커서가 누른 자리 근처에 나오면 그 칸을 누른 것이다
    enabled: bool,
    /// 글 칸을 누른 것으로 알아봤다. 뗀 뒤에 알린다
    on_field: bool,
}

/// 사람이 글 칸을 눌렀는지 안다. 앱은 칸의 자리를 알리지 않으므로(text-input-v3는 글 커서 자리뿐이다) 누르기와 앱의 글
/// 입력 요청을 함께 본다. 끌기와 스크롤은 누르기가 아니다. 아래가 누른 칸이다.
///
/// - 켜진 글 입력을 다시 켠다: GTK는 칸을 눌렀다 떼면 다시 켠다(gtk_text_click_gesture_released. 이 판의 text-input-v3에는
///   show_input_panel이 없어 gtk_im_context_activate_osk가 enable을 보낸다). 칸이 저절로 포커스를 받을 때는 한 번만 켠다.
/// - 누른 표면이 꺼진 글 입력을 켜고 글 커서가 누른 자리 근처에 나온다: Firefox는 뗀 뒤에 칸에 포커스를 주고 다시 켜지
///   않는다. 포커스를 받아 스스로 켠 칸(누른 창이 전에 포커스를 가진 칸, 누른 뒤 뜬 페이지의 자동 포커스 칸)의 글 커서는
///   보통 누른 자리에 없다. 새 창이나 팝업의 칸은 누른 표면이 아니다.
/// - 터미널(foot): 창 전체가 글 칸이고 다시 켜지 않는다. 글 입력이 켜진 창을 누르면 칸을 누른 것이다.
///
/// 한 누르기에 여러 번 알려도 된다. Firefox는 다른 칸을 누르면 먼저 다시 켜고, 칸을 옮기며 끈 뒤 또 켠다(숨으려던 키보드를
/// 돌려놓는다).
#[derive(Debug, Default)]
struct TapWatch {
    press: Option<Pressing>,
    /// 사람이 칸을 눌렀다. 다음 알림에서 알린다
    tapped: bool,
}

impl TapWatch {
    /// 누르기가 시작됐다. 이미 누르는 중이면(둘째 손가락, 다른 단추) 여러 손가락의 제스처이므로 그 누르기를 그만둔다
    fn press(&mut self, start: Point<f64, Logical>) {
        self.press = if self
            .press
            .as_ref()
            .is_some_and(|press| press.released.is_none())
        {
            None
        } else {
            Some(Pressing {
                start,
                released: None,
                enabled: false,
                on_field: false,
            })
        };
    }

    /// 누르기를 그만둔다 (손가락을 거뒀다, 가장자리 스와이프로 잡았다)
    fn cancel(&mut self) {
        self.press = None;
    }

    /// 뗐다. `whole_field`면 누른 표면 전체가 글 칸이다(터미널)
    fn release(&mut self, end: Point<f64, Logical>, whole_field: bool, now: Instant) {
        let Some(press) = self.press.as_mut().filter(|press| press.released.is_none()) else {
            return;
        };
        let (dx, dy) = (end.x - press.start.x, end.y - press.start.y);
        if dx.hypot(dy) >= TAP_SLOP {
            self.press = None;
            return;
        }
        press.released = Some(now);
        press.on_field |= whole_field;
        self.settle(now);
    }

    /// 포커스를 가진 앱이 켜진 글 입력을 다시 켰다. 누른 표면의 앱이다
    fn enabled_again(&mut self, now: Instant) {
        if let Some(press) = self.press.as_mut() {
            press.on_field = true;
            self.settle(now);
        }
    }

    /// 누른 표면이 꺼진 글 입력을 켰다
    fn enabled(&mut self) {
        if let Some(press) = self.press.as_mut() {
            press.enabled = true;
        }
    }

    /// 앱이 글 커서의 자리를 알렸다 (전역 y, 아래쪽 끝은 뺀다)
    fn cursor(&mut self, top: i32, bottom: i32, now: Instant) {
        if let Some(press) = self.press.as_mut().filter(|press| press.enabled) {
            let y = press.start.y;
            press.on_field |=
                f64::from(top - CURSOR_REACH) <= y && y < f64::from(bottom + CURSOR_REACH);
            self.settle(now);
        }
    }

    /// 칸을 누른 것으로 알아봤고 뗐으면 알린다. 뗀 지 오래됐으면 앱이 스스로 한 일이다
    fn settle(&mut self, now: Instant) {
        if let Some(press) = self.press.as_mut()
            && let Some(released) = press.released
            && std::mem::take(&mut press.on_field)
        {
            self.tapped |= now.duration_since(released) <= TAP_WINDOW;
        }
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
    let keysym = match key {
        OskKey::Char(character) => xkb::utf32_to_keysym(u32::from(character)),
        OskKey::BackSpace => Keysym::BackSpace,
        OskKey::Return => Keysym::Return,
    };
    if let Some(code) = find_key(keymap, layout, keysym, 0) {
        return Ok(Press { code, shift: false });
    }
    match key {
        OskKey::Char(_) => find_key(keymap, layout, keysym, 1)
            .map(|code| Press { code, shift: true })
            .ok_or(key.character()),
        _ => Err(key.character()),
    }
}

/// 키를 지금 키 배열의 키 코드로 바꾸고, Shift가 필요하면 함께 누를 Shift 키를 찾는다. 칠 수 없으면 그 글자다.
/// Shift 키가 없는 배열에서 Shift 글자(대문자, !)를 그냥 누르면 다른 글자가 되므로 그것도 칠 수 없다
fn plan(
    keymap: &xkb::Keymap,
    layout: u32,
    key: OskKey,
) -> Result<(Keycode, Option<Keycode>), char> {
    let press = resolve(keymap, layout, key)?;
    if !press.shift {
        return Ok((press.code, None));
    }
    find_key(keymap, layout, Keysym::Shift_L, 0)
        .map(|shift| (press.code, Some(shift)))
        .ok_or(key.character())
}

/// 화면 키보드 프로토콜을 쓸 수 있는 클라이언트: 컴포지터가 셸로 띄운 특권 클라이언트(bunt-shell, 로그인 화면의
/// bunt-greeter). 입력기와 잠금 화면도 컴포지터가 띄우지만 셸이 아니다
fn may_use(client: Option<&ClientState>) -> bool {
    client.is_some_and(|data| data.privileged)
}

impl GlobalDispatch<BuntOskV1, ()> for BuntState {
    fn bind(
        _state: &mut Self,
        _display: &DisplayHandle,
        _client: &Client,
        resource: New<BuntOskV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }

    fn can_view(client: Client, _global_data: &()) -> bool {
        may_use(client.get_data::<ClientState>())
    }
}

impl Dispatch<BuntOskV1, ()> for BuntState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &BuntOskV1,
        request: bunt_osk_v1::Request,
        _data: &(),
        _display: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            bunt_osk_v1::Request::Press { key } => match parse_key(&key) {
                Ok(key) => {
                    if let Err(refused) = state.on_screen_key(key) {
                        tracing::debug!("on-screen keyboard: dropped {key:?}: {refused:?}");
                    }
                }
                Err(err) => tracing::warn!("on-screen keyboard: {err}"),
            },
            bunt_osk_v1::Request::WatchTextInput => {
                if !state.osk.watchers.contains(resource) {
                    // 지금 상태를 바로 알린다. 이벤트 루프가 돌 때마다 정하므로 마지막으로 정한 값이 지금 값이다
                    resource.text_input(u32::from(state.osk.published));
                    state.osk.watchers.push(resource.clone());
                }
            }
            bunt_osk_v1::Request::Destroy => {}
        }
    }

    fn destroyed(state: &mut Self, _client: ClientId, resource: &BuntOskV1, _data: &()) {
        state.osk.watchers.retain(|watcher| watcher != resource);
    }
}

/// 같은 표면이거나 부표면까지 같은 뿌리인지. 앱은 창의 내용을 부표면에 그리기도 한다(Firefox). 팝업은 다른 표면이다
fn same_surface(left: &WlSurface, right: &WlSurface) -> bool {
    let root = |surface: &WlSurface| {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        root
    };
    root(left) == root(right)
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
    /// 셸이 누른 키 하나를 실제 키보드처럼 넣는다: Shift가 필요하면 Shift를 누른 채 눌렀다 뗀다. 컴포지터 단축키는
    /// 거치지 않는다(에이전트의 PressKeys와 같다). 입력기가 키보드를 잡고 있으면 그 잡기로 간다. 요청은 셸이 보낸
    /// 차례로 오므로 키도 누른 차례로 들어간다
    fn on_screen_key(&mut self, key: OskKey) -> Result<(), KeysRefused> {
        // 잠긴 동안 키는 잠금 화면으로 간다. 셸의 키보드는 그리지도 않으므로 받지 않는다
        if self.session_lock.is_locked() {
            return Err(KeysRefused::Locked);
        }
        let Some(keyboard) = self.seat.get_keyboard() else {
            return Ok(());
        };
        let planned = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap();
            let layout = xkb.active_layout().0;
            // 안전: 키맵의 참조를 이 닫힘 밖으로 내보내지 않는다
            let keymap = unsafe { xkb.keymap() };
            plan(keymap, layout, key)
        });
        let (code, shift) = planned.map_err(KeysRefused::NotOnLayout)?;
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
        if let Some(shift) = shift {
            send(self, shift, KeyState::Pressed);
        }
        send(self, code, KeyState::Pressed);
        send(self, code, KeyState::Released);
        if let Some(shift) = shift {
            send(self, shift, KeyState::Released);
        }
        Ok(())
    }

    /// 이벤트 루프가 한 바퀴 돌 때마다 부른다(udev.rs, 클라이언트에 보내기 전): 글 입력 상태가 바뀌었거나 사람이 칸을
    /// 눌렀으면 셸에 알린다. 잠긴 동안에는 끈 것으로 알린다. 잠금 화면의 암호 칸에 셸의 키보드를 띄우지 않는다
    pub fn publish_text_input(&mut self) {
        let active = !self.session_lock.is_locked() && self.text_input_active();
        if active != self.osk.published {
            self.osk.published = active;
            tracing::debug!("text input {}", if active { "on" } else { "off" });
            for watcher in &self.osk.watchers {
                watcher.text_input(u32::from(active));
            }
        }
        if std::mem::take(&mut self.osk.tap.tapped) && active {
            tracing::debug!("text field tapped");
            for watcher in &self.osk.watchers {
                watcher.tapped();
            }
        }
    }

    /// 손가락이나 단추가 `surface`의 `start`(전역 좌표)를 눌렀다
    pub(crate) fn note_press(&mut self, start: Point<f64, Logical>, surface: Option<&WlSurface>) {
        self.osk.tap.press(start);
        self.osk.pressed = surface.cloned();
    }

    /// 누르기를 뗐다 (`end`는 뗀 자리, 전역 좌표)
    pub(crate) fn note_release(&mut self, end: Point<f64, Logical>) {
        let whole_field = self.pressed_terminal();
        self.osk.tap.release(end, whole_field, Instant::now());
    }

    /// 누르기를 그만뒀다
    pub(crate) fn note_press_cancelled(&mut self) {
        self.osk.tap.cancel();
    }

    /// 누른 표면이 켜진 터미널의 글 입력을 가졌는지
    fn pressed_terminal(&self) -> bool {
        let Some(terminal) = &self.osk.terminal else {
            return false;
        };
        let Some(pressed) = self.osk.pressed.as_ref() else {
            return false;
        };
        let mut found = false;
        self.seat
            .text_input()
            .with_active_text_input(|text_input, focus| {
                found = text_input.id() == *terminal && same_surface(pressed, focus);
            });
        found
    }

    /// 포커스를 가진 앱이 글 입력을 켜는 요청(enable)을 보냈다 (ime.rs)
    pub(crate) fn enable_requested(&mut self, text_input: &ZwpTextInputV3) {
        let handle = self.seat.text_input();
        let mut again = false;
        handle.with_active_text_input(|active, _| again = active == text_input);
        let Some(pressed) = self.osk.pressed.as_ref() else {
            return;
        };
        if again {
            // 키보드 포커스가 앱의 다른 표면에 있어도(팝업의 칸) 다시 켜는 것은 그 칸을 누른 것이다
            if pressed.id().same_client_as(&text_input.id()) {
                self.osk.tap.enabled_again(Instant::now());
            }
        } else if handle
            .focus()
            .is_some_and(|focus| same_surface(pressed, &focus))
        {
            self.osk.tap.enabled();
        }
    }

    /// 포커스를 가진 앱이 글 커서의 자리를 알렸다 (`y`는 그 표면 안, ime.rs). GTK는 칸을 그리기 전에 높이 0으로 먼저 알린다
    pub(crate) fn cursor_rectangle_requested(&mut self, y: i32, height: i32) {
        if height <= 0 {
            return;
        }
        let origin = self
            .seat
            .text_input()
            .focus()
            .and_then(|focus| self.surface_origin(&focus));
        if let Some((origin, _)) = origin {
            let top = origin.y + y;
            self.osk.tap.cursor(top, top + height, Instant::now());
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
    fn shifted_keys_are_pressed_with_the_shift_key() {
        let Some(keymap) = us_keymap() else {
            eprintln!("xkeyboard-config is not installed; skipping the key plan test");
            return;
        };
        let shift = find_key(&keymap, 0, Keysym::Shift_L, 0);
        assert!(shift.is_some());
        let code = |evdev: u32| Keycode::new(evdev + 8);
        assert_eq!(plan(&keymap, 0, OskKey::Char('a')), Ok((code(KEY_A), None)));
        assert_eq!(
            plan(&keymap, 0, OskKey::Char('Q')),
            Ok((code(KEY_Q), shift))
        );
        assert_eq!(
            plan(&keymap, 0, OskKey::Char('!')),
            Ok((code(KEY_1), shift))
        );
        assert_eq!(
            plan(&keymap, 0, OskKey::BackSpace),
            Ok((code(KEY_BACKSPACE), None))
        );
        assert_eq!(plan(&keymap, 0, OskKey::Char('€')), Err('€'));
    }

    /// Shift 키가 없는 배열에서는 Shift 글자를 치지 않는다. 그냥 누르면 다른 글자(q)가 되기 때문이다
    #[test]
    fn shifted_keys_are_refused_without_a_shift_key() {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let Some(keymap) = xkb::Keymap::new_from_string(
            &context,
            "xkb_keymap {
                xkb_keycodes { minimum = 8; maximum = 255; <AD01> = 24; };
                xkb_types { include \"complete\" };
                xkb_compat { include \"complete\" };
                xkb_symbols { key <AD01> { [ q, Q ] }; };
            };"
            .to_owned(),
            xkb::KEYMAP_FORMAT_TEXT_V1,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        ) else {
            eprintln!("xkeyboard-config is not installed; skipping the no-shift test");
            return;
        };
        assert_eq!(
            plan(&keymap, 0, OskKey::Char('q')),
            Ok((Keycode::new(KEY_Q + 8), None))
        );
        assert_eq!(plan(&keymap, 0, OskKey::Char('Q')), Err('Q'));
        assert_eq!(plan(&keymap, 0, OskKey::Return), Err('\r'));
    }

    /// 화면 키보드 프로토콜은 컴포지터가 셸로 띄운 클라이언트만 쓴다. 앱(컴포지터가 띄우지 않았다), 입력기, 잠금
    /// 화면은 전역을 보지 못한다
    #[test]
    fn only_the_shell_may_use_the_keyboard_protocol() {
        let shell = ClientState {
            privileged: true,
            ..ClientState::default()
        };
        let input_method = ClientState {
            input_method: true,
            ..ClientState::default()
        };
        let lock_screen = ClientState {
            lock_screen: true,
            ..ClientState::default()
        };
        assert!(may_use(Some(&shell)));
        assert!(!may_use(Some(&ClientState::default())));
        assert!(!may_use(Some(&input_method)));
        assert!(!may_use(Some(&lock_screen)));
        // 컴포지터가 모르는 클라이언트 데이터 (붙지 않은 클라이언트)
        assert!(!may_use(None));
    }

    /// 누르기와 글 입력 요청을 차례로 넣고 칸을 눌렀다고 본 수를 돌려준다
    struct Tap {
        watch: TapWatch,
        now: Instant,
    }

    impl Tap {
        fn new() -> Self {
            Self {
                watch: TapWatch::default(),
                now: Instant::now(),
            }
        }

        /// y 300을 누른다
        fn press(&mut self) -> &mut Self {
            self.watch.press(Point::from((100.0, 300.0)));
            self
        }

        fn release(&mut self) -> &mut Self {
            self.watch
                .release(Point::from((103.0, 301.0)), false, self.now);
            self
        }

        fn wait(&mut self, time: Duration) -> &mut Self {
            self.now += time;
            self
        }

        fn enable(&mut self) -> &mut Self {
            self.watch.enabled();
            self
        }

        fn enable_again(&mut self) -> &mut Self {
            self.watch.enabled_again(self.now);
            self
        }

        /// 글 커서가 그 y에 나온다
        fn cursor(&mut self, y: i32) -> &mut Self {
            self.watch.cursor(y, y + 20, self.now);
            self
        }

        fn tapped(&mut self) -> bool {
            std::mem::take(&mut self.watch.tapped)
        }
    }

    /// GTK: 이미 포커스를 가진 칸을 누르면 뗀 뒤에 글 입력을 다시 켠다. 글 커서가 먼 곳이어도(문서의 빈 곳) 칸을 누른 것이다
    #[test]
    fn enabling_again_after_a_tap_is_a_tapped_field() {
        let mut tap = Tap::new();
        assert!(!tap.press().release().tapped());
        // 바쁜 앱은 한참 뒤에 켠다
        assert!(tap.wait(Duration::from_secs(3)).enable_again().tapped());
        // 너무 늦으면 누른 것과 상관없다
        assert!(!tap.wait(TAP_WINDOW * 2).enable_again().tapped());
    }

    /// GTK: 포커스가 없는 칸을 누르면 누르는 동안 켜고, 뗀 뒤에 다시 켠다
    #[test]
    fn enabling_during_a_tap_counts_when_released() {
        let mut tap = Tap::new();
        assert!(!tap.press().enable().cursor(290).tapped());
        assert!(tap.release().tapped());
        assert!(tap.enable_again().tapped());
        // 뗄 때까지는 끌기일 수 있다: 칸에서 시작한 스크롤은 누른 것이 아니다
        tap.press().enable().cursor(290);
        tap.watch
            .release(Point::from((100.0, 360.0)), false, tap.now);
        assert!(!tap.tapped());
        assert!(!tap.enable_again().tapped());
    }

    /// Firefox: 뗀 뒤에 칸에 포커스를 주며 켠다. 바쁘면 글 커서를 나중에 알린다
    #[test]
    fn a_cursor_appearing_where_it_was_tapped_is_a_tapped_field() {
        let mut tap = Tap::new();
        assert!(!tap.press().release().tapped());
        assert!(!tap.wait(Duration::from_secs(3)).enable().tapped());
        assert!(tap.cursor(290).tapped());
        // 위아래로 조금 떨어진 곳은 칸 안이다 (글 커서는 칸보다 작다, 여러 줄 칸의 빈 곳)
        assert!(
            tap.press()
                .release()
                .enable()
                .cursor(300 - CURSOR_REACH)
                .tapped()
        );
        assert!(
            tap.press()
                .release()
                .enable()
                .cursor(300 + CURSOR_REACH - 20)
                .tapped()
        );
        // 칸을 옮기며 끈 뒤 또 켜도 그 누르기의 결과다
        assert!(
            tap.wait(Duration::from_secs(1))
                .enable()
                .cursor(290)
                .tapped()
        );
        // 앱이 한참 뒤에 그 자리의 칸을 켠 것은 상관없다
        tap.press().release().wait(TAP_WINDOW * 2);
        assert!(!tap.enable().cursor(290).tapped());
    }

    /// 포커스를 받아 스스로 켠 칸: 누른 창이 전에 포커스를 가진 칸, 누른 뒤 뜬 페이지의 자동 포커스 칸
    #[test]
    fn a_cursor_appearing_elsewhere_is_not_a_tapped_field() {
        let mut tap = Tap::new();
        assert!(!tap.press().enable().cursor(600).release().tapped());
        assert!(!tap.press().release().enable().cursor(600).tapped());
        assert!(
            !tap.press()
                .release()
                .enable()
                .cursor(300 - CURSOR_REACH - 21)
                .tapped()
        );
        assert!(
            !tap.press()
                .release()
                .enable()
                .cursor(300 + CURSOR_REACH + 1)
                .tapped()
        );
        // 그 칸을 눌렀으면 GTK가 뗀 뒤에 다시 켠다
        assert!(
            tap.press()
                .enable()
                .cursor(600)
                .release()
                .enable_again()
                .tapped()
        );
    }

    /// 켜지 않았으면 글 커서가 움직여도(친 글) 누른 것이 아니다
    #[test]
    fn a_cursor_moving_without_enabling_is_not_a_tapped_field() {
        let mut tap = Tap::new();
        assert!(!tap.press().release().cursor(290).tapped());
    }

    /// 누르기 없이 창이 열리며 칸이 포커스를 받거나 앱이 스스로 다시 켠 것은 누른 것이 아니다
    #[test]
    fn enabling_without_a_tap_is_not_a_tapped_field() {
        let mut tap = Tap::new();
        assert!(!tap.enable().cursor(290).tapped());
        assert!(!tap.enable_again().tapped());
    }

    #[test]
    fn several_fingers_and_cancelled_presses_are_not_a_tap() {
        let mut tap = Tap::new();
        // 둘째 손가락
        tap.press().press().enable().cursor(290).release();
        assert!(!tap.enable_again().tapped());
        // 앱에서 거둔 손가락
        tap.press().enable().cursor(290);
        tap.watch.cancel();
        assert!(!tap.release().enable_again().tapped());
    }

    /// 터미널은 창 전체가 글 칸이고 글 입력을 다시 켜지 않는다
    #[test]
    fn tapping_a_terminal_is_a_tapped_field() {
        let mut tap = Tap::new();
        tap.press();
        tap.watch
            .release(Point::from((100.0, 300.0)), true, tap.now);
        assert!(tap.tapped());
        // 끌면 (선택, 스크롤) 아니다
        tap.press();
        tap.watch
            .release(Point::from((100.0, 400.0)), true, tap.now);
        assert!(!tap.tapped());
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
}
