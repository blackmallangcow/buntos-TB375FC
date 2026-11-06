// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 포커스 없이 클립보드를 읽고 쓰는 길: ext_data_control_v1과 zwlr_data_control_v1 (ARCHITECTURE 5.2). 터미널의
//! `wl-copy`, `wl-paste`와 셸의 클립보드 기록(shell/src/clipboard.rs)이 쓴다. 앱끼리의 복사와 붙여넣기(wl_data_device)와
//! 같은 선택을 함께 쓴다.
//!
//! - 두 프로토콜을 다 연다: ext는 표준이고, Debian의 wl-clipboard와 많은 도구는 아직 zwlr만 안다.
//! - 기본 선택(primary selection)은 없다. 컴포지터가 zwp_primary_selection을 열지 않으므로 맞춘다.
//! - 누가 쓰는가 ([`may_use`]):
//!   - 샌드박스 앱(보안 컨텍스트로 붙은 클라이언트)에는 보이지 않는다. 샌드박스 앱이 포커스 없이 클립보드를 읽으면 샌드박스의
//!     뜻이 없다. 그래서 wp_security_context_manager_v1도 함께 연다: Flatpak은 이 프로토콜이 있으면 앱에 따로 만든
//!     소켓을 주고, 없으면 세션의 소켓을 그대로 넘겨 컴포지터가 앱을 알아보지 못한다. 앱은 샌드박스 안에서 이 프로토콜을
//!     다시 쓰지 못한다(프로토콜의 규칙).
//!   - 입력기와 잠금 화면에는 보이지 않는다. 쓸 일이 없다. fcitx5의 클립보드 모듈이 이 프로토콜을 보면 복사한 글을 제 메모리에
//!     따로 모은다.
//!   - 잠긴 동안에는 새 연결에 보이지 않는다: 자리를 비운 사이 `wl-paste`로 읽지 못하게 한다(에이전트 접근 정책의 "잠긴 동안
//!     아무것도 읽지 못한다"와 같은 뜻). 이미 붙은 프로그램은 막지 못한다. smithay의 제안(offer) 처리가 컴포지터에 열려 있지
//!     않아서다.
//! - 샌드박스 밖의 같은 사용자 프로그램은 쓸 수 있다. 같은 사용자의 프로그램을 가려내는 일은 에이전트 접근 정책의 한계와 같다
//!   (ARCHITECTURE 6.5): 정책은 에이전트 데몬이 지키는 것이고, 같은 사용자로 도는 AI가 접근성 버스에 직접 붙는 것도 막지 못한다.
//!   에이전트 API에는 클립보드를 읽는 메서드가 없다.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use smithay::{
    reexports::wayland_server::{Client, DisplayHandle},
    wayland::{
        security_context::{
            SecurityContext, SecurityContextHandler, SecurityContextListenerSource,
            SecurityContextState,
        },
        selection::{ext_data_control, wlr_data_control},
    },
};

use crate::state::{BuntState, ClientState};

pub struct ClipboardAccess {
    ext: ext_data_control::DataControlState,
    wlr: wlr_data_control::DataControlState,
    /// 전역을 거르는 필터가 함께 본다. 필터는 BuntState를 받지 못한다
    locked: Arc<AtomicBool>,
}

impl ClipboardAccess {
    pub fn new(dh: &DisplayHandle) -> Self {
        let locked = Arc::new(AtomicBool::new(false));
        // 프로토콜의 규칙: 보안 컨텍스트로 붙은 클라이언트는 이 프로토콜로 제 제한을 벗어나지 못해야 한다
        SecurityContextState::new::<BuntState, _>(dh, |client: &Client| {
            !client
                .get_data::<ClientState>()
                .is_some_and(|data| data.sandboxed)
        });
        Self {
            ext: ext_data_control::DataControlState::new::<BuntState, _>(
                dh,
                None,
                filter(locked.clone()),
            ),
            wlr: wlr_data_control::DataControlState::new::<BuntState, _>(
                dh,
                None,
                filter(locked.clone()),
            ),
            locked,
        }
    }

    pub fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::Relaxed);
    }
}

fn filter(locked: Arc<AtomicBool>) -> impl for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static {
    move |client| {
        may_use(
            client.get_data::<ClientState>(),
            locked.load(Ordering::Relaxed),
        )
    }
}

/// 이 클라이언트에 클립보드 프로토콜을 보일지. 모듈 설명을 본다
fn may_use(client: Option<&ClientState>, locked: bool) -> bool {
    !locked && client.is_some_and(|data| !data.sandboxed && !data.input_method && !data.lock_screen)
}

impl ext_data_control::DataControlHandler for BuntState {
    fn data_control_state(&self) -> &ext_data_control::DataControlState {
        &self.clipboard_access.ext
    }
}

impl wlr_data_control::DataControlHandler for BuntState {
    fn data_control_state(&self) -> &wlr_data_control::DataControlState {
        &self.clipboard_access.wlr
    }
}

/// 샌드박스(Flatpak 등)가 앱에 줄 소켓을 만들었다. 그 소켓으로 붙는 클라이언트는 모두 샌드박스 앱이다
impl SecurityContextHandler for BuntState {
    fn context_created(&mut self, source: SecurityContextListenerSource, context: SecurityContext) {
        tracing::debug!(
            "security context: {:?} {:?}",
            context.sandbox_engine,
            context.app_id
        );
        let inserted =
            self.loop_handle
                .insert_source(source, |stream, _, state: &mut BuntState| {
                    let client = ClientState {
                        sandboxed: true,
                        ..ClientState::default()
                    };
                    if let Err(err) = state.display_handle.insert_client(stream, Arc::new(client)) {
                        tracing::warn!("failed to add a sandboxed client: {err}");
                    }
                });
        if let Err(err) = inserted {
            tracing::warn!("failed to listen for sandboxed clients: {}", err.error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ordinary_clients_may_use_the_clipboard_protocols() {
        let client = |change: fn(&mut ClientState)| {
            let mut data = ClientState::default();
            change(&mut data);
            data
        };
        let ordinary = ClientState::default();
        let shell = client(|data| data.privileged = true);
        assert!(may_use(Some(&ordinary), false));
        assert!(may_use(Some(&shell), false));
        for refused in [
            client(|data| data.sandboxed = true),
            client(|data| data.input_method = true),
            client(|data| data.lock_screen = true),
        ] {
            assert!(!may_use(Some(&refused), false));
        }
        // 모르는 클라이언트와 잠긴 동안의 새 연결
        assert!(!may_use(None, false));
        assert!(!may_use(Some(&ordinary), true));
        assert!(!may_use(Some(&shell), true));
    }
}
