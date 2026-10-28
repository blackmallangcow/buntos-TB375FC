// SPDX-FileCopyrightText: 2017 Victor Berger and Victoria Brekenfeld
// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later AND MIT
//
// Smithay smallvil의 grabs/move_grab.rs와 grabs/resize_grab.rs(MIT)를 바탕으로 했다.

//! 창 이동·크기 조절 포인터 그랩 (xdg_toplevel.move, xdg_toplevel.resize)

use std::cell::RefCell;

use smithay::{
    desktop::{Space, Window},
    input::pointer::{
        AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent, GesturePinchBeginEvent,
        GesturePinchEndEvent, GesturePinchUpdateEvent, GestureSwipeBeginEvent,
        GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData as PointerGrabStartData,
        MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{compositor::with_states, shell::xdg::SurfaceCachedState},
};

use crate::state::BuntState;

/// linux/input-event-codes.h의 BTN_LEFT
const BTN_LEFT: u32 = 0x110;

/// 그랩이 따로 다루지 않는 포인터 이벤트를 그대로 넘긴다.
macro_rules! forward_other_events {
    () => {
        fn relative_motion(
            &mut self,
            data: &mut BuntState,
            handle: &mut PointerInnerHandle<'_, BuntState>,
            focus: Option<(WlSurface, Point<f64, Logical>)>,
            event: &RelativeMotionEvent,
        ) {
            handle.relative_motion(data, focus, event);
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
    };
}

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
        let mut new_location: Point<i32, Logical> =
            (self.initial_window_location.to_f64() + delta).to_i32_round();
        // macOS처럼 창의 위쪽 끝(제목 막대)은 메뉴 막대를 넘지 못한다. 메뉴 막대 밑으로 들어간 제목
        // 막대는 다시 잡을 수 없기 때문이다. 아래쪽 Dock은 넘어가도 된다
        if let Some(top) = data.window_top() {
            new_location.y = new_location.y.max(top);
        }
        data.space
            .map_element(self.window.clone(), new_location, true);
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

    forward_other_events!();
}

/// 크기를 바꾸는 창 가장자리
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeEdges {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
}

impl From<xdg_toplevel::ResizeEdge> for ResizeEdges {
    fn from(edge: xdg_toplevel::ResizeEdge) -> Self {
        // 프로토콜 값은 위(1), 아래(2), 왼쪽(4), 오른쪽(8) 비트의 조합이다
        let bits = edge as u32;
        Self {
            top: bits & 1 != 0,
            bottom: bits & 2 != 0,
            left: bits & 4 != 0,
            right: bits & 8 != 0,
        }
    }
}

pub struct ResizeSurfaceGrab {
    start_data: PointerGrabStartData<BuntState>,
    window: Window,
    edges: ResizeEdges,
    initial_rect: Rectangle<i32, Logical>,
    last_window_size: Size<i32, Logical>,
}

impl ResizeSurfaceGrab {
    pub fn start(
        start_data: PointerGrabStartData<BuntState>,
        window: Window,
        edges: ResizeEdges,
        initial_rect: Rectangle<i32, Logical>,
    ) -> Option<Self> {
        let toplevel = window.toplevel()?;
        ResizeState::with(toplevel.wl_surface(), |state| {
            *state = ResizeState::Resizing {
                edges,
                initial_rect,
            };
        });
        Some(Self {
            start_data,
            window,
            edges,
            initial_rect,
            last_window_size: initial_rect.size,
        })
    }
}

impl PointerGrab<BuntState> for ResizeSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        _focus: Option<(WlSurface, Point<f64, Logical>)>,
        event: &MotionEvent,
    ) {
        handle.motion(data, None, event);
        let Some(toplevel) = self.window.toplevel() else {
            return;
        };

        // 왼쪽·위쪽 가장자리를 끌면 포인터가 움직인 반대 방향으로 커진다
        let mut delta = event.location - self.start_data.location;
        if self.edges.left {
            delta.x = -delta.x;
        }
        if self.edges.top {
            delta.y = -delta.y;
        }
        let mut size = self.initial_rect.size;
        if self.edges.left || self.edges.right {
            size.w = (f64::from(size.w) + delta.x) as i32;
        }
        if self.edges.top || self.edges.bottom {
            size.h = (f64::from(size.h) + delta.y) as i32;
        }
        // 위쪽 가장자리를 끌어도 창의 위쪽 끝은 메뉴 막대를 넘지 못한다 (옮기기와 같다)
        if self.edges.top
            && let Some(top) = data.window_top()
        {
            let bottom = self.initial_rect.loc.y + self.initial_rect.size.h;
            size.h = size.h.min(bottom - top);
        }

        let (min_size, max_size) = with_states(toplevel.wl_surface(), |states| {
            let mut guard = states.cached_state.get::<SurfaceCachedState>();
            let data = guard.current();
            (data.min_size, data.max_size)
        });
        // 최대 크기 0은 제한이 없다는 뜻이다
        let limit = |max: i32| if max == 0 { i32::MAX } else { max };
        self.last_window_size = Size::from((
            size.w.max(min_size.w.max(1)).min(limit(max_size.w)),
            size.h.max(min_size.h.max(1)).min(limit(max_size.h)),
        ));

        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
            state.size = Some(self.last_window_size);
        });
        toplevel.send_pending_configure();
    }

    fn button(
        &mut self,
        data: &mut BuntState,
        handle: &mut PointerInnerHandle<'_, BuntState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().contains(&BTN_LEFT) {
            return;
        }
        handle.unset_grab(self, data, event.serial, event.time, true);

        let Some(toplevel) = self.window.toplevel() else {
            return;
        };
        toplevel.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Resizing);
            state.size = Some(self.last_window_size);
        });
        toplevel.send_pending_configure();
        ResizeState::with(toplevel.wl_surface(), |state| {
            *state = ResizeState::WaitingForLastCommit {
                edges: self.edges,
                initial_rect: self.initial_rect,
            };
        });
    }

    forward_other_events!();
}

/// 크기 조절 진행 상태. 창 표면의 데이터 맵에 둔다.
#[derive(Debug, Clone, Copy, Default)]
enum ResizeState {
    #[default]
    Idle,
    Resizing {
        edges: ResizeEdges,
        initial_rect: Rectangle<i32, Logical>,
    },
    /// 포인터를 놓은 뒤 클라이언트의 마지막 커밋을 기다린다
    WaitingForLastCommit {
        edges: ResizeEdges,
        initial_rect: Rectangle<i32, Logical>,
    },
}

impl ResizeState {
    fn with<T>(surface: &WlSurface, f: impl FnOnce(&mut Self) -> T) -> T {
        with_states(surface, |states| {
            states.data_map.insert_if_missing(RefCell::<Self>::default);
            let state = states.data_map.get::<RefCell<Self>>().unwrap();
            f(&mut state.borrow_mut())
        })
    }

    fn commit(&mut self) -> Option<(ResizeEdges, Rectangle<i32, Logical>)> {
        match *self {
            Self::Resizing {
                edges,
                initial_rect,
            } => Some((edges, initial_rect)),
            Self::WaitingForLastCommit {
                edges,
                initial_rect,
            } => {
                *self = Self::Idle;
                Some((edges, initial_rect))
            }
            Self::Idle => None,
        }
    }
}

/// 창이 새 크기로 커밋하면, 왼쪽·위쪽 가장자리를 끈 경우 반대쪽 가장자리가 제자리에 있도록 창을 옮긴다.
pub fn handle_resize_commit(space: &mut Space<Window>, surface: &WlSurface) {
    let Some(window) = space
        .elements()
        .find(|window| {
            window
                .toplevel()
                .is_some_and(|toplevel| toplevel.wl_surface() == surface)
        })
        .cloned()
    else {
        return;
    };
    let Some(mut location) = space.element_location(&window) else {
        return;
    };
    let Some((edges, initial_rect)) = ResizeState::with(surface, ResizeState::commit) else {
        return;
    };
    if !edges.left && !edges.top {
        return;
    }
    let size = window.geometry().size;
    if edges.left {
        location.x = initial_rect.loc.x + initial_rect.size.w - size.w;
    }
    if edges.top {
        location.y = initial_rect.loc.y + initial_rect.size.h - size.h;
    }
    space.map_element(window, location, false);
}
