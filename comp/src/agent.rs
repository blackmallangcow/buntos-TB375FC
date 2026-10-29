// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 에이전트(buntd-agent)가 부르는 컴포지터 기능: 창 목록, 창 활성화, 키·포인터 입력
//! (D-Bus의 에이전트 전용 메서드, ARCHITECTURE 6장). 입력은 사용자의 seat에 넣으며 실제 입력과
//! 같은 길을 거친다. 키는 컴포지터 단축키를 거치지 않는다. 에이전트 전용 seat는 뒤에 둔다

use smithay::{
    backend::input::{ButtonState, KeyState},
    desktop::{Window, layer_map_for_output},
    input::keyboard::{FilterResult, Keycode, xkb},
    reexports::wayland_server::Resource,
    utils::{Rectangle, SERIAL_COUNTER},
    wayland::{
        compositor::with_states, shell::xdg::XdgToplevelSurfaceData, text_input::TextInputSeat,
    },
};

use crate::{
    dbus::{AgentRequest, Chord, Refused, Typed, WindowInfo},
    state::BuntState,
};

/// evdev 키 코드 + 8 (xkb 키 코드)의 범위
const KEYCODES: std::ops::RangeInclusive<u32> = 8..=255;
/// 셸의 관리자 인증 창(polkit 인증 에이전트)의 레이어 이름. 이 창은 사람만 답한다
const AUTH_NAMESPACE: &str = "bunt-polkit-agent";
/// 글 입력에 한 번에 확정하는 바이트 수. Wayland 메시지 하나는 4KB를 넘지 못한다
const COMMIT_BYTES: usize = 2048;

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
