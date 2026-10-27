// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 렌더 요소 묶음과 배경화면

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                memory::MemoryRenderBuffer, memory::MemoryRenderBufferRenderElement,
                surface::WaylandSurfaceRenderElement,
            },
            gles::GlesRenderer,
        },
    },
    desktop::space::SpaceRenderElements,
    utils::{Physical, Rectangle, Size, Transform},
};

use crate::blur::BlurElement;

smithay::backend::renderer::element::render_elements! {
    pub OutputRenderElements<=GlesRenderer>;
    Space = SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Memory = MemoryRenderBufferRenderElement<GlesRenderer>,
    Surface = WaylandSurfaceRenderElement<GlesRenderer>,
    Blur = BlurElement,
}

impl std::fmt::Debug for OutputRenderElements {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Space(element) => f.debug_tuple("Space").field(element).finish(),
            Self::Memory(element) => f.debug_tuple("Memory").field(element).finish(),
            Self::Surface(element) => f.debug_tuple("Surface").field(element).finish(),
            Self::Blur(element) => f.debug_tuple("Blur").field(element).finish(),
            Self::_GenericCatcher(_) => f.write_str("GenericCatcher"),
        }
    }
}

/// 위에서 아래로 이어지는 그라데이션 배경
pub fn wallpaper(top: [u8; 3], bottom: [u8; 3], size: Size<i32, Physical>) -> MemoryRenderBuffer {
    let (width, height) = (size.w.max(1), size.h.max(1));
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let t = if height > 1 {
            y as f32 / (height - 1) as f32
        } else {
            0.0
        };
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        let [r, g, b] = [
            mix(top[0], bottom[0]),
            mix(top[1], bottom[1]),
            mix(top[2], bottom[2]),
        ];
        for _ in 0..width {
            // Argb8888은 리틀 엔디언에서 B, G, R, A 순서다
            pixels.extend_from_slice(&[b, g, r, 0xff]);
        }
    }
    MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Argb8888,
        (width, height),
        1,
        Transform::Normal,
        Some(vec![Rectangle::from_size((width, height).into())]),
    )
}
