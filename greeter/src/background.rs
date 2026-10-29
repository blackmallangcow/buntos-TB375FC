// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 흐린 배경화면 (macOS 로그인 창). 그림을 작게 줄이고 상자 흐림을 세 번 걸면 가우스 흐림에
//! 가깝다. 작은 그림을 화면 크기로 늘려 그리므로 그리는 비용도 작다

use std::path::PathBuf;

use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

/// 시스템 기본 배경화면 (image/mkosi.extra/usr/share/glib-2.0/schemas/90_buntos.gschema.override)
const DEFAULT_WALLPAPER: &str = "/usr/share/backgrounds/buntos/buntos-dawn.png";
const BACKGROUND_SCHEMA: &str = "org.gnome.desktop.background";
/// 밝게·어둡게 (color-scheme)
const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
/// 줄인 그림의 너비. 1280px 화면에서 흐림 반지름이 약 30px이 된다
const SMALL_WIDTH: usize = 160;
const BLUR_RADIUS: usize = 4;
const BLUR_PASSES: usize = 3;

/// 배경 그림. 배경화면이 없으면 빈 그림이고 창 바탕색이 보인다
pub fn picture() -> gtk::Picture {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .hexpand(true)
        .vexpand(true)
        .can_target(false)
        .build();
    if let Some(texture) = wallpaper().and_then(|path| blurred(&path)) {
        picture.set_paintable(Some(&texture));
    }
    picture
}

/// 로그인 화면 사용자의 GSettings에는 사용자 값이 없으므로 시스템 기본값이 나온다.
/// 사용자 세션의 배경화면은 그 사용자의 홈에 있어 읽을 수 없다. 사용자로 도는 잠금 화면(bunt-lock)은 그
/// 사용자의 배경화면을 읽고, 데스크톱처럼(shell/src/wallpaper.rs) 어둡게 모드면 picture-uri-dark를 쓴다
fn wallpaper() -> Option<PathBuf> {
    let source = gio::SettingsSchemaSource::default();
    let dark = source
        .as_ref()
        .and_then(|source| source.lookup(INTERFACE_SCHEMA, true))
        .is_some_and(|_| {
            gio::Settings::new(INTERFACE_SCHEMA).string("color-scheme") == "prefer-dark"
        });
    let from_settings = source
        .and_then(|source| source.lookup(BACKGROUND_SCHEMA, true))
        .map(|_| {
            gio::Settings::new(BACKGROUND_SCHEMA).string(if dark {
                "picture-uri-dark"
            } else {
                "picture-uri"
            })
        })
        .and_then(|uri| gio::File::for_uri(&uri).path());
    from_settings
        .filter(|path| path.is_file())
        .or_else(|| Some(PathBuf::from(DEFAULT_WALLPAPER)).filter(|path| path.is_file()))
}

fn blurred(path: &std::path::Path) -> Option<gdk::Texture> {
    let texture = gdk::Texture::from_filename(path)
        .inspect_err(|err| eprintln!("bunt-greeter: {}: {err}", path.display()))
        .ok()?;
    let (width, height) = (texture.width() as usize, texture.height() as usize);
    if width == 0 || height == 0 {
        return None;
    }
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8);
    let (bytes, stride) = downloader.download_bytes();
    let small_width = SMALL_WIDTH.min(width);
    let small_height = (height * small_width / width).max(1);
    let mut small = shrink(&bytes, stride, width, height, small_width, small_height);
    for _ in 0..BLUR_PASSES {
        box_blur(&mut small, small_width, small_height, BLUR_RADIUS);
    }
    let pixels: Vec<u8> = small.iter().map(|value| value.round() as u8).collect();
    Some(
        gdk::MemoryTexture::new(
            small_width as i32,
            small_height as i32,
            gdk::MemoryFormat::R8g8b8,
            &glib::Bytes::from_owned(pixels),
            small_width * 3,
        )
        .upcast(),
    )
}

/// 영역 평균으로 줄인다 (RGB)
fn shrink(
    source: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    small_width: usize,
    small_height: usize,
) -> Vec<f32> {
    let mut out = vec![0.0; small_width * small_height * 3];
    for y in 0..small_height {
        let (top, bottom) = (
            y * height / small_height,
            ((y + 1) * height / small_height).max(y * height / small_height + 1),
        );
        for x in 0..small_width {
            let (left, right) = (
                x * width / small_width,
                ((x + 1) * width / small_width).max(x * width / small_width + 1),
            );
            let mut sum = [0u32; 3];
            for row in top..bottom {
                let line = &source[row * stride..];
                for column in left..right {
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(line[column * 3 + channel]);
                    }
                }
            }
            let count = ((bottom - top) * (right - left)) as f32;
            for (channel, total) in sum.iter().enumerate() {
                out[(y * small_width + x) * 3 + channel] = *total as f32 / count;
            }
        }
    }
    out
}

/// 가로, 세로로 한 번씩 상자 흐림을 건다. 가장자리 밖은 가장자리 값으로 본다
fn box_blur(pixels: &mut [f32], width: usize, height: usize, radius: usize) {
    let window = (2 * radius + 1) as f32;
    let mut line = Vec::new();
    let mut pass = |count: usize, length: usize, index: &dyn Fn(usize, usize) -> usize| {
        for i in 0..count {
            line.clear();
            line.extend((0..length).map(|j| {
                let at = index(i, j);
                [pixels[at], pixels[at + 1], pixels[at + 2]]
            }));
            for j in 0..length {
                let mut sum = [0.0; 3];
                for offset in 0..=2 * radius {
                    let k = (j + offset).saturating_sub(radius).min(length - 1);
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += line[k][channel];
                    }
                }
                let at = index(i, j);
                for (channel, total) in sum.iter().enumerate() {
                    pixels[at + channel] = total / window;
                }
            }
        }
    };
    pass(height, width, &|row, column| (row * width + column) * 3);
    pass(width, height, &|column, row| (row * width + column) * 3);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_averages_areas() {
        // 2x2 그림: 흰색과 검은색이 반씩 → 1x1 회색
        let source = [255, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255, 255];
        let small = shrink(&source, 6, 2, 2, 1, 1);
        assert_eq!(small, vec![127.5, 127.5, 127.5]);
    }

    #[test]
    fn blurring_keeps_flat_colors_and_spreads_edges() {
        let mut flat = vec![100.0; 5 * 3 * 3];
        box_blur(&mut flat, 5, 3, 2);
        assert!(flat.iter().all(|value| (value - 100.0).abs() < 1e-3));

        // 한 줄의 가운데만 밝으면 양옆으로 퍼진다
        let mut line = vec![0.0; 5 * 3];
        line[6..9].copy_from_slice(&[255.0, 255.0, 255.0]);
        box_blur(&mut line, 5, 1, 1);
        // 픽셀 1, 2는 85, 픽셀 4는 0
        assert!((line[3] - 85.0).abs() < 1e-3);
        assert!((line[6] - 85.0).abs() < 1e-3);
        assert_eq!(line[12], 0.0);
    }
}
