// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 스크린샷 (macOS의 ⇧⌘3, ⇧⌘4와 ⌃를 더한 클립보드 복사). 포인터가 있는 화면 전체나 끌어서 고른
//! 영역을, 커서를 뺀 같은 장면을 따로 그려 읽는다(udev.rs의 render_output). 파일은 PNG로 사진
//! 폴더에 저장하고, 클립보드에는 image/png로 넣는다. 다 되면 알림을 띄운다. 영역을 고르는 동안
//! 포인터는 십자선이 되고 앱은 포인터와 키를 받지 않는다. Esc나 다른 단추로 그만둔다.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
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
    input::pointer::{CursorIcon, CursorImageStatus},
    output::Output,
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

/// ⇧⌘4로 영역을 고르는 중
pub struct AreaSelection {
    clipboard: bool,
    /// 누른 자리. 누르기 전이면 None
    start: Option<Point<f64, Logical>>,
    current: Point<f64, Logical>,
    /// 고르기 전의 커서. 마치면 되돌린다
    cursor: CursorImageStatus,
    fill: SolidColorBuffer,
    /// 위, 아래, 왼쪽, 오른쪽 테두리
    border: [SolidColorBuffer; 4],
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
    /// 누르기 전이면 None
    pub fn layout(&mut self) -> Option<Vec<(Point<i32, Logical>, &SolidColorBuffer)>> {
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
        if self.screenshot_area.is_some() || self.screenshot_capture.is_some() {
            return;
        }
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or_default();
        if area {
            let cursor = std::mem::replace(
                &mut self.cursor_status,
                CursorImageStatus::Named(CursorIcon::Crosshair),
            );
            self.screenshot_area = Some(AreaSelection {
                clipboard,
                start: None,
                current: pointer,
                cursor,
                fill: SolidColorBuffer::new((0, 0), SELECTION_FILL),
                border: std::array::from_fn(|_| SolidColorBuffer::new((0, 0), SELECTION_BORDER)),
            });
            return;
        }
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

    /// 영역을 고르는 중이면 포인터 움직임을 받는다. 앱에는 보내지 않는다
    pub fn screenshot_pointer_moved(&mut self, pos: Point<f64, Logical>) -> bool {
        let Some(selection) = &mut self.screenshot_area else {
            return false;
        };
        selection.current = pos;
        true
    }

    /// 영역을 고르는 중이면 단추를 받는다: 왼쪽 단추를 눌러 끌고 떼면 찍는다. 다른 단추는 그만둔다
    pub fn screenshot_button(&mut self, button: u32, state: ButtonState) -> bool {
        let Some(selection) = &mut self.screenshot_area else {
            return false;
        };
        match (button, state) {
            (BUTTON_LEFT, ButtonState::Pressed) => selection.start = Some(selection.current),
            (BUTTON_LEFT, ButtonState::Released) => self.finish_selection(),
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
        self.screenshot_capture = Some(Capture {
            output,
            area: Some(Rectangle::new(area.loc - output_geo.loc, area.size)),
            clipboard: selection.clipboard,
        });
    }

    /// 고르기를 마친다. 커서를 되돌리고 포인터 아래의 앱에 포인터를 다시 보낸다
    fn end_selection(&mut self) -> Option<AreaSelection> {
        let selection = self.screenshot_area.take()?;
        self.cursor_status = selection.cursor.clone();
        let time = self.start_time.elapsed().as_millis() as u32;
        self.pointer_moved(selection.current, time);
        Some(selection)
    }

    /// 찍은 화면을 클립보드에 넣거나 PNG 파일로 저장하고 알린다. 파일은 따로 도는 스레드가 쓴다
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
        let dir = pictures_dir();
        let path = unique_path(&dir, &file_name(&tr));
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut args = FluentArgs::new();
        args.set("name", name);
        let saved = tr.get_with("screenshot-saved", &args);
        let failed = tr.get("screenshot-failed");
        let commands = self.dbus.sender();
        std::thread::spawn(move || {
            let result = std::fs::create_dir_all(&dir)
                .and_then(|()| File::create(&path))
                .and_then(|file| write_png(&image, BufWriter::new(file)));
            let body = match result {
                Ok(()) => saved,
                Err(err) => {
                    tracing::warn!("failed to save {}: {err}", path.display());
                    failed
                }
            };
            let _ = commands.send(Command::Notify { title, body });
        });
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
        renderer
            .copy_framebuffer(
                &framebuffer,
                Rectangle::from_size(size.to_logical(1).to_buffer(1, Transform::Normal)),
                Fourcc::Abgr8888,
            )
            .map_err(|err| err.to_string())?
    };
    // smithay는 텍스처에 그릴 때 위아래를 맞춰 그리므로 되읽은 줄은 맨 위 줄부터다. 되읽기의
    // flipped()는 대상과 상관없이 늘 true라 보지 않는다 (VM에서 확인했다)
    let pixels = renderer
        .map_texture(&mapping)
        .map_err(|err| err.to_string())?;
    let (width, height) = (size.w as usize, size.h as usize);
    let stride = width * 4;
    if pixels.len() < stride * height {
        return Err(format!("read {} bytes for {width}x{height}", pixels.len()));
    }
    let region = match area {
        Some(area) => area
            .to_physical_precise_round(scale)
            .intersection(Rectangle::from_size(size))
            .ok_or("the area is outside the screen")?,
        None => Rectangle::from_size(size),
    };
    let mut rgba = Vec::with_capacity(region.size.w as usize * region.size.h as usize * 4);
    for row in region.loc.y as usize..(region.loc.y + region.size.h) as usize {
        let start = row * stride + region.loc.x as usize * 4;
        rgba.extend_from_slice(&pixels[start..start + region.size.w as usize * 4]);
    }
    Ok(Image {
        width: region.size.w as u32,
        height: region.size.h as u32,
        rgba,
    })
}

/// PNG로 쓴다. 화면은 불투명하므로 알파를 빼고 RGB로 쓴다
pub fn write_png(image: &Image, writer: impl Write) -> std::io::Result<()> {
    let mut encoder = png::Encoder::new(writer, image.width, image.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    // 큰 화면도 금방 쓰도록 빠른 압축
    encoder.set_compression(png::Compression::Fast);
    let (pixels, _) = image.rgba.as_chunks::<4>();
    let rgb: Vec<u8> = pixels
        .iter()
        .flat_map(|&[red, green, blue, _]| [red, green, blue])
        .collect();
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&rgb)
        .map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)
}

/// 사진 폴더: xdg-user-dirs의 XDG_PICTURES_DIR, 없으면 ~/Pictures
fn pictures_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| pictures_entry(&text, &home))
        .unwrap_or_else(|| home.join("Pictures"))
}

/// user-dirs.dirs의 XDG_PICTURES_DIR ("$HOME/사진")
fn pictures_entry(text: &str, home: &Path) -> Option<PathBuf> {
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("XDG_PICTURES_DIR="))?
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
    fn pictures_folder_comes_from_user_dirs() {
        let home = Path::new("/home/user");
        let text =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/바탕화면\"\nXDG_PICTURES_DIR=\"$HOME/사진\"\n";
        assert_eq!(pictures_entry(text, home), Some(home.join("사진")));
        assert_eq!(pictures_entry("XDG_PICTURES_DIR=\"$HOME/\"", home), None);
        assert_eq!(
            pictures_entry("XDG_PICTURES_DIR=\"/data/pics\"", home),
            Some(PathBuf::from("/data/pics"))
        );
        assert_eq!(pictures_entry("", home), None);
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
