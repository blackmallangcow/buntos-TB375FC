// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SHM 버퍼의 손상 줄이기. GTK의 cairo 렌더러처럼 고친 곳 대신 버퍼 전체를 손상으로 알리는 앱에서,
//! 앞 커밋의 사본과 타일 단위로 견주어 실제로 바뀐 타일만 손상으로 남긴다. 컴포지터는 손상만 텍스처에
//! 다시 올리고 다시 그리므로, 소프트웨어 렌더러(llvmpipe)에서는 창 전체를 다시 합성하는 것보다 견주기가
//! 훨씬 싸다. 시스템 모니터의 그래프나 글자 칸의 깜박이는 커서가 창 전체를 다시 그리게 했고, 창의 그림자
//! 여백이 Dock 밑까지 닿아 Dock 흐림도 매번 다시 했다.

use std::cell::RefCell;

use smithay::{
    reexports::wayland_server::protocol::{wl_shm, wl_surface::WlSurface},
    utils::{Buffer, Rectangle},
    wayland::{
        compositor::{BufferAssignment, Damage, SurfaceAttributes, with_states},
        shm,
    },
};

/// 견주는 타일 크기 (픽셀). 작을수록 손상이 정확하고 사각형이 많아진다
const TILE_W: usize = 64;
const TILE_H: usize = 32;
/// 버퍼의 이만큼 이상을 한 번에 손상으로 알리면 앱이 고친 곳을 따로 알리지 않는 것으로 본다
const FULL_DAMAGE: f64 = 0.9;
const BYTES_PER_PIXEL: usize = 4;

/// 표면이 마지막으로 커밋한 버퍼 내용의 사본
struct Shadow {
    width: usize,
    height: usize,
    stride: usize,
    format: wl_shm::Format,
    pixels: Vec<u8>,
}

type ShadowCell = RefCell<Option<Shadow>>;

/// 커밋한 표면의 손상을 실제로 바뀐 곳으로 줄인다. smithay가 손상을 읽기(on_commit_buffer_handler)
/// 전에 부른다. 앱이 고친 곳을 따로 알리면 그대로 두고 사본도 버린다
pub fn narrow_damage(surface: &WlSurface) {
    with_states(surface, |states| {
        let mut guard = states.cached_state.get::<SurfaceAttributes>();
        let attributes = guard.current();
        let Some(BufferAssignment::NewBuffer(buffer)) = attributes.buffer.as_ref() else {
            return;
        };
        let buffer = buffer.clone();
        states.data_map.insert_if_missing(ShadowCell::default);
        let Some(shadow) = states.data_map.get::<ShadowCell>() else {
            return;
        };
        let scale = attributes.buffer_scale.max(1);
        let damage = &attributes.damage;
        let narrowed = shm::with_buffer_contents(&buffer, |ptr, len, data| {
            let (width, height) = (data.width.max(0) as usize, data.height.max(0) as usize);
            let (stride, offset) = (data.stride.max(0) as usize, data.offset.max(0) as usize);
            let mut shadow = shadow.borrow_mut();
            let fits = offset
                .checked_add(stride * height)
                .is_some_and(|end| end <= len);
            if !matches!(
                data.format,
                wl_shm::Format::Argb8888 | wl_shm::Format::Xrgb8888
            ) || width == 0
                || height == 0
                || stride < width * BYTES_PER_PIXEL
                || !fits
                || !covers_buffer(damage, width, height, scale)
            {
                *shadow = None;
                return None;
            }
            // 안전: 범위는 위에서 확인했고, 풀은 이 함수 안에서 매핑되어 있다
            let current = unsafe { std::slice::from_raw_parts(ptr.add(offset), stride * height) };
            let same_layout = shadow.as_ref().is_some_and(|previous| {
                (
                    previous.width,
                    previous.height,
                    previous.stride,
                    previous.format,
                ) == (width, height, stride, data.format)
            });
            let rects = same_layout.then(|| {
                let previous = shadow.as_ref().expect("same layout means a shadow");
                changed_rects(&previous.pixels, current, width, height, stride)
            });
            // 다음 커밋과 견줄 사본을 남긴다
            match shadow.as_mut() {
                Some(previous) if same_layout => previous.pixels.copy_from_slice(current),
                _ => {
                    *shadow = Some(Shadow {
                        width,
                        height,
                        stride,
                        format: data.format,
                        pixels: current.to_vec(),
                    })
                }
            }
            rects
        });
        if let Ok(Some(rects)) = narrowed {
            attributes.damage = rects.into_iter().map(Damage::Buffer).collect();
        }
    });
}

/// 손상 하나가 버퍼 거의 전체를 덮는지 (표면 좌표의 손상은 버퍼 배율을 곱한다)
fn covers_buffer(damage: &[Damage], width: usize, height: usize, scale: i32) -> bool {
    let (width, height) = (width as f64, height as f64);
    damage.iter().any(|damage| {
        let (w, h) = match damage {
            Damage::Buffer(rect) => (f64::from(rect.size.w), f64::from(rect.size.h)),
            Damage::Surface(rect) => (
                f64::from(rect.size.w) * f64::from(scale),
                f64::from(rect.size.h) * f64::from(scale),
            ),
        };
        w.clamp(0.0, width) * h.clamp(0.0, height) >= FULL_DAMAGE * width * height
    })
}

/// 두 버퍼 내용에서 바뀐 타일을 이어 붙인 사각형 (버퍼 좌표). 한 줄의 이웃한 타일은 가로로, 같은
/// 너비의 사각형은 다음 줄과 세로로 잇는다
fn changed_rects(
    previous: &[u8],
    current: &[u8],
    width: usize,
    height: usize,
    stride: usize,
) -> Vec<Rectangle<i32, Buffer>> {
    let mut rects: Vec<Rectangle<i32, Buffer>> = Vec::new();
    for top in (0..height).step_by(TILE_H) {
        let bottom = (top + TILE_H).min(height);
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for left in (0..width).step_by(TILE_W) {
            let right = (left + TILE_W).min(width);
            let changed = (top..bottom).any(|y| {
                let start = y * stride + left * BYTES_PER_PIXEL;
                let end = y * stride + right * BYTES_PER_PIXEL;
                previous[start..end] != current[start..end]
            });
            if !changed {
                continue;
            }
            match runs.last_mut() {
                Some((_, run_end)) if *run_end == left => *run_end = right,
                _ => runs.push((left, right)),
            }
        }
        for (left, right) in runs {
            let (x, w, y) = (left as i32, (right - left) as i32, top as i32);
            let h = (bottom - top) as i32;
            match rects
                .iter_mut()
                .find(|rect| rect.loc.x == x && rect.size.w == w && rect.loc.y + rect.size.h == y)
            {
                Some(rect) => rect.size.h += h,
                None => rects.push(Rectangle::new((x, y).into(), (w, h).into())),
            }
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: usize, height: usize) -> Vec<u8> {
        vec![0; width * height * BYTES_PER_PIXEL]
    }

    fn set(pixels: &mut [u8], width: usize, x: usize, y: usize) {
        pixels[(y * width + x) * BYTES_PER_PIXEL] = 255;
    }

    #[test]
    fn same_contents_have_no_damage() {
        let pixels = image(200, 100);
        assert!(changed_rects(&pixels, &pixels, 200, 100, 200 * 4).is_empty());
    }

    #[test]
    fn a_changed_pixel_damages_its_tile_only() {
        let (width, height) = (200, 100);
        let previous = image(width, height);
        let mut current = previous.clone();
        set(&mut current, width, 70, 40);
        let rects = changed_rects(&previous, &current, width, height, width * 4);
        assert_eq!(
            rects,
            vec![Rectangle::new((64, 32).into(), (64, 32).into())]
        );
    }

    #[test]
    fn neighboring_tiles_join_into_one_rectangle() {
        let (width, height) = (200, 100);
        let previous = image(width, height);
        let mut current = previous.clone();
        // 가로로 이웃한 두 타일, 다음 줄에도 같은 두 타일
        for (x, y) in [(10, 5), (70, 5), (10, 40), (70, 40)] {
            set(&mut current, width, x, y);
        }
        let rects = changed_rects(&previous, &current, width, height, width * 4);
        assert_eq!(rects, vec![Rectangle::new((0, 0).into(), (128, 64).into())]);
    }

    #[test]
    fn edge_tiles_stop_at_the_buffer() {
        let (width, height) = (100, 50);
        let previous = image(width, height);
        let mut current = previous.clone();
        set(&mut current, width, 99, 49);
        let rects = changed_rects(&previous, &current, width, height, width * 4);
        assert_eq!(
            rects,
            vec![Rectangle::new((64, 32).into(), (36, 18).into())]
        );
    }

    #[test]
    fn only_damage_over_most_of_the_buffer_counts_as_full() {
        let whole = Damage::Buffer(Rectangle::new((0, 0).into(), (850, 718).into()));
        let corner = Damage::Buffer(Rectangle::new((0, 0).into(), (100, 100).into()));
        let everything =
            Damage::Surface(Rectangle::new((0, 0).into(), (i32::MAX, i32::MAX).into()));
        assert!(covers_buffer(&[whole], 850, 718, 1));
        assert!(!covers_buffer(&[corner], 850, 718, 1));
        assert!(covers_buffer(&[everything], 850, 718, 2));
    }
}
