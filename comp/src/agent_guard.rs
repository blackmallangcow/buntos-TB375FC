// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 사람과 AI가 한 컴퓨터를 함께 쓸 때 사람을 먼저 둔다 (AI 동시 사용 설계 0장, 4장, 6장).
//!
//! - **사람 입력 기록:** 실제 장치에서 온 입력(키, 포인터 움직임·단추·휠, 터치, 셸의 화면 키보드)만 적는다. 에이전트의
//!   입력은 실제 장치의 길(input.rs의 process_input_event)을 거치지 않으므로, 사람의 입력인지는 출처로 정확히 안다.
//! - **R2 사람 우선:** 사람이 `yield-ms`(comp.toml `[agent]`, 기본 1500) 안에 입력했거나, 키·단추를 누르고 있거나, 사람이
//!   시작한 포인터 잡기(옮기기, 크기 바꾸기, 메뉴, 끌어 놓기, 스크린샷 영역 고르기)가 있으면 에이전트의 입력과 포커스
//!   바꾸기를 UserActive로 거절하고 다시 해 볼 때를 알린다. 키·글은 사람의 키보드와, 포인터 동작은 사람의 포인터와
//!   견주고, 키보드 포커스를 바꾸는 일은 둘 다와 견준다. 사람이 키를 누르고 있으면 포인터 동작도 거절한다: 에이전트의
//!   누르기가 사람의 ⇧·⌘와 섞인다. 사람이 연 메뉴가 키보드를 받는 동안은 키·글도 거절한다: 키가 그 메뉴로 간다.
//! - **R3 새 창:** 사람이 앱에 글을 치는 중이면 다른 앱의 새 창은 키보드를 가져가지 않고 사람의 창 뒤에 뜬다(macOS의
//!   협조적 활성화). 판단은 여기서, 창 놓기는 state.rs에서 한다.
//! - **R4 멈춤:** 사람이 메뉴 막대(셸)나 단축키(⌃⌥⌘.)로 AI 제어를 멈추면 에이전트의 모든 입력과 포커스 바꾸기를 Paused로
//!   거절한다. 읽기는 된다. 멈춤은 컴포지터가 갖고 사람만 푼다. 데몬이 다시 시작해도 그대로다.
//! - **활동:** 데몬이 알린 AI 이름과 대상 창은 마지막 알림 뒤 5초 동안 "활동 중"이다. 셸이 메뉴 막대에 보인다.
//!   대상 요소의 강조 테두리는 agent_target.rs가 그린다.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use smithay::{reexports::calloop::RegistrationToken, utils::Serial};

use crate::dbus::Refused;

/// 사람 입력 뒤 에이전트가 기다리는 기본 시간 (comp.toml `[agent] yield-ms`)
pub const DEFAULT_YIELD_MS: u64 = 1500;
/// 마지막 ReportAgentActivity 뒤 활동 중으로 보이는 시간
pub const ACTIVE_FOR: Duration = Duration::from_secs(5);
/// 기억해 두는 에이전트 입력의 serial 수. 에이전트가 메뉴를 연 누르기의 serial이 그 뒤에 친 키에 밀려나지 않게 넉넉히 둔다
const AGENT_SERIALS: usize = 512;
/// AI 이름(ReportAgentActivity의 client)의 최대 글자 수. 메뉴 막대에 보인다
const CLIENT_CHARS: usize = 64;

/// 사람의 실제 입력 기록. 에이전트의 입력은 이것을 바꾸지 않는다
#[derive(Debug, Default)]
pub struct HumanInput {
    /// 마지막 키 (누르기와 떼기)
    last_key: Option<Instant>,
    /// 마지막 키 누르기와, 그것이 앱에 친 글인지(컴포지터 단축키도 아니고 셸 표면에 간 것도 아니다) (R3)
    last_press: Option<(Instant, bool)>,
    /// 마지막 포인터 움직임·단추·휠과 터치
    last_pointer: Option<Instant>,
    /// 마지막 단추 누르기와 손가락 닿기. 그 뒤로는 글을 치는 중이 아니다 (R3)
    last_click: Option<Instant>,
    /// 누르고 있는 키 (xkb 키 코드)
    keys: Vec<u32>,
    /// 누르고 있는 포인터 단추 (linux 단추 번호)
    buttons: Vec<u32>,
}

impl HumanInput {
    /// 키 하나. `typed`면 앱에 친 글이다: 컴포지터 단축키가 가로채지 않았고 셸 표면에 가지 않았다
    pub fn key(&mut self, code: u32, pressed: bool, typed: bool, now: Instant) {
        self.last_key = Some(now);
        if pressed {
            if !self.keys.contains(&code) {
                self.keys.push(code);
            }
            self.last_press = Some((now, typed));
        } else {
            self.keys.retain(|held| *held != code);
        }
    }

    /// 셸의 화면 키보드로 친 키 (컴포지터가 눌렀다 바로 뗀다)
    pub fn tapped(&mut self, typed: bool, now: Instant) {
        self.last_key = Some(now);
        self.last_press = Some((now, typed));
    }

    /// 포인터 움직임, 휠, 손가락 움직임과 떼기
    pub fn pointer(&mut self, now: Instant) {
        self.last_pointer = Some(now);
    }

    /// 포인터 단추 하나
    pub fn button(&mut self, button: u32, pressed: bool, now: Instant) {
        self.last_pointer = Some(now);
        if pressed {
            if !self.buttons.contains(&button) {
                self.buttons.push(button);
            }
            self.last_click = Some(now);
        } else {
            self.buttons.retain(|held| *held != button);
        }
    }

    /// 손가락이 닿았다
    pub fn touch_down(&mut self, now: Instant) {
        self.last_pointer = Some(now);
        self.last_click = Some(now);
    }

    /// 사람이 `window` 안에 키·포인터·터치를 썼거나 키나 단추를 누르고 있다 (프레임 바닥, frame_floor.rs)
    pub fn active_within(&self, window: Duration, now: Instant) -> bool {
        !self.keys.is_empty()
            || !self.buttons.is_empty()
            || [self.last_key, self.last_pointer, self.last_click]
                .into_iter()
                .flatten()
                .any(|at| now.saturating_duration_since(at) <= window)
    }

    /// 세션을 잃으면(가상 터미널 전환) 누르고 있던 키와 단추를 떼는 이벤트가 오지 않는다
    pub fn release_all(&mut self) {
        self.keys.clear();
        self.buttons.clear();
    }
}

/// 장치 기록 밖에서 컴포지터가 그때그때 알려 주는 사람의 포인터 상태
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerHold {
    /// 화면에 손가락이 닿아 있다
    pub fingers: bool,
    /// 사람이 시작한 포인터 잡기(옮기기, 크기 바꾸기, 메뉴, 끌어 놓기)나 스크린샷 영역 고르기가 있다
    pub grab: bool,
    /// 사람이 연 메뉴가 키보드를 받고 있다. 에이전트의 키가 그 메뉴로 가므로 키·글도 미룬다
    pub menu: bool,
}

/// 에이전트의 동작이 사람의 무엇과 부딪치는지
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Touches {
    pub keyboard: bool,
    pub pointer: bool,
}

impl Touches {
    /// 키와 글 (R2a)
    pub const KEYBOARD: Self = Self {
        keyboard: true,
        pointer: false,
    };
    /// 포인터 누르기와 휠 (R2b)
    pub const POINTER: Self = Self {
        keyboard: false,
        pointer: true,
    };
    /// 키보드 포커스를 바꾸는 일: 창 활성화, 다른 표면으로 포커스를 옮기는 누르기 (R2a와 R2b)
    pub const FOCUS: Self = Self {
        keyboard: true,
        pointer: true,
    };
}

/// 셸과 에이전트에 알리는 AI 제어 상태 (GetAgentState, AgentStateChanged)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentStatus {
    pub paused: bool,
    /// 마지막 ReportAgentActivity 뒤 5초가 지나지 않았다
    pub active: bool,
    /// 마지막으로 알린 AI 클라이언트의 이름
    pub client: String,
    /// 마지막으로 알린 대상 창의 pid, 앱 ID, 제목. 창을 찾지 못했으면 앱 ID는 비고 제목은 알린 그대로다
    pub pid: u32,
    pub app_id: String,
    pub title: String,
}

/// 마지막으로 알린 에이전트 활동
#[derive(Debug, Default)]
struct Activity {
    client: String,
    pid: u32,
    app_id: String,
    title: String,
    at: Option<Instant>,
}

pub struct AgentGuard {
    /// 사람 입력 뒤 에이전트가 기다리는 시간
    yield_for: Duration,
    pub human: HumanInput,
    paused: bool,
    /// 에이전트가 넣은 키·단추의 serial (최근 것). 그 serial로 시작한 잡기(에이전트가 연 메뉴)는 사람의 것이 아니다
    agent_serials: VecDeque<Serial>,
    activity: Activity,
    /// 셸과 에이전트에 마지막으로 알린 상태
    published: AgentStatus,
    /// 활동이 끝날 때(active가 꺼질 때)를 기다리는 타이머
    pub activity_timer: Option<RegistrationToken>,
}

impl AgentGuard {
    pub fn new(yield_ms: u64) -> Self {
        Self {
            yield_for: Duration::from_millis(yield_ms),
            human: HumanInput::default(),
            paused: false,
            agent_serials: VecDeque::new(),
            activity: Activity::default(),
            published: AgentStatus::default(),
            activity_timer: None,
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// R4와 R2: 멈췄으면 Paused, 사람이 그 장치를 쓰는 중이면 UserActive(다시 해 볼 때까지의 ms)
    pub fn admit(&self, touches: Touches, hold: PointerHold, now: Instant) -> Result<(), Refused> {
        if self.paused {
            return Err(Refused::Paused);
        }
        match self.busy(touches, hold, now) {
            Some(retry_ms) => Err(Refused::UserActive { retry_ms }),
            None => Ok(()),
        }
    }

    /// 사람이 그 장치를 쓰는 중이면 다시 해 볼 때까지의 ms (1 이상). 키·단추를 누르고 있거나 잡고 있으면 언제 끝날지
    /// 모르므로 yield-ms를 알린다
    pub fn busy(&self, touches: Touches, hold: PointerHold, now: Instant) -> Option<u64> {
        let whole = ceil_millis(self.yield_for).max(1);
        let human = &self.human;
        let keys_held = !human.keys.is_empty();
        let mut waits = Vec::new();
        if touches.keyboard {
            waits.push((keys_held || hold.menu).then_some(whole));
            waits.push(left(human.last_key, self.yield_for, now));
        }
        if touches.pointer {
            let held = keys_held || !human.buttons.is_empty() || hold.fingers || hold.grab;
            waits.push(held.then_some(whole));
            waits.push(left(human.last_pointer, self.yield_for, now));
        }
        waits.into_iter().flatten().max()
    }

    /// 대상을 받는 키·글(PressKeysOn, TypeTextOn, CommitTextOn)을 넣어도 되는지. 멈췄으면 Paused(R4), 키보드 포커스의
    /// 뿌리 표면이 대상 창이 아니면 FocusMoved(R1), 사람이 키보드를 쓰는 중이면 UserActive(R2a)다. 포커스가 옮겨 간 것은
    /// 기다려도 풀리지 않으므로 사람이 쓰는 중인 것보다 먼저 알린다
    pub fn keyboard_target<S: PartialEq>(
        &self,
        focus: Option<&S>,
        target: &S,
        hold: PointerHold,
        now: Instant,
    ) -> Result<(), Refused> {
        if self.paused {
            return Err(Refused::Paused);
        }
        if focus != Some(target) {
            return Err(Refused::FocusMoved);
        }
        self.admit(Touches::KEYBOARD, hold, now)
    }

    /// R3: 새 창이 키보드 포커스를 가져가지 않고 사람의 창 뒤에 떠야 하는지. 사람이 앱에 글을 치는 중이고, 새 창이
    /// 사람이 치고 있는 앱(`same_app`)의 것이 아니다. 그 앱이 연 창(⌘N, 대화 상자)은 사람이 한 일이다
    pub fn new_window_waits(&self, same_app: bool, now: Instant) -> bool {
        !same_app && self.human_typing(now)
    }

    /// R3: 사람이 앱에 글을 치는 중이다. 마지막 키 누르기가 앱에 친 글이고, 그것이 yield-ms 안이거나 키를 아직 누르고
    /// 있다. 그 뒤에 단추를 눌렀거나 손가락이 닿았으면(사람이 다른 것을 골랐다) 치는 중이 아니다
    pub fn human_typing(&self, now: Instant) -> bool {
        let human = &self.human;
        let Some((at, true)) = human.last_press else {
            return false;
        };
        if human.last_click.is_some_and(|click| click >= at) {
            return false;
        }
        !human.keys.is_empty() || left(Some(at), self.yield_for, now).is_some()
    }

    /// 에이전트가 넣은 키·단추의 serial을 적는다
    pub fn note_agent_serial(&mut self, serial: Serial) {
        if self.agent_serials.len() == AGENT_SERIALS {
            self.agent_serials.pop_front();
        }
        self.agent_serials.push_back(serial);
    }

    pub fn agent_serials(&self) -> impl Iterator<Item = Serial> + '_ {
        self.agent_serials.iter().copied()
    }

    /// 데몬이 알린 활동 (ReportAgentActivity). 앱 ID와 제목은 컴포지터가 그 창에서 채운 것이다
    pub fn report(&mut self, client: &str, pid: u32, app_id: String, title: String, now: Instant) {
        self.activity = Activity {
            client: client_name(client),
            pid,
            app_id,
            title,
            at: Some(now),
        };
    }

    /// 활동 중(active)이 꺼질 때까지 남은 시간. 활동 중이 아니면 None
    pub fn active_left(&self, now: Instant) -> Option<Duration> {
        let at = self.activity.at?;
        ACTIVE_FOR
            .checked_sub(now.saturating_duration_since(at))
            .filter(|left| !left.is_zero())
    }

    pub fn status(&self, now: Instant) -> AgentStatus {
        AgentStatus {
            paused: self.paused,
            active: self.active_left(now).is_some(),
            client: self.activity.client.clone(),
            pid: self.activity.pid,
            app_id: self.activity.app_id.clone(),
            title: self.activity.title.clone(),
        }
    }

    /// 마지막으로 알린 뒤 상태가 바뀌었으면 그 상태. 알린 것으로 적는다
    pub fn changed(&mut self, now: Instant) -> Option<AgentStatus> {
        let status = self.status(now);
        if status == self.published {
            return None;
        }
        self.published = status.clone();
        Some(status)
    }
}

/// 메뉴 막대에 보일 AI 이름: 제어 문자를 빼고 CLIENT_CHARS 글자까지
fn client_name(client: &str) -> String {
    client
        .chars()
        .filter(|character| !character.is_control())
        .take(CLIENT_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

/// 사람 입력 뒤 기다릴 시간이 남았으면 그 ms (올림, 1 이상)
fn left(last: Option<Instant>, window: Duration, now: Instant) -> Option<u64> {
    let elapsed = now.saturating_duration_since(last?);
    let left = window.checked_sub(elapsed).filter(|left| !left.is_zero())?;
    Some(ceil_millis(left).max(1))
}

fn ceil_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos().div_ceil(1_000_000)).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> (AgentGuard, Instant) {
        (AgentGuard::new(DEFAULT_YIELD_MS), Instant::now())
    }

    fn after(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    const IDLE: PointerHold = PointerHold {
        fingers: false,
        grab: false,
        menu: false,
    };

    #[test]
    fn nothing_is_refused_before_the_person_does_anything() {
        let (guard, start) = guard();
        for touches in [Touches::KEYBOARD, Touches::POINTER, Touches::FOCUS] {
            assert_eq!(guard.admit(touches, IDLE, start), Ok(()));
        }
        assert!(!guard.human_typing(start));
    }

    /// R2a: 사람의 키 입력 뒤 yield-ms 동안 에이전트의 키·글과 활성화를 거절하고 남은 시간을 알린다
    #[test]
    fn keys_wait_for_the_persons_typing() {
        let (mut guard, start) = guard();
        guard.human.key(38, true, true, start);
        guard.human.key(38, false, true, after(start, 100));
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, after(start, 600)),
            Err(Refused::UserActive { retry_ms: 1000 })
        );
        assert_eq!(
            guard.admit(Touches::FOCUS, IDLE, after(start, 600)),
            Err(Refused::UserActive { retry_ms: 1000 })
        );
        // 포인터 동작은 사람의 키보드와 견주지 않는다 (누르고 있는 키가 없으면)
        assert_eq!(
            guard.admit(Touches::POINTER, IDLE, after(start, 600)),
            Ok(())
        );
        // 남은 시간은 올림하고, 끝나면 받는다
        assert_eq!(
            guard.busy(Touches::KEYBOARD, IDLE, after(start, 1599)),
            Some(1)
        );
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, after(start, 1600)),
            Ok(())
        );
    }

    /// 사람이 키를 누르고 있으면(⇧를 쥔 채) 언제 끝날지 모르므로 yield-ms를 알린다. 포인터 누르기도 그 수정 키와 섞이므로
    /// 거절한다 (시험 14)
    #[test]
    fn a_held_key_refuses_keys_and_clicks_until_it_is_released() {
        let (mut guard, start) = guard();
        guard.human.key(50, true, true, start);
        let late = after(start, 10_000);
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, late),
            Err(Refused::UserActive { retry_ms: 1500 })
        );
        assert_eq!(
            guard.admit(Touches::POINTER, IDLE, late),
            Err(Refused::UserActive { retry_ms: 1500 })
        );
        guard.human.key(50, false, true, late);
        assert_eq!(
            guard.admit(Touches::POINTER, IDLE, after(start, 10_001)),
            Ok(())
        );
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, after(start, 11_500)),
            Ok(())
        );
    }

    /// R2b: 포인터 움직임·단추·휠·터치 뒤, 단추를 누르는 동안, 손가락이 닿은 동안, 사람이 시작한 잡기 동안
    #[test]
    fn pointer_actions_wait_for_the_persons_pointer() {
        let (mut guard, start) = guard();
        guard.human.pointer(start);
        assert_eq!(
            guard.admit(Touches::POINTER, IDLE, after(start, 500)),
            Err(Refused::UserActive { retry_ms: 1000 })
        );
        // 사람의 포인터만 움직였으면 에이전트의 키는 받는다
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, after(start, 500)),
            Ok(())
        );
        let late = after(start, 5000);
        assert_eq!(guard.admit(Touches::POINTER, IDLE, late), Ok(()));
        guard.human.button(272, true, late);
        assert_eq!(
            guard.admit(Touches::POINTER, IDLE, after(start, 9000)),
            Err(Refused::UserActive { retry_ms: 1500 })
        );
        guard.human.button(272, false, after(start, 9000));
        assert_eq!(
            guard.admit(Touches::POINTER, IDLE, after(start, 10_500)),
            Ok(())
        );
        for hold in [
            PointerHold {
                fingers: true,
                ..IDLE
            },
            PointerHold { grab: true, ..IDLE },
        ] {
            assert_eq!(
                guard.admit(Touches::POINTER, hold, after(start, 20_000)),
                Err(Refused::UserActive { retry_ms: 1500 })
            );
            assert_eq!(
                guard.admit(Touches::KEYBOARD, hold, after(start, 20_000)),
                Ok(())
            );
        }
    }

    /// 사람이 연 메뉴가 키보드를 받는 동안은 에이전트의 키·글도 미룬다: 그 키는 사람의 메뉴로 간다(VM 시험). 메뉴는
    /// 포인터 잡기이기도 해 누르기와 휠도 미룬다
    #[test]
    fn a_menu_the_person_opened_holds_keys_and_clicks() {
        let (guard, start) = guard();
        let menu = PointerHold {
            grab: true,
            menu: true,
            ..IDLE
        };
        for touches in [Touches::KEYBOARD, Touches::POINTER, Touches::FOCUS] {
            assert_eq!(
                guard.admit(touches, menu, after(start, 10_000)),
                Err(Refused::UserActive { retry_ms: 1500 })
            );
        }
        assert_eq!(
            guard.keyboard_target(Some(&"editor"), &"editor", menu, start),
            Err(Refused::UserActive { retry_ms: 1500 })
        );
        // 사람이 창을 끄는 잡기(메뉴가 아니다)는 다른 창에 치는 에이전트의 글을 막지 않는다
        let drag = PointerHold { grab: true, ..IDLE };
        assert_eq!(guard.admit(Touches::KEYBOARD, drag, start), Ok(()));
    }

    /// 두 장치를 다 보는 활성화는 더 오래 기다릴 쪽을 알린다
    #[test]
    fn focus_changes_wait_for_both_devices() {
        let (mut guard, start) = guard();
        guard.human.key(38, true, true, start);
        guard.human.key(38, false, true, start);
        guard.human.pointer(after(start, 1000));
        assert_eq!(
            guard.admit(Touches::FOCUS, IDLE, after(start, 1200)),
            Err(Refused::UserActive { retry_ms: 1300 })
        );
    }

    /// R4: 멈춘 동안은 사람이 쉬어도 모두 거절하고, 사람이 다시 하기 전에는 풀리지 않는다
    #[test]
    fn pausing_refuses_everything_until_the_person_resumes() {
        let (mut guard, start) = guard();
        guard.set_paused(true);
        for touches in [Touches::KEYBOARD, Touches::POINTER, Touches::FOCUS] {
            assert_eq!(
                guard.admit(touches, IDLE, after(start, 60_000)),
                Err(Refused::Paused)
            );
        }
        // 멈춤이 사람의 입력보다 먼저다
        guard.human.key(38, true, true, start);
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, start),
            Err(Refused::Paused)
        );
        guard.set_paused(false);
        guard.human.key(38, false, true, start);
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, after(start, 1500)),
            Ok(())
        );
    }

    /// R3: 사람이 앱에 글을 치는 중이면 새 창이 키보드를 가져가지 않는다. 단축키(⌥⏎로 터미널)나 셸(모든 앱 패널에서
    /// ⏎)로 연 창, 사람이 누른 뒤에 뜬 창은 포커스를 받는다
    #[test]
    fn new_windows_wait_only_while_the_person_types_into_an_app() {
        let (mut guard, start) = guard();
        guard.human.key(38, true, true, start);
        guard.human.key(38, false, true, after(start, 50));
        assert!(guard.human_typing(after(start, 1000)));
        assert!(!guard.human_typing(after(start, 1500)));
        // 셸 표면에 쳤거나 단축키였다
        guard.human.key(36, true, false, after(start, 2000));
        guard.human.key(36, false, false, after(start, 2050));
        assert!(!guard.human_typing(after(start, 2100)));
        // 화면 키보드로 앱에 쳤다
        guard.human.tapped(true, after(start, 3000));
        assert!(guard.human_typing(after(start, 3100)));
        // 그 뒤 사람이 단추를 눌렀다 (Dock에서 앱을 열었다)
        guard.human.button(272, true, after(start, 3200));
        guard.human.button(272, false, after(start, 3250));
        assert!(!guard.human_typing(after(start, 3300)));
        // 손가락도 같다
        guard.human.key(38, true, true, after(start, 4000));
        guard.human.key(38, false, true, after(start, 4000));
        guard.human.touch_down(after(start, 4100));
        assert!(!guard.human_typing(after(start, 4200)));
        // 앱에 친 키를 누르고 있는 동안은 yield-ms가 지나도 치는 중이다 (지우기 키를 누르고 있다)
        guard.human.key(22, true, true, after(start, 5000));
        assert!(guard.human_typing(after(start, 9000)));
        guard.human.key(22, false, true, after(start, 9000));
        assert!(!guard.human_typing(after(start, 10_500)));
    }

    /// R1: 대상을 받는 키·글은 키보드 포커스의 뿌리 표면이 그 창일 때만 넣는다. 멈춤이 먼저고, 포커스가 옮겨 간 것은
    /// 사람이 치는 중인 것보다 먼저 알린다
    #[test]
    fn targeted_keys_need_the_focus_on_their_window() {
        let (mut guard, start) = guard();
        let (editor, settings) = ("editor", "settings");
        assert_eq!(
            guard.keyboard_target(Some(&editor), &editor, IDLE, start),
            Ok(())
        );
        // 사람이 설정 창을 눌러 포커스를 옮겼다
        assert_eq!(
            guard.keyboard_target(Some(&settings), &editor, IDLE, start),
            Err(Refused::FocusMoved)
        );
        assert_eq!(
            guard.keyboard_target(None, &editor, IDLE, start),
            Err(Refused::FocusMoved)
        );
        // 사람이 그 창에 치는 중이면 기다린다. 다른 창에 치는 중이면 포커스가 옮겨 갔다고 알린다
        guard.human.key(38, true, true, start);
        guard.human.key(38, false, true, after(start, 20));
        assert_eq!(
            guard.keyboard_target(Some(&editor), &editor, IDLE, after(start, 520)),
            Err(Refused::UserActive { retry_ms: 1000 })
        );
        assert_eq!(
            guard.keyboard_target(Some(&settings), &editor, IDLE, after(start, 520)),
            Err(Refused::FocusMoved)
        );
        // 사람의 포인터만으로는 키를 막지 않는다
        guard.human.pointer(after(start, 3000));
        assert_eq!(
            guard.keyboard_target(Some(&editor), &editor, IDLE, after(start, 3100)),
            Ok(())
        );
        guard.set_paused(true);
        assert_eq!(
            guard.keyboard_target(Some(&settings), &editor, IDLE, after(start, 9000)),
            Err(Refused::Paused)
        );
    }

    /// R3: 사람이 치는 앱의 새 창(⌘N, 대화 상자)은 포커스를 받고, 다른 앱의 새 창은 사람이 치는 동안 뒤에 뜬다
    #[test]
    fn only_other_apps_wait_while_the_person_types() {
        let (mut guard, start) = guard();
        assert!(!guard.new_window_waits(false, start));
        guard.human.key(38, true, true, start);
        guard.human.key(38, false, true, start);
        let soon = after(start, 300);
        assert!(guard.new_window_waits(false, soon));
        assert!(!guard.new_window_waits(true, soon));
        assert!(!guard.new_window_waits(false, after(start, 1500)));
    }

    /// 가상 터미널로 갔다 오면 떼기가 오지 않은 키와 단추를 잊는다
    #[test]
    fn losing_the_session_forgets_held_keys_and_buttons() {
        let (mut guard, start) = guard();
        guard.human.key(37, true, true, start);
        guard.human.button(272, true, start);
        guard.human.release_all();
        let late = after(start, 2000);
        assert_eq!(guard.admit(Touches::FOCUS, IDLE, late), Ok(()));
    }

    #[test]
    fn activity_covers_recent_input_and_held_keys() {
        let mut human = HumanInput::default();
        let start = Instant::now();
        let window = Duration::from_secs(3);
        assert!(!human.active_within(window, start));
        // 손가락이 닿고 움직인 뒤 3초까지
        human.touch_down(start);
        human.pointer(after(start, 500));
        assert!(human.active_within(window, after(start, 3400)));
        assert!(!human.active_within(window, after(start, 3600)));
        // 키를 누르고 있는 동안은 계속이다 (화살표 키로 넘기기)
        human.key(116, true, false, after(start, 4000));
        assert!(human.active_within(window, after(start, 60_000)));
        human.key(116, false, false, after(start, 60_000));
        assert!(human.active_within(window, after(start, 62_000)));
        assert!(!human.active_within(window, after(start, 64_000)));
    }

    #[test]
    fn a_zero_yield_still_waits_for_held_keys() {
        let mut guard = AgentGuard::new(0);
        let start = Instant::now();
        guard.human.key(38, true, true, start);
        guard.human.key(38, false, true, start);
        assert_eq!(guard.admit(Touches::KEYBOARD, IDLE, start), Ok(()));
        guard.human.key(50, true, true, start);
        assert_eq!(
            guard.admit(Touches::KEYBOARD, IDLE, start),
            Err(Refused::UserActive { retry_ms: 1 })
        );
    }

    /// 활동은 마지막 알림 뒤 5초 동안이다. 바뀐 상태만 알린다
    #[test]
    fn activity_lasts_five_seconds_after_the_last_report() {
        let (mut guard, start) = guard();
        assert_eq!(guard.changed(start), None);
        guard.report(
            "Claude",
            42,
            "org.gnome.TextEditor".into(),
            "새 문서".into(),
            start,
        );
        let status = guard.changed(start).expect("a report changes the state");
        assert_eq!(
            status,
            AgentStatus {
                paused: false,
                active: true,
                client: "Claude".into(),
                pid: 42,
                app_id: "org.gnome.TextEditor".into(),
                title: "새 문서".into(),
            }
        );
        // 같은 대상을 다시 알리면 상태는 그대로이고 5초를 새로 잰다
        guard.report(
            "Claude",
            42,
            "org.gnome.TextEditor".into(),
            "새 문서".into(),
            after(start, 3000),
        );
        assert_eq!(guard.changed(after(start, 3000)), None);
        assert_eq!(
            guard.active_left(after(start, 4000)),
            Some(Duration::from_secs(4))
        );
        assert_eq!(guard.changed(after(start, 7999)), None);
        let ended = guard.changed(after(start, 8000)).expect("activity ends");
        assert!(!ended.active);
        // 끝난 뒤에도 마지막 AI와 대상은 남는다
        assert_eq!((ended.client.as_str(), ended.pid), ("Claude", 42));
        assert_eq!(guard.active_left(after(start, 8000)), None);
        // 멈춤도 바뀐 상태다
        guard.set_paused(true);
        assert!(
            guard
                .changed(after(start, 9000))
                .is_some_and(|status| status.paused)
        );
        assert_eq!(guard.changed(after(start, 9000)), None);
    }

    #[test]
    fn client_names_are_short_and_printable() {
        assert_eq!(client_name("  Claude Code\n"), "Claude Code");
        assert_eq!(client_name(&"가".repeat(100)).chars().count(), CLIENT_CHARS);
        assert_eq!(client_name("a\u{1b}[31mb"), "a[31mb");
    }

    #[test]
    fn agent_serials_keep_the_latest_ones() {
        let (mut guard, _) = guard();
        for raw in 0..(AGENT_SERIALS as u32 + 10) {
            guard.note_agent_serial(Serial::from(raw));
        }
        let kept: Vec<Serial> = guard.agent_serials().collect();
        assert_eq!(kept.len(), AGENT_SERIALS);
        assert_eq!(kept.first().copied(), Some(Serial::from(10)));
        assert_eq!(
            kept.last().copied(),
            Some(Serial::from(AGENT_SERIALS as u32 + 9))
        );
    }
}
