// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 스크린샷 (macOS의 ⇧⌘3, ⇧⌘4와 ⌃를 더한 클립보드 복사, ⇧⌘5 도구 막대). 포인터가 있는 화면 전체나
//! 끌어서 고른 영역은 커서를 뺀 같은 장면을 따로 그려 읽고(udev.rs의 render_output), 고른 창 하나는 그
//! 창만 따로 그린다(둥근 모서리는 비운다). 파일은 PNG로 저장 위치(⇧⌘5의 옵션, 기본은 사진 폴더)에
//! 저장하고, 클립보드에는 image/png로 넣는다. 다 되면 셸이 화면 모서리에 미리 보기를 띄우게
//! 알리거나(플로팅 썸네일) 알림을 띄운다. 영역을 고르는 동안 포인터는 십자선이 되고 앱은 포인터와
//! 키를 받지 않는다. 스페이스는 창 고르기로 바꾸고, Esc나 다른 단추로 그만둔다.
//! ⇧⌘5 도구 막대의 선택 부분은 macOS처럼 끌었다 떼도 찍지 않는다. 고칠 수 있는 선택 영역(selection.rs)을
//! 마지막 자리에 띄워 두고, 끌어 옮기거나 크기를 고친 뒤 도구 막대의 캡처(Enter)가 그 영역을 찍는다.
//! 그동안 도구 막대는 영역 위에 남아 포인터와 키를 받는다. 마지막 영역은 다음 ⇧⌘5를 위해 기억한다.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use bunt_i18n::{FluentArgs, Sources, Translations};
use smithay::{
    backend::{
        allocator::Fourcc,
        input::ButtonState,
        renderer::{
            Bind, ExportMem, Offscreen,
            damage::OutputDamageTracker,
            element::{
                Element, Id, Kind,
                solid::{SolidColorBuffer, SolidColorRenderElement},
            },
            gles::{GlesRenderer, GlesTexture},
        },
    },
    desktop::{PopupManager, Window, layer_map_for_output},
    input::pointer::{CursorIcon, CursorImageStatus},
    output::Output,
    reexports::{
        calloop::timer::{TimeoutAction, Timer},
        wayland_server::{Resource, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Point, Rectangle, Scale, Size, Transform},
    wayland::{
        compositor::{TraversalAction, with_surface_tree_downward},
        selection::data_device::set_data_device_selection,
    },
};

use crate::{
    dbus::Command,
    render::OutputRenderElements,
    selection::{self, Drag, Grip, Overlay},
    state::{BuntState, ClientState},
    texture_shader,
};

pub(crate) const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../i18n/ko/bunt-comp.ftl")),
    ("en", include_str!("../../i18n/en/bunt-comp.ftl")),
];

/// 클립보드에 넣는 형식
pub const PNG_MIME: &str = "image/png";
/// linux 왼쪽 단추
const BUTTON_LEFT: u32 = 0x110;
/// 고르는 영역의 채움과 테두리 (미리 곱한 RGBA). macOS처럼 반투명 회색에 흰 테두리
const SELECTION_FILL: [f32; 4] = [0.15, 0.15, 0.15, 0.3];
const SELECTION_BORDER: [f32; 4] = [0.9, 0.9, 0.9, 0.9];
/// 창 고르기에서 가리킨 창을 덮는 색 (미리 곱한 RGBA). macOS처럼 옅은 파랑
const WINDOW_TINT: [f32; 4] = [0.05, 0.13, 0.27, 0.27];
/// 창 찍기의 비치는 부분(둥근 모서리)은 비운다
const TRANSPARENT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];
/// ⇧⌘5 도구 막대의 레이어 표면 이름 (shell/src/screenshot.rs). 고칠 수 있는 선택 영역은 이 막대 아래에
/// 그리고, 막대 위의 포인터는 막대에 보낸다
pub const TOOLBAR_NAMESPACE: &str = "bunt-screenshot";
/// 선택 영역을 띄운 뒤 도구 막대가 나타나기를 기다리는 시간. 그때까지 막대가 없으면(셸이 띄우라고 한 뒤 막대가
/// 사라졌다) 영역을 거둔다. 막대 없이 남은 영역은 앱의 포인터를 막는다
const TOOLBAR_WAIT: Duration = Duration::from_secs(1);
/// 도구 막대가 사라진 뒤 거두기까지 기다리는 시간. 그사이 막대가 다시 뜨면(⇧⌘5를 빨리 두 번) 그대로 둔다
const TOOLBAR_GONE_WAIT: Duration = Duration::from_millis(300);

/// 무엇을 찍는가 (⇧⌘5 도구 막대의 방식)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 포인터가 있는 화면 전체
    Screen,
    /// 끌어서 고른 영역
    Area,
    /// 눌러서 고른 창 하나
    Window,
}

impl Mode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "screen" => Some(Self::Screen),
            "area" => Some(Self::Area),
            "window" => Some(Self::Window),
            _ => None,
        }
    }
}

/// 찍은 것을 둘 곳 (⇧⌘5 옵션의 "저장 위치")
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Location {
    #[default]
    Pictures,
    Desktop,
    Documents,
    Clipboard,
}

impl Location {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "pictures" => Some(Self::Pictures),
            "desktop" => Some(Self::Desktop),
            "documents" => Some(Self::Documents),
            "clipboard" => Some(Self::Clipboard),
            _ => None,
        }
    }
}

/// 셸이 알린 스크린샷 옵션 (셸 설정의 screenshot-location, screenshot-thumbnail)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub location: Location,
    /// 저장한 뒤 셸이 화면 모서리에 미리 보기를 띄운다(플로팅 썸네일). 끄면 알림을 띄운다
    pub thumbnail: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            location: Location::Pictures,
            thumbnail: true,
        }
    }
}

/// 다음 프레임에 찍을 스크린샷
pub struct Capture {
    pub output: Output,
    /// 출력 안의 논리 좌표 영역. 없으면 화면 전체
    pub area: Option<Rectangle<i32, Logical>>,
    pub clipboard: bool,
}

/// 찍은 화면: RGBA 픽셀, 위 줄부터
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// ⇧⌘4로 영역이나 창을 고르는 중. ⇧⌘5의 선택 부분이면 고칠 수 있는 선택 영역을 띄워 둔 중이다
pub struct AreaSelection {
    clipboard: bool,
    /// 창 고르기(⇧⌘4 뒤 스페이스, ⇧⌘5의 선택한 윈도우): 가리킨 창을 칠하고 누르면 그 창을 찍는다
    window: bool,
    /// 창 고르기에서 포인터 아래의 창과 그 화면 영역
    hovered: Option<(Window, Rectangle<i32, Logical>)>,
    /// 고른 뒤 찍기 전에 기다리는 초 (⇧⌘5의 타이머)
    delay: u32,
    /// 누른 자리. 누르기 전이면 None
    start: Option<Point<f64, Logical>>,
    current: Point<f64, Logical>,
    /// 고르기 전의 커서. 마치면 되돌린다
    cursor: CursorImageStatus,
    fill: SolidColorBuffer,
    /// 위, 아래, 왼쪽, 오른쪽 테두리
    border: [SolidColorBuffer; 4],
    /// 창 고르기에서 가리킨 창을 덮는 색
    tint: SolidColorBuffer,
    /// ⇧⌘5의 선택 부분: 끌어서 고치는 영역. 떼도 찍지 않고 도구 막대의 캡처가 찍는다
    editable: Option<Editable>,
}

/// ⇧⌘5의 고칠 수 있는 선택 영역
struct Editable {
    rect: Rectangle<i32, Logical>,
    /// 영역이 놓인 화면. 옮기고 늘여도 이 밖으로 나가지 않는다
    bounds: Rectangle<i32, Logical>,
    /// 누른 채 끄는 중과, 그 끌기가 영역을 둘 화면. 바깥을 눌러 새로 고르면 누른 화면이고, 새 영역을 고른
    /// 뒤에야 `bounds`가 된다
    drag: Option<(Drag, Rectangle<i32, Logical>)>,
    overlay: Overlay,
}

impl AreaSelection {
    fn new(
        clipboard: bool,
        window: bool,
        delay: u32,
        current: Point<f64, Logical>,
        cursor: CursorImageStatus,
    ) -> Self {
        Self {
            clipboard,
            window,
            hovered: None,
            delay,
            start: None,
            current,
            cursor,
            fill: SolidColorBuffer::new((0, 0), SELECTION_FILL),
            border: std::array::from_fn(|_| SolidColorBuffer::new((0, 0), SELECTION_BORDER)),
            tint: SolidColorBuffer::new((0, 0), WINDOW_TINT),
            editable: None,
        }
    }

    /// 키를 컴포지터가 받는지. ⇧⌘5의 선택 영역을 띄운 동안에는 도구 막대가 받는다(Enter는 찍고 Esc는 닫는다)
    pub fn takes_keys(&self) -> bool {
        self.editable.is_none()
    }

    /// 화면 하나(`output`, 전역 논리 좌표)에 그릴 요소 (앞의 것부터). 전에는 udev.rs가 layout의 버퍼로
    /// 요소를 만들었고, ⇧⌘5의 선택 영역이 손잡이 그림을 쓰면서 여기서 만든다
    pub fn elements(
        &mut self,
        renderer: &mut GlesRenderer,
        output: Rectangle<i32, Logical>,
        scale: Scale<f64>,
    ) -> Vec<OutputRenderElements> {
        if let Some(editable) = &mut self.editable {
            return editable
                .overlay
                .elements(renderer, editable.rect, output, scale);
        }
        self.layout()
            .into_iter()
            .flatten()
            .map(|(location, buffer)| {
                let location = (location - output.loc).to_physical_precise_round(scale);
                OutputRenderElements::Solid(SolidColorRenderElement::from_buffer(
                    buffer,
                    location,
                    scale,
                    1.0,
                    Kind::Unspecified,
                ))
            })
            .collect()
    }

    /// 화면 좌표의 고른 영역. 누르기 전이면 None
    fn rect(&self) -> Option<Rectangle<i32, Logical>> {
        let start = self.start?;
        let (left, top) = (
            start.x.min(self.current.x).floor(),
            start.y.min(self.current.y).floor(),
        );
        let (right, bottom) = (
            start.x.max(self.current.x).ceil(),
            start.y.max(self.current.y).ceil(),
        );
        Some(Rectangle::new(
            (left as i32, top as i32).into(),
            ((right - left) as i32, (bottom - top) as i32).into(),
        ))
    }

    /// 영역의 테두리와 채움을 지금 크기로 맞추고 화면 좌표의 자리와 함께 앞의 것부터 돌려준다.
    /// 누르기 전이면 None. 창 고르기면 가리킨 창을 덮는 색 하나다
    fn layout(&mut self) -> Option<Vec<(Point<i32, Logical>, &SolidColorBuffer)>> {
        if self.window {
            let rect = self.hovered.as_ref()?.1;
            self.tint.resize(rect.size);
            return Some(vec![(rect.loc, &self.tint)]);
        }
        let rect = self.rect()?;
        let Size { w, h, .. } = rect.size;
        let places: [(Point<i32, Logical>, (i32, i32)); 4] = [
            (rect.loc, (w, 1)),
            (rect.loc + Point::from((0, h - 1)), (w, 1)),
            (rect.loc, (1, h)),
            (rect.loc + Point::from((w - 1, 0)), (1, h)),
        ];
        for (buffer, (_, size)) in self.border.iter_mut().zip(places) {
            buffer.resize(size);
        }
        self.fill.resize(rect.size);
        let mut parts: Vec<(Point<i32, Logical>, &SolidColorBuffer)> = self
            .border
            .iter()
            .zip(places)
            .map(|(buffer, (place, _))| (place, buffer))
            .collect();
        parts.push((rect.loc, &self.fill));
        Some(parts)
    }
}

impl BuntState {
    /// 스크린샷 단축키. 영역이면 고르기를 시작하고, 아니면 포인터가 있는 화면을 다음 프레임에 찍는다
    pub fn screenshot(&mut self, area: bool, clipboard: bool) {
        let mode = if area { Mode::Area } else { Mode::Screen };
        self.start_screenshot(mode, clipboard, 0);
    }

    /// 스크린샷을 시작한다 (단축키, ⇧⌘5 도구 막대). 화면은 `delay`초 뒤에 포인터가 있는 화면을
    /// 찍고, 영역과 창은 고르기를 시작해 고른 뒤 `delay`초 기다려 찍는다. 저장 위치가 클립보드면
    /// 클립보드에 넣는다
    pub fn start_screenshot(&mut self, mode: Mode, clipboard: bool, delay: u32) {
        // 잠긴 동안에는 찍지 않는다. 잠글 때 고르던 영역과 찍을 일은 버린다 (session_lock.rs)
        if self.session_lock.is_locked()
            || self.screenshot_area.is_some()
            || self.screenshot_capture.is_some()
        {
            return;
        }
        let clipboard = clipboard || self.screenshot_options.location == Location::Clipboard;
        if mode == Mode::Screen {
            self.after(delay, move |state| state.capture_screen(clipboard));
            return;
        }
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or_default();
        let window = mode == Mode::Window;
        let cursor = std::mem::replace(
            &mut self.cursor_status,
            CursorImageStatus::Named(selection_cursor(window)),
        );
        self.screenshot_area = Some(AreaSelection::new(
            clipboard, window, delay, pointer, cursor,
        ));
        if window {
            self.hover_window(pointer);
        }
        self.queue_redraw_all();
    }

    /// ⇧⌘5 도구 막대의 선택 부분: `show`면 고칠 수 있는 선택 영역을 마지막 자리(처음이면 포인터가 있는
    /// 화면 가운데)에 띄우고, 아니면 거둔다. 다른 고르기나 찍기가 진행 중이거나 잠긴 동안에는 띄우지 않는다
    pub fn show_screenshot_area(&mut self, show: bool) {
        if !show {
            if self
                .screenshot_area
                .as_ref()
                .is_some_and(|selection| selection.editable.is_some())
            {
                self.end_selection();
            }
            return;
        }
        if self.session_lock.is_locked()
            || self.screenshot_area.is_some()
            || self.screenshot_capture.is_some()
        {
            return;
        }
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or_default();
        let outputs: Vec<Rectangle<i32, Logical>> = self
            .space
            .outputs()
            .filter_map(|output| self.space.output_geometry(output))
            .collect();
        let Some((rect, bounds)) = selection::initial(self.screenshot_last_area, &outputs, pointer)
        else {
            return;
        };
        let cursor = self.cursor_status.clone();
        let mut selection = AreaSelection::new(false, false, 0, pointer, cursor);
        selection.editable = Some(Editable {
            rect,
            bounds,
            drag: None,
            overlay: Overlay::default(),
        });
        self.screenshot_area = Some(selection);
        let time = self.start_time.elapsed().as_millis() as u32;
        // 포인터를 앱에서 떼고 그 자리에 맞는 커서로 바꾼다 (도구 막대 위면 막대가 받는다)
        self.pointer_moved(pointer, time);
        self.queue_redraw_all();
        self.watch_screenshot_toolbar(TOOLBAR_WAIT);
    }

    /// ⇧⌘5 도구 막대의 찍기. 선택 부분은 고친 선택 영역을(막대가 숨으며 거뒀으면 기억한 영역을) `delay`초 뒤에
    /// 찍고, 영역이 없으면 ⇧⌘4처럼 끌어서 고르게 한다. 다른 방식은 start_screenshot과 같다
    pub fn toolbar_screenshot(&mut self, mode: Mode, delay: u32) {
        if mode != Mode::Area {
            self.start_screenshot(mode, false, delay);
            return;
        }
        let clipboard = self.screenshot_options.location == Location::Clipboard;
        match self.take_selected_area() {
            // 찍을 일이 이미 있으면(⇧⌘3을 막 눌렀다) 그것을 덮어쓰지 않는다 (start_screenshot과 같다)
            Some(rect)
                if !self.session_lock.is_locked()
                    && self.screenshot_area.is_none()
                    && self.screenshot_capture.is_none() =>
            {
                self.capture_area(rect, clipboard, delay)
            }
            Some(_) => {}
            None => self.start_screenshot(Mode::Area, false, delay),
        }
    }

    /// ⇧⌘5의 선택 영역 (화면 좌표): 띄워 둔 영역이면 거두고 그 영역을, 아니면(막대가 숨으며 거뒀으면)
    /// 기억한 영역을 준다. 선택 부분 캡처와 선택 부분 기록(recording.rs)이 쓴다
    pub(crate) fn take_selected_area(&mut self) -> Option<Rectangle<i32, Logical>> {
        let shown = self
            .screenshot_area
            .as_ref()
            .and_then(|selection| selection.editable.as_ref())
            .map(|editable| editable.rect);
        if shown.is_some() {
            self.end_selection();
        }
        shown.or(self.screenshot_last_area)
    }

    /// ⇧⌘5 도구 막대의 레이어 표면이 사라졌다(막대를 숨겼거나 셸이 끝났다). 막대 없이 남은 선택 영역은
    /// 앱의 포인터를 막으므로 잠시 뒤에도 막대가 없으면 거둔다 (state.rs의 layer_destroyed가 부른다). 전에는
    /// 바로 거둬서, 막대를 빨리 숨겼다 띄우면 새 막대가 뜨기 전에 사라진 옛 막대 때문에 새 영역이 거둬졌다
    pub fn screenshot_toolbar_closed(&mut self) {
        self.watch_screenshot_toolbar(TOOLBAR_GONE_WAIT);
    }

    /// `delay` 뒤에 ⇧⌘5 도구 막대 없이 남은 선택 영역을 거둔다
    fn watch_screenshot_toolbar(&mut self, delay: Duration) {
        let timer = Timer::from_duration(delay);
        if let Err(err) = self.loop_handle.insert_source(timer, |_, _, state| {
            state.check_screenshot_toolbar();
            TimeoutAction::Drop
        }) {
            tracing::warn!("failed to watch the screenshot toolbar: {err}");
        }
    }

    /// 선택 영역을 띄웠는데 셸의 도구 막대가 어느 화면에도 없으면 거둔다
    fn check_screenshot_toolbar(&mut self) {
        let editing = self
            .screenshot_area
            .as_ref()
            .is_some_and(|selection| selection.editable.is_some());
        if editing && !self.space.outputs().any(|output| toolbar(output).is_some()) {
            tracing::info!("the screenshot toolbar is gone; closing its selection");
            self.end_selection();
        }
    }

    /// 포인터가 있는 화면을 다음 프레임에 찍는다
    fn capture_screen(&mut self, clipboard: bool) {
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or_default();
        let output = self
            .space
            .output_under(pointer)
            .next()
            .or_else(|| self.space.outputs().next())
            .cloned();
        if let Some(output) = output {
            self.screenshot_capture = Some(Capture {
                output,
                area: None,
                clipboard,
            });
        }
    }

    /// `delay`초 뒤에 한다 (0이면 바로). 그때 화면이 잠겨 있으면 하지 않는다: 창 찍기는 잠긴 장면이 아니라
    /// 창을 따로 그린다. 화면 기록의 타이머도 쓴다(recording.rs)
    pub(crate) fn after(&mut self, delay: u32, then: impl FnOnce(&mut BuntState) + 'static) {
        if delay == 0 {
            if !self.session_lock.is_locked() {
                then(self);
            }
            return;
        }
        let mut then = Some(then);
        let timer = Timer::from_duration(Duration::from_secs(delay.into()));
        if let Err(err) = self.loop_handle.insert_source(timer, move |_, _, state| {
            if let Some(then) = then.take()
                && !state.session_lock.is_locked()
            {
                then(state);
                // 화면을 찍는 것은 다음 프레임을 그릴 때다 (udev.rs의 render_output)
                state.queue_redraw_all();
            }
            TimeoutAction::Drop
        }) {
            tracing::warn!("failed to start the screenshot timer: {err}");
        }
    }

    /// 영역을 고르는 중이면 포인터 움직임을 받는다. 앱에는 보내지 않는다. ⇧⌘5의 선택 영역을 끄는 중이
    /// 아니고 포인터가 도구 막대 위에 있으면 막대가 받게 false를 돌려준다
    pub fn screenshot_pointer_moved(&mut self, pos: Point<f64, Logical>) -> bool {
        let to_toolbar = self.toolbar_gets_pointer(pos);
        let Some(selection) = &self.screenshot_area else {
            return false;
        };
        if selection.editable.is_some() && to_toolbar {
            return false;
        }
        // ⇧⌘5의 선택 영역에서는 포인터가 그 자리에서 잡을 것의 커서가 된다
        if let Some(grip) = self.move_selection(pos) {
            self.cursor_status = CursorImageStatus::Named(grip.cursor());
        }
        true
    }

    /// 고르기를 `pos`로 옮긴다: ⇧⌘5의 선택 영역은 끄는 중이면 영역을 고치고, ⇧⌘4는 끄는 영역이나 가리킨 창을 바꾼다.
    /// ⇧⌘5의 선택 영역이면 그 자리에서 잡을 것을 돌려준다(포인터의 커서 모양). 포인터와 손가락(touch.rs)이 함께 쓴다.
    /// 전에는 screenshot_pointer_moved 안에 있었다
    fn move_selection(&mut self, pos: Point<f64, Logical>) -> Option<Grip> {
        let selection = self.screenshot_area.as_mut()?;
        selection.current = pos;
        if let Some(editable) = &mut selection.editable {
            let (grip, changed) = match &editable.drag {
                Some((drag, bounds)) => {
                    let rect = drag.rect_at(pos, *bounds);
                    let changed = rect != editable.rect;
                    editable.rect = rect;
                    (drag.grip, changed)
                }
                None => (selection::grip_at(editable.rect, pos), false),
            };
            if changed {
                self.queue_redraw_all();
            }
            return Some(grip);
        }
        if selection.window {
            self.hover_window(pos);
        }
        // 끄는 영역이나 가리킨 창이 바뀌었다
        self.queue_redraw_all();
        None
    }

    /// 창 고르기: 포인터 아래의 창을 칠한다 (메뉴 막대·Dock 같은 셸 표면은 고르지 않는다)
    fn hover_window(&mut self, pos: Point<f64, Logical>) {
        let hovered = self
            .space
            .element_under(pos)
            .map(|(window, _)| window.clone())
            .and_then(|window| {
                let rect = self.space.element_geometry(&window)?;
                Some((window, rect))
            });
        if let Some(selection) = &mut self.screenshot_area {
            selection.hovered = hovered;
        }
    }

    /// 스페이스: 영역 고르기와 창 고르기를 바꾼다. 영역을 끄는 중이거나 ⇧⌘5의 선택 영역이면 그대로 둔다
    pub fn toggle_screenshot_window(&mut self) {
        let Some(selection) = &mut self.screenshot_area else {
            return;
        };
        if selection.start.is_some() || selection.editable.is_some() {
            return;
        }
        selection.window = !selection.window;
        selection.hovered = None;
        let (window, current) = (selection.window, selection.current);
        self.cursor_status = CursorImageStatus::Named(selection_cursor(window));
        if window {
            self.hover_window(current);
        }
        self.queue_redraw_all();
    }

    /// 영역을 고르는 중이면 단추를 받는다: 왼쪽 단추를 눌러 끌고 떼면 찍는다(창 고르기는 누른 창을
    /// 찍는다). 다른 단추는 그만둔다. ⇧⌘5의 선택 영역은 왼쪽 단추로 옮기거나 크기를 고치기만 하고 다른
    /// 단추는 듣지 않는다. 끄는 중이 아니고 포인터가 도구 막대 위에 있으면 막대가 받게 false를 돌려준다
    pub fn screenshot_button(&mut self, button: u32, state: ButtonState) -> bool {
        let pos = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location());
        let to_toolbar = pos.is_some_and(|pos| self.toolbar_gets_pointer(pos));
        let Some(selection) = &self.screenshot_area else {
            return false;
        };
        if selection.editable.is_some() && to_toolbar {
            return false;
        }
        let pos = pos.unwrap_or(selection.current);
        if let Some(grip) = self.press_selection(pos, button, state) {
            self.cursor_status = CursorImageStatus::Named(grip.cursor());
        }
        true
    }

    /// `pos`에서 누르거나 뗀 단추 하나를 고르기에 준다. 왼쪽 단추를 눌러 끌고 떼면 찍는다(창 고르기는 누른 창을 찍는다).
    /// 다른 단추는 그만둔다. ⇧⌘5의 선택 영역은 왼쪽 단추로 옮기거나 크기를 고치기만 하고 다른 단추는 듣지 않으며, 끌기를
    /// 마치면 그 자리에서 잡을 것을 돌려준다(포인터의 커서 모양). 포인터와 손가락(touch.rs)이 함께 쓴다. 전에는
    /// screenshot_button 안에 있었다
    fn press_selection(
        &mut self,
        pos: Point<f64, Logical>,
        button: u32,
        state: ButtonState,
    ) -> Option<Grip> {
        let selection = self.screenshot_area.as_mut()?;
        if let Some(editable) = &mut selection.editable {
            match (button, state) {
                (BUTTON_LEFT, ButtonState::Pressed) if editable.drag.is_none() => {
                    let grip = selection::grip_at(editable.rect, pos);
                    // 바깥에서 새로 고르는 영역은 누른 화면 안에 둔다
                    let bounds = match grip {
                        Grip::Outside => self
                            .space
                            .output_under(pos)
                            .next()
                            .and_then(|output| self.space.output_geometry(output))
                            .unwrap_or(editable.bounds),
                        _ => editable.bounds,
                    };
                    editable.drag = Some((Drag::new(grip, pos, editable.rect), bounds));
                }
                (BUTTON_LEFT, ButtonState::Released) if editable.drag.is_some() => {
                    // 새 영역을 고른 끌기만 영역이 놓인 화면을 바꾼다. 전에는 누르는 순간 바꿔서, 다른 화면을
                    // 누르기만 해도 앞 화면에 남은 영역을 다음 끌기가 그 화면으로 끌어갔다
                    if let Some((drag, bounds)) = editable.drag.take()
                        && drag.grip == Grip::Outside
                        && editable.rect != drag.started_with()
                    {
                        editable.bounds = bounds;
                    }
                    self.screenshot_last_area = Some(editable.rect);
                    return Some(selection::grip_at(editable.rect, pos));
                }
                _ => {}
            }
            return None;
        }
        match (button, state) {
            (BUTTON_LEFT, ButtonState::Pressed) if !selection.window => {
                selection.start = Some(selection.current)
            }
            (BUTTON_LEFT, ButtonState::Released) => self.finish_selection(),
            (BUTTON_LEFT, ButtonState::Pressed) => {}
            (_, ButtonState::Pressed) => {
                self.end_selection();
            }
            _ => {}
        }
        None
    }

    /// 영역을 고르는 동안 손가락 하나가 닿았다 (touch.rs). 왼쪽 단추를 누른 포인터처럼 받는다. ⇧⌘5의 도구 막대(와 그
    /// 메뉴)를 누른 손가락은 막대가 받게 false를 돌려준다. 손가락에는 커서가 없으므로 커서 모양은 그대로 둔다
    pub(crate) fn screenshot_touch_down(&mut self, pos: Point<f64, Logical>) -> bool {
        if self.screenshot_area.is_none() || self.toolbar_under(pos) {
            return false;
        }
        self.move_selection(pos);
        self.press_selection(pos, BUTTON_LEFT, ButtonState::Pressed);
        true
    }

    /// 고르기를 받은 손가락이 움직였다
    pub(crate) fn screenshot_touch_motion(&mut self, pos: Point<f64, Logical>) {
        self.move_selection(pos);
    }

    /// 고르기를 받은 손가락을 뗐다: ⇧⌘4는 고른 영역(창 고르기는 누른 창)을 찍고, ⇧⌘5의 선택 영역은 끌기를 마친다
    pub(crate) fn screenshot_touch_up(&mut self, pos: Point<f64, Logical>) {
        self.move_selection(pos);
        self.press_selection(pos, BUTTON_LEFT, ButtonState::Released);
    }

    /// 고르기를 받은 손가락을 장치가 거뒀다(떼기는 오지 않는다): ⇧⌘4는 찍지 않고 그만두고, ⇧⌘5의 선택 영역은 끌던
    /// 자리에서 끌기를 마친다
    pub(crate) fn screenshot_touch_cancel(&mut self) {
        let Some(selection) = &self.screenshot_area else {
            return;
        };
        if selection.editable.is_some() {
            let pos = selection.current;
            self.press_selection(pos, BUTTON_LEFT, ButtonState::Released);
        } else {
            self.cancel_screenshot();
        }
    }

    /// Esc: 영역 고르기를 그만둔다
    pub fn cancel_screenshot(&mut self) {
        self.end_selection();
    }

    fn finish_selection(&mut self) {
        let Some(selection) = self.end_selection() else {
            return;
        };
        let (clipboard, delay) = (selection.clipboard, selection.delay);
        if selection.window {
            // 가리킨 창이 없는 곳(바탕화면)을 누르면 찍지 않는다
            if let Some((window, _)) = selection.hovered {
                self.after(delay, move |state| {
                    state.capture_window_shot(&window, clipboard)
                });
            }
            return;
        }
        // 끌지 않고 누르기만 했으면 찍지 않는다
        let Some(rect) = selection
            .rect()
            .filter(|rect| rect.size.w > 1 && rect.size.h > 1)
        else {
            return;
        };
        self.capture_area(rect, clipboard, delay);
    }

    /// 화면 좌표의 영역을 `delay`초 뒤 다음 프레임에 찍는다. 화면 둘에 걸치면 먼저 걸친 화면 안쪽만 찍는다.
    /// 전에는 finish_selection 안에 있었고, ⇧⌘5의 선택 영역도 찍으면서 따로 뺐다
    fn capture_area(&mut self, rect: Rectangle<i32, Logical>, clipboard: bool, delay: u32) {
        let Some((output, output_geo)) = self.space.outputs().find_map(|output| {
            self.space
                .output_geometry(output)
                .filter(|geo| geo.overlaps(rect))
                .map(|geo| (output.clone(), geo))
        }) else {
            return;
        };
        let Some(area) = rect.intersection(output_geo) else {
            return;
        };
        let capture = Capture {
            output,
            area: Some(Rectangle::new(area.loc - output_geo.loc, area.size)),
            clipboard,
        };
        self.after(delay, move |state| state.screenshot_capture = Some(capture));
    }

    /// 창 하나만 따로 그려 찍는다 (다른 창, 셸, 커서는 넣지 않는다). 기다리는 동안 창이 닫혔으면 실패다
    fn capture_window_shot(&mut self, window: &Window, clipboard: bool) {
        let image = self
            .capture_window(window, TRANSPARENT)
            .inspect_err(|err| tracing::warn!("failed to capture the window: {err}"))
            .ok()
            .map(|(image, _)| image);
        self.screenshot_taken(clipboard, image);
    }

    /// 고르기를 마친다. 커서를 되돌리고 포인터 아래의 앱에 포인터를 다시 보낸다. ⇧⌘5의 선택 영역은 다음
    /// ⇧⌘5를 위해 기억한다
    fn end_selection(&mut self) -> Option<AreaSelection> {
        let selection = self.screenshot_area.take()?;
        if let Some(editable) = &selection.editable {
            self.screenshot_last_area = Some(editable.rect);
        }
        self.cursor_status = selection.cursor.clone();
        // 포인터가 ⇧⌘5 도구 막대 위에 있는 동안에는 current를 고치지 않으므로 지금 포인터 자리로 보낸다
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or(selection.current);
        let time = self.start_time.elapsed().as_millis() as u32;
        self.pointer_moved(pointer, time);
        self.queue_redraw_all();
        Some(selection)
    }

    /// ⇧⌘5의 선택 영역을 띄운 동안 포인터를 도구 막대(와 그 메뉴)에 보낼지: 끄는 중이 아니고, 막대의 메뉴가
    /// 포인터를 잡았거나 포인터 아래가 막대다. 휠도 이것으로 막대에 보낸다(input.rs)
    pub(crate) fn toolbar_gets_pointer(&self, pos: Point<f64, Logical>) -> bool {
        let dragging = self
            .screenshot_area
            .as_ref()
            .and_then(|selection| selection.editable.as_ref())
            .is_none_or(|editable| editable.drag.is_some());
        if dragging {
            return false;
        }
        if self
            .seat
            .get_pointer()
            .is_some_and(|pointer| pointer.is_grabbed())
        {
            return true;
        }
        self.toolbar_under(pos)
    }

    /// 그 자리 아래가 ⇧⌘5 도구 막대나 그 메뉴인지. 손가락(touch.rs)도 쓰면서 toolbar_gets_pointer에서 뺐다
    pub(crate) fn toolbar_under(&self, pos: Point<f64, Logical>) -> bool {
        let Some((surface, _)) = self.surface_under(pos) else {
            return false;
        };
        let root = self.root_surface(&surface);
        self.space
            .outputs()
            .any(|output| toolbar(output).is_some_and(|toolbar| toolbar == root))
    }

    /// 찍은 화면을 클립보드에 넣거나 PNG 파일로 저장 위치에 저장한다. 파일은 따로 도는 스레드가 쓰고,
    /// 다 쓰면 셸에 알려 미리 보기를 띄우게 하거나(플로팅 썸네일) 알림을 띄운다
    pub fn screenshot_taken(&mut self, clipboard: bool, image: Option<Image>) {
        let tr = Translations::load(TRANSLATIONS);
        let title = tr.get("screenshot-title");
        let Some(image) = image else {
            self.dbus.send(Command::Notify {
                title,
                body: tr.get("screenshot-failed"),
            });
            return;
        };
        if clipboard {
            set_data_device_selection(
                &self.display_handle,
                &self.seat,
                vec![PNG_MIME.to_owned()],
                Arc::new(image),
            );
            self.dbus.send(Command::Notify {
                title,
                body: tr.get("screenshot-copied"),
            });
            return;
        }
        let dir = save_dir(
            self.screenshot_options.location,
            ("XDG_PICTURES_DIR", "Pictures"),
        );
        let path = unique_path(&dir, &file_name(&tr, "screenshot-file"), "png");
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut args = FluentArgs::new();
        args.set("name", name);
        let saved = tr.get_with("screenshot-saved", &args);
        let failed = tr.get("screenshot-failed");
        let thumbnail = self.screenshot_options.thumbnail;
        let commands = self.dbus.sender();
        std::thread::spawn(move || {
            let result = std::fs::create_dir_all(&dir)
                .and_then(|()| File::create(&path))
                .and_then(|file| write_png(&image, BufWriter::new(file)));
            let command = match result {
                Ok(()) if thumbnail => {
                    Command::ScreenshotSaved(path.to_string_lossy().into_owned())
                }
                Ok(()) => Command::Notify { title, body: saved },
                Err(err) => {
                    tracing::warn!("failed to save {}: {err}", path.display());
                    Command::Notify {
                        title,
                        body: failed,
                    }
                }
            };
            let _ = commands.send(command);
        });
    }
}

/// 화면에 띄운 ⇧⌘5 도구 막대의 표면 (셸이 띄운 것만)
fn toolbar(output: &Output) -> Option<WlSurface> {
    let map = layer_map_for_output(output);
    map.layers()
        .find(|layer| layer.namespace() == TOOLBAR_NAMESPACE && is_privileged(layer.wl_surface()))
        .map(|layer| layer.wl_surface().clone())
}

/// 컴포지터가 띄운 셸의 표면인지. 다른 앱이 같은 이름의 레이어를 만들어 포인터를 가로채지 못하게 한다
fn is_privileged(surface: &WlSurface) -> bool {
    surface
        .client()
        .and_then(|client| client.get_data::<ClientState>().map(|data| data.privileged))
        .unwrap_or(false)
}

/// 고르는 영역의 요소(`overlay`)를 화면의 요소 목록(앞의 것부터)에 넣는다. ⇧⌘5 도구 막대와 그 메뉴는 영역
/// 위에 남아야 하므로 막대의 마지막 요소 바로 뒤에, 막대가 없으면 `start`(창과 막대의 첫 요소) 앞에 넣는다
pub fn place_overlay(
    elements: &mut Vec<OutputRenderElements>,
    start: usize,
    overlay: Vec<OutputRenderElements>,
    output: &Output,
) {
    if overlay.is_empty() {
        return;
    }
    let ids = toolbar(output)
        .map(|surface| surface_ids(&surface))
        .unwrap_or_default();
    insert_below(elements, start, overlay, &ids);
}

/// `overlay`를 `ids`의 요소 가운데 마지막(가장 아래) 것 바로 뒤에, 없으면 `start`에 끼운다
fn insert_below(
    elements: &mut Vec<OutputRenderElements>,
    start: usize,
    overlay: Vec<OutputRenderElements>,
    ids: &[Id],
) {
    let start = start.min(elements.len());
    let at = elements[start..]
        .iter()
        .rposition(|element| ids.contains(element.id()))
        .map_or(start, |index| start + index + 1);
    elements.splice(at..at, overlay);
}

/// 표면과 그 하위 표면, 팝업의 렌더 요소 ID (render.rs의 surface_ids와 같다)
pub(crate) fn surface_ids(root: &WlSurface) -> Vec<Id> {
    let mut ids = Vec::new();
    let mut add_tree = |root: &WlSurface| {
        with_surface_tree_downward(
            root,
            (),
            |_, _, _| TraversalAction::DoChildren(()),
            |surface, _, _| ids.push(Id::from_wayland_resource(surface)),
            |_, _, _| true,
        );
    };
    add_tree(root);
    for (popup, _) in PopupManager::popups_for_surface(root) {
        add_tree(popup.wl_surface());
    }
    ids
}

/// 영역 고르기는 십자선, 창 고르기는 손가락 커서
fn selection_cursor(window: bool) -> CursorIcon {
    if window {
        CursorIcon::Pointer
    } else {
        CursorIcon::Crosshair
    }
}

/// 장면(앞의 요소부터)을 화면 크기의 버퍼에 따로 그려 RGBA로 읽는다. `area`가 있으면 그 영역만.
/// 회전한 화면도 보이는 방향 그대로 찍는다
pub fn capture_scene(
    renderer: &mut GlesRenderer,
    elements: &[&OutputRenderElements],
    output: &Output,
    area: Option<Rectangle<i32, Logical>>,
    clear: [f32; 4],
) -> Result<Image, String> {
    let mode = output.current_mode().ok_or("the output has no mode")?;
    let scale = output.current_scale().fractional_scale();
    let size = output.current_transform().transform_size(mode.size);
    let region = match area {
        Some(area) => area
            .to_physical_precise_round(scale)
            .intersection(Rectangle::from_size(size))
            .ok_or("the area is outside the screen")?,
        None => Rectangle::from_size(size),
    };
    let mut texture: GlesTexture = renderer
        .create_buffer(
            Fourcc::Abgr8888,
            size.to_logical(1).to_buffer(1, Transform::Normal),
        )
        .map_err(|err| err.to_string())?;
    let mapping = {
        let elements = texture_shader::precise(renderer, elements);
        let mut framebuffer = renderer.bind(&mut texture).map_err(|err| err.to_string())?;
        let mut damage = OutputDamageTracker::new(size, scale, Transform::Normal);
        damage
            .render_output(renderer, &mut framebuffer, 0, &elements, clear)
            .map_err(|err| format!("{err:?}"))?;
        // 고른 영역(창 하나)만 되읽는다. 전에는 화면 전체를 되읽고 CPU에서 잘랐다
        renderer
            .copy_framebuffer(
                &framebuffer,
                Rectangle::new(
                    (region.loc.x, region.loc.y).into(),
                    (region.size.w, region.size.h).into(),
                ),
                Fourcc::Abgr8888,
            )
            .map_err(|err| err.to_string())?
    };
    // smithay는 텍스처에 그릴 때 위아래를 맞춰 그리므로 되읽은 줄은 맨 위 줄부터다. 되읽기의
    // flipped()는 대상과 상관없이 늘 true라 보지 않는다 (VM에서 확인했다)
    let pixels = renderer
        .map_texture(&mapping)
        .map_err(|err| err.to_string())?;
    let (width, height) = (region.size.w as usize, region.size.h as usize);
    let length = width * height * 4;
    if pixels.len() < length {
        return Err(format!("read {} bytes for {width}x{height}", pixels.len()));
    }
    let rgba = pixels[..length].to_vec();
    Ok(Image {
        width: region.size.w as u32,
        height: region.size.h as u32,
        rgba,
    })
}

/// PNG로 쓴다. 화면은 불투명하므로 알파를 빼고 RGB로 쓴다. 비치는 픽셀이 있으면(창 찍기의 둥근
/// 모서리) 알파를 남기고, 미리 곱한 색을 PNG의 곱하지 않은 색으로 되돌린다
pub fn write_png(image: &Image, writer: impl Write) -> std::io::Result<()> {
    let (pixels, _) = image.rgba.as_chunks::<4>();
    let opaque = pixels.iter().all(|pixel| pixel[3] == u8::MAX);
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(if opaque {
        png::ColorType::Rgb
    } else {
        png::ColorType::Rgba
    });
    encoder.set_depth(png::BitDepth::Eight);
    // 큰 화면도 금방 쓰도록 빠른 압축
    encoder.set_compression(png::Compression::Fast);
    let data: Vec<u8> = if opaque {
        pixels
            .iter()
            .flat_map(|&[red, green, blue, _]| [red, green, blue])
            .collect()
    } else {
        pixels
            .iter()
            .flat_map(|&pixel| unpremultiply(pixel))
            .collect()
    };
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&data)
        .map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)
}

/// 알파를 미리 곱한 RGBA를 곱하지 않은 RGBA로 되돌린다
fn unpremultiply([red, green, blue, alpha]: [u8; 4]) -> [u8; 4] {
    if alpha == 0 {
        return [0; 4];
    }
    let alpha16 = u16::from(alpha);
    let channel = |value: u8| ((u16::from(value) * 255 + alpha16 / 2) / alpha16).min(255) as u8;
    [channel(red), channel(green), channel(blue), alpha]
}

/// 저장 위치의 폴더. 클립보드는 파일을 담지 못하므로 `clipboard`(xdg-user-dirs의 키와 홈 폴더 아래 이름)
/// 폴더를 쓴다. 스크린샷은 클립보드에 넣으므로 이 폴더를 쓰지 않고(사진 폴더를 준다), 화면 기록은 동영상
/// 폴더에 둔다. 전에는 screenshot_taken 안에 있었고 화면 기록도 쓰면서 뺐다
pub(crate) fn save_dir(location: Location, clipboard: (&str, &str)) -> PathBuf {
    match location {
        Location::Desktop => user_dir("XDG_DESKTOP_DIR", "Desktop"),
        Location::Documents => user_dir("XDG_DOCUMENTS_DIR", "Documents"),
        Location::Pictures => user_dir("XDG_PICTURES_DIR", "Pictures"),
        Location::Clipboard => user_dir(clipboard.0, clipboard.1),
    }
}

/// xdg-user-dirs의 폴더(user-dirs.dirs의 `key`), 없으면 홈 폴더 아래 `fallback`
fn user_dir(key: &str, fallback: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| user_dir_entry(&text, key, &home))
        .unwrap_or_else(|| home.join(fallback))
}

/// user-dirs.dirs의 한 항목 (XDG_PICTURES_DIR="$HOME/사진")
fn user_dir_entry(text: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))?
        .trim()
        .trim_matches('"');
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    // 홈 폴더 자체를 가리키면 사진 폴더가 없는 것이다 (xdg-user-dirs의 약속)
    (path.is_absolute() && path != home).then_some(path)
}

/// 이미 있으면 " 2", " 3"을 붙인 경로. 확장자(`extension`)는 화면 기록도 쓰면서 받게 했다
pub(crate) fn unique_path(dir: &Path, name: &str, extension: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.{extension}"));
    let mut number = 2;
    while path.exists() {
        path = dir.join(format!("{name} {number}.{extension}"));
        number += 1;
    }
    path
}

/// macOS처럼 찍은 때를 넣은 이름 ("스크린샷 2026-10-29 오전 9.52.13"). `message`는 이름 틀의 메시지 ID다
/// (screenshot-file, 화면 기록은 recording-file)
pub(crate) fn file_name(tr: &Translations, message: &str) -> String {
    let now = LocalTime::now();
    let (hour, period) = match now.hour {
        0 => (12, "screenshot-am"),
        hour @ 1..=11 => (hour, "screenshot-am"),
        12 => (12, "screenshot-pm"),
        hour => (hour - 12, "screenshot-pm"),
    };
    let mut args = FluentArgs::new();
    args.set(
        "date",
        format!("{:04}-{:02}-{:02}", now.year, now.month, now.day),
    );
    args.set("period", tr.get(period));
    args.set(
        "time",
        format!("{hour}.{:02}.{:02}", now.minute, now.second),
    );
    tr.get_with(message, &args)
}

/// 시스템 시간대(/etc/localtime, TZ)의 지금 시각
struct LocalTime {
    year: i32,
    month: i32,
    day: i32,
    hour: i32,
    minute: i32,
    second: i32,
}

impl LocalTime {
    fn now() -> Self {
        // 안전: time과 localtime_r에 넘기는 값은 이 함수의 지역 변수다
        let tm = unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&now, &mut tm);
            tm
        };
        Self {
            year: tm.tm_year + 1900,
            month: tm.tm_mon + 1,
            day: tm.tm_mday,
            hour: tm.tm_hour,
            minute: tm.tm_min,
            second: tm.tm_sec,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_have_the_same_messages() {
        bunt_i18n::check_same_messages(TRANSLATIONS).unwrap();
    }

    #[test]
    fn file_names_follow_macos() {
        let ko = Translations::from_source("ko", TRANSLATIONS[0].1);
        let name = file_name(&ko, "screenshot-file");
        assert!(name.starts_with("스크린샷 20"), "{name}");
        assert!(name.contains(" 오전 ") || name.contains(" 오후 "), "{name}");
        let name = file_name(&ko, "recording-file");
        assert!(name.starts_with("화면 기록 20"), "{name}");
        let en = Translations::from_source("en", TRANSLATIONS[1].1);
        let name = file_name(&en, "screenshot-file");
        assert!(
            name.starts_with("Screenshot 20") && name.contains(" at "),
            "{name}"
        );
    }

    #[test]
    fn folders_come_from_user_dirs() {
        let home = Path::new("/home/user");
        let pictures = |text| user_dir_entry(text, "XDG_PICTURES_DIR", home);
        let text =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/바탕화면\"\nXDG_PICTURES_DIR=\"$HOME/사진\"\n";
        assert_eq!(pictures(text), Some(home.join("사진")));
        assert_eq!(
            user_dir_entry(text, "XDG_DESKTOP_DIR", home),
            Some(home.join("바탕화면"))
        );
        assert_eq!(pictures("XDG_PICTURES_DIR=\"$HOME/\""), None);
        assert_eq!(
            pictures("XDG_PICTURES_DIR=\"/data/pics\""),
            Some(PathBuf::from("/data/pics"))
        );
        // 이름이 같게 시작하는 다른 항목은 보지 않는다
        assert_eq!(pictures("XDG_PICTURES_DIRS=\"/elsewhere\""), None);
        assert_eq!(pictures(""), None);
    }

    #[test]
    fn see_through_pixels_keep_alpha_without_premultiplying() {
        assert_eq!(unpremultiply([64, 0, 32, 128]), [128, 0, 64, 128]);
        assert_eq!(unpremultiply([10, 20, 30, 255]), [10, 20, 30, 255]);
        assert_eq!(unpremultiply([0, 0, 0, 0]), [0, 0, 0, 0]);
        let image = Image {
            width: 1,
            height: 1,
            rgba: vec![64, 0, 32, 128],
        };
        let mut bytes = Vec::new();
        write_png(&image, &mut bytes).unwrap();
        // IHDR의 색 형식: 6은 RGBA, 불투명한 화면은 2(RGB)
        assert_eq!(bytes[25], 6);
    }

    #[test]
    fn modes_and_locations_have_names() {
        assert_eq!(Mode::parse("window"), Some(Mode::Window));
        assert_eq!(Mode::parse("screen"), Some(Mode::Screen));
        assert_eq!(Mode::parse("video"), None);
        assert_eq!(Location::parse("desktop"), Some(Location::Desktop));
        assert_eq!(Location::parse("clipboard"), Some(Location::Clipboard));
        assert_eq!(Location::parse("trash"), None);
        assert_eq!(Options::default().location, Location::Pictures);
        assert!(Options::default().thumbnail);
    }

    #[test]
    fn the_selection_goes_under_the_toolbar_and_its_menu() {
        use smithay::backend::renderer::utils::CommitCounter;
        let solid = |id: &Id| {
            OutputRenderElements::Solid(SolidColorRenderElement::new(
                id.clone(),
                Rectangle::from_size((1, 1).into()),
                CommitCounter::default(),
                [0.0; 4],
                Kind::Unspecified,
            ))
        };
        let [cursor, menu, toolbar, window, overlay] = std::array::from_fn(|_| Id::new());
        let order = |elements: &[OutputRenderElements]| -> Vec<Id> {
            elements
                .iter()
                .map(|element| element.id().clone())
                .collect()
        };
        // 커서 다음(1)부터가 창과 막대다. 도구 막대의 메뉴와 막대 아래, 창 위에 넣는다
        let mut elements = vec![
            solid(&cursor),
            solid(&menu),
            solid(&toolbar),
            solid(&window),
        ];
        insert_below(
            &mut elements,
            1,
            vec![solid(&overlay)],
            &[toolbar.clone(), menu.clone()],
        );
        assert_eq!(
            order(&elements),
            [&cursor, &menu, &toolbar, &overlay, &window].map(Id::clone)
        );
        // 막대가 없으면(⇧⌘4) 창과 막대 모두의 위, 커서 아래
        let mut elements = vec![solid(&cursor), solid(&window)];
        insert_below(&mut elements, 1, vec![solid(&overlay)], &[]);
        assert_eq!(
            order(&elements),
            [&cursor, &overlay, &window].map(Id::clone)
        );
        // 시작 자리 앞(커서)은 막대로 보지 않는다
        let mut elements = vec![solid(&toolbar), solid(&window)];
        insert_below(
            &mut elements,
            1,
            vec![solid(&overlay)],
            std::slice::from_ref(&toolbar),
        );
        assert_eq!(
            order(&elements),
            [&toolbar, &overlay, &window].map(Id::clone)
        );
        // 목록보다 큰 시작 자리는 끝이다
        let mut elements = vec![solid(&window)];
        insert_below(&mut elements, 5, vec![solid(&overlay)], &[]);
        assert_eq!(order(&elements), [&window, &overlay].map(Id::clone));
    }

    #[test]
    fn only_the_dragged_area_takes_the_keys() {
        let _gl = crate::testing::gl_lock();
        let Some(mut renderer) = crate::testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let output = Rectangle::new((100, 0).into(), (640, 480).into());
        let scale = Scale::from(1.0);
        // ⇧⌘4: 누르기 전에는 그릴 것이 없고, 끄는 동안 채움과 테두리 넷
        let mut dragged = AreaSelection::new(
            false,
            false,
            0,
            (150.0, 40.0).into(),
            CursorImageStatus::default_named(),
        );
        assert!(dragged.takes_keys());
        assert!(dragged.elements(&mut renderer, output, scale).is_empty());
        dragged.start = Some((120.0, 20.0).into());
        let elements = dragged.elements(&mut renderer, output, scale);
        assert_eq!(elements.len(), 5);
        // 화면 안 좌표로 옮긴다: 왼쪽 위 테두리는 (20, 20)
        assert_eq!(elements[0].geometry(scale).loc, (20, 20).into());
        // ⇧⌘5의 선택 영역: 키는 도구 막대가 받고, 손잡이 여덟에 테두리 넷과 바깥 띠 넷
        let mut editable = AreaSelection::new(
            false,
            false,
            0,
            (0.0, 0.0).into(),
            CursorImageStatus::default_named(),
        );
        editable.editable = Some(Editable {
            rect: Rectangle::new((200, 100).into(), (200, 100).into()),
            bounds: output,
            drag: None,
            overlay: Overlay::default(),
        });
        assert!(!editable.takes_keys());
        assert_eq!(editable.elements(&mut renderer, output, scale).len(), 16);
    }

    #[test]
    fn png_has_the_screen_size() {
        let image = Image {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        };
        let mut bytes = Vec::new();
        write_png(&image, &mut bytes).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        // IHDR의 너비와 높이
        assert_eq!(&bytes[16..24], &[0, 0, 0, 2, 0, 0, 0, 1]);
    }
}
