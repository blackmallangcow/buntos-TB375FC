// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 커서 테마(xcursor)의 커서 이미지. 기본 화살표와 앱이 커서 모양(wp_cursor_shape_v1)으로 고른
//! 커서(글자 입력, 손가락, 크기 조절 등)를 시스템 설정에서 고른 크기로 그린다.

use std::{cell::RefCell, collections::HashMap, io::Read, rc::Rc};

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    input::pointer::CursorIcon,
    utils::{Logical, Point, Transform},
};
use xcursor::{
    CursorTheme,
    parser::{Image, parse_xcursor},
};

/// 고를 수 있는 커서 크기 (논리 픽셀)
pub const SIZE_RANGE: (u32, u32) = (16, 64);

pub struct Cursor {
    theme: CursorTheme,
    size: u32,
    /// 커서 이름마다 읽은 이미지. 테마에 없는 이름은 기본 화살표를 쓴다
    images: RefCell<HashMap<CursorIcon, Rc<Vec<Image>>>>,
    default: Rc<Vec<Image>>,
}

impl Cursor {
    pub fn load(theme: &str, size: u32) -> Self {
        let theme = CursorTheme::load(theme);
        let default = load_images(&theme, CursorIcon::Default).unwrap_or_else(|err| {
            tracing::warn!("cursor theme: {err}; using fallback cursor");
            vec![fallback()]
        });
        Self {
            theme,
            size: size.clamp(SIZE_RANGE.0, SIZE_RANGE.1),
            images: RefCell::default(),
            default: Rc::new(default),
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn set_size(&mut self, size: u32) {
        self.size = size.clamp(SIZE_RANGE.0, SIZE_RANGE.1);
    }

    fn images(&self, icon: CursorIcon) -> Rc<Vec<Image>> {
        if icon == CursorIcon::Default {
            return self.default.clone();
        }
        self.images
            .borrow_mut()
            .entry(icon)
            .or_insert_with(|| match load_images(&self.theme, icon) {
                Ok(images) => Rc::new(images),
                Err(err) => {
                    tracing::debug!("cursor {}: {err}; using the default arrow", icon.name());
                    self.default.clone()
                }
            })
            .clone()
    }

    /// 커서 이미지를 크기 × 배율 픽셀로 맞춘 버퍼와 논리 좌표의 핫스폿. 테마에 그 크기가 없으면
    /// 가까운 큰 이미지를 줄여(없으면 큰 쪽으로 늘려) 만든다. 버퍼가 화면 픽셀과 같은 크기여야
    /// DRM이 커서를 합성 없이 커서 평면에 올린다. 애니메이션 커서는 첫 프레임만 쓴다
    pub fn buffer(
        &self,
        icon: CursorIcon,
        scale: i32,
    ) -> (MemoryRenderBuffer, Point<f64, Logical>) {
        let scale = scale.max(1);
        let wanted = self.size * scale as u32;
        let images = self.images(icon);
        let image = nearest(&images, wanted);
        let factor = f64::from(wanted) / f64::from(image.size.max(1));
        let (pixels, width, height) = if image.size == wanted {
            (image.pixels_rgba.clone(), image.width, image.height)
        } else {
            resample(image, factor)
        };
        // xcursor의 pixels_rgba는 파일의 바이트 그대로(리틀 엔디언 ARGB, 곧 B, G, R, A 순서)다.
        // 커서 평면과 같은 Argb8888이어야 DRM이 커서를 화면 합성 없이 커서 평면에 올린다
        let buffer = MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (width as i32, height as i32),
            scale,
            Transform::Normal,
            None,
        );
        let hotspot = Point::from((
            f64::from(image.xhot) * factor / f64::from(scale),
            f64::from(image.yhot) * factor / f64::from(scale),
        ));
        (buffer, hotspot)
    }
}

/// 크기가 `wanted` 이상인 이미지 가운데 가장 작은 것. 없으면 가장 큰 것
fn nearest(images: &[Image], wanted: u32) -> &Image {
    images
        .iter()
        .filter(|image| image.size >= wanted)
        .min_by_key(|image| image.size)
        .or_else(|| images.iter().max_by_key(|image| image.size))
        .expect("cursor has at least one image")
}

/// 넓이 평균(박스 필터)으로 크기를 바꾼다. 픽셀은 알파를 곱한 값이라 그대로 평균해도 된다
fn resample(image: &Image, factor: f64) -> (Vec<u8>, u32, u32) {
    let (src_w, src_h) = (image.width as usize, image.height as usize);
    let width = ((src_w as f64) * factor).round().max(1.0) as usize;
    let height = ((src_h as f64) * factor).round().max(1.0) as usize;
    let mut pixels = vec![0u8; width * height * 4];
    for y in 0..height {
        let (y0, y1) = (
            y as f64 / factor,
            ((y + 1) as f64 / factor).min(src_h as f64),
        );
        for x in 0..width {
            let (x0, x1) = (
                x as f64 / factor,
                ((x + 1) as f64 / factor).min(src_w as f64),
            );
            let mut sum = [0.0f64; 4];
            let mut total = 0.0;
            let mut sy = y0.floor() as usize;
            while (sy as f64) < y1 && sy < src_h {
                let weight_y = ((sy + 1) as f64).min(y1) - (sy as f64).max(y0);
                let mut sx = x0.floor() as usize;
                while (sx as f64) < x1 && sx < src_w {
                    let weight = weight_y * (((sx + 1) as f64).min(x1) - (sx as f64).max(x0));
                    let at = (sy * src_w + sx) * 4;
                    for (channel, value) in sum.iter_mut().enumerate() {
                        *value += f64::from(image.pixels_rgba[at + channel]) * weight;
                    }
                    total += weight;
                    sx += 1;
                }
                sy += 1;
            }
            let at = (y * width + x) * 4;
            for (channel, value) in sum.iter().enumerate() {
                pixels[at + channel] = (value / total.max(f64::EPSILON)).round() as u8;
            }
        }
    }
    (pixels, width as u32, height as u32)
}

/// 커서 이름(과 다른 이름)으로 테마의 xcursor 파일을 찾아 읽는다
fn load_images(theme: &CursorTheme, icon: CursorIcon) -> Result<Vec<Image>, String> {
    let path = std::iter::once(icon.name())
        .chain(icon.alt_names().iter().copied())
        .find_map(|name| theme.load_icon(name))
        .ok_or_else(|| format!("theme has no {} cursor", icon.name()))?;
    let mut data = Vec::new();
    std::fs::File::open(&path)
        .and_then(|mut file| file.read_to_end(&mut data))
        .map_err(|err| format!("{}: {err}", path.display()))?;
    parse_xcursor(&data)
        .filter(|images| !images.is_empty())
        .ok_or_else(|| format!("{}: not an xcursor file", path.display()))
}

/// 테마가 없을 때 쓰는 작은 사각형 커서
fn fallback() -> Image {
    const SIZE: u32 = 12;
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let edge = x == 0 || y == 0 || x == SIZE - 1 || y == SIZE - 1;
            let value = if edge { 0 } else { 255 };
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    Image {
        size: SIZE,
        width: SIZE,
        height: SIZE,
        xhot: 0,
        yhot: 0,
        delay: 0,
        pixels_argb: pixels.clone(),
        pixels_rgba: pixels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(size: u32, value: u8) -> Image {
        let pixels = vec![value; (size * size * 4) as usize];
        Image {
            size,
            width: size,
            height: size,
            xhot: size / 2,
            yhot: size / 2,
            delay: 0,
            pixels_argb: pixels.clone(),
            pixels_rgba: pixels,
        }
    }

    #[test]
    fn picks_the_smallest_image_that_is_big_enough() {
        let images = [square(24, 0), square(32, 0), square(48, 0)];
        assert_eq!(nearest(&images, 20).size, 24);
        assert_eq!(nearest(&images, 30).size, 32);
        assert_eq!(nearest(&images, 96).size, 48);
    }

    #[test]
    fn resampling_keeps_flat_colors_and_scales_the_size() {
        let (pixels, width, height) = resample(&square(24, 200), 16.0 / 24.0);
        assert_eq!((width, height), (16, 16));
        assert!(pixels.iter().all(|&value| value == 200));
        let (_, width, height) = resample(&square(24, 10), 2.0);
        assert_eq!((width, height), (48, 48));
    }
}
