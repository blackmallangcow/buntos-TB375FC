// SPDX-FileCopyrightText: 2017 Victor Berger and Victoria Brekenfeld
// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later AND MIT
//
// Smithay smallvil의 grabs/move_grab.rs(MIT)를 바탕으로 했다.

//! 창 이동 포인터 그랩 (xdg_toplevel.move)

use smithay::{
    desktop::Window,
    input::pointer::{
        AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
        GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent,
        GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData,
        MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point},
};

use crate::state::BuntState;

/// linux/input-event-codes.h의 BTN_LEFT
const BTN_LEFT: u32 = 0x110;

pub struct MoveSurfaceGrab {
    pub start_data: PointerGrabStartData<BuntState>,
    pub window: Window,
    pub initial_window_location: Point<i32, Logical>,
}

impl PointerGrab<BuntState> for MoveSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        // 그랩 중에는 어떤 클라이언트도 포인터 포커스를 받지 않는다
        handle.motion(data, None, event);

        let delta = event.location - self.start_data.location;
        let new_location = self.initial_window_location.to_f64() + delta;
        data.space
            .map_element(self.window.clone(), new_location.to_i32_round(), true);
    }

    fn relative_motion(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if !handle.current_pressed().contains(&BTN_LEFT) {
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        details: AxisFrame,
    ) {
        handle.axis(data, details)
    }

    fn frame(&mut self, data: &mut BuntState, handle: &mut PointerInnerHandle<'_, BuntState>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event)
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event)
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event)
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event)
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event)
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event)
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event)
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event)
    }

    fn start_data(&self) -> &PointerGrabStartData<BuntState> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut BuntState) {}
}
