// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 입력기 (한글 입력). 앱은 text-input-v3로 글자를 받고, 입력기(fcitx5)는 input-method-v2로
//! 키보드를 잡아 조합한 글자를 보내며, 쓰지 않은 키는 virtual-keyboard-v1로 돌려보낸다.
//! 입력기와 가상 키보드 프로토콜은 키 입력을 엿보거나 대신 보낼 수 있으므로 컴포지터가 직접 띄운
//! 입력기의 연결에만 연다. 입력기가 갑자기 끝나면 잠시 뒤 다시 띄운다.

use std::time::{Duration, Instant};

use smithay::{
    delegate_input_method_manager, delegate_text_input_manager, delegate_virtual_keyboard_manager,
    desktop::{PopupKind, PopupManager, utils::bbox_from_surface_tree},
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle},
    wayland::input_method::{InputMethodHandler, PopupSurface},
};

use crate::state::{BuntState, ClientState};

/// 입력기가 끝난 뒤 다시 띄우기까지 기다리는 시간
const RESTART_DELAY: Duration = Duration::from_secs(2);
/// 이보다 짧게 돌다 끝나기를 거듭하면 다시 띄우지 않는다
const STABLE_RUN: Duration = Duration::from_secs(60);
const MAX_RESTARTS: u32 = 5;

/// 입력기 프로세스를 지켜보는 상태
#[derive(Default)]
pub struct InputMethodProcess {
    started: Option<Instant>,
    restarts: u32,
}

impl BuntState {
    /// 설정의 입력기를 입력기 프로토콜을 쓸 수 있는 클라이언트로 띄운다
    pub fn spawn_input_method(&mut self) {
        let command = self.config.session.input_method.clone();
        if command.is_empty() {
            return;
        }
        let client = ClientState {
            input_method: true,
            ..ClientState::default()
        };
        let Some(mut child) = self.spawn_trusted(&command, client) else {
            return;
        };
        self.input_method_process.started = Some(Instant::now());
        let exited = self.input_method_exited.clone();
        std::thread::spawn(move || {
            let _ = child.wait();
            let _ = exited.send(());
        });
    }

    pub fn input_method_exited(&mut self) {
        let process = &mut self.input_method_process;
        if process
            .started
            .is_some_and(|started| started.elapsed() >= STABLE_RUN)
        {
            process.restarts = 0;
        }
        process.restarts += 1;
        if process.restarts > MAX_RESTARTS {
            tracing::warn!("the input method keeps exiting; not restarting it again");
            return;
        }
        tracing::warn!("the input method exited; restarting it");
        let timer =
            self.loop_handle
                .insert_source(Timer::from_duration(RESTART_DELAY), |_, _, state| {
                    state.spawn_input_method();
                    TimeoutAction::Drop
                });
        if let Err(err) = timer {
            tracing::warn!("failed to schedule the input method restart: {}", err.error);
        }
    }

    /// 입력기 팝업(한자 후보 창 등)을 글자 커서 바로 아래에 둔다. 아래에 자리가 없으면 위에 두고,
    /// 화면 옆으로 나가지 않게 한다. 위치는 부모 표면 기준이다
    pub fn place_input_method_popup(&self, popup: &PopupSurface) {
        let cursor = popup.text_input_rectangle();
        let mut location = Point::from((cursor.loc.x, cursor.loc.y + cursor.size.h));
        let size = bbox_from_surface_tree(popup.wl_surface(), (0, 0)).size;
        if let Some((origin, area)) = popup
            .get_parent()
            .and_then(|parent| self.surface_origin(&parent.surface))
        {
            if origin.y + location.y + size.h > area.loc.y + area.size.h {
                location.y = cursor.loc.y - size.h;
            }
            let right = area.loc.x + area.size.w - size.w - origin.x;
            location.x = location.x.min(right).max(area.loc.x - origin.x);
        }
        popup.set_location(location);
    }

    /// 표면 왼쪽 위의 전역 위치와 그 표면이 있는 출력 영역. 창과 레이어 표면만 안다
    fn surface_origin(
        &self,
        surface: &WlSurface,
    ) -> Option<(Point<i32, Logical>, Rectangle<i32, Logical>)> {
        let output_geo = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))?;
        if let Some(window) = self.window_for_surface(surface) {
            let location = self.space.element_location(&window)?;
            return Some((location - window.geometry().loc, output_geo));
        }
        self.layer_origin(surface)
            .map(|origin| (origin, output_geo))
    }
}

impl InputMethodHandler for BuntState {
    fn new_popup(&mut self, surface: PopupSurface) {
        self.place_input_method_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::from(surface)) {
            tracing::warn!("failed to track the input method popup: {err}");
        }
    }

    fn popup_repositioned(&mut self, surface: PopupSurface) {
        self.place_input_method_popup(&surface);
    }

    fn dismiss_popup(&mut self, surface: PopupSurface) {
        if let Some(parent) = surface.get_parent().map(|parent| parent.surface.clone()) {
            let _ = PopupManager::dismiss_popup(&parent, &PopupKind::from(surface));
        }
    }

    /// 팝업 위치의 기준. smithay는 팝업 위치에서 이 사각형의 위치를 빼서 그리므로, 부모가 창이나
    /// xdg 팝업이면 그 표면 안의 창 영역(그림자를 뺀 부분)을, 레이어 표면이면 원점을 돌려준다
    fn parent_geometry(&self, parent: &WlSurface) -> Rectangle<i32, Logical> {
        if let Some(window) = self.window_for_surface(parent) {
            return window.geometry();
        }
        if let Some(popup) = self.popups.find_popup(parent) {
            return popup.geometry();
        }
        Rectangle::default()
    }
}

delegate_text_input_manager!(BuntState);
delegate_input_method_manager!(BuntState);
delegate_virtual_keyboard_manager!(BuntState);
