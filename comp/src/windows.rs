// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창 다루기 단축키: 앱 전환(⌘⇥), 같은 앱의 윈도우 전환(⌘`), 앱 종료(⌘Q), 최소화(⌘M), 가리기
//! (⌘H, ⌥⌘H), 전체 화면(⌃⌘F). 최소화하거나 가린 창은 공간에서 내려 두었다가, 그 창을 활성화하면
//! (Dock, ⌘⇥, 에이전트) 제자리에 다시 놓는다. 앱은 같은 연결(클라이언트)의 창들이다.

use std::cell::Cell;

use smithay::{
    desktop::{PopupKind, PopupManager, Window},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Resource, backend::ClientId},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::state::BuntState;

/// 창을 공간에서 내린 까닭
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hidden {
    /// ⌘M: 이 창만 내렸다
    Minimized,
    /// ⌘H, ⌥⌘H: 앱의 창을 모두 내렸다. 그중 하나를 활성화하면 함께 돌아온다
    App,
}

/// 공간에서 내린 창과 돌아올 자리
pub struct HiddenWindow {
    pub window: Window,
    pub location: Point<i32, Logical>,
    pub kind: Hidden,
}

/// ⌘를 누른 채 ⇥나 `를 거듭 누르는 동안의 전환. ⌘를 떼면 끝난다
pub struct Switching {
    /// 같은 앱의 윈도우를 돈다 (아니면 앱을 돈다)
    windows: bool,
    /// 전환을 시작할 때의 최근 순서 (앱 전환은 앱마다 가장 최근 창 하나)
    order: Vec<Window>,
    index: usize,
}

/// 창의 사용자 데이터: 공간에서 내렸는지. 창 목록(셸)에는 최소화로 알린다
#[derive(Default)]
struct HiddenMark(Cell<bool>);

pub fn is_hidden(window: &Window) -> bool {
    window
        .user_data()
        .get::<HiddenMark>()
        .is_some_and(|mark| mark.0.get())
}

fn set_hidden(window: &Window, hidden: bool) {
    window
        .user_data()
        .get_or_insert(HiddenMark::default)
        .0
        .set(hidden);
}

/// 창을 연 앱(클라이언트)
pub fn client_of(window: &Window) -> Option<ClientId> {
    window
        .toplevel()?
        .wl_surface()
        .client()
        .map(|client| client.id())
}

impl BuntState {
    /// 공간에 보이는 창과 내린 창 모두
    pub fn all_windows(&self) -> impl Iterator<Item = &Window> {
        self.space
            .elements()
            .chain(self.hidden.iter().map(|hidden| &hidden.window))
    }

    fn is_shown(&self, window: &Window) -> bool {
        self.space.elements().any(|shown| shown == window)
    }

    /// 포커스를 받은 창을 최근 순서 맨 앞에 둔다. 앱·윈도우를 전환하는 중에는 ⌘를 뗄 때 한다
    pub fn note_focus(&mut self, window: &Window) {
        if self.switching.is_some() {
            return;
        }
        self.recent.retain(|other| other != window);
        self.recent.insert(0, window.clone());
    }

    /// 닫힌 창을 최근 순서, 내린 창, 진행 중인 전환에서 지운다
    pub fn forget_recent(&mut self, window: &Window) {
        self.recent.retain(|other| other != window);
        self.hidden.retain(|hidden| hidden.window != *window);
        if let Some(switching) = &mut self.switching {
            switching.order.retain(|other| other != window);
            if switching.order.is_empty() {
                self.switching = None;
            } else {
                switching.index = switching.index.min(switching.order.len() - 1);
            }
        }
    }

    /// 내린 창을 제자리에 다시 놓는다. 가린 앱의 창이면 그 앱의 가린 창을 모두 놓는다
    pub fn restore_hidden(&mut self, window: &Window) {
        let Some(kind) = self
            .hidden
            .iter()
            .find(|hidden| hidden.window == *window)
            .map(|hidden| hidden.kind)
        else {
            return;
        };
        let client = client_of(window);
        let (restore, keep): (Vec<HiddenWindow>, Vec<HiddenWindow>) =
            std::mem::take(&mut self.hidden)
                .into_iter()
                .partition(|hidden| match kind {
                    Hidden::Minimized => hidden.window == *window,
                    Hidden::App => {
                        hidden.kind == Hidden::App && client_of(&hidden.window) == client
                    }
                });
        self.hidden = keep;
        for hidden in restore {
            set_hidden(&hidden.window, false);
            self.space
                .map_element(hidden.window.clone(), hidden.location, false);
            self.refresh_window_info(&hidden.window);
        }
    }

    /// 창을 공간에서 내린다. 그 창의 팝업 메뉴는 닫는다
    fn hide_window(&mut self, window: &Window, kind: Hidden) {
        let Some(location) = self.space.element_location(window) else {
            return;
        };
        if let Some(toplevel) = window.toplevel() {
            for (popup, _) in PopupManager::popups_for_surface(toplevel.wl_surface()) {
                if let PopupKind::Xdg(popup) = popup {
                    popup.send_popup_done();
                }
            }
            window.set_activated(false);
            toplevel.send_pending_configure();
        }
        self.space.unmap_elem(window);
        set_hidden(window, true);
        self.hidden.push(HiddenWindow {
            window: window.clone(),
            location,
            kind,
        });
        self.refresh_window_info(window);
    }

    /// 창을 내린 뒤 포커스를 줄 창: 최근 순서로 보이는 창 (`prefer` 앱의 창이 먼저)
    fn focus_next_shown(&mut self, prefer: Option<ClientId>) {
        let shown: Vec<Window> = self
            .recent
            .iter()
            .filter(|window| self.is_shown(window))
            .cloned()
            .collect();
        let next = prefer
            .and_then(|client| {
                shown
                    .iter()
                    .find(|window| client_of(window).as_ref() == Some(&client))
            })
            .or(shown.first())
            .cloned()
            .or_else(|| self.space.elements().last().cloned());
        let serial = SERIAL_COUNTER.next_serial();
        match next {
            Some(window) => self.focus_window(&window, serial),
            None => self.clear_focus(serial),
        }
    }

    /// ⌘M, 창 목록(셸)의 최소화 요청
    pub fn minimize(&mut self, window: &Window) {
        if !self.is_shown(window) {
            return;
        }
        self.hide_window(window, Hidden::Minimized);
        self.focus_next_shown(client_of(window));
    }

    pub fn minimize_active(&mut self) {
        if let Some(window) = self.active_window() {
            self.minimize(&window);
        }
    }

    /// ⌘H: 활성 앱의 창을 모두 가리고 다음 앱에 포커스를 준다
    pub fn hide_active_app(&mut self) {
        let Some(active) = self.active_window() else {
            return;
        };
        let client = client_of(&active);
        let windows: Vec<Window> = self
            .space
            .elements()
            .filter(|window| client_of(window) == client)
            .cloned()
            .collect();
        for window in &windows {
            self.hide_window(window, Hidden::App);
        }
        self.focus_next_shown(None);
    }

    /// ⌥⌘H: 활성 앱이 아닌 앱의 창을 모두 가린다
    pub fn hide_other_apps(&mut self) {
        let Some(active) = self.active_window() else {
            return;
        };
        let client = client_of(&active);
        let windows: Vec<Window> = self
            .space
            .elements()
            .filter(|window| client_of(window) != client)
            .cloned()
            .collect();
        for window in &windows {
            self.hide_window(window, Hidden::App);
        }
    }

    /// ⌘Q: 활성 앱의 창(내린 창 포함)을 모두 닫는다. 앱은 마지막 창이 닫히면 끝난다
    pub fn quit_active_app(&mut self) {
        let Some(active) = self.active_window() else {
            return;
        };
        let client = client_of(&active);
        for window in self.all_windows() {
            if client_of(window) == client
                && let Some(toplevel) = window.toplevel()
            {
                toplevel.send_close();
            }
        }
    }

    /// ⌃⌘F
    pub fn toggle_fullscreen(&mut self) {
        let Some(toplevel) = self
            .active_window()
            .and_then(|window| window.toplevel().cloned())
        else {
            return;
        };
        let on = !Self::has_state(&toplevel, xdg_toplevel::State::Fullscreen);
        self.set_window_state(&toplevel, xdg_toplevel::State::Fullscreen, on);
    }

    /// ⌘⇥(`step` 1), ⇧⌘⇥(-1): 최근에 쓴 차례로 다른 앱의 가장 최근 창을 앞으로 가져온다. 가린 앱도
    /// 차례에 들고, 고르면 돌아온다. ⌘를 누른 채 거듭 누르면 그다음 앱으로 간다
    pub fn switch_app(&mut self, step: isize) {
        if !self
            .switching
            .as_ref()
            .is_some_and(|switching| !switching.windows)
        {
            let mut order: Vec<Window> = Vec::new();
            for window in &self.recent {
                let client = client_of(window);
                if !order.iter().any(|other| client_of(other) == client) {
                    order.push(window.clone());
                }
            }
            // 앱이 하나뿐이면 그 앱이 가려져 있을 때만 할 일이 있다
            if order.len() == 1 && !self.is_shown(&order[0]) {
                self.focus_window(&order[0].clone(), SERIAL_COUNTER.next_serial());
                return;
            }
            if order.len() < 2 {
                return;
            }
            self.switching = Some(Switching {
                windows: false,
                order,
                index: 0,
            });
        }
        self.step_switching(step);
    }

    /// ⌘`(`step` 1), ⇧⌘`(-1): 활성 앱의 보이는 창을 최근에 쓴 차례로 돈다
    pub fn switch_window(&mut self, step: isize) {
        if !self
            .switching
            .as_ref()
            .is_some_and(|switching| switching.windows)
        {
            let Some(active) = self.active_window() else {
                return;
            };
            let client = client_of(&active);
            let order: Vec<Window> = self
                .recent
                .iter()
                .filter(|window| client_of(window) == client && self.is_shown(window))
                .cloned()
                .collect();
            if order.len() < 2 {
                return;
            }
            self.switching = Some(Switching {
                windows: true,
                order,
                index: 0,
            });
        }
        self.step_switching(step);
    }

    fn step_switching(&mut self, step: isize) {
        let Some(switching) = &mut self.switching else {
            return;
        };
        let len = switching.order.len() as isize;
        switching.index = (switching.index as isize + step).rem_euclid(len) as usize;
        let window = switching.order[switching.index].clone();
        self.focus_window(&window, SERIAL_COUNTER.next_serial());
    }

    /// ⌘를 떼면 전환을 마치고, 마지막으로 고른 창을 최근 순서 맨 앞에 둔다
    pub fn finish_switching(&mut self) {
        if self.switching.take().is_none() {
            return;
        }
        if let Some(window) = self.active_window() {
            self.note_focus(&window);
        }
    }
}
