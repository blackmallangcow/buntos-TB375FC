// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 키보드·포인터 입력 처리와 컴포지터 단축키

use smithay::{
    backend::{
        input::{
            AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
            KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
        },
        session::Session,
    },
    input::{
        keyboard::{FilterResult, Keysym, ModifiersState},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::{dbus::Command, state::BuntState};

/// 컴포지터가 가로채는 단축키 동작
enum Action {
    /// Ctrl+Alt+Backspace: 세션 끝내기
    Quit,
    /// Alt+Enter: 터미널 열기
    Terminal,
    /// Alt+F4: 포커스된 창 닫기
    CloseWindow,
    /// Ctrl+Alt+F1..F12: 가상 터미널 전환
    SwitchVt(i32),
    /// Ctrl+Space(macOS 기본), 한/영 키: 입력 소스 전환. 입력 칸에 포커스가 없어도 동작한다
    ToggleInputSource,
}

fn shortcut(modifiers: &ModifiersState, sym: Keysym) -> Option<Action> {
    let raw = sym.raw();
    let first_vt = Keysym::XF86_Switch_VT_1.raw();
    if (first_vt..=Keysym::XF86_Switch_VT_12.raw()).contains(&raw) {
        return Some(Action::SwitchVt((raw - first_vt + 1) as i32));
    }
    if sym == Keysym::Hangul
        || (sym == Keysym::space
            && modifiers.ctrl
            && !modifiers.alt
            && !modifiers.shift
            && !modifiers.logo)
    {
        Some(Action::ToggleInputSource)
    } else if sym == Keysym::BackSpace && modifiers.ctrl && modifiers.alt {
        Some(Action::Quit)
    } else if sym == Keysym::Return && modifiers.alt {
        Some(Action::Terminal)
    } else if sym == Keysym::F4 && modifiers.alt {
        Some(Action::CloseWindow)
    } else {
        None
    }
}

impl BuntState {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => self.on_keyboard::<I>(event),
            InputEvent::PointerMotion { event } => {
                let Some(pointer) = self.seat.get_pointer() else {
                    return;
                };
                let pos = self.clamp_to_outputs(pointer.current_location() + event.delta());
                self.pointer_moved(pos, event.time_msec());
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let Some(output_geo) = self
                    .space
                    .outputs()
                    .next()
                    .and_then(|output| self.space.output_geometry(output))
                else {
                    return;
                };
                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();
                self.pointer_moved(pos, event.time_msec());
            }
            InputEvent::PointerButton { event } => self.on_button::<I>(event),
            InputEvent::PointerAxis { event } => self.on_axis::<I>(event),
            _ => {}
        }
    }

    fn on_keyboard<I: InputBackend>(&mut self, event: I::KeyboardKeyEvent) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let time = Event::time_msec(&event);
        let pressed = event.state() == KeyState::Pressed;
        let action = keyboard.input::<Option<Action>, _>(
            self,
            event.key_code(),
            event.state(),
            serial,
            time,
            |state, modifiers, handle| {
                let code = handle.raw_code().raw();
                // 단축키로 가로챈 키는 뗄 때도 앱(과 입력기)에 보내지 않는다
                if !pressed {
                    return match state.suppressed_keys.iter().position(|key| *key == code) {
                        Some(index) => {
                            state.suppressed_keys.swap_remove(index);
                            FilterResult::Intercept(None)
                        }
                        None => FilterResult::Forward,
                    };
                }
                match shortcut(modifiers, handle.modified_sym()) {
                    Some(action) => {
                        state.suppressed_keys.push(code);
                        FilterResult::Intercept(Some(action))
                    }
                    None => FilterResult::Forward,
                }
            },
        );
        if let Some(Some(action)) = action {
            self.run(action);
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Quit => self.loop_signal.stop(),
            Action::Terminal => {
                let terminal = self.config.session.terminal.clone();
                self.spawn(&terminal);
            }
            Action::CloseWindow => {
                let focus = self
                    .seat
                    .get_keyboard()
                    .and_then(|keyboard| keyboard.current_focus());
                if let Some(window) = focus.and_then(|surface| self.window_for_surface(&surface))
                    && let Some(toplevel) = window.toplevel()
                {
                    toplevel.send_close();
                }
            }
            Action::SwitchVt(vt) => {
                if let Err(err) = self.backend.session.change_vt(vt) {
                    tracing::warn!("failed to switch to VT {vt}: {err}");
                }
            }
            Action::ToggleInputSource => self.dbus.send(Command::ToggleInputMethod),
        }
    }

    fn clamp_to_outputs(&self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        let Some(area) = self
            .space
            .outputs()
            .filter_map(|output| self.space.output_geometry(output))
            .reduce(|a, b| a.merge(b))
        else {
            return pos;
        };
        let x = pos.x.clamp(
            f64::from(area.loc.x),
            f64::from(area.loc.x + area.size.w - 1),
        );
        let y = pos.y.clamp(
            f64::from(area.loc.y),
            f64::from(area.loc.y + area.size.h - 1),
        );
        (x, y).into()
    }

    fn pointer_moved(&mut self, pos: Point<f64, Logical>, time: u32) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        // 화면 가장자리에 닿으면 숨은 막대를 꺼내고, 꺼낸 막대를 떠나면 숨길 때를 잰다
        self.update_bars(Some(pos));
        let under = self.surface_under(pos);
        let serial = SERIAL_COUNTER.next_serial();
        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pos,
                serial,
                time,
            },
        );
        pointer.frame(self);
    }

    fn on_button<I: InputBackend>(&mut self, event: I::PointerButtonEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let state = event.state();

        // 키보드를 받는 레이어 표면(on_demand)을 누르면 그 표면에 포커스를 준다.
        // 메뉴바·Dock처럼 키보드를 받지 않는 레이어를 누를 때는 창 포커스를 바꾸지 않는다
        if state == ButtonState::Pressed && !pointer.is_grabbed() {
            let pos = pointer.current_location();
            if let Some(layer) = self.keyboard_layer_under(pos) {
                self.focus_layer(&layer, serial);
            } else if !self.is_over_top_layer(pos) {
                match self
                    .space
                    .element_under(pos)
                    .map(|(window, _)| window.clone())
                {
                    Some(window) => self.focus_window(&window, serial),
                    None => self.clear_focus(serial),
                }
            }
        }

        pointer.button(
            self,
            &ButtonEvent {
                button: event.button_code(),
                state,
                serial,
                time: event.time_msec(),
            },
        );
        pointer.frame(self);
    }

    fn on_axis<I: InputBackend>(&mut self, event: I::PointerAxisEvent) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let source = event.source();
        let amount = |axis: Axis| {
            event
                .amount(axis)
                .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0)
        };

        let mut frame = AxisFrame::new(event.time_msec()).source(source);
        for axis in [Axis::Horizontal, Axis::Vertical] {
            let value = amount(axis);
            if value != 0.0 {
                frame = frame.value(axis, value);
                if let Some(discrete) = event.amount_v120(axis) {
                    frame = frame.v120(axis, discrete as i32);
                }
            }
            if source == AxisSource::Finger && event.amount(axis) == Some(0.0) {
                frame = frame.stop(axis);
            }
        }
        pointer.axis(self, frame);
        pointer.frame(self);
    }
}
