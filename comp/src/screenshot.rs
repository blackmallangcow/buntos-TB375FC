// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 스크린샷 (macOS의 ⇧⌘3, ⇧⌘4와 ⌃를 더한 클립보드 복사, ⇧⌘5 도구 막대). 포인터가 있는 화면 전체나
//! 끌어서 고른 영역은 커서를 뺀 같은 장면을 따로 그려 읽고(udev.rs의 render_output), 고른 창 하나는 그
//! 창만 따로 그린다(둥근 모서리는 비운다). 파일은 PNG로 저장 위치(⇧⌘5의 옵션, 기본은 사진 폴더)에
//! 저장하고, 클립보드에는 image/png로 넣는다. 다 되면 셸이 화면 모서리에 미리 보기를 띄우게
//! 알리거나(플로팅 썸네일) 알림을 띄운다. 영역을 고르는 동안 포인터는 십자선이 되고 앱은 포인터와
//! 키를 받지 않는다. 스페이스는 창 고르기로 바꾸고, Esc나 다른 단추로 그만둔다.

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
            element::solid::SolidColorBuffer,
            gles::{GlesRenderer, GlesTexture},
        },
    },
    desktop::Window,
    input::pointer::{CursorIcon, CursorImageStatus},
    output::Output,
    reexports::calloop::timer::{TimeoutAction, Timer},
    utils::{Logical, Point, Rectangle, Size, Transform},
    wayland::selection::data_device::set_data_device_selection,
};

use crate::{dbus::Command, render::OutputRenderElements, state::BuntState};

const TRANSLATIONS: &Sources = &[
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

/// ⇧⌘4로 영역이나 창을 고르는 중
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
}

impl AreaSelection {
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
    pub fn layout(&mut self) -> Option<Vec<(Point<i32, Logical>, &SolidColorBuffer)>> {
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
        self.screenshot_area = Some(AreaSelection {
            clipboard,
            window,
            hovered: None,
            delay,
            start: None,
            current: pointer,
            cursor,
            fill: SolidColorBuffer::new((0, 0), SELECTION_FILL),
            border: std::array::from_fn(|_| SolidColorBuffer::new((0, 0), SELECTION_BORDER)),
            tint: SolidColorBuffer::new((0, 0), WINDOW_TINT),
        });
        if window {
            self.hover_window(pointer);
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
    /// 창을 따로 그린다
    fn after(&mut self, delay: u32, then: impl FnOnce(&mut BuntState) + 'static) {
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

    /// 영역을 고르는 중이면 포인터 움직임을 받는다. 앱에는 보내지 않는다
    pub fn screenshot_pointer_moved(&mut self, pos: Point<f64, Logical>) -> bool {
        let Some(selection) = &mut self.screenshot_area else {
            return false;
        };
        selection.current = pos;
        if selection.window {
            self.hover_window(pos);
        }
        true
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

    /// 스페이스: 영역 고르기와 창 고르기를 바꾼다. 영역을 끄는 중에는 그대로 둔다
    pub fn toggle_screenshot_window(&mut self) {
        let Some(selection) = &mut self.screenshot_area else {
            return;
        };
        if selection.start.is_some() {
            return;
        }
        selection.window = !selection.window;
        selection.hovered = None;
        let (window, current) = (selection.window, selection.current);
        self.cursor_status = CursorImageStatus::Named(selection_cursor(window));
        if window {
            self.hover_window(current);
        }
    }

    /// 영역을 고르는 중이면 단추를 받는다: 왼쪽 단추를 눌러 끌고 떼면 찍는다(창 고르기는 누른 창을
    /// 찍는다). 다른 단추는 그만둔다
    pub fn screenshot_button(&mut self, button: u32, state: ButtonState) -> bool {
        let Some(selection) = &mut self.screenshot_area else {
            return false;
        };
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
        true
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

    /// 고르기를 마친다. 커서를 되돌리고 포인터 아래의 앱에 포인터를 다시 보낸다
    fn end_selection(&mut self) -> Option<AreaSelection> {
        let selection = self.screenshot_area.take()?;
        self.cursor_status = selection.cursor.clone();
        let time = self.start_time.elapsed().as_millis() as u32;
        self.pointer_moved(selection.current, time);
        Some(selection)
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
        let dir = match self.screenshot_options.location {
            Location::Desktop => user_dir("XDG_DESKTOP_DIR", "Desktop"),
            Location::Documents => user_dir("XDG_DOCUMENTS_DIR", "Documents"),
            Location::Pictures | Location::Clipboard => user_dir("XDG_PICTURES_DIR", "Pictures"),
        };
        let path = unique_path(&dir, &file_name(&tr));
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
        let mut framebuffer = renderer.bind(&mut texture).map_err(|err| err.to_string())?;
        let mut damage = OutputDamageTracker::new(size, scale, Transform::Normal);
        damage
            .render_output(renderer, &mut framebuffer, 0, elements, clear)
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

/// 이미 있으면 " 2", " 3"을 붙인 경로
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.png"));
    let mut number = 2;
    while path.exists() {
        path = dir.join(format!("{name} {number}.png"));
        number += 1;
    }
    path
}

/// macOS처럼 찍은 때를 넣은 이름 ("스크린샷 2026-10-29 오전 9.52.13")
fn file_name(tr: &Translations) -> String {
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
    tr.get_with("screenshot-file", &args)
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
        let name = file_name(&ko);
        assert!(name.starts_with("스크린샷 20"), "{name}");
        assert!(name.contains(" 오전 ") || name.contains(" 오후 "), "{name}");
        let en = Translations::from_source("en", TRANSLATIONS[1].1);
        let name = file_name(&en);
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
