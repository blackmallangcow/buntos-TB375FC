// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 앱과 배경화면을 그리는 텍스처 셰이더. smithay 0.7.0의 기본 셰이더에서 정밀도만 고쳤다
//!
//! 기본 셰이더는 `precision mediump float`인데, 그 정밀도로는 큰 화면의 텍스처 좌표를 담지 못한다.
//! Mesa는 조각 셰이더의 정밀도를 정점 셰이더의 출력에도 옮기고(`nir_link_varying_precision`), 26.3 이후의
//! Panfrost는 그런 베어링을 정점마다 16비트 부동소수로 저장한 뒤 보간한다. 손상 사각형의 모서리 좌표가
//! 그렇게 반올림되면 0.5~1 구간의 간격이 2^-11이라, 너비 2944 텍스처에서는 1.44화소다. 그래서 일부만
//! 다시 그린 사각형이 화면 오른쪽과 아래쪽에서 최대 0.7화소 어긋나 글자와 테두리가 번졌다. 사각형 끝이 바깥
//! 줄까지 번지기도 해서, 앱이 그 줄을 다시 그리지 않으면 자국으로 남을 수 있다. 화면 전체를 그릴 때는 모서리가
//! 0과 1이라 어긋나지 않고, 베어링을 32비트로 다루는 드라이버(llvmpipe 등)에서는 일어나지 않는다.
//!
//! 같은 고침이 smithay master에 있다(d28cf3808). 그것이 든 릴리스로 올리면 이 모듈을 지운다

use smithay::{
    backend::renderer::{
        element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
        gles::{GlesError, GlesFrame, GlesRenderer, GlesTexProgram},
        utils::{CommitCounter, DamageSet, OpaqueRegions},
    },
    utils::{Buffer, Physical, Point, Rectangle, Scale, Transform},
};

/// smithay 0.7.0의 `texture.frag`(MIT)에서 precision 줄만 바꿨다. highp가 없는 GPU도 있어 mediump로 물러선다
const SHADER: &str = r#"#version 100

//_DEFINES_

#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif

#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif

uniform float alpha;
varying vec2 v_coords;

#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

void main() {
    vec4 color = texture2D(tex, v_coords);

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0) * alpha;
#else
    color = color * alpha;
#endif

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// 렌더러의 GL 컨텍스트에 붙여 두는 셰이더. 만들지 못했으면 None이다
struct Compiled(Option<GlesTexProgram>);

/// 이 렌더러의 셰이더. 처음 부를 때 만든다
fn program(renderer: &mut GlesRenderer) -> Option<GlesTexProgram> {
    if let Some(Compiled(program)) = renderer.egl_context().user_data().get::<Compiled>() {
        return program.clone();
    }
    let program = renderer
        .compile_custom_texture_shader(SHADER, &[])
        .inspect_err(|err| tracing::warn!("failed to compile the texture shader: {err:?}"))
        .ok();
    renderer
        .egl_context()
        .user_data()
        .insert_if_missing(|| Compiled(program.clone()));
    program
}

/// 이 모듈의 셰이더로 그리는 요소. 다른 것은 감싼 요소와 같다. smithay는 렌더러의 기본 셰이더를 바꾸는 길을
/// 열어 두지 않고, 덮어쓰기는 프레임마다라 요소를 그릴 때 건다. 셰이더를 직접 넘기는 요소(최소화 효과)는 그대로다
pub struct Precise<'a, E> {
    element: &'a E,
    program: Option<GlesTexProgram>,
}

/// 화면이나 텍스처에 그리기 전에 요소 목록을 감싼다. 셰이더를 만들지 못했으면 smithay의 기본 셰이더로 그린다
pub fn precise<'a, E>(renderer: &mut GlesRenderer, elements: &'a [E]) -> Vec<Precise<'a, E>> {
    let program = program(renderer);
    elements
        .iter()
        .map(|element| Precise {
            element,
            program: program.clone(),
        })
        .collect()
}

impl<E: Element> Element for Precise<'_, E> {
    fn id(&self) -> &Id {
        self.element.id()
    }

    fn current_commit(&self) -> CommitCounter {
        self.element.current_commit()
    }

    fn location(&self, scale: Scale<f64>) -> Point<i32, Physical> {
        self.element.location(scale)
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        self.element.src()
    }

    fn transform(&self) -> Transform {
        self.element.transform()
    }

    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.element.geometry(scale)
    }

    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.element.damage_since(scale, commit)
    }

    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.element.opaque_regions(scale)
    }

    fn alpha(&self) -> f32 {
        self.element.alpha()
    }

    fn kind(&self) -> Kind {
        self.element.kind()
    }
}

impl<E: RenderElement<GlesRenderer>> RenderElement<GlesRenderer> for Precise<'_, E> {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let Some(program) = &self.program else {
            return self.element.draw(frame, src, dst, damage, opaque_regions);
        };
        frame.override_default_tex_program(program.clone(), Vec::new());
        let drawn = self.element.draw(frame, src, dst, damage, opaque_regions);
        frame.clear_tex_program_override();
        drawn
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        self.element.underlying_storage(renderer)
    }
}

#[cfg(test)]
mod tests {
    use smithay::backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, Offscreen, damage::OutputDamageTracker,
            element::memory::MemoryRenderBuffer, gles::GlesTexture,
        },
    };

    use super::*;
    use crate::testing::{self, Frames};

    /// 화소마다 다른 Argb8888 그림. 어긋나면 거의 모든 화소가 달라진다
    fn noise(width: i32, height: i32, mut seed: u32) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            pixels.extend_from_slice(&(seed | 0xff00_0000).to_le_bytes());
        }
        pixels
    }

    /// 화면(RGBA)이 그림(Argb8888)과 다른 화소 수
    fn differing(shown: &[u8], picture: &[u8]) -> usize {
        let (shown, _) = shown.as_chunks::<4>();
        let (picture, _) = picture.as_chunks::<4>();
        shown
            .iter()
            .zip(picture)
            .filter(|(shown, picture)| {
                (shown[0], shown[1], shown[2]) != (picture[2], picture[1], picture[0])
            })
            .count()
    }

    /// 그림의 한 사각형을 앱이 다시 그린 것처럼 알린다. 내용은 `picture`에 이미 있다
    fn commit(buffer: &mut MemoryRenderBuffer, picture: &[u8], (x, y, w, h): (i32, i32, i32, i32)) {
        let mut context = buffer.render();
        context
            .draw(|memory| {
                memory.copy_from_slice(picture);
                Ok::<_, ()>(vec![Rectangle::new((x, y).into(), (w, h).into())])
            })
            .expect("draw");
    }

    /// 사각형들을 차례로 다시 그려도 화면이 그림과 화소까지 같다. llvmpipe에서는 mediump도 32비트라 고치기
    /// 전에도 통과한다. 고침은 Panfrost가 있는 기기에서 BUNT_TEST_GPU=1로 돌려 본다
    #[test]
    fn partial_redraws_stay_pixel_exact() {
        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let (width, height) = (2944, 1840);
        let mut picture = noise(width, height, 1);
        let mut buffer = MemoryRenderBuffer::from_slice(
            &picture,
            Fourcc::Argb8888,
            (width, height),
            1,
            Transform::Normal,
            None,
        );
        let mut frames = Frames::new(&mut renderer, (width, height).into(), 1.0);
        let element = testing::background(&mut renderer, &buffer);
        let shown = frames.render(&mut renderer, &[element]);
        assert_eq!(differing(&shown, &picture), 0, "the full frame");

        let rects = [
            (0, 1190, 2944, 650),
            (1800, 100, 900, 300),
            (2300, 900, 500, 500),
            (100, 100, 800, 500),
            (2500, 1500, 111, 77),
            (0, 1189, 2944, 1),
        ];
        for (index, (x, y, w, h)) in rects.into_iter().enumerate() {
            let next = noise(width, height, index as u32 + 2);
            let stride = width as usize * 4;
            for row in y as usize..(y + h) as usize {
                let start = row * stride + x as usize * 4;
                let end = start + w as usize * 4;
                picture[start..end].copy_from_slice(&next[start..end]);
            }
            commit(&mut buffer, &picture, (x, y, w, h));
            let element = testing::background(&mut renderer, &buffer);
            let shown = frames.render(&mut renderer, &[element]);
            assert_eq!(differing(&shown, &picture), 0, "rect {x},{y} {w}x{h}");
        }
    }

    /// 패널이 뜰 때는 그 위까지 넓게 다시 그리고, 내려갈 때는 패널 자리만 다시 그린다. 뜰 때 패널 위 끝이 한
    /// 줄 위로 번지면(화면 키보드를 올린 화면의 y 1189에서 보인다) 내려간 뒤에도 그 줄이 남는다. 이 시험도
    /// BUNT_TEST_GPU=1일 때만 고침을 가른다
    #[test]
    fn hiding_a_panel_leaves_no_line_above_it() {
        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let (width, height) = (2944_i32, 1840_i32);
        let panel_top = 1190;
        // 16비트로 반올림하면 위로 0.35화소 늘어나는 자리: 1182 / 1840 ≈ 1316 / 2048 → 1182.4
        let wider_top = 1182;
        let flat = |color: [u8; 4], picture: &mut [u8], from: i32| {
            let (_, rows) = picture.split_at_mut((from * width * 4) as usize);
            rows.as_chunks_mut::<4>()
                .0
                .iter_mut()
                .for_each(|pixel| *pixel = color);
        };
        let (dark, light) = ([60, 30, 30, 255], [200, 200, 200, 255]);
        let mut picture = vec![0; (width * height * 4) as usize];
        flat(dark, &mut picture, 0);
        let mut buffer = MemoryRenderBuffer::from_slice(
            &picture,
            Fourcc::Argb8888,
            (width, height),
            1,
            Transform::Normal,
            None,
        );
        let mut frames = Frames::new(&mut renderer, (width, height).into(), 1.0);
        let element = testing::background(&mut renderer, &buffer);
        frames.render(&mut renderer, &[element]);

        flat(light, &mut picture, panel_top);
        commit(
            &mut buffer,
            &picture,
            (0, wider_top, width, height - wider_top),
        );
        let element = testing::background(&mut renderer, &buffer);
        let shown = frames.render(&mut renderer, &[element]);
        let up = differing(&shown, &picture);

        flat(dark, &mut picture, panel_top);
        commit(
            &mut buffer,
            &picture,
            (0, panel_top, width, height - panel_top),
        );
        let element = testing::background(&mut renderer, &buffer);
        let shown = frames.render(&mut renderer, &[element]);
        let down = differing(&shown, &picture);
        assert_eq!((up, down), (0, 0), "pixels off with the panel up and down");
    }

    /// 감싼 요소는 넘긴 셰이더로 그린다. 표면 요소와 그림 요소가 같은 길로 그려지므로 그림 요소로 본다
    #[test]
    fn wrapped_elements_draw_with_the_program() {
        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        const FLAT: &str = r#"#version 100
//_DEFINES_
#if defined(EXTERNAL)
#extension GL_OES_EGL_image_external : require
#endif
precision mediump float;
#if defined(EXTERNAL)
uniform samplerExternalOES tex;
#else
uniform sampler2D tex;
#endif
uniform float alpha;
varying vec2 v_coords;
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif
void main() {
    gl_FragColor = vec4(0.0, 1.0, 0.0, 1.0) * texture2D(tex, v_coords).a * alpha;
}
"#;
        let flat = renderer
            .compile_custom_texture_shader(FLAT, &[])
            .expect("the test shader");
        let size = (8, 8);
        let picture = vec![0xff; 8 * 8 * 4];
        let buffer = MemoryRenderBuffer::from_slice(
            &picture,
            Fourcc::Argb8888,
            size,
            1,
            Transform::Normal,
            None,
        );
        let element = testing::background(&mut renderer, &buffer);
        let wrapped = [Precise {
            element: &element,
            program: Some(flat),
        }];
        let mut target: GlesTexture = renderer
            .create_buffer(Fourcc::Abgr8888, size.into())
            .expect("a target");
        let mut framebuffer = renderer.bind(&mut target).expect("bind");
        OutputDamageTracker::new(size, 1.0, Transform::Normal)
            .render_output(&mut renderer, &mut framebuffer, 0, &wrapped, [0.0; 4])
            .expect("render");
        let mapping = renderer
            .copy_framebuffer(
                &framebuffer,
                Rectangle::from_size(size.into()),
                Fourcc::Abgr8888,
            )
            .expect("read back");
        let pixels = renderer.map_texture(&mapping).expect("map");
        assert_eq!(testing::pixel(pixels, 8, 4, 4), [0, 255, 0, 255]);
    }

    /// 같은 셰이더를 다시 만들지 않는다
    #[test]
    fn the_program_is_compiled_once() {
        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let elements: [crate::render::OutputRenderElements; 0] = [];
        assert!(precise(&mut renderer, &elements).is_empty());
        let first = renderer.egl_context().user_data().get::<Compiled>();
        assert!(first.is_some_and(|compiled| compiled.0.is_some()));
    }
}
