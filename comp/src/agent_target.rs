// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AI 대상 강조 (AI 동시 사용 설계 6장): 에이전트가 다루려는 요소의 자리를 AI 색(systemPurple) 둥근 테두리로 잠깐
//! 보인다. 사람의 시스템 포커스 링(강조 색)과 다른 색·모양이다.
//!
//! - 데몬이 ShowAgentTarget으로 창 기준 논리 사각형을 알리면 TARGET_SHOWN_FOR(1.2초) 동안 보인다. 새 강조는 앞의 것을
//!   바꾼다. 사람만 답하는 창이 떠 있거나 사람이 AI 제어를 멈춘 동안에는 받지 않는다(agent.rs).
//! - 창을 따라 움직인다: 그릴 때마다 창의 지금 자리에 놓는다. 창 밖으로 나간 부분은 자르고, 창이 내려지면 그리지 않는다.
//! - 입력을 받지 않는 그림일 뿐이다. 대상 창의 맨 앞 표면 바로 위에 그린다(udev.rs): 그 창을 덮은 창과 막대는 테두리도
//!   덮어, 사람이 쓰는 창 위에 AI 테두리가 뜨지 않는다. 에이전트의 창 그림(capture_window)은 창만 그리므로 들지 않고,
//!   사람의 스크린샷에도 커서처럼 넣지 않는다. 화면 기록에는 든다.
//! - 잠긴 동안에는 그리지 않는다. 보일 때와 사라질 때만 다시 그린다(계속 그리는 루프가 없다).

use std::{collections::HashMap, time::Duration};

use smithay::{
    backend::renderer::{
        element::{Id, Kind},
        gles::{
            GlesError, GlesPixelProgram, GlesRenderer, Uniform, UniformName, UniformType,
            element::PixelShaderElement,
        },
    },
    desktop::Window,
    reexports::calloop::{
        RegistrationToken,
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Rectangle},
};

use crate::{dbus::Refused, state::BuntState};

/// 대상 강조를 보이는 시간
const TARGET_SHOWN_FOR: Duration = Duration::from_millis(1200);

/// 강조할 대상: 창과 그 창 영역 기준 논리 사각형(창 안으로 자른 것)
struct Target {
    window: Window,
    rect: Rectangle<i32, Logical>,
}

/// 보이는 대상 강조, 그것을 거둘 타이머, 테두리를 그리는 것 (BuntState::agent_target)
#[derive(Default)]
pub struct AgentTarget {
    shown: Option<Target>,
    timer: Option<RegistrationToken>,
    highlight: Highlight,
}

impl AgentTarget {
    /// 출력 하나에 그릴 테두리. `target`은 BuntState::agent_target_rect의 자리다 (udev.rs)
    pub fn element(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &str,
        output_geo: Rectangle<i32, Logical>,
        target: Option<Rectangle<i32, Logical>>,
        scale: f64,
    ) -> Option<PixelShaderElement> {
        self.highlight
            .element(renderer, output, output_geo, target, scale)
    }
}

impl BuntState {
    /// 그 창 기준 사각형을 AI 색 테두리로 TARGET_SHOWN_FOR 동안 보인다(ShowAgentTarget). 창 밖으로 나간 부분은
    /// 자르고(접근성 extents는 스크롤로 가려진 부분까지 알린다), 다 나갔으면 그리지 않는다. 새 강조는 앞의 것을 바꾼다
    pub(crate) fn agent_show_target(
        &mut self,
        pid: u32,
        title: &str,
        rect: Rectangle<i32, Logical>,
    ) -> Result<(), Refused> {
        let window = self.agent_window(pid, title).ok_or(Refused::NoWindow)?;
        let bounds = Rectangle::from_size(window.geometry().size);
        self.clear_agent_target();
        let Some(rect) = rect.intersection(bounds).filter(|rect| !rect.is_empty()) else {
            return Ok(());
        };
        self.agent_target.shown = Some(Target { window, rect });
        let timer = self.loop_handle.insert_source(
            Timer::from_duration(TARGET_SHOWN_FOR),
            |_, _, state| {
                state.agent_target.timer = None;
                state.clear_agent_target();
                TimeoutAction::Drop
            },
        );
        match timer {
            Ok(token) => self.agent_target.timer = Some(token),
            Err(err) => {
                tracing::warn!("failed to time the agent's target outline: {}", err.error);
                self.agent_target.shown = None;
            }
        }
        self.queue_redraw_all();
        Ok(())
    }

    /// 대상 강조를 거두고 그 자리를 다시 그린다 (시간이 다 됐거나 사람이 AI 제어를 멈췄다)
    pub(crate) fn clear_agent_target(&mut self) {
        if let Some(token) = self.agent_target.timer.take() {
            self.loop_handle.remove(token);
        }
        if self.agent_target.shown.take().is_some() {
            self.queue_redraw_all();
        }
    }

    /// 대상 강조의 자리(전역 논리 좌표): 창의 지금 자리에 창 기준 사각형을 놓는다(창을 따라 움직인다). 창이 내려졌거나
    /// 닫혔으면 None (udev.rs가 프레임마다 묻는다)
    pub fn agent_target_rect(&self) -> Option<Rectangle<i32, Logical>> {
        let target = self.agent_target.shown.as_ref()?;
        let geometry = self.space.element_geometry(&target.window)?;
        Rectangle::new(geometry.loc + target.rect.loc, target.rect.size).intersection(geometry)
    }

    /// 대상 창의 표면(하위 표면과 열린 메뉴 포함)의 렌더 요소 ID. 테두리를 그 창의 맨 앞 표면 바로 위에 끼운다(udev.rs):
    /// 그 창을 덮은 창과 막대는 테두리도 덮는다. 사람의 창 위에 AI 테두리가 떠 사람의 창을 겨누는 것처럼 보이지 않게 한다
    pub fn agent_target_ids(&self) -> Vec<Id> {
        self.agent_target
            .shown
            .as_ref()
            .and_then(|target| target.window.toplevel())
            .map(|toplevel| crate::screenshot::surface_ids(toplevel.wl_surface()))
            .unwrap_or_default()
    }
}

/// 테두리 선의 두께, 바깥 빛의 너비, 대상 사각형의 모서리 반지름 (논리 픽셀)
const STROKE: f32 = 2.0;
const GLOW: f32 = 8.0;
const CORNER: f32 = 6.0;
/// 바깥 빛의 가장 진한 불투명도
const GLOW_ALPHA: f32 = 0.3;
/// systemPurple (HIG 2025 값, theme/tokens.css의 `--purple`·`--agent`와 같다. 셸의 메뉴 막대 AI 표시와 같은 색이다).
/// 컴포지터는 밝은·어두운 화면을 모르므로 밝은 화면의 값 rgb(203, 48, 224)를 쓴다. 어두운 화면의 값은 rgb(219, 52, 242)다
const COLOR: [f32; 3] = [203.0 / 255.0, 48.0 / 255.0, 224.0 / 255.0];

/// 둥근 사각형 테두리와 옅은 바깥 빛. 선은 대상 사각형 바로 바깥에 그려 대상을 가리지 않고, 안쪽은 투명하다.
/// 가장자리는 물리 픽셀 하나만큼 부드럽게 한다. smithay 픽셀 셰이더 규약(#version 없이 GLSL ES 1.00, v_coords·size·alpha·
/// DEBUG_FLAGS)을 따른다
const SHADER: &str = r#"
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

varying vec2 v_coords;
uniform vec2 size;
uniform float alpha;
#if defined(DEBUG_FLAGS)
uniform float tint;
#endif

uniform vec4 rect;
uniform float corner;
uniform float stroke_width;
uniform float glow_width;
uniform float glow_alpha;
uniform float scale;
uniform vec3 color;

float rounded_box(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + vec2(r);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

void main() {
    vec2 p = v_coords * size;
    float d = rounded_box(p - (rect.xy + rect.zw * 0.5), rect.zw * 0.5, corner);
    float px = 1.0 / scale;
    float half_width = stroke_width * 0.5;
    float line = clamp((half_width - abs(d - half_width)) / px + 0.5, 0.0, 1.0);
    float fade = clamp(1.0 - (d - stroke_width) / glow_width, 0.0, 1.0);
    float glow = d > stroke_width ? fade * fade * glow_alpha : 0.0;
    float a = max(line, glow) * alpha;
    gl_FragColor = vec4(color * a, a);
#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        gl_FragColor = vec4(0.0, 0.2, 0.0, 0.2) + gl_FragColor * 0.8;
#endif
}
"#;

/// 테두리를 그릴 요소의 자리(출력 기준 논리 좌표)와 그 안의 대상 사각형. 대상(전역 논리 좌표) 둘레에 선과 빛의 자리를
/// 둔다. 그 출력에 걸치지 않으면 None
fn placement(
    target: Rectangle<i32, Logical>,
    output: Rectangle<i32, Logical>,
) -> Option<(Rectangle<i32, Logical>, Rectangle<i32, Logical>)> {
    let margin = (STROKE + GLOW).ceil() as i32;
    let area = Rectangle::new(
        (target.loc.x - margin, target.loc.y - margin).into(),
        (target.size.w + 2 * margin, target.size.h + 2 * margin).into(),
    );
    if target.size.w <= 0 || target.size.h <= 0 || !area.overlaps(output) {
        return None;
    }
    let local = Rectangle::new(
        (area.loc.x - output.loc.x, area.loc.y - output.loc.y).into(),
        area.size,
    );
    let inner = Rectangle::new((margin, margin).into(), target.size);
    Some((local, inner))
}

/// 대상 사각형의 모서리 반지름. 작은 요소는 반지름이 크기의 반을 넘지 않는다
fn corner(rect: Rectangle<i32, Logical>) -> f32 {
    CORNER.min(rect.size.w.min(rect.size.h) as f32 / 2.0)
}

fn uniforms(rect: Rectangle<i32, Logical>, scale: f64) -> Vec<Uniform<'static>> {
    vec![
        Uniform::new(
            "rect",
            [
                rect.loc.x as f32,
                rect.loc.y as f32,
                rect.size.w as f32,
                rect.size.h as f32,
            ],
        ),
        Uniform::new("corner", corner(rect)),
        Uniform::new("stroke_width", STROKE),
        Uniform::new("glow_width", GLOW),
        Uniform::new("glow_alpha", GLOW_ALPHA),
        Uniform::new("scale", scale.max(0.1) as f32),
        Uniform::new("color", COLOR),
    ]
}

fn compile(renderer: &mut GlesRenderer) -> Result<GlesPixelProgram, GlesError> {
    renderer.compile_custom_pixel_shader(
        SHADER,
        &[
            UniformName::new("rect", UniformType::_4f),
            UniformName::new("corner", UniformType::_1f),
            UniformName::new("stroke_width", UniformType::_1f),
            UniformName::new("glow_width", UniformType::_1f),
            UniformName::new("glow_alpha", UniformType::_1f),
            UniformName::new("scale", UniformType::_1f),
            UniformName::new("color", UniformType::_3f),
        ],
    )
}

/// 테두리를 그린 요소 하나와 그 자리
struct Drawn {
    area: Rectangle<i32, Logical>,
    rect: Rectangle<i32, Logical>,
    scale: f64,
    element: PixelShaderElement,
}

/// 대상 강조 테두리를 그리는 셰이더와 화면마다 그린 요소
#[derive(Default)]
struct Highlight {
    /// 셰이더. 아직 만들지 않았으면 None, 만들지 못했으면 Some(None)
    program: Option<Option<GlesPixelProgram>>,
    /// 출력 이름마다 그린 요소. 자리가 그대로면 같은 요소(같은 ID)를 다시 내어 그 영역을 새로 그리지 않는다
    drawn: HashMap<String, Drawn>,
}

impl Highlight {
    /// 출력 하나에 그릴 테두리. `target`은 대상 사각형(전역 논리 좌표)이고, 없거나 그 출력에 걸치지 않으면 그리지 않는다
    fn element(
        &mut self,
        renderer: &mut GlesRenderer,
        output: &str,
        output_geo: Rectangle<i32, Logical>,
        target: Option<Rectangle<i32, Logical>>,
        scale: f64,
    ) -> Option<PixelShaderElement> {
        let Some((area, rect)) = target.and_then(|target| placement(target, output_geo)) else {
            self.drawn.remove(output);
            return None;
        };
        if let Some(drawn) = self.drawn.get(output)
            && drawn.area == area
            && drawn.rect == rect
            && drawn.scale == scale
        {
            return Some(drawn.element.clone());
        }
        let program = self.program(renderer)?;
        let element = PixelShaderElement::new(
            program,
            area,
            None,
            1.0,
            uniforms(rect, scale),
            Kind::Unspecified,
        );
        self.drawn.insert(
            output.to_string(),
            Drawn {
                area,
                rect,
                scale,
                element: element.clone(),
            },
        );
        Some(element)
    }

    fn program(&mut self, renderer: &mut GlesRenderer) -> Option<GlesPixelProgram> {
        self.program
            .get_or_insert_with(|| match compile(renderer) {
                Ok(program) => Some(program),
                Err(err) => {
                    tracing::warn!("cannot make the AI target outline shader: {err}");
                    None
                }
            })
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outline_sits_around_the_target_on_each_output() {
        let target = Rectangle::new((100, 50).into(), (200, 30).into());
        let left_output = Rectangle::new((0, 0).into(), (1280, 800).into());
        let (area, rect) = placement(target, left_output).unwrap();
        // 선과 빛의 자리(10)만큼 둘레를 둔다
        assert_eq!(area, Rectangle::new((90, 40).into(), (220, 50).into()));
        assert_eq!(rect, Rectangle::new((10, 10).into(), (200, 30).into()));
        // 오른쪽 출력에서는 그 출력 기준 좌표다
        let right_output = Rectangle::new((1280, 0).into(), (1280, 800).into());
        let target = Rectangle::new((1300, 100).into(), (50, 50).into());
        assert_eq!(
            placement(target, right_output).unwrap().0.loc,
            (10, 90).into()
        );
        // 걸치지 않는 출력과 크기가 없는 대상에는 그리지 않는다
        assert_eq!(placement(target, left_output), None);
        let far = Rectangle::new((3000, 100).into(), (50, 50).into());
        assert_eq!(placement(far, right_output), None);
        let empty = Rectangle::new((100, 100).into(), (0, 20).into());
        assert_eq!(placement(empty, left_output), None);
        // 작은 요소의 모서리는 크기의 반을 넘지 않는다
        assert_eq!(corner(Rectangle::from_size((200, 30).into())), CORNER);
        assert_eq!(corner(Rectangle::from_size((200, 8).into())), 4.0);
    }

    /// 테두리를 llvmpipe로 그려 본다: 대상 바로 바깥의 선은 systemPurple이고, 대상 안쪽과 빛 밖은 바탕 그대로다.
    /// 소프트웨어 EGL 장치가 없으면 건너뛴다
    #[test]
    fn the_outline_is_drawn_in_purple_around_the_target() {
        use crate::{render::OutputRenderElements, testing};

        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let size = (120, 80);
        let mut frames = testing::Frames::new(&mut renderer, size.into(), 1.0);
        let output = Rectangle::from_size(size.into());
        let target = Rectangle::new((30, 30).into(), (60, 20).into());
        let mut highlight = Highlight::default();
        let element = highlight
            .element(&mut renderer, "test", output, Some(target), 1.0)
            .expect("the outline shader compiles");
        // 자리가 그대로면 같은 요소를 다시 낸다
        let again = highlight
            .element(&mut renderer, "test", output, Some(target), 1.0)
            .unwrap();
        use smithay::backend::renderer::element::Element;
        assert_eq!(element.id(), again.id());
        let pixels = frames.render(&mut renderer, &[OutputRenderElements::Highlight(element)]);
        let at = |x, y| testing::pixel(&pixels, size.0 as usize, x, y);
        // 위쪽 선의 가운데 (대상 위 1픽셀)
        let line = at(60, 29);
        let purple = [203u8, 48, 224];
        for (got, want) in line.iter().zip(purple) {
            assert!(got.abs_diff(want) <= 24, "line {line:?}");
        }
        // 대상 안쪽은 검은 바탕 그대로다
        assert_eq!(at(60, 40), [0, 0, 0, 255]);
        // 바깥 빛은 옅고, 빛 밖은 바탕 그대로다
        let glow = at(60, 25);
        assert!(glow[2] > 0 && glow[2] < 120, "glow {glow:?}");
        assert_eq!(at(5, 5), [0, 0, 0, 255]);
        // 대상이 없으면 그리지 않고 요소를 버린다
        assert!(
            highlight
                .element(&mut renderer, "test", output, None, 1.0)
                .is_none()
        );
        assert!(highlight.drawn.is_empty());
    }
}
