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

/// 그랩이 따로 다루지 않는 포인터 이벤트를 그대로 넘긴다. 그랩이 풀릴 때 할 일(unset)은 그랩마다 다르므로 넣지 않는다
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
        drag_window(
            data,
            &self.window,
            self.initial_window_location,
            event.location - self.start_data.location,
        );
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

    fn unset(&mut self, _data: &mut BuntState) {}
}

/// 창을 처음 자리(`initial`)에서 끈 만큼(`delta`) 옮긴다. 포인터와 손가락의 옮기기가 함께 쓴다. 전에는
/// MoveSurfaceGrab::motion 안에 있었다
fn drag_window(
    data: &mut BuntState,
    window: &Window,
    initial: Point<i32, Logical>,
    delta: Point<f64, Logical>,
) {
    let location = dragged_location(initial, delta, data.window_top());
    data.space.map_element(window.clone(), location, true);
}

/// 끈 창이 놓일 자리. macOS처럼 창의 위쪽 끝(제목 막대)은 메뉴 막대(`top`)를 넘지 못한다. 메뉴 막대 밑으로 들어간 제목
/// 막대는 다시 잡을 수 없기 때문이다. 아래쪽 Dock은 넘어가도 된다
fn dragged_location(
    initial: Point<i32, Logical>,
    delta: Point<f64, Logical>,
    top: Option<i32>,
) -> Point<i32, Logical> {
    let mut location: Point<i32, Logical> = (initial.to_f64() + delta).to_i32_round();
    if let Some(top) = top {
        location.y = location.y.max(top);
    }
    location
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

/// 크기를 바꾸는 중인 창. 포인터와 손가락의 크기 조절이 함께 쓴다. 전에는 ResizeSurfaceGrab에 있었다
struct Resize {
    window: Window,
    edges: ResizeEdges,
    initial_rect: Rectangle<i32, Logical>,
    last_window_size: Size<i32, Logical>,
}

impl Resize {
    /// 크기 조절을 시작한다: 창에 크기를 바꾸는 중이라고 알리고(resizing) 진행 상태를 창 표면에 둔다. 전에는 알리기를
    /// resize_request(state.rs)가 했고, 손가락의 크기 조절도 쓰면서 여기로 옮겼다
    fn start(
        window: Window,
        edges: ResizeEdges,
        initial_rect: Rectangle<i32, Logical>,
    ) -> Option<Self> {
        let toplevel = window.toplevel()?;
        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
        });
        toplevel.send_pending_configure();
        ResizeState::with(toplevel.wl_surface(), |state| {
            *state = ResizeState::Resizing {
                edges,
                initial_rect,
            };
        });
        Some(Self {
            window,
            edges,
            initial_rect,
            last_window_size: initial_rect.size,
        })
    }

    /// 누른 자리에서 `delta`만큼 끌었을 때의 크기를 창에 청한다
    fn update(&mut self, data: &BuntState, delta: Point<f64, Logical>) {
        let Some(toplevel) = self.window.toplevel() else {
            return;
        };
        let limits = with_states(toplevel.wl_surface(), |states| {
            let mut guard = states.cached_state.get::<SurfaceCachedState>();
            let data = guard.current();
            (data.min_size, data.max_size)
        });
        self.last_window_size = resized(
            self.initial_rect,
            self.edges,
            delta,
            limits,
            data.window_top(),
        );
        toplevel.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
            state.size = Some(self.last_window_size);
        });
        toplevel.send_pending_configure();
    }

    /// 끌기를 마친다: 마지막 크기를 알리고 resizing을 끈 뒤, 창의 마지막 커밋에서 반대쪽 가장자리를 제자리에 둔다
    /// (handle_resize_commit). 그랩이 풀릴 때(unset) 부른다. 전에는 포인터 단추를 뗄 때만 해서, 끄는 중에 화면을 잠가
    /// 그랩이 풀리면 창이 resizing인 채로 남았다
    fn finish(&self) {
        let Some(toplevel) = self.window.toplevel() else {
            return;
        };
        // 끄는 중에 창이 닫혔다. smithay는 사라진 표면의 그랩을 풀 때도 unset을 부르고, 사라진 표면의 상태는 읽을 수 없다
        if !toplevel.alive() {
            return;
        }
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
}

/// 크기 조절의 새 크기. 왼쪽·위쪽 가장자리를 끌면 끈 반대 방향으로 커진다. 위쪽 가장자리를 끌어도 창의 위쪽 끝은 메뉴
/// 막대(`top`)를 넘지 못한다(옮기기와 같다). 앱이 정한 (최소, 최대) 크기(`limits`)를 지키고(최대 0은 제한이 없다) 1보다
/// 작아지지 않는다. 전에는 ResizeSurfaceGrab::motion 안에 있었다
fn resized(
    initial_rect: Rectangle<i32, Logical>,
    edges: ResizeEdges,
    mut delta: Point<f64, Logical>,
    (min_size, max_size): (Size<i32, Logical>, Size<i32, Logical>),
    top: Option<i32>,
) -> Size<i32, Logical> {
    if edges.left {
        delta.x = -delta.x;
    }
    if edges.top {
        delta.y = -delta.y;
    }
    let mut size = initial_rect.size;
    if edges.left || edges.right {
        size.w = (f64::from(size.w) + delta.x) as i32;
    }
    if edges.top || edges.bottom {
        size.h = (f64::from(size.h) + delta.y) as i32;
    }
    if edges.top
        && let Some(top) = top
    {
        let bottom = initial_rect.loc.y + initial_rect.size.h;
        size.h = size.h.min(bottom - top);
    }
    let limit = |max: i32| if max == 0 { i32::MAX } else { max };
    Size::from((
        size.w.max(min_size.w.max(1)).min(limit(max_size.w)),
        size.h.max(min_size.h.max(1)).min(limit(max_size.h)),
    ))
}

pub struct ResizeSurfaceGrab {
    start_data: PointerGrabStartData<BuntState>,
    resize: Resize,
}

impl ResizeSurfaceGrab {
    pub fn start(
        start_data: PointerGrabStartData<BuntState>,
        window: Window,
        edges: ResizeEdges,
        initial_rect: Rectangle<i32, Logical>,
    ) -> Option<Self> {
        Some(Self {
            start_data,
            resize: Resize::start(window, edges, initial_rect)?,
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
        self.resize
            .update(data, event.location - self.start_data.location);
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
        // 마무리는 unset이 한다
        handle.unset_grab(self, data, event.serial, event.time, true);
    }

    forward_other_events!();

    fn unset(&mut self, _data: &mut BuntState) {
        self.resize.finish();
    }
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
    /// 포인터나 손가락을 놓은 뒤 클라이언트의 마지막 커밋을 기다린다
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn edges(edge: xdg_toplevel::ResizeEdge) -> ResizeEdges {
        edge.into()
    }

    /// 앱이 크기를 정하지 않았다 (최소 0, 최대 0)
    fn free() -> (Size<i32, Logical>, Size<i32, Logical>) {
        (Size::from((0, 0)), Size::from((0, 0)))
    }

    #[test]
    fn dragged_windows_stay_below_the_menu_bar() {
        // 끈 만큼 옮기고 반올림한다
        assert_eq!(
            dragged_location((100, 100).into(), (10.4, -20.6).into(), Some(28)),
            Point::from((110, 79))
        );
        // 제목 막대는 메뉴 막대 아래 끝(28)을 넘지 못한다. 옆으로는 그대로 간다
        assert_eq!(
            dragged_location((100, 100).into(), (-500.0, -500.0).into(), Some(28)),
            Point::from((-400, 28))
        );
        // 메뉴 막대가 늘 숨으면(위 끝 0) 화면 위 끝까지, 막대를 모르면 제한이 없다
        assert_eq!(
            dragged_location((0, 50).into(), (0.0, -80.0).into(), Some(0)),
            Point::from((0, 0))
        );
        assert_eq!(
            dragged_location((0, 50).into(), (0.0, -80.0).into(), None),
            Point::from((0, -30))
        );
    }

    #[test]
    fn resize_edges_come_from_the_protocol_bits() {
        let corner = edges(xdg_toplevel::ResizeEdge::TopLeft);
        assert!(corner.top && corner.left && !corner.bottom && !corner.right);
        let side = edges(xdg_toplevel::ResizeEdge::Right);
        assert!(side.right && !side.left && !side.top && !side.bottom);
    }

    #[test]
    fn windows_grow_away_from_the_dragged_edge() {
        let window = rect(100, 100, 400, 300);
        // 오른쪽 아래 모서리: 끈 방향으로 커진다
        assert_eq!(
            resized(
                window,
                edges(xdg_toplevel::ResizeEdge::BottomRight),
                (50.0, 20.0).into(),
                free(),
                Some(28)
            ),
            Size::from((450, 320))
        );
        // 왼쪽 위 모서리: 왼쪽·위로 끌면 커진다
        assert_eq!(
            resized(
                window,
                edges(xdg_toplevel::ResizeEdge::TopLeft),
                (-50.0, -20.0).into(),
                free(),
                Some(28)
            ),
            Size::from((450, 320))
        );
        // 오른쪽 변만 끌면 높이는 그대로다
        assert_eq!(
            resized(
                window,
                edges(xdg_toplevel::ResizeEdge::Right),
                (30.0, 99.0).into(),
                free(),
                None
            ),
            Size::from((430, 300))
        );
    }

    #[test]
    fn resizing_keeps_the_limits_and_the_menu_bar() {
        let window = rect(100, 100, 400, 300);
        let bottom_right = edges(xdg_toplevel::ResizeEdge::BottomRight);
        // 앱의 최소·최대 크기를 지킨다
        let limits = (Size::from((200, 150)), Size::from((600, 0)));
        assert_eq!(
            resized(
                window,
                bottom_right,
                (-1000.0, -1000.0).into(),
                limits,
                None
            ),
            Size::from((200, 150))
        );
        assert_eq!(
            resized(window, bottom_right, (1000.0, 1000.0).into(), limits, None),
            Size::from((600, 1300))
        );
        // 앱이 크기를 정하지 않아도 1보다 작아지지 않는다
        assert_eq!(
            resized(
                window,
                bottom_right,
                (-1000.0, -1000.0).into(),
                free(),
                None
            ),
            Size::from((1, 1))
        );
        // 위쪽 변을 끌어도 창의 위쪽 끝은 메뉴 막대 아래 끝(28)까지다: 아래 끝 400 - 28
        assert_eq!(
            resized(
                window,
                edges(xdg_toplevel::ResizeEdge::Top),
                (0.0, -500.0).into(),
                free(),
                Some(28)
            ),
            Size::from((400, 372))
        );
    }
}
