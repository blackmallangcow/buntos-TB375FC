// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창 목록과 창 조작 (wlr-foreign-toplevel-management-unstable-v1, 버전 3).
//! 셸이 메뉴바의 앱 메뉴, Dock의 실행 표시, 전체 화면 표시에 쓴다. 다른 앱의 창을 보고
//! 다룰 수 있는 프로토콜이므로 특권 클라이언트(컴포지터가 직접 띄운 셸)에게만 보인다.

use std::cell::RefCell;

use smithay::{
    desktop::Window,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_protocols_wlr::foreign_toplevel::v1::server::{
            zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
            zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            backend::ClientId, protocol::wl_surface::WlSurface,
        },
    },
    utils::SERIAL_COUNTER,
    wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData},
};

use crate::{
    state::{BuntState, ClientState},
    windows,
};

const VERSION: u32 = 3;

#[derive(Default)]
pub struct ForeignToplevelState {
    managers: Vec<ZwlrForeignToplevelManagerV1>,
}

impl ForeignToplevelState {
    pub fn new(display: &DisplayHandle) -> Self {
        display.create_global::<BuntState, ZwlrForeignToplevelManagerV1, ()>(VERSION, ());
        Self::default()
    }
}

/// 셸에 알린 창의 핸들과 마지막으로 보낸 값. 창의 사용자 데이터에 둔다.
#[derive(Default)]
struct Announced(RefCell<AnnouncedInner>);

#[derive(Default)]
struct AnnouncedInner {
    handles: Vec<ZwlrForeignToplevelHandleV1>,
    title: String,
    app_id: String,
    states: Vec<u8>,
}

/// 핸들의 사용자 데이터: 대상 창의 표면
pub struct HandleData {
    surface: WlSurface,
}

/// 창의 (제목, 앱 ID)
pub fn title_and_app_id(window: &Window) -> (String, String) {
    let Some(toplevel) = window.toplevel() else {
        return Default::default();
    };
    with_states(toplevel.wl_surface(), |states| {
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
    })
}

/// 프로토콜의 state 배열: 상태 값(u32)을 기계 바이트 순서로 이어 붙인다. 최소화하거나 가린 창은
/// 최소화로 알린다
fn encoded_states(window: &Window) -> Vec<u8> {
    use zwlr_foreign_toplevel_handle_v1::State;
    let Some(toplevel) = window.toplevel() else {
        return Vec::new();
    };
    let mut states: Vec<u8> = toplevel.with_pending_state(|pending| {
        [
            (xdg_toplevel::State::Maximized, State::Maximized),
            (xdg_toplevel::State::Activated, State::Activated),
            (xdg_toplevel::State::Fullscreen, State::Fullscreen),
        ]
        .into_iter()
        .filter(|(xdg, _)| pending.states.contains(*xdg))
        .flat_map(|(_, state)| (state as u32).to_ne_bytes())
        .collect()
    });
    if windows::is_hidden(window) {
        states.extend((State::Minimized as u32).to_ne_bytes());
    }
    states
}

impl BuntState {
    /// 화면에 처음 나온 창을 셸에 알린다. 이미 알린 창이면 아무것도 하지 않는다.
    pub fn announce_window(&mut self, window: &Window) {
        if window.user_data().get::<Announced>().is_some() || window.geometry().size.is_empty() {
            return;
        }
        let (title, app_id) = title_and_app_id(window);
        window.user_data().get_or_insert(|| {
            Announced(RefCell::new(AnnouncedInner {
                title,
                app_id,
                states: encoded_states(window),
                ..Default::default()
            }))
        });
        for manager in self.foreign_toplevels.managers.clone() {
            self.send_new_handle(&manager, window);
        }
    }

    /// 관리자 하나에 창의 새 핸들과 지금 값을 보낸다.
    fn send_new_handle(&self, manager: &ZwlrForeignToplevelManagerV1, window: &Window) {
        let (Some(announced), Some(toplevel)) =
            (window.user_data().get::<Announced>(), window.toplevel())
        else {
            return;
        };
        let Ok(client) = self.display_handle.get_client(manager.id()) else {
            return;
        };
        let data = HandleData {
            surface: toplevel.wl_surface().clone(),
        };
        let Ok(handle) = client.create_resource::<ZwlrForeignToplevelHandleV1, _, BuntState>(
            &self.display_handle,
            manager.version(),
            data,
        ) else {
            return;
        };
        manager.toplevel(&handle);
        let mut inner = announced.0.borrow_mut();
        handle.title(inner.title.clone());
        handle.app_id(inner.app_id.clone());
        handle.state(inner.states.clone());
        handle.done();
        inner.handles.push(handle);
    }

    /// 창의 제목, 앱 ID, 상태(활성, 최대화, 전체 화면)가 바뀌었으면 셸에 알린다.
    pub fn refresh_window_info(&self, window: &Window) {
        let Some(announced) = window.user_data().get::<Announced>() else {
            return;
        };
        let (title, app_id) = title_and_app_id(window);
        let states = encoded_states(window);
        let mut inner = announced.0.borrow_mut();
        let mut changed = false;
        if inner.title != title {
            inner
                .handles
                .iter()
                .for_each(|handle| handle.title(title.clone()));
            inner.title = title;
            changed = true;
        }
        if inner.app_id != app_id {
            inner
                .handles
                .iter()
                .for_each(|handle| handle.app_id(app_id.clone()));
            inner.app_id = app_id;
            changed = true;
        }
        if inner.states != states {
            inner
                .handles
                .iter()
                .for_each(|handle| handle.state(states.clone()));
            inner.states = states;
            changed = true;
        }
        if changed {
            inner.handles.iter().for_each(|handle| handle.done());
        }
    }

    pub fn refresh_all_windows(&self) {
        for window in self.all_windows() {
            self.refresh_window_info(window);
        }
    }

    /// 창이 닫혔음을 셸에 알린다.
    pub fn forget_window(&self, window: &Window) {
        if let Some(announced) = window.user_data().get::<Announced>() {
            let mut inner = announced.0.borrow_mut();
            inner.handles.drain(..).for_each(|handle| handle.closed());
        }
    }
}

impl GlobalDispatch<ZwlrForeignToplevelManagerV1, ()> for BuntState {
    fn bind(
        state: &mut Self,
        _display: &DisplayHandle,
        _client: &Client,
        resource: New<ZwlrForeignToplevelManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let manager = data_init.init(resource, ());
        let windows: Vec<Window> = state.all_windows().cloned().collect();
        for window in &windows {
            state.send_new_handle(&manager, window);
        }
        state.foreign_toplevels.managers.push(manager);
    }

    fn can_view(client: Client, _global_data: &()) -> bool {
        client
            .get_data::<ClientState>()
            .is_some_and(|data| data.privileged)
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for BuntState {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZwlrForeignToplevelManagerV1,
        request: zwlr_foreign_toplevel_manager_v1::Request,
        _data: &(),
        _display: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Request::Stop = request {
            state
                .foreign_toplevels
                .managers
                .retain(|manager| manager != resource);
            resource.finished();
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &ZwlrForeignToplevelManagerV1,
        _data: &(),
    ) {
        state
            .foreign_toplevels
            .managers
            .retain(|manager| manager != resource);
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, HandleData> for BuntState {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZwlrForeignToplevelHandleV1,
        request: zwlr_foreign_toplevel_handle_v1::Request,
        data: &HandleData,
        _display: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::Request;
        let Some(window) = state.window_for_surface(&data.surface) else {
            return;
        };
        let Some(toplevel) = window.toplevel().cloned() else {
            return;
        };
        match request {
            Request::Activate { .. } => {
                state.focus_window(&window, SERIAL_COUNTER.next_serial());
            }
            Request::Close => toplevel.send_close(),
            Request::SetMaximized => {
                state.set_window_state(&toplevel, xdg_toplevel::State::Maximized, true)
            }
            Request::UnsetMaximized => {
                state.set_window_state(&toplevel, xdg_toplevel::State::Maximized, false)
            }
            Request::SetFullscreen { .. } => {
                state.set_window_state(&toplevel, xdg_toplevel::State::Fullscreen, true)
            }
            Request::UnsetFullscreen => {
                state.set_window_state(&toplevel, xdg_toplevel::State::Fullscreen, false)
            }
            Request::SetMinimized => state.minimize(&window),
            Request::UnsetMinimized => {
                state.focus_window(&window, SERIAL_COUNTER.next_serial());
            }
            // set_rectangle은 최소화 애니메이션의 목표 위치라 쓰지 않는다
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: ClientId,
        resource: &ZwlrForeignToplevelHandleV1,
        data: &HandleData,
    ) {
        if let Some(window) = state.window_for_surface(&data.surface)
            && let Some(announced) = window.user_data().get::<Announced>()
        {
            announced
                .0
                .borrow_mut()
                .handles
                .retain(|handle| handle != resource);
        }
    }
}
