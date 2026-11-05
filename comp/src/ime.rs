// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 입력기 (한글 입력). 앱은 text-input-v3로 글자를 받고, 입력기(fcitx5)는 input-method-v2로
//! 키보드를 잡아 조합한 글자를 보내며, 쓰지 않은 키는 virtual-keyboard-v1로 돌려보낸다.
//! 입력기와 가상 키보드 프로토콜은 키 입력을 엿보거나 대신 보낼 수 있으므로 컴포지터가 직접 띄운
//! 입력기의 연결에만 연다. 입력기가 갑자기 끝나면 잠시 뒤 다시 띄운다.
//!
//! 조합 중에 키보드 포커스가 다른 표면으로 옮겨 가면(다른 창을 누르기 등) 조합하던 글자를 그 칸에
//! 확정한다(macOS와 같다). 앱(GTK)은 text-input-v3의 leave를 받으면 조합 글자를 버리고, fcitx5는
//! input-method-v2에서 확정을 앱 쪽에 맡기므로(ClientUnfocusCommit) 아무도 확정하지 않으면 글자가
//! 사라진다. 그래서 입력기가 보낸 조합 글자를 기억해 두었다가 포커스를 옮기기 직전에 확정한다.

use std::time::{Duration, Instant};

use smithay::{
    delegate_virtual_keyboard_manager,
    desktop::{PopupKind, PopupManager, utils::bbox_from_surface_tree},
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_protocols::wp::text_input::zv3::server::{
            zwp_text_input_manager_v3::ZwpTextInputManagerV3,
            zwp_text_input_v3::{self, ZwpTextInputV3},
        },
        wayland_protocols_misc::zwp_input_method_v2::server::{
            zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
            zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
            zwp_input_method_v2::{self, ZwpInputMethodV2},
            zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2,
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, Resource, backend::ClientId,
            delegate_dispatch, delegate_global_dispatch, protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Point, Rectangle},
    wayland::{
        input_method::{
            InputMethodHandler, InputMethodKeyboardUserData, InputMethodManagerGlobalData,
            InputMethodManagerState, InputMethodPopupSurfaceUserData, InputMethodUserData,
            PopupSurface,
        },
        text_input::{TextInputManagerState, TextInputSeat, TextInputUserData},
    },
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

/// 입력기가 앱에 보이고 있는 조합 중인 글자. input-method-v2의 set_preedit_string은 다음 commit에
/// 적용되고, 한 commit 주기에 보내지 않으면 조합 글자가 없어진 것이다
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Preedit {
    /// 이번 commit 주기에 입력기가 보낸 조합 글자
    pending: Option<String>,
    /// 앱에 보이고 있는 조합 글자. 비었으면 조합 중이 아니다
    shown: String,
    /// 포커스를 옮기며 컴포지터가 대신 확정한 글자. 입력기가 바로 뒤에 같은 글자를 확정하면
    /// 두 번 들어가지 않게 버린다 (비활성화될 때 스스로 확정하는 입력기)
    flushed: Option<String>,
}

impl Preedit {
    fn set_preedit(&mut self, text: &str) {
        self.pending = Some(text.to_owned());
        self.flushed = None;
    }

    /// 입력기가 확정한 글자를 앱에 보낼지. 컴포지터가 이미 확정한 글자면 버린다
    fn should_forward_commit(&mut self, text: &str) -> bool {
        if self.flushed.as_deref() == Some(text) {
            self.flushed = None;
            return false;
        }
        true
    }

    fn commit(&mut self) {
        self.shown = self.pending.take().unwrap_or_default();
        self.flushed = None;
    }

    /// 앱이 글 입력을 켜거나 껐다. 그 칸의 조합은 끝났다 (GTK는 끌 때 조합 글자를 버린다)
    fn reset(&mut self) {
        self.pending = None;
        self.shown.clear();
    }

    /// 포커스를 옮기기 전에 확정할 조합 글자를 꺼낸다
    fn take_for_flush(&mut self) -> Option<String> {
        self.pending = None;
        let text = std::mem::take(&mut self.shown);
        (!text.is_empty()).then_some(text)
    }
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

    /// 키보드 포커스가 `next`로 옮겨 가기 직전에 부른다. 다른 표면으로 옮겨 가는데 조합 중인 글자가
    /// 있으면 지금 켜진 글 입력에 확정한다. 확정이 leave보다 먼저 가야 앱이 받는다(leave를 받은 앱은
    /// 조합 글자를 버린다). 입력기는 뒤이은 비활성화로 조합을 끝낸다
    pub fn commit_preedit_before_focus(&mut self, next: Option<&WlSurface>) {
        let unchanged = self
            .seat
            .get_keyboard()
            .is_some_and(|keyboard| keyboard.current_focus().as_ref() == next);
        if unchanged {
            return;
        }
        let Some(text) = self.preedit.take_for_flush() else {
            return;
        };
        let text_input = self.seat.text_input();
        let mut committed = false;
        text_input.with_active_text_input(|text_input, _| {
            text_input.commit_string(Some(text.clone()));
            committed = true;
        });
        if committed {
            text_input.done(false);
            tracing::debug!("committed the preedit before moving the keyboard focus");
            self.preedit.flushed = Some(text);
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
    pub(crate) fn surface_origin(
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

// 앱 쪽(text-input-v3)과 입력기 쪽(input-method-v2)은 smithay에 맡기되, 조합 글자를 기억하고 입력기의
// commit을 앱에 그대로 적용하려고 입력기와 글 입력 객체의 요청은 먼저 본다 (smithay의 delegate 매크로가
// 하는 일을 나눠 적었다)
delegate_global_dispatch!(BuntState: [ZwpTextInputManagerV3: ()] => TextInputManagerState);
delegate_dispatch!(BuntState: [ZwpTextInputManagerV3: ()] => TextInputManagerState);
delegate_global_dispatch!(BuntState: [ZwpInputMethodManagerV2: InputMethodManagerGlobalData] => InputMethodManagerState);
delegate_dispatch!(BuntState: [ZwpInputMethodManagerV2: ()] => InputMethodManagerState);
delegate_dispatch!(BuntState: [ZwpInputMethodKeyboardGrabV2: InputMethodKeyboardUserData<BuntState>] => InputMethodManagerState);
delegate_dispatch!(BuntState: [ZwpInputPopupSurfaceV2: InputMethodPopupSurfaceUserData] => InputMethodManagerState);
delegate_virtual_keyboard_manager!(BuntState);

impl Dispatch<ZwpInputMethodV2, InputMethodUserData<BuntState>> for BuntState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwpInputMethodV2,
        request: zwp_input_method_v2::Request,
        data: &InputMethodUserData<Self>,
        dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match &request {
            zwp_input_method_v2::Request::SetPreeditString { text, .. } => {
                state.preedit.set_preedit(text);
            }
            zwp_input_method_v2::Request::CommitString { text } => {
                if !state.preedit.should_forward_commit(text) {
                    tracing::debug!("dropped a commit the compositor already made");
                    return;
                }
            }
            zwp_input_method_v2::Request::Commit { .. } => {
                // 입력기의 serial이 그사이 보낸 done 수와 다르면 smithay는 앱에 done(0)을 보내 상태를
                // 버리게 한다. foot은 serial이 다른 done을 버리고 확정 글자를 다음 done까지 미뤄, 조합 중
                // Enter에서 "요"가 줄바꿈 뒤에 들어갔다. 명세(input-method-v2 commit)대로 serial이 달라도
                // 바뀐 것을 그대로 적용하도록 앱에는 늘 그 글 입력의 serial로 done을 보낸다
                state.preedit.commit();
                state.seat.text_input().done(false);
                return;
            }
            _ => {}
        }
        <InputMethodManagerState as Dispatch<ZwpInputMethodV2, InputMethodUserData<Self>, Self>>::request(
            state, client, resource, request, data, dhandle, data_init,
        );
    }

    fn destroyed(
        state: &mut Self,
        client: ClientId,
        resource: &ZwpInputMethodV2,
        data: &InputMethodUserData<Self>,
    ) {
        state.preedit = Preedit::default();
        <InputMethodManagerState as Dispatch<ZwpInputMethodV2, InputMethodUserData<Self>, Self>>::destroyed(
            state, client, resource, data,
        );
    }
}

impl Dispatch<ZwpTextInputV3, TextInputUserData> for BuntState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwpTextInputV3,
        request: zwp_text_input_v3::Request,
        data: &TextInputUserData,
        dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        // 포커스가 없는 앱의 요청은 smithay가 버리므로 세지 않는다
        if state
            .seat
            .text_input()
            .focus()
            .is_some_and(|focus| focus.id().same_client_as(&resource.id()))
        {
            match &request {
                // 키보드 포커스를 가진 앱이 글 입력을 켜거나 끄면 입력기는 그 칸과의 조합을 새로 시작한다.
                // 사람이 글 칸을 눌렀는지는 누르기와 함께 본다 (osk.rs)
                zwp_text_input_v3::Request::Enable => {
                    state.preedit.reset();
                    state.enable_requested(resource);
                }
                zwp_text_input_v3::Request::Disable => state.preedit.reset(),
                zwp_text_input_v3::Request::SetCursorRectangle {
                    x,
                    y,
                    width,
                    height,
                } => {
                    state.cursor_rectangle_requested(*x, *y, *width, *height);
                }
                zwp_text_input_v3::Request::Commit => state.commit_requested(resource),
                _ => {}
            }
        }
        if let zwp_text_input_v3::Request::SetContentType { hint, purpose } = &request {
            state.osk.content_type(resource, *hint, *purpose);
        }
        <TextInputManagerState as Dispatch<ZwpTextInputV3, TextInputUserData, Self>>::request(
            state, client, resource, request, data, dhandle, data_init,
        );
    }

    fn destroyed(
        state: &mut Self,
        client: ClientId,
        resource: &ZwpTextInputV3,
        data: &TextInputUserData,
    ) {
        <TextInputManagerState as Dispatch<ZwpTextInputV3, TextInputUserData, Self>>::destroyed(
            state, client, resource, data,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(preedit: &Preedit) -> &str {
        &preedit.shown
    }

    #[test]
    fn preedit_follows_input_method_commits() {
        let mut preedit = Preedit::default();
        preedit.set_preedit("하");
        assert_eq!(
            shown(&preedit),
            "",
            "set_preedit_string는 commit에서 적용된다"
        );
        preedit.commit();
        assert_eq!(shown(&preedit), "하");
        preedit.set_preedit("한");
        preedit.commit();
        assert_eq!(shown(&preedit), "한");
        // 조합 글자 없이 commit하면 조합이 끝난 것이다 (글자를 확정했거나 지웠다)
        preedit.commit();
        assert_eq!(shown(&preedit), "");
        assert_eq!(preedit.take_for_flush(), None);
    }

    #[test]
    fn flushing_takes_the_shown_preedit_once() {
        let mut preedit = Preedit::default();
        preedit.set_preedit("한");
        preedit.commit();
        assert_eq!(preedit.take_for_flush().as_deref(), Some("한"));
        assert_eq!(preedit.take_for_flush(), None);
    }

    #[test]
    fn a_late_commit_of_the_flushed_preedit_is_dropped_once() {
        let mut preedit = Preedit {
            flushed: Some("한".into()),
            ..Preedit::default()
        };
        assert!(!preedit.should_forward_commit("한"));
        assert!(preedit.should_forward_commit("한"));
    }

    #[test]
    fn new_input_after_a_flush_is_kept() {
        let mut preedit = Preedit {
            flushed: Some("한".into()),
            ..Preedit::default()
        };
        // 입력기가 새 조합을 시작했으면 그 뒤의 확정은 새 글자다
        preedit.set_preedit("ㅎ");
        assert!(preedit.should_forward_commit("한"));
        let mut preedit = Preedit {
            flushed: Some("한".into()),
            ..Preedit::default()
        };
        // 다른 글자의 확정은 그대로 보내고, commit 주기가 끝나면 기억을 지운다
        assert!(preedit.should_forward_commit("."));
        preedit.commit();
        assert!(preedit.should_forward_commit("한"));
    }

    #[test]
    fn enabling_or_disabling_ends_the_composition() {
        let mut preedit = Preedit::default();
        preedit.set_preedit("한");
        preedit.commit();
        preedit.set_preedit("하");
        preedit.reset();
        assert_eq!(preedit, Preedit::default());
    }
}
