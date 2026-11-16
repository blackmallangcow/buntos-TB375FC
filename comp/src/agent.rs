// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 에이전트(buntd-agent)가 부르는 컴포지터 기능: 창 목록, 창 활성화, 키·포인터 입력
//! (D-Bus의 에이전트 전용 메서드, ARCHITECTURE 6장). 입력은 사용자의 seat에 넣으며 실제 입력과
//! 같은 길을 거친다. 키는 컴포지터 단축키를 거치지 않는다. 에이전트 전용 seat는 뒤에 둔다.
//! 셸의 레이어 표면(메뉴 막대, Dock, 모든 앱 패널과 그 열린 메뉴)도 목록과 포인터 누르기로 다룬다.
//! 사람만 답하는 창(AUTH_NAMESPACES)은 목록에 들지 않고 누를 수도 없다.
//!
//! 사람이 먼저다(agent_guard.rs). 대상을 받는 입력(PressKeysOn 등)은 같은 디스패치 안에서 키보드 포커스나 그 점의 표면이
//! 그 창인지 본 뒤 넣는다(R1). 사람이 쓰는 중이면(R2) 입력과 포커스 바꾸기를 거절하고, 사람이 멈췄으면(R4) 모두
//! 거절한다. 잠금과 사람만 답하는 창은 그보다 먼저 본다

use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, KeyState},
    desktop::{Window, WindowSurfaceType, layer_map_for_output},
    input::{
        keyboard::{FilterResult, Keycode, xkb},
        pointer::AxisFrame,
    },
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::with_states, shell::xdg::XdgToplevelSurfaceData, text_input::TextInputSeat,
    },
};

use crate::{
    agent_guard::{ACTIVE_FOR, PointerHold, Touches},
    dbus::{AgentRequest, Chord, Command, LayerInfo, Refused, Typed, WindowInfo},
    state::BuntState,
};
use smithay::utils::IsAlive;
// 셸의 레이어 표면(메뉴 막대, Dock 등)과 그 열린 메뉴 누르기
use smithay::{
    reexports::wayland_server::protocol::wl_surface::WlSurface, wayland::shell::wlr_layer::Layer,
};
// 창 스크린샷과 창 안의 점 누르기
use crate::{
    render::OutputRenderElements,
    screenshot::{self, Image},
};
use smithay::{
    backend::renderer::element::AsRenderElements,
    output::Output,
    utils::{Logical, Point, Scale},
};

/// evdev 키 코드 + 8 (xkb 키 코드)의 범위
const KEYCODES: std::ops::RangeInclusive<u32> = 8..=255;
/// 사람만 답하는 셸 창의 레이어 이름: 관리자 인증 창(polkit 인증 에이전트), Wi-Fi 암호 창(NetworkManager
/// 비밀 에이전트), Bluetooth 페어링 창(BlueZ 페어링 에이전트), 클립보드 기록(모든 앱 패널의 클립보드 보기.
/// 에이전트가 읽거나 항목을 골라 앱에 붙이지 못하게 한다), AI 제어 창(메뉴 막대의 AI 표시를 누르면 뜨는 멈추기·다시
/// 하기. AI가 스스로 멈춤을 풀지 못한다). 하나였던 이름을 목록으로 넓혔다
const AUTH_NAMESPACES: [&str; 5] = [
    "bunt-polkit-agent",
    "bunt-secret-agent",
    "bunt-pairing-agent",
    "bunt-clipboard",
    "bunt-agent-control",
];
/// 글 입력에 한 번에 확정하는 바이트 수. Wayland 메시지 하나는 4KB를 넘지 못한다
const COMMIT_BYTES: usize = 2048;
/// 창 스크린샷에서 창의 비치는 부분(둥근 모서리 등) 뒤에 까는 색 (미리 곱한 RGBA)
const BACKDROP: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// 휠 눈금 하나의 거리. 실제 휠의 이벤트와 같다(v120 120이 15 논리 픽셀, input.rs의 on_axis)
const NOTCH: f64 = 15.0;
/// 포인터가 새 표면에 들어간 뒤 휠을 돌리기까지 기다리는 시간. Firefox는 들어옴(enter) 바로 뒤에 온
/// 휠을 버린다. 사람의 포인터도 창에 들어와 머문 뒤에 휠을 돌린다
const ENTER_PAUSE: Duration = Duration::from_millis(100);

impl BuntState {
    pub fn agent_request(&mut self, request: AgentRequest) {
        // 사람이 Esc로 닫은 메뉴의 잡기가 남아 있으면 먼저 푼다. 남은 잡기를 사람의 잡기로 보거나 닫힌 메뉴를 키보드
        // 포커스로 보지 않게 한다 (touch.rs)
        self.end_finished_menu_grab();
        // 화면이 잠긴 동안에는 에이전트가 아무것도 읽거나 하지 못한다(창 목록, 창 그림 포함). 잠금을 푸는 것은
        // 사람이 잠금 화면에 친 암호뿐이다 (session_lock.rs)
        let locked = self.session_lock.is_locked().then_some(Refused::Locked);
        // 인증 창이 떠 있는 동안에는 에이전트가 입력을 넣거나 포커스를 옮기지 못한다. 에이전트가 대신
        // 답하거나, 사람이 칠 암호가 다른 창으로 가는 일을 막는다
        let refused = locked.or_else(|| self.auth_dialog_open().then_some(Refused::AuthDialog));
        // 사람이 AI 제어를 멈춘 동안에는 입력과 포커스 바꾸기를 모두 거절한다(R4). 읽기는 된다
        let stopped = refused.or_else(|| self.agent_guard.paused().then_some(Refused::Paused));
        let now = Instant::now();
        match request {
            AgentRequest::Windows(reply) => {
                let _ = reply.send(match locked {
                    Some(why) => Err(why),
                    None => Ok(self.agent_windows()),
                });
            }
            AgentRequest::TextInput(reply) => {
                let _ = reply.send(match locked {
                    Some(why) => Err(why),
                    None => Ok(self.text_input_active()),
                });
            }
            AgentRequest::Activate { pid, title, reply } => {
                let _ = reply.send(match stopped {
                    Some(why) => Err(why),
                    None => self.agent_activate(pid, &title, now),
                });
            }
            // 대상이 없는 옛 키·글(옛 데몬 호환)도 사람이 치는 중이면 거절한다 (R2a)
            AgentRequest::Keys { chords, reply } => {
                let _ = reply.send(
                    self.gate(stopped, Touches::KEYBOARD, now)
                        .map(|()| self.agent_keys(&chords)),
                );
            }
            AgentRequest::Type { text, reply } => {
                let _ = reply.send(
                    self.gate(stopped, Touches::KEYBOARD, now)
                        .and_then(|()| self.agent_type(&text)),
                );
            }
            AgentRequest::Click {
                x,
                y,
                button,
                reply,
            } => {
                let pos = Point::<f64, Logical>::from((x, y));
                let _ = reply.send(match stopped {
                    Some(why) => Err(why),
                    // 화면 키보드는 사람만 누른다. 에이전트가 누르면 막은 앱에도 글을 칠 수 있다 (osk.rs)
                    None if self.over_keyboard(pos) => Err(Refused::Keyboard),
                    // 사람이 포인터를 쓰는 중이면(R2b), 누르기가 키보드 포커스를 옮기면 키보드를 쓰는 중이어도(R2a) 거절한다
                    None => self
                        .admit(self.press_touches(pos), now)
                        .map(|()| self.agent_click(x, y, button)),
                });
            }
            AgentRequest::Scroll {
                x,
                y,
                dx,
                dy,
                reply,
            } => match self.gate(stopped, Touches::POINTER, now) {
                Ok(()) => self.agent_scroll(WheelAt::Screen((x, y).into()), dx, dy, reply),
                Err(why) => {
                    let _ = reply.send(Err(why));
                }
            },
            AgentRequest::Commit { text, reply } => {
                let _ = reply.send(
                    self.gate(stopped, Touches::KEYBOARD, now)
                        .map(|()| self.commit_text(&text)),
                );
            }
            // 그 창만 그리므로 인증 창은 들지 않는다. 읽기라 인증 창이 떠 있어도 막지 않지만, 잠긴 동안에는 막는다
            AgentRequest::Capture { pid, title, reply } => {
                let _ = reply.send(match locked {
                    Some(why) => Err(why),
                    None => self.agent_capture(pid, &title),
                });
            }
            AgentRequest::ClickWindow {
                pid,
                title,
                x,
                y,
                button,
                reply,
            } => {
                let _ = reply.send(match stopped {
                    Some(why) => Err(why),
                    None => {
                        self.agent_click_window(pid, &title, WindowPoint::Pixels(x, y), button, now)
                    }
                });
            }
            AgentRequest::Layers(reply) => {
                let _ = reply.send(match locked {
                    Some(why) => Err(why),
                    None => Ok(listed(self.placed_layers())),
                });
            }
            AgentRequest::ClickLayer {
                pid,
                namespace,
                x,
                y,
                button,
                reply,
            } => {
                let _ = reply.send(match stopped {
                    Some(why) => Err(why),
                    None => self.agent_click_layer(pid, &namespace, (x, y), button, now),
                });
            }
            AgentRequest::ScrollLayer {
                pid,
                namespace,
                x,
                y,
                dx,
                dy,
                reply,
            } => {
                let checked = match stopped {
                    Some(why) => Err(why),
                    None => self
                        .layer_point(pid, &namespace, (x, y))
                        .and_then(|_| self.admit(Touches::POINTER, now)),
                };
                match checked {
                    Ok(()) => self.agent_scroll(
                        WheelAt::Layer {
                            pid,
                            namespace,
                            local: (x, y),
                        },
                        dx,
                        dy,
                        reply,
                    ),
                    Err(why) => {
                        let _ = reply.send(Err(why));
                    }
                }
            }
            // 창 그림(Capture)과 같다: 읽기라 인증 창이 떠 있어도 막지 않지만, 잠긴 동안에는 막는다
            AgentRequest::CaptureLayer {
                pid,
                namespace,
                reply,
            } => {
                let _ = reply.send(match locked {
                    Some(why) => Err(why),
                    None => self.agent_capture_layer(pid, &namespace),
                });
            }
            // 대상을 받는 키·글: 키보드 포커스가 그 창일 때만, 같은 디스패치 안에서 넣는다 (R1)
            AgentRequest::KeysOn {
                pid,
                title,
                chords,
                reply,
            } => {
                let _ = reply.send(
                    stopped
                        .map_or(Ok(()), Err)
                        .and_then(|()| self.keyboard_target(pid, &title, now))
                        .map(|()| self.agent_keys(&chords)),
                );
            }
            AgentRequest::TypeOn {
                pid,
                title,
                text,
                reply,
            } => {
                let _ = reply.send(
                    stopped
                        .map_or(Ok(()), Err)
                        .and_then(|()| self.keyboard_target(pid, &title, now))
                        .and_then(|()| self.agent_type(&text)),
                );
            }
            AgentRequest::CommitOn {
                pid,
                title,
                text,
                reply,
            } => {
                let _ = reply.send(
                    stopped
                        .map_or(Ok(()), Err)
                        .and_then(|()| self.keyboard_target(pid, &title, now))
                        .map(|()| self.commit_text(&text)),
                );
            }
            AgentRequest::ClickOn {
                pid,
                title,
                x,
                y,
                button,
                reply,
            } => {
                let _ = reply.send(match stopped {
                    Some(why) => Err(why),
                    None => self.agent_click_window(
                        pid,
                        &title,
                        WindowPoint::Logical(x, y),
                        button,
                        now,
                    ),
                });
            }
            AgentRequest::ScrollOn {
                pid,
                title,
                x,
                y,
                dx,
                dy,
                reply,
            } => {
                let local = Point::from((x, y));
                let checked = match stopped {
                    Some(why) => Err(why),
                    None => self.scroll_target(pid, &title, local, now),
                };
                match checked {
                    Ok(window) => {
                        self.agent_scroll(WheelAt::Window { window, local }, dx, dy, reply)
                    }
                    Err(why) => {
                        let _ = reply.send(Err(why));
                    }
                }
            }
            // 활동 알림은 입력이 아니라 멈춘 동안에도 받는다: 사람이 무엇을 읽는지 메뉴 막대에서 본다. 잠긴 동안에는
            // 창을 찾지 않는다
            AgentRequest::Activity {
                client,
                pid,
                title,
                reply,
            } => {
                let _ = reply.send(match locked {
                    Some(why) => Err(why),
                    None => {
                        self.agent_report(&client, pid, &title, now);
                        Ok(())
                    }
                });
            }
            // 대상 강조는 사람만 답하는 창이 떠 있거나 멈춘 동안에는 그리지 않는다: AI가 무엇을 하려는 것처럼 보이지 않게 한다
            AgentRequest::ShowTarget {
                pid,
                title,
                x,
                y,
                width,
                height,
                reply,
            } => {
                let _ = reply.send(match stopped {
                    Some(why) => Err(why),
                    None => self.agent_show_target(
                        pid,
                        &title,
                        Rectangle::new((x, y).into(), (width, height).into()),
                    ),
                });
            }
        }
    }

    /// 멈춤(R4)이나 앞서 본 거절이 없으면 사람이 쓰는 중인지(R2) 본다
    fn gate(
        &self,
        stopped: Option<Refused>,
        touches: Touches,
        now: Instant,
    ) -> Result<(), Refused> {
        match stopped {
            Some(why) => Err(why),
            None => self.admit(touches, now),
        }
    }

    /// R4와 R2 (agent_guard.rs): 멈췄으면 Paused, 사람이 그 장치를 쓰는 중이면 UserActive
    fn admit(&self, touches: Touches, now: Instant) -> Result<(), Refused> {
        self.agent_guard.admit(touches, self.pointer_hold(), now)
    }

    /// 사람의 손가락, 포인터 잡기, 키보드를 받는 사람의 메뉴
    fn pointer_hold(&self) -> PointerHold {
        let grab = self.human_grab();
        PointerHold {
            fingers: self.touch.fingers_down(),
            grab,
            // 키보드 포커스가 메뉴(팝업)에 있고 그 잡기가 사람의 것이다
            menu: grab
                && self
                    .keyboard_focus()
                    .is_some_and(|focus| self.popups.find_popup(&focus).is_some()),
        }
    }

    /// 사람이 시작한 포인터 잡기(옮기기, 크기 바꾸기, 메뉴, 끌어 놓기)나 스크린샷 영역 고르기가 있는지. 에이전트의
    /// 누르기·키로 연 메뉴의 잡기는 그 serial로 가린다: 에이전트는 제가 연 메뉴의 항목을 누를 수 있다
    fn human_grab(&self) -> bool {
        self.screenshot_area.is_some()
            || self.seat.get_pointer().is_some_and(|pointer| {
                pointer.is_grabbed()
                    && !self
                        .agent_guard
                        .agent_serials()
                        .any(|serial| pointer.has_grab(serial))
            })
    }

    /// 에이전트가 창을 앞으로 가져와 포커스를 준다. 그 serial(키보드 들어옴)도 적어 둔다: 앱이 그 serial로 메뉴의 잡기를
    /// 시작하면 사람의 잡기로 보지 않는다
    fn agent_focus_window(&mut self, window: &Window) {
        let serial = SERIAL_COUNTER.next_serial();
        self.agent_guard.note_agent_serial(serial);
        self.focus_window(window, serial);
    }

    /// 그 점을 누를 때 사람의 무엇과 부딪치는지: 키보드 포커스가 옮겨 가면 키보드도
    fn press_touches(&self, pos: Point<f64, Logical>) -> Touches {
        if self.press_moves_focus(pos) {
            Touches::FOCUS
        } else {
            Touches::POINTER
        }
    }

    /// 그 점을 누르면 키보드 포커스가 다른 표면으로 가는지. 누르기가 포커스를 정하는 차례(input.rs의 focus_at)를
    /// 그대로 따라 본다
    fn press_moves_focus(&self, pos: Point<f64, Logical>) -> bool {
        // 잡기가 있으면 누르기는 잡기로 가고 포커스를 정하지 않는다 (press_button)
        if self
            .seat
            .get_pointer()
            .is_some_and(|pointer| pointer.is_grabbed())
        {
            return false;
        }
        let current = self
            .keyboard_focus()
            .map(|surface| self.root_surface(&surface));
        if let Some(layer) = self.keyboard_layer_under(pos) {
            return current.as_ref() != Some(&layer);
        }
        if self.is_over_top_layer(pos) {
            return false;
        }
        match self.space.element_under(pos) {
            // 키보드를 독점하는 레이어(모든 앱 패널 등)가 떠 있으면 창을 눌러도 키보드는 그대로다 (focus_window)
            Some((window, _)) => self.exclusive_layer().is_none() && !self.window_has_focus(window),
            None => current != self.desktop_layer_under(pos),
        }
    }

    /// 키보드 포커스(의 뿌리 표면)가 이 창에 있는지. 그 창의 열린 메뉴에 있어도 그 창이다
    fn holds_keyboard(&self, window: &Window) -> bool {
        let Some(toplevel) = window.toplevel() else {
            return false;
        };
        self.keyboard_focus()
            .is_some_and(|focus| self.root_surface(&focus) == *toplevel.wl_surface())
    }

    /// 키보드 포커스가 이 창에 있고 창이 화면에 놓여 있는지. 아니면 활성화가 포커스를 바꾼다
    fn window_has_focus(&self, window: &Window) -> bool {
        self.space.element_geometry(window).is_some() && self.holds_keyboard(window)
    }

    /// R1과 R2a: 키·글을 넣을 그 창이 키보드 포커스를 가졌고(그 창의 열린 메뉴도 그 창이다) 사람이 치는 중이 아닌지
    fn keyboard_target(&self, pid: u32, title: &str, now: Instant) -> Result<(), Refused> {
        let window = self.agent_window(pid, title).ok_or(Refused::NoWindow)?;
        let toplevel = window.toplevel().ok_or(Refused::NoWindow)?;
        let focus = self
            .keyboard_focus()
            .map(|surface| self.root_surface(&surface));
        self.agent_guard.keyboard_target(
            focus.as_ref(),
            toplevel.wl_surface(),
            self.pointer_hold(),
            now,
        )
    }

    /// 창 기준 점(논리 좌표)에 포인터가 닿는지. 사람의 포인터와 같은 길(surface_under)로 그 점의 표면을 찾는다: 그 창의
    /// 표면이나 하위 표면, 열린 메뉴면 닿는다
    fn reach(&self, window: &Window, local: Point<f64, Logical>) -> Reach {
        let Some(toplevel) = window.toplevel().filter(|_| window.alive()) else {
            return Reach::Never(Refused::NoWindow);
        };
        // 최소화하거나 가린 창
        let Some(geometry) = self.space.element_geometry(window) else {
            return Reach::Behind;
        };
        let pos = geometry.loc.to_f64() + local;
        let under = self
            .surface_under(pos)
            .map(|(surface, _)| self.root_surface(&surface));
        if under.as_ref() == Some(toplevel.wl_surface()) {
            return Reach::At(pos);
        }
        if outside(local, geometry.size.to_f64()) {
            return Reach::Never(Refused::OffWindow {
                width: geometry.size.w,
                height: geometry.size.h,
            });
        }
        // 메뉴 막대, Dock, 알림, 화면 키보드처럼 창 위에 늘 있는 표면은 창을 앞으로 가져와도 덮는다
        if self.is_over_top_layer(pos) {
            return Reach::Never(Refused::Covered);
        }
        Reach::Behind
    }

    /// ScrollOn의 창: 그 점이 보이면 그대로 돌린다. 창이 내려졌거나 다른 창에 가렸으면 앞으로 가져오는데, 그러면
    /// 키보드 포커스가 바뀌므로 사람이 키보드를 쓰는 중이어도 거절한다
    fn scroll_target(
        &mut self,
        pid: u32,
        title: &str,
        local: Point<f64, Logical>,
        now: Instant,
    ) -> Result<Window, Refused> {
        let window = self.agent_window(pid, title).ok_or(Refused::NoWindow)?;
        match self.reach(&window, local) {
            Reach::At(_) => self.admit(Touches::POINTER, now)?,
            Reach::Never(why) => return Err(why),
            Reach::Behind => {
                self.admit(Touches::FOCUS, now)?;
                self.agent_focus_window(&window);
                if !matches!(self.reach(&window, local), Reach::At(_)) {
                    return Err(Refused::Covered);
                }
            }
        }
        Ok(window)
    }

    /// 휠을 돌릴 전역 논리 좌표. 처음과 100ms 뒤에 다시 돌릴 때 같은 확인을 한다
    fn wheel_point(&self, at: &WheelAt) -> Result<Point<f64, Logical>, Refused> {
        match at {
            WheelAt::Screen(pos) => Ok(*pos),
            WheelAt::Window { window, local } => match self.reach(window, *local) {
                Reach::At(pos) => Ok(pos),
                Reach::Behind => Err(Refused::Covered),
                Reach::Never(why) => Err(why),
            },
            WheelAt::Layer {
                pid,
                namespace,
                local,
            } => self.layer_point(*pid, namespace, *local),
        }
    }

    /// 포인터가 새 표면에 들어가 기다린 뒤 휠을 돌리기 직전에 다시 본다: 그사이 잠겼거나, 사람만 답하는 창이 떴거나,
    /// 사람이 멈췄거나 포인터를 썼거나(R4, R2b), 대상이 가렸으면(R1) 돌리지 않는다
    fn wheel_again(&self, at: &WheelAt) -> Result<Point<f64, Logical>, Refused> {
        if self.session_lock.is_locked() {
            return Err(Refused::Locked);
        }
        if self.auth_dialog_open() {
            return Err(Refused::AuthDialog);
        }
        self.admit(Touches::POINTER, Instant::now())?;
        self.wheel_point(at)
    }

    /// 사람이 AI 제어를 멈추거나(true) 다시 한다(false): 셸의 메뉴 막대(SetAgentPaused)와 단축키(agent.pause). 에이전트는
    /// 이 길에 닿지 못한다
    pub fn set_agent_paused(&mut self, paused: bool) {
        if self.agent_guard.paused() == paused {
            return;
        }
        self.agent_guard.set_paused(paused);
        tracing::info!(
            "AI control {} by the person",
            if paused { "paused" } else { "resumed" }
        );
        // 멈추면 보이던 대상 강조도 거둔다
        if paused {
            self.clear_agent_target();
        }
        self.publish_agent_state();
    }

    /// 단축키(agent.pause, 기본 ⌃⌥⌘.)로 멈춤을 바꾼다
    pub fn toggle_agent_paused(&mut self) {
        let paused = !self.agent_guard.paused();
        self.set_agent_paused(paused);
    }

    /// AI 제어 상태가 바뀌었으면 D-Bus 사본에 적고 셸과 에이전트에 알린다 (AgentStateChanged)
    fn publish_agent_state(&mut self) {
        if let Some(status) = self.agent_guard.changed(Instant::now()) {
            self.dbus.update(|snapshot| snapshot.agent = status.clone());
            self.dbus.send(Command::AgentStateChanged(status));
        }
    }

    /// 데몬이 알린 활동(ReportAgentActivity). 대상 창을 찾으면 그 앱 ID와 지금 제목을 채운다. 마지막 알림 뒤
    /// ACTIVE_FOR가 지나면 활동이 끝났다고 알린다
    fn agent_report(&mut self, client: &str, pid: u32, title: &str, now: Instant) {
        let window = (pid != 0).then(|| self.agent_window(pid, title)).flatten();
        let (app_id, title) = match window {
            Some(window) => (
                window_app_id(&window).unwrap_or_default(),
                window_title(&window).unwrap_or_default(),
            ),
            None => (String::new(), title.to_string()),
        };
        self.agent_guard.report(client, pid, app_id, title, now);
        self.publish_agent_state();
        if let Some(token) = self.agent_guard.activity_timer.take() {
            self.loop_handle.remove(token);
        }
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(ACTIVE_FOR), |_, _, state| {
                state.activity_timer_fired()
            });
        match timer {
            Ok(token) => self.agent_guard.activity_timer = Some(token),
            Err(err) => tracing::warn!("failed to time the agent's activity: {}", err.error),
        }
    }

    /// 활동 타이머: 마지막 알림 뒤 ACTIVE_FOR가 지났으면 활동이 끝났다고 알린다. 그사이 새 알림이 왔으면 남은 만큼 더
    /// 기다린다
    fn activity_timer_fired(&mut self) -> TimeoutAction {
        match self.agent_guard.active_left(Instant::now()) {
            Some(left) => TimeoutAction::ToDuration(left),
            None => {
                self.agent_guard.activity_timer = None;
                self.publish_agent_state();
                TimeoutAction::Drop
            }
        }
    }

    fn auth_dialog_open(&self) -> bool {
        self.space.outputs().any(|output| {
            layer_map_for_output(output)
                .layers()
                .any(|layer| human_only(layer.namespace()))
        })
    }

    // 레이어 표면의 pid도 같은 길로 알아내도록 표면 하나를 받는 surface_pid로 나눴다
    fn client_pid(&self, window: &Window) -> u32 {
        window
            .toplevel()
            .map_or(0, |toplevel| self.surface_pid(toplevel.wl_surface()))
    }

    /// 출력마다 아래층부터 쌓인 차례의 레이어 표면과 그것을 지금 그리는 자리(전역 논리 좌표). 미끄러지는
    /// 막대는 옮긴 자리이고, 다 숨은 막대는 원래 자리다(그 점은 surface_under가 막대로 찾지 않는다)
    fn placed_layers(&self) -> Vec<PlacedLayer> {
        let mut placed = Vec::new();
        for output in self.space.outputs() {
            let Some(output_geo) = self.space.output_geometry(output) else {
                continue;
            };
            // bar_places도 레이어 맵을 잠그므로 맵을 잠그기 전에 읽는다
            let bars = self.bar_places(output);
            let map = layer_map_for_output(output);
            for kind in [Layer::Background, Layer::Bottom, Layer::Top, Layer::Overlay] {
                for layer in map.layers_on(kind) {
                    let Some(mut area) = map.layer_geometry(layer) else {
                        continue;
                    };
                    if let Some(at) = bars
                        .iter()
                        .find(|bar| bar.layer == *layer)
                        .and_then(|bar| bar.at)
                    {
                        area.loc = at;
                    }
                    area.loc += output_geo.loc;
                    placed.push(PlacedLayer {
                        pid: self.surface_pid(layer.wl_surface()),
                        namespace: layer.namespace().to_string(),
                        surface: layer.wl_surface().clone(),
                        area,
                        scale: output.current_scale().fractional_scale(),
                    });
                }
            }
        }
        placed
    }

    /// 그 pid의 레이어 표면(이름이 같은 것의 맨 위) 안의 한 점을 포인터로 누른다. (x, y)는 표면 기준 논리
    /// 좌표(셸 창의 접근성 좌표)다. GTK는 열린 메뉴(팝오버)의 요소도 그 창 기준으로 알리므로, 표면 밖의 점도
    /// 그 표면의 팝업 위면 누른다. 그 점에서 포인터가 실제로 이 표면이나 그 팝업에 가지 않으면(자동으로 숨은
    /// 막대, 다른 표면에 가림) 누르지 않는다: 숨은 막대 자리를 누르면 그 아래 창이 눌린다
    fn agent_click_layer(
        &mut self,
        pid: u32,
        namespace: &str,
        (x, y): (f64, f64),
        button: u32,
        now: Instant,
    ) -> Result<(f64, f64), Refused> {
        let pos = self.layer_point(pid, namespace, (x, y))?;
        // 사람이 포인터를 쓰는 중이면(R2b), 키보드를 받는 셸 표면으로 포커스를 옮기는 누르기는 키보드를 쓰는 중이어도
        self.admit(self.press_touches(pos), now)?;
        self.agent_click(pos.x, pos.y, button);
        Ok((pos.x, pos.y))
    }

    /// 그 pid의 레이어 표면 안의 점(표면 기준 논리 좌표)의 전역 논리 좌표. 누르기(ClickLayer)와 휠(ScrollLayer)이
    /// 같이 쓴다. 그 점에서 포인터가 이 표면이나 그 팝업에 가지 않으면 NotShown이다
    fn layer_point(
        &self,
        pid: u32,
        namespace: &str,
        (x, y): (f64, f64),
    ) -> Result<Point<f64, Logical>, Refused> {
        let placed = self.placed_layers();
        let layer = choose_layer(&placed, pid, namespace)?;
        let pos = layer.area.loc.to_f64() + Point::<f64, Logical>::from((x, y));
        // 사람의 포인터와 같은 길(surface_under)로 그 점의 표면을 찾는다. 팝업과 하위 표면은 뿌리로 올라간다
        let reaches = self
            .surface_under(pos)
            .is_some_and(|(under, _)| self.root_surface(&under) == layer.surface);
        if !reaches {
            return Err(Refused::NotShown);
        }
        Ok(pos)
    }

    /// 그 pid의 레이어 표면(이름이 같은 것의 맨 위)만 따로 그려 읽는다 (capture_window와 같다). 표면의 왼쪽
    /// 위가 그림의 (0, 0)이고, 표면 밖으로 나온 팝업(열린 메뉴)은 잘린다. 사람만 답하는 창과 화면 키보드는
    /// choose_layer가 거절한다
    fn agent_capture_layer(&mut self, pid: u32, namespace: &str) -> Result<(Image, f64), Refused> {
        let placed = self.placed_layers();
        let layer = choose_layer(&placed, pid, namespace)?;
        let size = layer.area.size;
        let surface = layer.surface.clone();
        let (layer, output) = self
            .space
            .outputs()
            .find_map(|output| {
                let map = layer_map_for_output(output);
                let layer = map.layer_for_surface(&surface, WindowSurfaceType::TOPLEVEL)?;
                Some((layer.clone(), output.clone()))
            })
            .ok_or(Refused::NoWindow)?;
        let scale = output.current_scale().fractional_scale();
        let renderer = self.backend.renderer();
        let elements: Vec<OutputRenderElements> =
            layer.render_elements(renderer, Point::from((0, 0)), Scale::from(scale), 1.0);
        let scene: Vec<&OutputRenderElements> = elements.iter().collect();
        let area = Rectangle::from_size(size);
        let image = screenshot::capture_scene(renderer, &scene, &output, Some(area), BACKDROP)
            .map_err(|err| {
                tracing::warn!("agent: cannot draw the {namespace:?} surface of pid {pid}: {err}");
                Refused::CaptureFailed
            })?;
        Ok((image, scale))
    }

    fn agent_windows(&self) -> Vec<WindowInfo> {
        let focus = self
            .seat
            .get_keyboard()
            .and_then(|keyboard| keyboard.current_focus())
            .map(|surface| self.root_surface(&surface));
        // 최소화하거나 가린 창은 돌아올 자리로 알린다. 활성화하면 그 자리에 다시 놓인다
        let shown = self
            .space
            .elements()
            .map(|window| (window, self.space.element_geometry(window)));
        let hidden = self.hidden.iter().map(|hidden| {
            let size = hidden.window.geometry().size;
            (&hidden.window, Some(Rectangle::new(hidden.location, size)))
        });
        shown
            .chain(hidden)
            .filter_map(|(window, geometry)| {
                let toplevel = window.toplevel()?;
                let geometry = geometry?;
                let (title, app_id) = with_states(toplevel.wl_surface(), |states| {
                    states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()
                        .map(|data| {
                            let data = data.lock().unwrap();
                            (
                                data.title.clone().unwrap_or_default(),
                                data.app_id.clone().unwrap_or_default(),
                            )
                        })
                        .unwrap_or_default()
                });
                Some(WindowInfo {
                    pid: self.client_pid(window),
                    app_id,
                    title,
                    x: geometry.loc.x,
                    y: geometry.loc.y,
                    width: geometry.size.w,
                    height: geometry.size.h,
                    focused: focus.as_ref() == Some(toplevel.wl_surface()),
                })
            })
            .collect()
    }

    /// 그 pid의 창 가운데 제목이 같은 창, 없으면 맨 위 창을 앞으로 가져온다. 최소화하거나 가린 창도
    /// 고를 수 있고, 고르면 제자리에 다시 놓인다. 창이 없으면 false. 키보드 포커스를 바꾸므로 사람이 키보드나 포인터를
    /// 쓰는 중이면 거절한다(R2). 이미 포커스를 가진 창이면 아무것도 바꾸지 않으므로 거절하지 않는다
    fn agent_activate(&mut self, pid: u32, title: &str, now: Instant) -> Result<bool, Refused> {
        let Some(window) = self.agent_window(pid, title) else {
            return Ok(false);
        };
        if self.window_has_focus(&window) {
            return Ok(true);
        }
        self.admit(Touches::FOCUS, now)?;
        self.agent_focus_window(&window);
        Ok(true)
    }

    /// 수정 키를 누르고, 키를 눌렀다 떼고, 수정 키를 거꾸로 뗀다. 키의 serial을 적어 두어, 에이전트의 키로 연 메뉴의
    /// 잡기를 사람의 것으로 보지 않는다 (agent_guard.rs)
    fn agent_keys(&mut self, chords: &[Chord]) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        for chord in chords {
            let shifted = chord.modifiers.contains(&xkb::Keysym::Shift_L);
            let key = match self.keycode_for(chord.key) {
                Some(code) => Some(code),
                None if shifted => self.key_for(chord.key, 1),
                None => None,
            };
            let mut codes = Vec::new();
            for keysym in &chord.modifiers {
                codes.push(self.keycode_for(*keysym));
            }
            codes.push(key);
            let codes: Option<Vec<Keycode>> = codes.into_iter().collect();
            let Some(codes) = codes else {
                tracing::warn!("agent: no key in the keymap for {chord:?}");
                continue;
            };
            let send = |state: &mut BuntState, code: Keycode, key_state: KeyState| {
                let time = state.start_time.elapsed().as_millis() as u32;
                let serial = SERIAL_COUNTER.next_serial();
                state.agent_guard.note_agent_serial(serial);
                keyboard.input::<(), _>(state, code, key_state, serial, time, |_, _, _| {
                    FilterResult::Forward
                });
            };
            for code in &codes {
                send(self, *code, KeyState::Pressed);
            }
            for code in codes.iter().rev() {
                send(self, *code, KeyState::Released);
            }
        }
    }

    /// 지금 배열에서 그 기호를 내는 키 코드 (수정 없이, 첫 단계에서)
    pub(crate) fn keycode_for(&mut self, keysym: xkb::Keysym) -> Option<Keycode> {
        self.key_for(keysym, 0)
    }

    /// 그 단계(0: 수정 없이, 1: Shift)에서 기호를 내는 키 코드
    fn key_for(&mut self, keysym: xkb::Keysym, level: u32) -> Option<Keycode> {
        let keyboard = self.seat.get_keyboard()?;
        keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().unwrap();
            let layout = xkb.active_layout();
            // 안전: 키맵의 참조를 이 닫힘 밖으로 내보내지 않는다
            let keymap = unsafe { xkb.keymap() };
            KEYCODES.map(Keycode::new).find(|code| {
                keymap
                    .key_get_syms_by_level(*code, layout.0, level)
                    .contains(&keysym)
            })
        })
    }

    /// 글을 친다. 포커스를 가진 앱의 글 입력이 켜져 있으면 거기에 확정하고, 아니면 글자마다 키를 찾아
    /// 친다 (대문자와 기호는 Shift와 함께). 키로 칠 때 하나라도 없는 글자가 있으면 치지 않는다
    fn agent_type(&mut self, text: &str) -> Result<Typed, Refused> {
        if self.commit_text(text) {
            return Ok(Typed::TextInput);
        }
        let mut chords = Vec::new();
        for character in text.chars() {
            let keysym = xkb::utf32_to_keysym(u32::from(character));
            let chord = if self.key_for(keysym, 0).is_some() {
                Chord {
                    modifiers: Vec::new(),
                    key: keysym,
                }
            } else if self.key_for(keysym, 1).is_some() {
                Chord {
                    modifiers: vec![xkb::Keysym::Shift_L],
                    key: keysym,
                }
            } else {
                return Err(Refused::NotOnLayout(character));
            };
            chords.push(chord);
        }
        self.agent_keys(&chords);
        Ok(Typed::Keys)
    }

    pub(crate) fn text_input_active(&self) -> bool {
        let mut active = false;
        self.seat
            .text_input()
            .with_active_text_input(|_, _| active = true);
        active
    }

    /// 포커스를 가진 앱의 켜진 글 입력(text-input-v3)에 입력기처럼 글을 확정한다. 한 번의 done에는
    /// 마지막 commit_string만 들으므로 조각마다 done을 보낸다. 켜진 글 입력이 없으면 false
    fn commit_text(&mut self, text: &str) -> bool {
        if !self.text_input_active() {
            return false;
        }
        let text_input = self.seat.text_input();
        for chunk in chunks(text, COMMIT_BYTES) {
            text_input.with_active_text_input(|text_input, _| {
                text_input.commit_string(Some(chunk.to_string()));
            });
            text_input.done(false);
        }
        true
    }

    fn agent_click(&mut self, x: f64, y: f64, button: u32) {
        let time = self.start_time.elapsed().as_millis() as u32;
        self.pointer_moved((x, y).into(), time);
        for (state, at) in [
            (ButtonState::Pressed, time),
            (ButtonState::Released, time.saturating_add(1)),
        ] {
            // 에이전트의 누르기로 연 메뉴의 잡기는 사람의 것이 아니다 (agent_guard.rs)
            if let Some(serial) = self.press_button(button, state, at) {
                self.agent_guard.note_agent_serial(serial);
            }
        }
    }

    /// 포인터를 그 자리로 옮겨 휠을 돌리고 답한다. dx, dy는 눈금 수이고 오른쪽·아래가 +다. 시스템 설정의
    /// 스크롤 속도와 자연스러운 스크롤은 사람의 장치에만 쓴다. 포인터가 다른 표면에 들어가면
    /// ENTER_PAUSE 뒤에 그 자리에서 한 번 더 움직이고 돌린다. 그때 잠금, 사람만 답하는 창, 멈춤, 사람의 포인터,
    /// 대상(창이나 레이어 표면이 아직 그 점에 있는지)을 다시 본다
    fn agent_scroll(
        &mut self,
        at: WheelAt,
        dx: f64,
        dy: f64,
        reply: mpsc::Sender<Result<(), Refused>>,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            let _ = reply.send(Ok(()));
            return;
        };
        let pos = match self.wheel_point(&at) {
            Ok(pos) => pos,
            Err(why) => {
                let _ = reply.send(Err(why));
                return;
            }
        };
        let before = pointer.current_focus();
        let time = self.start_time.elapsed().as_millis() as u32;
        self.pointer_moved(pos, time);
        if pointer.current_focus() == before {
            self.agent_wheel(dx, dy);
            let _ = reply.send(Ok(()));
            return;
        }
        let timer = self.loop_handle.insert_source(
            Timer::from_duration(ENTER_PAUSE),
            move |_, _, state| {
                let _ = reply.send(state.wheel_again(&at).map(|pos| {
                    // 창이 움직였으면 그 창의 지금 자리다
                    let time = state.start_time.elapsed().as_millis() as u32;
                    state.pointer_moved(pos, time);
                    state.agent_wheel(dx, dy);
                }));
                TimeoutAction::Drop
            },
        );
        if let Err(err) = timer {
            tracing::warn!("failed to schedule the agent's wheel: {}", err.error);
        }
    }

    /// 포인터 아래의 표면에 휠 눈금을 보낸다
    fn agent_wheel(&mut self, dx: f64, dy: f64) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let time = self.start_time.elapsed().as_millis() as u32;
        let mut frame = AxisFrame::new(time).source(AxisSource::Wheel);
        for (axis, notches) in [(Axis::Horizontal, dx), (Axis::Vertical, dy)] {
            if notches != 0.0 {
                frame = frame
                    .value(axis, notches * NOTCH)
                    .v120(axis, (notches * 120.0).round() as i32);
            }
        }
        pointer.axis(self, frame);
        pointer.frame(self);
    }

    /// 그 pid의 창 가운데 제목이 같은 창, 없으면 맨 위 창 (agent_activate와 같은 차례). 최소화하거나
    /// 가린 창도 고른다
    pub(crate) fn agent_window(&self, pid: u32, title: &str) -> Option<Window> {
        let windows: Vec<&Window> = self
            .space
            .elements()
            .rev()
            .chain(self.hidden.iter().map(|hidden| &hidden.window))
            .filter(|window| self.client_pid(window) == pid)
            .collect();
        windows
            .iter()
            .find(|window| window_title(window).as_deref() == Some(title))
            .or(windows.first())
            .map(|window| (*window).clone())
    }

    /// 창을 그리는 출력: 창 가운데가 있는 출력, 없으면 창이 걸친 출력, 공간에서 내린 창이면 첫 출력.
    /// 창 스크린샷과 그 픽셀 좌표의 배율이 이 출력의 것이다
    fn window_output(&self, window: &Window) -> Option<Output> {
        let center = self.space.element_geometry(window).map(|geometry| {
            Point::<f64, Logical>::from((
                f64::from(geometry.loc.x) + f64::from(geometry.size.w) / 2.0,
                f64::from(geometry.loc.y) + f64::from(geometry.size.h) / 2.0,
            ))
        });
        center
            .and_then(|center| self.space.output_under(center).next().cloned())
            .or_else(|| self.space.outputs_for_element(window).into_iter().next())
            .or_else(|| self.space.outputs().next().cloned())
    }

    fn agent_capture(&mut self, pid: u32, title: &str) -> Result<(Image, f64), Refused> {
        let window = self.agent_window(pid, title).ok_or(Refused::NoWindow)?;
        self.capture_window(&window, BACKDROP).map_err(|err| {
            tracing::warn!("agent: cannot draw the window of pid {pid}: {err}");
            Refused::CaptureFailed
        })
    }

    /// 창 하나만 따로 그려 읽는다 (screenshot.rs의 capture_scene). 다른 창, 셸, 커서는 넣지 않으므로 가려진
    /// 부분도 보이고 막은 앱이나 인증 창이 섞이지 않는다. 창 영역(xdg 창 영역)의 왼쪽 위가 그림의 (0, 0)이고
    /// 그 밖으로 나온 그림자와 팝업은 잘린다. 비치는 부분은 `backdrop` 위에 그린다(에이전트는 흰 바탕,
    /// Dock의 최소화 칸은 투명)
    pub fn capture_window(
        &mut self,
        window: &Window,
        backdrop: [f32; 4],
    ) -> Result<(Image, f64), String> {
        let output = self.window_output(window).ok_or("there is no output")?;
        let scale = output.current_scale().fractional_scale();
        let geometry = window.geometry();
        // 표면의 원점은 창 영역보다 그림자만큼 왼쪽 위에 있다
        let origin = Point::<i32, Logical>::from((-geometry.loc.x, -geometry.loc.y))
            .to_physical_precise_round(scale);
        let renderer = self.backend.renderer();
        let elements: Vec<OutputRenderElements> =
            window.render_elements(renderer, origin, Scale::from(scale), 1.0);
        let scene: Vec<&OutputRenderElements> = elements.iter().collect();
        let area = Rectangle::from_size(geometry.size);
        let image = screenshot::capture_scene(renderer, &scene, &output, Some(area), backdrop)?;
        Ok((image, scale))
    }

    /// 창 안의 한 점을 포인터로 누른다(ClickWindow, ClickOn). 점은 창 스크린샷(capture_window)의 픽셀이거나 창 기준
    /// 논리 좌표다. 창이 앞에 없으면 먼저 가져온다. 키보드 포커스가 이미 이 창에 있으면 열린 메뉴가 닫히지 않게 그대로
    /// 둔다. 그 점이 창 밖이거나 그 위에 다른 것(이 창의 대화 상자, 다른 창, Dock·메뉴 막대·알림)이 있으면 누르지
    /// 않는다: 스크린샷에 보인 것과 다른 것을 누르게 된다. 창 기준 논리 좌표는 그 창의 열린 메뉴면 창 밖이어도 누른다.
    /// 사람이 포인터를 쓰는 중이면 거절하고, 창을 앞으로 가져와야 하면 키보드를 쓰는 중이어도 거절한다 (R2)
    fn agent_click_window(
        &mut self,
        pid: u32,
        title: &str,
        point: WindowPoint,
        button: u32,
        now: Instant,
    ) -> Result<(f64, f64), Refused> {
        let window = self.agent_window(pid, title).ok_or(Refused::NoWindow)?;
        let local = match point {
            WindowPoint::Pixels(x, y) => {
                let scale = self
                    .window_output(&window)
                    .map_or(1.0, |output| output.current_scale().fractional_scale());
                let size = window.geometry().size.to_f64();
                let local = Point::<f64, Logical>::from((x / scale, y / scale));
                if outside(local, size) {
                    let pixels = size.to_physical(scale);
                    return Err(Refused::Outside {
                        width: pixels.w.round() as u32,
                        height: pixels.h.round() as u32,
                    });
                }
                local
            }
            WindowPoint::Logical(x, y) => Point::from((x, y)),
        };
        // 창을 앞으로 가져와도 닿지 않으면(창 밖, 늘 위에 있는 막대) 포커스를 옮기지 않고 거절한다
        if let Reach::Never(why) = self.reach(&window, local) {
            return Err(why);
        }
        let focused = self.window_has_focus(&window);
        self.admit(
            if focused {
                Touches::POINTER
            } else {
                Touches::FOCUS
            },
            now,
        )?;
        if !focused {
            self.agent_focus_window(&window);
        }
        let pos = match self.reach(&window, local) {
            Reach::At(pos) => pos,
            Reach::Behind => return Err(Refused::Covered),
            Reach::Never(why) => return Err(why),
        };
        self.agent_click(pos.x, pos.y, button);
        Ok((pos.x, pos.y))
    }
}

/// 창 안의 점 (ClickWindow, ClickOn)
#[derive(Clone, Copy, Debug)]
enum WindowPoint {
    /// 창 스크린샷(CaptureWindow)의 픽셀
    Pixels(f64, f64),
    /// 창 기준 논리 좌표 (창 영역의 왼쪽 위가 (0, 0). AT-SPI의 창 좌표와 같은 틀)
    Logical(f64, f64),
}

/// 창 기준 점에 포인터가 닿는지 (BuntState::reach)
#[derive(Debug)]
enum Reach {
    /// 닿는다: 그 전역 논리 좌표
    At(Point<f64, Logical>),
    /// 창이 내려져 있거나 다른 창(그 창의 대화 상자 포함)이 덮었다. 창을 앞으로 가져오면 닿을 수 있다
    Behind,
    /// 창을 앞으로 가져와도 닿지 않는다: 창이 없거나, 창 밖이거나, 늘 위에 있는 셸 표면이 덮었다
    Never(Refused),
}

/// 휠을 돌릴 곳. 포인터가 새 표면에 들어가 기다린 뒤에 다시 찾는다
enum WheelAt {
    /// 옛 Scroll: 전역 논리 좌표
    Screen(Point<f64, Logical>),
    /// ScrollOn: 창 기준 논리 좌표. 창이 움직였으면 따라간다
    Window {
        window: Window,
        local: Point<f64, Logical>,
    },
    /// ScrollLayer: 레이어 표면 기준 논리 좌표
    Layer {
        pid: u32,
        namespace: String,
        local: (f64, f64),
    },
}

/// 창 기준 점이 창 영역 밖인지
fn outside(local: Point<f64, Logical>, size: smithay::utils::Size<f64, Logical>) -> bool {
    local.x < 0.0 || local.y < 0.0 || local.x >= size.w || local.y >= size.h
}

/// 사람만 답하는 셸 창의 레이어 이름인지
fn human_only(namespace: &str) -> bool {
    AUTH_NAMESPACES.contains(&namespace)
}

/// 셸의 화면 키보드인지. 에이전트에게 알리지 않고 누르게 하지도 않는다 (osk.rs)
fn on_screen_keyboard(namespace: &str) -> bool {
    namespace == crate::osk::KEYBOARD_NAMESPACE
}

/// 레이어 표면 하나와 지금 그리는 자리 (placed_layers). 시험은 표면 없이(`()`) 만든다
struct PlacedLayer<S = WlSurface> {
    pid: u32,
    namespace: String,
    surface: S,
    /// 전역 논리 좌표
    area: Rectangle<i32, Logical>,
    /// 그 표면이 있는 출력의 배율
    scale: f64,
}

/// 에이전트에게 알리는 레이어 표면 (GetLayers): 사람만 답하는 창과 화면 키보드는 뺀다
fn listed<S>(placed: Vec<PlacedLayer<S>>) -> Vec<LayerInfo> {
    placed
        .into_iter()
        // 화면 키보드도 뺀다: 에이전트가 키보드를 누르는 길을 열지 않는다
        .filter(|layer| !human_only(&layer.namespace) && !on_screen_keyboard(&layer.namespace))
        .map(|layer| LayerInfo {
            pid: layer.pid,
            namespace: layer.namespace,
            x: layer.area.loc.x,
            y: layer.area.loc.y,
            width: layer.area.size.w,
            height: layer.area.size.h,
            scale: layer.scale,
        })
        .collect()
}

/// 에이전트가 누를 레이어 표면: 쌓인 차례(아래부터) 가운데 pid와 이름이 같은 것의 맨 위. 사람만 답하는
/// 창은 떠 있어도 고르지 않는다
fn choose_layer<'a, S>(
    placed: &'a [PlacedLayer<S>],
    pid: u32,
    namespace: &str,
) -> Result<&'a PlacedLayer<S>, Refused> {
    if human_only(namespace) {
        return Err(Refused::AuthDialog);
    }
    // 화면 키보드는 사람만 누른다 (osk.rs)
    if on_screen_keyboard(namespace) {
        return Err(Refused::Keyboard);
    }
    placed
        .iter()
        .rev()
        .find(|layer| layer.pid == pid && layer.namespace == namespace)
        .ok_or(Refused::NoWindow)
}

/// 창 제목 (xdg_toplevel)
fn window_title(window: &Window) -> Option<String> {
    let toplevel = window.toplevel()?;
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().unwrap().title.clone())
    })
}

/// 창의 앱 ID (xdg_toplevel)
fn window_app_id(window: &Window) -> Option<String> {
    let toplevel = window.toplevel()?;
    with_states(toplevel.wl_surface(), |states| {
        states
            .data_map
            .get::<XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().unwrap().app_id.clone())
    })
}

/// 글자 경계에서 자른, 하나가 `limit` 바이트 이하인 조각들
fn chunks(text: &str, limit: usize) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let mut end = rest.len().min(limit);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (chunk, tail) = rest.split_at(end);
        rest = tail;
        Some(chunk)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_split_on_character_boundaries() {
        // 한글은 글자마다 3바이트다
        let text = "가나다라";
        assert_eq!(chunks(text, 7).collect::<Vec<_>>(), ["가나", "다라"]);
        assert_eq!(chunks(text, 12).collect::<Vec<_>>(), ["가나다라"]);
        assert_eq!(chunks("abc", 1).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(chunks("", 4).count(), 0);
    }

    /// 셸(pid 10)의 레이어 표면들: 배경, 메뉴 막대, Dock, 관리자 인증 창, 다른 앱(pid 20)의 막대
    fn shell_layers() -> Vec<PlacedLayer<()>> {
        let layer = |pid, namespace: &str, (x, y, w, h)| PlacedLayer {
            pid,
            namespace: namespace.into(),
            surface: (),
            area: Rectangle::new((x, y).into(), (w, h).into()),
            scale: 1.0,
        };
        vec![
            layer(10, "bunt-wallpaper", (0, 0, 1280, 800)),
            layer(10, "bunt-menubar", (0, 0, 1280, 24)),
            layer(10, "bunt-dock", (400, 720, 480, 80)),
            layer(20, "bunt-dock", (0, 760, 1280, 40)),
            layer(10, "bunt-polkit-agent", (440, 250, 400, 300)),
        ]
    }

    #[test]
    fn the_agent_sees_shell_surfaces_but_not_dialogs_people_answer() {
        let listed = listed(shell_layers());
        let names: Vec<(u32, &str)> = listed
            .iter()
            .map(|layer| (layer.pid, layer.namespace.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                (10, "bunt-wallpaper"),
                (10, "bunt-menubar"),
                (10, "bunt-dock"),
                (20, "bunt-dock"),
            ]
        );
        // 자리는 지금 그리는 전역 좌표다
        assert_eq!(
            (listed[2].x, listed[2].y, listed[2].width, listed[2].height),
            (400, 720, 480, 80)
        );
        for namespace in AUTH_NAMESPACES {
            assert!(human_only(namespace), "{namespace}");
        }
    }

    #[test]
    fn clicks_go_to_the_named_shell_surface_and_never_to_dialogs_people_answer() {
        let placed = shell_layers();
        // 메뉴 막대와 Dock은 pid와 이름이 같은 표면을 누른다. 다른 앱의 같은 이름은 고르지 않는다
        let menubar = choose_layer(&placed, 10, "bunt-menubar").unwrap();
        assert_eq!(menubar.area.loc, (0, 0).into());
        let dock = choose_layer(&placed, 10, "bunt-dock").unwrap();
        assert_eq!(dock.area.loc, (400, 720).into());
        assert_eq!(
            choose_layer(&placed, 20, "bunt-dock").unwrap().area.loc,
            (0, 760).into()
        );
        // 사람만 답하는 창은 떠 있어도, 없어도 거절한다
        for namespace in AUTH_NAMESPACES {
            assert_eq!(
                choose_layer(&placed, 10, namespace).err(),
                Some(Refused::AuthDialog)
            );
            assert_eq!(
                choose_layer::<()>(&[], 10, namespace).err(),
                Some(Refused::AuthDialog)
            );
        }
        // 없는 표면, 다른 pid의 셸 표면
        assert_eq!(
            choose_layer(&placed, 10, "bunt-launcher").err(),
            Some(Refused::NoWindow)
        );
        assert_eq!(
            choose_layer(&placed, 20, "bunt-menubar").err(),
            Some(Refused::NoWindow)
        );
    }

    /// 셸의 AI 제어 창(멈추기·다시 하기)은 사람만 답한다: 목록에 들지 않고, 누를 수 없고, 떠 있는 동안 에이전트의
    /// 입력은 모두 거절된다(auth_dialog_open). AI가 스스로 멈춤을 풀지 못한다
    #[test]
    fn the_ai_control_window_is_for_people_only() {
        assert!(human_only("bunt-agent-control"));
        let mut placed = shell_layers();
        placed.push(PlacedLayer {
            pid: 10,
            namespace: "bunt-agent-control".into(),
            surface: (),
            area: Rectangle::new((900, 24).into(), (300, 200).into()),
            scale: 1.0,
        });
        assert!(
            listed(placed)
                .iter()
                .all(|layer| layer.namespace != "bunt-agent-control")
        );
        assert_eq!(
            choose_layer::<()>(&[], 10, "bunt-agent-control").err(),
            Some(Refused::AuthDialog)
        );
    }

    /// 창 기준 논리 좌표의 창 밖: 왼쪽 위 끝은 안, 오른쪽·아래 끝은 밖이다
    #[test]
    fn points_outside_the_window_area() {
        let size = smithay::utils::Size::<f64, Logical>::from((800.0, 600.0));
        assert!(!outside((0.0, 0.0).into(), size));
        assert!(!outside((799.5, 599.5).into(), size));
        assert!(outside((800.0, 10.0).into(), size));
        assert!(outside((10.0, 600.0).into(), size));
        assert!(outside((-0.5, 10.0).into(), size));
        assert!(outside((10.0, -1.0).into(), size));
    }

    /// 셸의 화면 키보드는 에이전트에게 보이지 않고 눌리지도 않는다. 사람만 답하는 창과 달리 떠 있어도 에이전트의
    /// 다른 입력은 막지 않는다 (osk.rs)
    #[test]
    fn the_agent_never_sees_or_presses_the_on_screen_keyboard() {
        let mut placed = shell_layers();
        placed.push(PlacedLayer {
            pid: 10,
            namespace: crate::osk::KEYBOARD_NAMESPACE.into(),
            surface: (),
            area: Rectangle::new((0, 446).into(), (1280, 354).into()),
            scale: 1.0,
        });
        assert_eq!(
            choose_layer(&placed, 10, crate::osk::KEYBOARD_NAMESPACE).err(),
            Some(Refused::Keyboard)
        );
        assert!(
            listed(placed)
                .iter()
                .all(|layer| layer.namespace != crate::osk::KEYBOARD_NAMESPACE)
        );
        assert!(!human_only(crate::osk::KEYBOARD_NAMESPACE));
    }
}
