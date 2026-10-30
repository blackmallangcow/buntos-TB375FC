// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 렌더 요소 묶음, 숨는 막대의 자리, 배경화면

use std::collections::HashSet;

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                AsRenderElements, Element, Id, memory::MemoryRenderBuffer,
                memory::MemoryRenderBufferRenderElement, solid::SolidColorRenderElement,
                surface::WaylandSurfaceRenderElement,
            },
            gles::GlesRenderer,
        },
    },
    desktop::{LayerSurface, PopupManager, space::SpaceRenderElements},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform},
    wayland::compositor::{TraversalAction, with_surface_tree_downward},
};

use crate::blur::{BlurElement, Uncovered};

smithay::backend::renderer::element::render_elements! {
    pub OutputRenderElements<=GlesRenderer>;
    Space = SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>,
    Memory = MemoryRenderBufferRenderElement<GlesRenderer>,
    Surface = WaylandSurfaceRenderElement<GlesRenderer>,
    Blur = BlurElement,
    // 흐림 요소 위의 불투명한 요소 (blur.rs, 흐릴 범위에서 불투명 영역을 뺀다)
    Uncovered = Uncovered,
    // 스크린샷 영역 고르기 (screenshot.rs)
    Solid = SolidColorRenderElement,
    // 창 최소화 효과 (minimize.rs)
    Minimize = crate::minimize::EffectElement,
}

impl std::fmt::Debug for OutputRenderElements {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Space(element) => f.debug_tuple("Space").field(element).finish(),
            Self::Memory(element) => f.debug_tuple("Memory").field(element).finish(),
            Self::Surface(element) => f.debug_tuple("Surface").field(element).finish(),
            Self::Blur(element) => f.debug_tuple("Blur").field(element).finish(),
            Self::Uncovered(element) => f.debug_tuple("Uncovered").field(element).finish(),
            Self::Solid(element) => f.debug_tuple("Solid").field(element).finish(),
            Self::Minimize(element) => f.debug_tuple("Minimize").field(element).finish(),
            Self::_GenericCatcher(_) => f.write_str("GenericCatcher"),
        }
    }
}

pub type SpaceElement =
    SpaceRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>;

/// 다 보이지 않는 셸 막대 (state.rs의 bar_places)
pub struct BarPlace {
    pub layer: LayerSurface,
    /// 그릴 자리 (출력 안 논리 좌표). None이면 숨어서 그리지 않는다
    pub at: Option<Point<i32, Logical>>,
}

/// Space가 원래 자리에 그린 막대 요소를 빼고, 미끄러지는 막대는 옮긴 자리에 다시 만들어 같은
/// 순서(z)에 넣는다. 요소 ID는 표면에서 오므로 피해 추적과 흐림은 같은 요소가 움직인 것으로 본다
pub fn place_bars(
    renderer: &mut GlesRenderer,
    elements: &mut Vec<SpaceElement>,
    places: &[BarPlace],
    scale: Scale<f64>,
) {
    for place in places {
        let ids = surface_ids(&place.layer);
        let ours = |element: &SpaceElement| match element {
            SpaceRenderElements::Surface(surface) => ids.contains(surface.id()),
            _ => false,
        };
        let Some(first) = elements.iter().position(ours) else {
            continue;
        };
        elements.retain(|element| !ours(element));
        if let Some(at) = place.at {
            let moved = place
                .layer
                .render_elements::<WaylandSurfaceRenderElement<GlesRenderer>>(
                    renderer,
                    at.to_physical_precise_round(scale),
                    scale,
                    1.0,
                );
            elements.splice(
                first..first,
                moved.into_iter().map(SpaceRenderElements::Surface),
            );
        }
    }
}

/// 레이어 표면과 그 하위 표면, 팝업의 렌더 요소 ID
fn surface_ids(layer: &LayerSurface) -> HashSet<Id> {
    let mut ids = HashSet::new();
    let mut add_tree = |root: &WlSurface| {
        with_surface_tree_downward(
            root,
            (),
            |_, _, _| TraversalAction::DoChildren(()),
            |surface, _, _| {
                ids.insert(Id::from_wayland_resource(surface));
            },
            |_, _, _| true,
        );
    };
    add_tree(layer.wl_surface());
    for (popup, _) in PopupManager::popups_for_surface(layer.wl_surface()) {
        add_tree(popup.wl_surface());
    }
    ids
}

/// 위에서 아래로 이어지는 그라데이션 배경. 가로로는 색이 같으므로 폭 1픽셀로 만들고 그릴 때 화면 폭으로
/// 늘린다(불투명 영역도 함께 늘어난다). 전에는 화면 크기 그대로 만들어 CPU 사본과 텍스처를 함께 들었다
/// (2944×1840이면 약 43MB)
pub fn wallpaper(top: [u8; 3], bottom: [u8; 3], size: Size<i32, Physical>) -> MemoryRenderBuffer {
    let height = size.h.max(1);
    let mut pixels = Vec::with_capacity((height * 4) as usize);
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
        // Argb8888은 리틀 엔디언에서 B, G, R, A 순서다
        pixels.extend_from_slice(&[b, g, r, 0xff]);
    }
    MemoryRenderBuffer::from_slice(
        &pixels,
        Fourcc::Argb8888,
        (1, height),
        1,
        Transform::Normal,
        Some(vec![Rectangle::from_size((1, height).into())]),
    )
}
