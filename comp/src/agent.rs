// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 에이전트(buntd-agent)가 부르는 컴포지터 기능: 창 목록, 창 활성화, 키·포인터 입력
//! (D-Bus의 에이전트 전용 메서드, ARCHITECTURE 6장). 입력은 사용자의 seat에 넣으며 실제 입력과
//! 같은 길을 거친다. 키는 컴포지터 단축키를 거치지 않는다. 에이전트 전용 seat는 뒤에 둔다

use std::{sync::mpsc, time::Duration};

use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, KeyState},
    desktop::{Window, layer_map_for_output},
    input::{
        keyboard::{FilterResult, Keycode, xkb},
        pointer::AxisFrame,
    },
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_server::Resource,
    },
    utils::{Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::with_states, shell::xdg::XdgToplevelSurfaceData, text_input::TextInputSeat,
    },
};

use crate::{
    dbus::{AgentRequest, Chord, Refused, Typed, WindowInfo},
    state::BuntState,
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
/// 셸의 관리자 인증 창(polkit 인증 에이전트)의 레이어 이름. 이 창은 사람만 답한다
const AUTH_NAMESPACE: &str = "bunt-polkit-agent";
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
        // 인증 창이 떠 있는 동안에는 에이전트가 입력을 넣거나 포커스를 옮기지 못한다. 에이전트가 대신
        // 답하거나, 사람이 칠 암호가 다른 창으로 가는 일을 막는다
        let refused = self.auth_dialog_open().then_some(Refused::AuthDialog);
        match request {
            AgentRequest::Windows(reply) => {
                let _ = reply.send(self.agent_windows());
            }
            AgentRequest::TextInput(reply) => {
                let _ = reply.send(self.text_input_active());
            }
            AgentRequest::Activate { pid, title, reply } => {
                let _ = reply.send(match refused {
                    Some(why) => Err(why),
                    None => Ok(self.agent_activate(pid, &title)),
                });
            }
            AgentRequest::Keys { chords, reply } => {
                let _ = reply.send(match refused {
                    Some(why) => Err(why),
                    None => {
                        self.agent_keys(&chords);
                        Ok(())
                    }
                });
            }
            AgentRequest::Type { text, reply } => {
                let _ = reply.send(match refused {
                    Some(why) => Err(why),
                    None => self.agent_type(&text),
                });
            }
            AgentRequest::Click {
                x,
                y,
                button,
                reply,
            } => {
                let _ = reply.send(match refused {
                    Some(why) => Err(why),
                    None => {
                        self.agent_click(x, y, button);
                        Ok(())
                    }
                });
            }
            AgentRequest::Scroll {
                x,
                y,
                dx,
                dy,
                reply,
            } => match refused {
                Some(why) => {
                    let _ = reply.send(Err(why));
                }
                None => self.agent_scroll(x, y, dx, dy, reply),
            },
            AgentRequest::Commit { text, reply } => {
                let _ = reply.send(match refused {
                    Some(why) => Err(why),
                    None => Ok(self.commit_text(&text)),
                });
            }
            // 그 창만 그리므로 인증 창은 들지 않는다. 읽기라 인증 창이 떠 있어도 막지 않는다
            AgentRequest::Capture { pid, title, reply } => {
                let _ = reply.send(self.agent_capture(pid, &title));
            }
            AgentRequest::ClickWindow {
                pid,
                title,
                x,
                y,
                button,
                reply,
            } => {
                let _ = reply.send(match refused {
                    Some(why) => Err(why),
                    None => self.agent_click_window(pid, &title, (x, y), button),
                });
            }
        }
    }

    fn auth_dialog_open(&self) -> bool {
        self.space.outputs().any(|output| {
            layer_map_for_output(output)
                .layers()
                .any(|layer| layer.namespace() == AUTH_NAMESPACE)
        })
    }

    fn client_pid(&self, window: &Window) -> u32 {
        window
            .toplevel()
            .and_then(|toplevel| {
                self.display_handle
                    .get_client(toplevel.wl_surface().id())
                    .ok()
            })
            .and_then(|client| client.get_credentials(&self.display_handle).ok())
            .map_or(0, |credentials| credentials.pid as u32)
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
    /// 고를 수 있고, 고르면 제자리에 다시 놓인다
    fn agent_activate(&mut self, pid: u32, title: &str) -> bool {
        let windows: Vec<Window> = self
            .space
            .elements()
            .rev()
            .chain(self.hidden.iter().map(|hidden| &hidden.window))
            .filter(|window| self.client_pid(window) == pid)
            .cloned()
            .collect();
        let titled = windows.iter().find(|window| {
            window.toplevel().is_some_and(|toplevel| {
                with_states(toplevel.wl_surface(), |states| {
                    states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()
                        .is_some_and(|data| data.lock().unwrap().title.as_deref() == Some(title))
                })
            })
        });
        let Some(window) = titled.or(windows.first()).cloned() else {
            return false;
        };
        self.focus_window(&window, SERIAL_COUNTER.next_serial());
        true
    }

    /// 수정 키를 누르고, 키를 눌렀다 떼고, 수정 키를 거꾸로 뗀다
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
                keyboard.input::<(), _>(
                    state,
                    code,
                    key_state,
                    SERIAL_COUNTER.next_serial(),
                    time,
                    |_, _, _| FilterResult::Forward,
                );
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
        self.press_button(button, ButtonState::Pressed, time);
        self.press_button(button, ButtonState::Released, time.saturating_add(1));
    }

    /// 포인터를 (x, y)로 옮겨 휠을 돌리고 답한다. dx, dy는 눈금 수이고 오른쪽·아래가 +다. 시스템 설정의
    /// 스크롤 속도와 자연스러운 스크롤은 사람의 장치에만 쓴다. 포인터가 다른 표면에 들어가면
    /// ENTER_PAUSE 뒤에 그 자리에서 한 번 더 움직이고 돌린다
    fn agent_scroll(
        &mut self,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        reply: mpsc::Sender<Result<(), Refused>>,
    ) {
        let Some(pointer) = self.seat.get_pointer() else {
            let _ = reply.send(Ok(()));
            return;
        };
        let before = pointer.current_focus();
        let time = self.start_time.elapsed().as_millis() as u32;
        self.pointer_moved((x, y).into(), time);
        if pointer.current_focus() == before {
            self.agent_wheel(dx, dy);
            let _ = reply.send(Ok(()));
            return;
        }
        let timer = self.loop_handle.insert_source(
            Timer::from_duration(ENTER_PAUSE),
            move |_, _, state| {
                // 기다리는 사이에 인증 창이 떴으면 돌리지 않는다
                let _ = reply.send(if state.auth_dialog_open() {
                    Err(Refused::AuthDialog)
                } else {
                    let time = state.start_time.elapsed().as_millis() as u32;
                    state.pointer_moved((x, y).into(), time);
                    state.agent_wheel(dx, dy);
                    Ok(())
                });
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
    fn agent_window(&self, pid: u32, title: &str) -> Option<Window> {
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

    /// 창 안의 한 점을 포인터로 누른다. (x, y)는 창 스크린샷(capture_window)의 픽셀이다. 창이 앞에
    /// 없으면 먼저 가져온다. 키보드 포커스가 이미 이 창에 있으면 열린 메뉴가 닫히지 않게 그대로 둔다.
    /// 그 점이 창 밖이거나 그 위에 다른 것(이 창의 대화 상자, 다른 창, Dock·메뉴 막대·알림)이 있으면
    /// 누르지 않는다: 스크린샷에 보인 것과 다른 것을 누르게 된다
    fn agent_click_window(
        &mut self,
        pid: u32,
        title: &str,
        (x, y): (f64, f64),
        button: u32,
    ) -> Result<(f64, f64), Refused> {
        let window = self.agent_window(pid, title).ok_or(Refused::NoWindow)?;
        let surface = window
            .toplevel()
            .map(|toplevel| toplevel.wl_surface().clone());
        let focused = self
            .seat
            .get_keyboard()
            .and_then(|keyboard| keyboard.current_focus())
            .map(|focus| self.root_surface(&focus));
        if focused.is_none() || focused != surface || self.space.element_geometry(&window).is_none()
        {
            self.focus_window(&window, SERIAL_COUNTER.next_serial());
        }
        let geometry = self
            .space
            .element_geometry(&window)
            .ok_or(Refused::NoWindow)?;
        let scale = self
            .window_output(&window)
            .map_or(1.0, |output| output.current_scale().fractional_scale());
        let size = geometry.size.to_f64();
        let local = Point::<f64, Logical>::from((x / scale, y / scale));
        if local.x >= size.w || local.y >= size.h || local.x < 0.0 || local.y < 0.0 {
            let pixels = size.to_physical(scale);
            return Err(Refused::Outside {
                width: pixels.w.round() as u32,
                height: pixels.h.round() as u32,
            });
        }
        let pos = geometry.loc.to_f64() + local;
        let on_top = self
            .space
            .element_under(pos)
            .is_some_and(|(under, _)| *under == window);
        if !on_top || self.is_over_top_layer(pos) {
            return Err(Refused::Covered);
        }
        self.agent_click(pos.x, pos.y, button);
        Ok((pos.x, pos.y))
    }
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
}
