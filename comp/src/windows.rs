// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창 다루기 단축키: 앱 전환(⌘⇥), 같은 앱의 윈도우 전환(⌘`), 앱 종료(⌘Q), 최소화(⌘M과 창의 최소화
//! 단추), 가리기(⌘H, ⌥⌘H), 전체 화면(⌃⌘F). 최소화하거나 가린 창은 공간에서 내려 두었다가, 그 창을
//! 활성화하면(Dock, ⌘⇥, 에이전트) 제자리에 다시 놓는다. 앱은 같은 연결(클라이언트)의 창들이다.
//! 최소화한 창은 내리기 전에 그림을 떠 두고 셸에 알린다. 셸이 Dock의 최소화 칸에 늘어놓는다.

use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use smithay::{
    desktop::{PopupKind, PopupManager, Window},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Resource, backend::ClientId},
    },
    utils::{Logical, Point, SERIAL_COUNTER},
};

use crate::{
    dbus::{Command, MinimizedInfo},
    screenshot::Image,
    shortcuts::SwitcherKey,
    state::BuntState,
    toplevels::title_and_app_id,
};

/// Dock의 최소화 칸에 보일 창 그림의 긴 변(물리 픽셀). 가장 큰 Dock 아이콘(96px)을 2배 배율로
/// 그려도 흐려지지 않는다
const THUMBNAIL_SIZE: u32 = 256;
/// 창 그림의 비치는 부분(둥근 모서리)은 비운다
const TRANSPARENT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
/// 최소화한 창의 번호. 셸이 이 번호로 되돌려 달라고 한다
static NEXT_MINIMIZED_ID: AtomicU32 = AtomicU32::new(1);

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
    /// 최소화한 창의 번호와 내리기 전에 뜬 그림 (가린 창은 없다)
    minimized: Option<(u32, Option<Arc<Image>>)>,
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
        let minimized = self
            .hidden
            .iter()
            .any(|hidden| hidden.window == *window && hidden.minimized.is_some());
        self.hidden.retain(|hidden| hidden.window != *window);
        if minimized {
            self.publish_minimized();
        }
        if let Some(switching) = &mut self.switching {
            let before = switching.order.len();
            switching.order.retain(|other| other != window);
            if switching.order.is_empty() {
                self.switching = None;
            } else {
                switching.index = switching.index.min(switching.order.len() - 1);
            }
            if before != self.switching.as_ref().map_or(0, |s| s.order.len()) {
                self.publish_switcher();
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
        let minimized = restore.iter().any(|hidden| hidden.minimized.is_some());
        for hidden in restore {
            // 최소화한 창은 칸에서 나와 제자리로 가는 효과를 그린다. 창은 바로 놓여 입력을 받는다 (minimize.rs)
            if let Some((id, _)) = &hidden.minimized {
                self.start_restore_effect(&hidden.window, *id, hidden.location);
            }
            set_hidden(&hidden.window, false);
            self.space
                .map_element(hidden.window.clone(), hidden.location, false);
            self.refresh_window_info(&hidden.window);
        }
        if minimized {
            self.publish_minimized();
        }
    }

    /// 셸이 Dock의 최소화 칸에서 누른 창을 제자리에 놓고 포커스를 준다
    pub fn restore_minimized(&mut self, id: u32) {
        let window = self
            .hidden
            .iter()
            .find(|hidden| {
                hidden
                    .minimized
                    .as_ref()
                    .is_some_and(|(other, _)| *other == id)
            })
            .map(|hidden| hidden.window.clone());
        if let Some(window) = window {
            self.focus_window(&window, SERIAL_COUNTER.next_serial());
        }
    }

    /// 최소화한 창 목록을 D-Bus 사본에 적고 셸에 알린다 (먼저 내린 창이 앞)
    pub fn publish_minimized(&self) {
        let minimized: Vec<MinimizedInfo> = self
            .hidden
            .iter()
            .filter_map(|hidden| {
                let (id, thumbnail) = hidden.minimized.clone()?;
                let (title, app_id) = title_and_app_id(&hidden.window);
                Some(MinimizedInfo {
                    id,
                    app_id,
                    title,
                    thumbnail,
                })
            })
            .collect();
        self.dbus.update(|snapshot| snapshot.minimized = minimized);
        self.dbus.send(Command::MinimizedWindowsChanged);
    }

    /// Dock에 보일 창 그림. 창만 따로 그려 긴 변을 THUMBNAIL_SIZE로 줄인다
    fn thumbnail(&mut self, window: &Window) -> Option<Arc<Image>> {
        match self.capture_window(window, TRANSPARENT) {
            Ok((image, _)) => Some(Arc::new(shrink(image, THUMBNAIL_SIZE))),
            Err(err) => {
                tracing::warn!("cannot draw the minimized window: {err}");
                None
            }
        }
    }

    /// 창을 공간에서 내린다. 그 창의 팝업 메뉴는 닫는다
    fn hide_window(&mut self, window: &Window, kind: Hidden) {
        let Some(location) = self.space.element_location(window) else {
            return;
        };
        // 팝업을 닫고 비활성으로 바꾸기 전, 화면에 보이던 그대로 뜬다
        let minimized = (kind == Hidden::Minimized).then(|| {
            (
                NEXT_MINIMIZED_ID.fetch_add(1, Ordering::Relaxed),
                self.thumbnail(window),
            )
        });
        // 최소화 효과도 같은 모습에서 칸으로 빨려 든다 (minimize.rs)
        if let Some((id, _)) = &minimized {
            self.start_minimize_effect(window, *id);
        }
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
        let publish = minimized.is_some();
        self.hidden.push(HiddenWindow {
            window: window.clone(),
            location,
            kind,
            minimized,
        });
        self.refresh_window_info(window);
        if publish {
            self.publish_minimized();
        }
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

    /// ⌘M, 창의 최소화 단추(xdg_toplevel.set_minimized), 창 목록(셸)의 최소화 요청
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

    /// ⌘⇥(`step` 1), ⇧⌘⇥(-1): 최근에 쓴 차례로 다른 앱을 고른다. 셸이 앱 전환 창을 띄우고, ⌘를 떼면
    /// 고른 앱의 가장 최근 창을 앞으로 가져온다(macOS). 가린 앱도 차례에 들고, 고르면 돌아온다.
    /// ⌘를 누른 채 거듭 누르면 그다음 앱을 고른다
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

    /// 다음(앞) 앱이나 윈도우를 고른다. 윈도우는 바로 앞으로 가져오고, 앱은 전환 창에서 칠만 한다
    fn step_switching(&mut self, step: isize) {
        let Some(switching) = &mut self.switching else {
            return;
        };
        let len = switching.order.len() as isize;
        switching.index = (switching.index as isize + step).rem_euclid(len) as usize;
        if switching.windows {
            let window = switching.order[switching.index].clone();
            self.focus_window(&window, SERIAL_COUNTER.next_serial());
        } else {
            self.publish_switcher();
        }
    }

    /// ⌘를 떼면 전환을 마친다. 앱 전환이면 전환 창을 숨기고 고른 앱을 앞으로 가져온다. 마지막으로
    /// 고른 창이 최근 순서 맨 앞이 된다
    pub fn finish_switching(&mut self) {
        let Some(switching) = self.switching.take() else {
            return;
        };
        if !switching.windows {
            self.publish_switcher();
            if let Some(window) = switching.order.get(switching.index) {
                self.focus_window(&window.clone(), SERIAL_COUNTER.next_serial());
                return;
            }
        }
        if let Some(window) = self.active_window() {
            self.note_focus(&window);
        }
    }

    /// 앱 전환 창이 떠 있다 (⌘를 누른 채 ⌘⇥로 앱을 고르는 중)
    pub fn switching_apps(&self) -> bool {
        self.switching
            .as_ref()
            .is_some_and(|switching| !switching.windows)
    }

    /// 앱 전환 창의 키: Esc는 바꾸지 않고 닫고, Q는 고른 앱을 끝내고(목록에서 뺀다), H는 고른 앱을
    /// 가리고(목록에 남는다), ←·→·`는 앞뒤 앱을 고른다
    pub fn switcher_key_pressed(&mut self, key: SwitcherKey) {
        let Some(selected) = self
            .switching
            .as_ref()
            .filter(|switching| !switching.windows)
            .and_then(|switching| switching.order.get(switching.index).cloned())
        else {
            return;
        };
        let client = client_of(&selected);
        match key {
            SwitcherKey::Cancel => {
                self.switching = None;
                self.publish_switcher();
            }
            SwitcherKey::Step(step) => self.step_switching(step),
            SwitcherKey::Quit => {
                for window in self.all_windows() {
                    if client_of(window) == client
                        && let Some(toplevel) = window.toplevel()
                    {
                        toplevel.send_close();
                    }
                }
                if let Some(switching) = &mut self.switching {
                    switching.order.retain(|window| client_of(window) != client);
                    if switching.order.is_empty() {
                        self.switching = None;
                    } else {
                        switching.index = switching.index.min(switching.order.len() - 1);
                    }
                }
                self.publish_switcher();
            }
            SwitcherKey::Hide => {
                let had_focus = self
                    .active_window()
                    .is_some_and(|active| client_of(&active) == client);
                let windows: Vec<Window> = self
                    .space
                    .elements()
                    .filter(|window| client_of(window) == client)
                    .cloned()
                    .collect();
                for window in &windows {
                    self.hide_window(window, Hidden::App);
                }
                if had_focus {
                    self.focus_next_shown(None);
                }
            }
        }
    }

    /// 셸의 앱 전환 창에서 포인터로 가리킨(`activate`면 누른) 앱. 누르면 전환을 마치고 그 앱으로 간다
    pub fn select_switcher(&mut self, index: u32, activate: bool) {
        let Some(switching) = self
            .switching
            .as_mut()
            .filter(|switching| !switching.windows)
        else {
            return;
        };
        if index as usize >= switching.order.len() {
            return;
        }
        switching.index = index as usize;
        if activate {
            self.finish_switching();
        } else {
            self.publish_switcher();
        }
    }

    /// 앱 전환 창에 늘어놓을 앱(앱 ID, 최근 차례)과 고른 앱을 셸에 알린다. 전환 중이 아니면 숨긴다(-1)
    fn publish_switcher(&self) {
        let (apps, selected) = match &self.switching {
            Some(switching) if !switching.windows => (
                switching
                    .order
                    .iter()
                    .map(|window| title_and_app_id(window).1)
                    .collect(),
                switching.index as i32,
            ),
            _ => (Vec::new(), -1),
        };
        self.dbus.send(Command::AppSwitcher { apps, selected });
    }
}

/// 그림을 긴 변이 `max` 픽셀 이하가 되게 줄인다. 작은 그림은 그대로 둔다. 줄인 픽셀은 그 자리에 든
/// 원래 픽셀의 평균이다 (그린 그림은 알파를 미리 곱해 두었으므로 그대로 평균 내도 된다)
fn shrink(image: Image, max: u32) -> Image {
    let longest = image.width.max(image.height);
    if longest <= max || image.width == 0 || image.height == 0 {
        return image;
    }
    let width = (image.width * max / longest).max(1);
    let height = (image.height * max / longest).max(1);
    // 줄인 픽셀 i가 덮는 원래 픽셀 범위 (빈 범위가 없게 적어도 하나)
    let span = |i: u32, from: u32, to: u32| {
        let start = i * from / to;
        (start, ((i + 1) * from / to).max(start + 1))
    };
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let (top, bottom) = span(y, image.height, height);
        for x in 0..width {
            let (left, right) = span(x, image.width, width);
            let mut sum = [0u32; 4];
            for row in top..bottom {
                let start = (row * image.width + left) as usize * 4;
                let end = (row * image.width + right) as usize * 4;
                for pixel in image.rgba[start..end].as_chunks::<4>().0 {
                    for (total, channel) in sum.iter_mut().zip(pixel) {
                        *total += u32::from(*channel);
                    }
                }
            }
            let count = (bottom - top) * (right - left);
            rgba.extend(sum.map(|total| ((total + count / 2) / count) as u8));
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, rgba: Vec<u8>) -> Image {
        Image {
            width,
            height,
            rgba,
        }
    }

    #[test]
    fn thumbnails_average_the_pixels_they_cover() {
        // 4×2를 긴 변 2로: 왼쪽 2×2와 오른쪽 2×2의 평균
        let mut rgba = Vec::new();
        for value in [0u8, 100, 200, 255, 0, 100, 200, 255] {
            rgba.extend([value, value, value, 255]);
        }
        let small = shrink(image(4, 2, rgba), 2);
        assert_eq!((small.width, small.height), (2, 1));
        assert_eq!(small.rgba, vec![50, 50, 50, 255, 228, 228, 228, 255]);
    }

    #[test]
    fn small_images_stay_as_they_are() {
        let small = shrink(image(2, 1, vec![1; 8]), 256);
        assert_eq!((small.width, small.height, small.rgba), (2, 1, vec![1; 8]));
        // 아주 납작한 그림도 높이가 0이 되지 않는다
        let flat = shrink(image(1000, 1, vec![9; 4000]), 10);
        assert_eq!((flat.width, flat.height), (10, 1));
        assert_eq!(flat.rgba.len(), 40);
    }
}
