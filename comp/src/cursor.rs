// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 커서 테마(xcursor)에서 기본 화살표 이미지를 읽는다.

use std::io::Read;

use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Logical, Point, Transform},
};
use xcursor::{
    CursorTheme,
    parser::{Image, parse_xcursor},
};

pub struct Cursor {
    images: Vec<Image>,
    size: u32,
}

impl Cursor {
    pub fn load(theme: &str, size: u32) -> Self {
        let images = load_images(theme).unwrap_or_else(|err| {
            tracing::warn!("cursor theme {theme}: {err}; using fallback cursor");
            vec![fallback()]
        });
        Self { images, size }
    }

    /// 설정한 크기에 배율을 곱한 크기에 가장 가까운 이미지. 애니메이션 커서는 첫 프레임만 쓴다.
    fn image(&self, scale: i32) -> &Image {
        let wanted = i64::from(self.size) * i64::from(scale.max(1));
        self.images
            .iter()
            .min_by_key(|image| (wanted - i64::from(image.size)).abs())
            .expect("cursor has at least one image")
    }

    /// 배율에 맞는 커서 이미지의 버퍼와 논리 좌표의 핫스폿. 소수 배율 화면에서는 올림한 정수
    /// 배율의 이미지를 줄여 그린다. 테마에 큰 이미지가 없으면 있는 이미지를 키워 그린다
    pub fn buffer(&self, scale: i32) -> (MemoryRenderBuffer, Point<f64, Logical>) {
        let image = self.image(scale);
        let buffer_scale = (f64::from(image.size) / f64::from(self.size.max(1)))
            .round()
            .clamp(1.0, f64::from(scale.max(1))) as i32;
        let buffer = MemoryRenderBuffer::from_slice(
            &image.pixels_rgba,
            Fourcc::Abgr8888,
            (image.width as i32, image.height as i32),
            buffer_scale,
            Transform::Normal,
            None,
        );
        let hotspot = Point::from((
            f64::from(image.xhot) / f64::from(buffer_scale),
            f64::from(image.yhot) / f64::from(buffer_scale),
        ));
        (buffer, hotspot)
    }
}

fn load_images(theme: &str) -> Result<Vec<Image>, String> {
    let path = CursorTheme::load(theme)
        .load_icon("default")
        .ok_or("theme has no default cursor")?;
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
