// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창 최소화 효과 (macOS 27 시스템 설정의 "최소화된 윈도우 애니메이션": 지니 효과, 크기 조절 효과).
//! ⌘M이나 최소화 단추로 창을 내리기 전에 창을 텍스처로 따로 그려 두고, 그 그림이 Dock의 최소화 칸으로
//! 빨려 들어가게 그린다. 칸에서 다시 꺼내면(Dock의 칸이나 앱 아이콘, ⌘⇥, 에이전트) 창은 바로 제자리에
//! 놓여 입력을 받고, 그림이 칸에서 나와 제자리로 가는 동안에는 공간의 창 대신 그림을 그린다.
//! - 고른 효과는 셸이 시스템 설정 값을 알린다(SetMinimizeEffect)
//! - 칸의 자리는 셸이 Dock 표면 안 좌표로 알리고(SetMinimizedSlots), 알리기 전에는 Dock 오른쪽 끝으로 어림한다
//! - 진행은 시작 시각에서 재므로 그릴 때마다 자리가 정해진다(panels.rs의 막대 미끄러짐처럼). 도는 중에
//!   거꾸로 하면 그 자리에서 돌아선다
//! - 지니 효과는 셰이더가 그림을 휜다: 화면의 점마다 그림의 어느 점인지 거꾸로 찾는다(Shape::source와 같은 계산)
//! - 효과가 도는 동안 숨은 Dock을 꺼내 둔다(macOS)

use std::time::{Duration, Instant};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Offscreen, Texture,
            damage::OutputDamageTracker,
            element::{AsRenderElements, Element, Id, Kind, RenderElement},
            gles::{
                GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform,
                UniformName, UniformType,
            },
            utils::CommitCounter,
        },
    },
    desktop::{Window, layer_map_for_output},
    reexports::wayland_server::Resource,
    utils::{Buffer, IsAlive, Logical, Physical, Point, Rectangle, Scale, Transform},
};

use crate::{
    render::{OutputRenderElements, SpaceElement},
    screenshot,
    state::{BuntState, ClientState},
};

/// 지니 효과의 시간 (macOS에서 0.5초쯤)
const GENIE: Duration = Duration::from_millis(500);
/// 크기 조절 효과의 시간 (theme/tokens.css의 --duration-slow)
const SCALE: Duration = Duration::from_millis(320);
/// 지니 효과의 앞 단계 몫: 창 아래쪽이 휘며 칸까지 늘어난다. 나머지 단계에서 위쪽이 칸으로 빨려 든다
const BEND_PART: f64 = 0.4;
/// 셸의 Dock 레이어 이름 (state.rs의 DOCK_NAMESPACE)
const DOCK_NAMESPACE: &str = "bunt-dock";
/// 셸이 칸을 알리기 전에 어림하는 칸의 변과, Dock 판 오른쪽의 그림자·안쪽 여백·테두리 (shell/data/style.css)
const GUESSED_SLOT: i32 = 48;
const DOCK_RIGHT_CHROME: i32 = 28 + 10 + 1;
/// Dock이 없을 때 칸을 어림하는 화면 아래 가장자리와의 틈
const BOTTOM_GAP: i32 = 8;
/// 셸이 알릴 수 있는 칸의 수. 넘는 것은 버린다
pub const MAX_SLOTS: usize = 256;

/// 그림을 휘는 셰이더 (smithay의 사용자 텍스처 셰이더 규칙: EXTERNAL, NO_ALPHA, DEBUG_FLAGS를 다룬다).
/// v_coords는 그리는 사각형(효과가 덮는 범위) 안의 0..1 자리다. rows는 그림의 맨 윗줄·맨 아랫줄과 깔때기가
/// 시작하고 끝나는 y, span은 창 쪽과 칸 쪽의 가로 범위, bend는 깔때기가 휜 정도, size는 그리는 사각형의
/// 픽셀 크기다(가장자리를 픽셀 하나 너비로 부드럽게 한다). 모두 그리는 사각형 기준의 0..1 값이다
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

uniform vec4 rows;
uniform vec4 span;
uniform float bend;
uniform vec2 size;

void main() {
    vec2 p = v_coords;
    float row = (p.y - rows.x) / max(rows.y - rows.x, 0.000001);
    float f = clamp((p.y - rows.z) / max(rows.w - rows.z, 0.000001), 0.0, 1.0);
    f = f * f * (3.0 - 2.0 * f) * bend;
    float left = mix(span.x, span.z, f);
    float right = mix(span.y, span.w, f);
    float column = (p.x - left) / max(right - left, 0.000001);
    float cover_x = clamp(min(p.x - left, right - p.x) * size.x + 0.5, 0.0, 1.0);
    float cover_y = clamp(min(p.y - rows.x, rows.y - p.y) * size.y + 0.5, 0.0, 1.0);
    float cover = cover_x * cover_y;
    if (cover <= 0.0) {
        discard;
    }
    vec4 color = texture2D(tex, vec2(clamp(column, 0.0, 1.0), clamp(row, 0.0, 1.0)));

#if defined(NO_ALPHA)
    color = vec4(color.rgb, 1.0);
#endif
    color = color * (alpha * cover);

#if defined(DEBUG_FLAGS)
    if (tint == 1.0)
        color = vec4(0.0, 0.2, 0.0, 0.2) + color * 0.8;
#endif

    gl_FragColor = color;
}
"#;

/// 최소화 효과 (org.buntos.shell의 minimize-effect)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Effect {
    /// 지니 효과: 창이 휘며 칸으로 빨려 든다 (macOS의 기본값)
    #[default]
    Genie,
    /// 크기 조절 효과: 창이 그대로 줄며 칸으로 간다
    Scale,
}

impl Effect {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "genie" => Some(Self::Genie),
            "scale" => Some(Self::Scale),
            _ => None,
        }
    }

    fn duration(self) -> Duration {
        match self {
            Self::Genie => GENIE,
            Self::Scale => SCALE,
        }
    }
}

/// 효과 한 순간의 모양 (화면 좌표, 논리 픽셀). 그림의 줄은 top에서 bottom까지 고르게 놓이고, y줄의 가로
/// 범위는 창 쪽(from)에서 칸 쪽(to)으로 깔때기(funnel의 두 y 사이)를 따라 좁아진다. bend가 0이면 곧은 사각형이다
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub top: f64,
    pub bottom: f64,
    pub from: (f64, f64),
    pub to: (f64, f64),
    pub funnel: (f64, f64),
    pub bend: f64,
}

impl Shape {
    /// 곧은 사각형
    fn flat(rect: Rectangle<f64, Logical>) -> Self {
        let span = (rect.loc.x, rect.loc.x + rect.size.w);
        Self {
            top: rect.loc.y,
            bottom: rect.loc.y + rect.size.h,
            from: span,
            to: span,
            funnel: (rect.loc.y, rect.loc.y + rect.size.h),
            bend: 0.0,
        }
    }

    /// y줄의 (왼쪽, 오른쪽)
    pub fn span(&self, y: f64) -> (f64, f64) {
        let f = smoothstep(self.funnel.0, self.funnel.1, y) * self.bend;
        (
            lerp(self.from.0, self.to.0, f),
            lerp(self.from.1, self.to.1, f),
        )
    }

    /// 그림이 덮는 범위. 가로 범위는 y를 따라 한쪽으로만 움직이므로 맨 윗줄과 맨 아랫줄에서 가장 넓다
    pub fn bounds(&self) -> Rectangle<f64, Logical> {
        let (top_left, top_right) = self.span(self.top);
        let (bottom_left, bottom_right) = self.span(self.bottom);
        let (left, right) = (top_left.min(bottom_left), top_right.max(bottom_right));
        Rectangle::new(
            (left, self.top).into(),
            (right - left, self.bottom - self.top).into(),
        )
    }

    /// 화면의 점이 그림의 어디(가로, 세로 0..1)인지. 그림 밖이면 None. 셰이더(SHADER)와 같은 계산을 시험에서
    /// 견주려고 둔다
    #[cfg(test)]
    fn source(&self, point: Point<f64, Logical>) -> Option<(f64, f64)> {
        let row = (point.y - self.top) / (self.bottom - self.top).max(f64::EPSILON);
        let (left, right) = self.span(point.y);
        let column = (point.x - left) / (right - left).max(f64::EPSILON);
        ((0.0..=1.0).contains(&row) && (0.0..=1.0).contains(&column)).then_some((column, row))
    }
}

/// 진행(0은 창 자리, 1은 칸)에서의 모양. `window`는 창 영역, `slot`은 칸 안의 창 그림 자리다
pub fn shape(
    effect: Effect,
    window: Rectangle<f64, Logical>,
    slot: Rectangle<f64, Logical>,
    progress: f64,
) -> Shape {
    let t = progress.clamp(0.0, 1.0);
    let (window_top, window_bottom) = (window.loc.y, window.loc.y + window.size.h);
    let (slot_top, slot_bottom) = (slot.loc.y, slot.loc.y + slot.size.h);
    // 칸이 창 위쪽보다 위에 있으면(창을 Dock 아래로 끌어 둔 때) 휠 길이 없으므로 크기 조절로 그린다
    if effect == Effect::Scale || slot_top <= window_top {
        return Shape::flat(lerp_rect(window, slot, smoothstep(0.0, 1.0, t)));
    }
    let bending = smoothstep(0.0, 1.0, t / BEND_PART);
    let sliding = ((t - BEND_PART) / (1.0 - BEND_PART)).clamp(0.0, 1.0);
    Shape {
        // 위쪽은 뒷 단계에 빨라지며 칸으로 들어간다
        top: lerp(window_top, slot_top, sliding * sliding),
        bottom: lerp(window_bottom, slot_bottom, bending),
        from: (window.loc.x, window.loc.x + window.size.w),
        to: (slot.loc.x, slot.loc.x + slot.size.w),
        funnel: (window_top, slot_top),
        bend: bending,
    }
}

/// 칸(`slot`) 가운데에 창 비율을 지켜 맞춘 그림 자리 (Dock이 창 그림을 칸에 그리는 대로)
pub fn fit(
    window: Rectangle<i32, Logical>,
    slot: Rectangle<f64, Logical>,
) -> Rectangle<f64, Logical> {
    let (width, height) = (
        f64::from(window.size.w.max(1)),
        f64::from(window.size.h.max(1)),
    );
    let scale = (slot.size.w / width).min(slot.size.h / height);
    let size = (width * scale, height * scale);
    Rectangle::new(
        (
            slot.loc.x + (slot.size.w - size.0) / 2.0,
            slot.loc.y + (slot.size.h - size.1) / 2.0,
        )
            .into(),
        size.into(),
    )
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn lerp_rect(
    from: Rectangle<f64, Logical>,
    to: Rectangle<f64, Logical>,
    t: f64,
) -> Rectangle<f64, Logical> {
    Rectangle::new(
        (lerp(from.loc.x, to.loc.x, t), lerp(from.loc.y, to.loc.y, t)).into(),
        (
            lerp(from.size.w, to.size.w, t),
            lerp(from.size.h, to.size.h, t),
        )
            .into(),
    )
}

/// 0에서 1로 부드럽게 (천천히 시작해 천천히 멈춘다). `start`와 `end`가 같으면 계단이다
fn smoothstep(start: f64, end: f64, value: f64) -> f64 {
    let t = ((value - start) / (end - start).max(f64::EPSILON)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 효과의 진행. `from`에서 시작해 `since`부터 `duration`에 걸쳐 고르게 `to`로 간다. 늦추고 당기는 것은
/// shape가 단계마다 한다
#[derive(Clone, Copy, Debug, PartialEq)]
struct Motion {
    from: f64,
    to: f64,
    since: Instant,
    duration: Duration,
}

impl Motion {
    /// `full`은 0에서 1까지 가는 시간이다. 가운데에서 시작하면 남은 거리만큼만 걸린다
    fn new(from: f64, to: f64, now: Instant, full: Duration) -> Self {
        Self {
            from,
            to,
            since: now,
            duration: full.mul_f64((to - from).abs().min(1.0)),
        }
    }

    fn value(&self, now: Instant) -> f64 {
        if self.duration.is_zero() {
            return self.to;
        }
        let t = (now.saturating_duration_since(self.since).as_secs_f64()
            / self.duration.as_secs_f64())
        .min(1.0);
        lerp(self.from, self.to, t)
    }

    fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) >= self.duration
    }

    /// `to`로 돌아선다. 지금 자리에서 시작한다
    fn turn(&mut self, to: f64, now: Instant, full: Duration) {
        *self = Self::new(self.value(now), to, now, full);
    }
}

/// 도는 효과 하나
struct Running {
    id: Id,
    commit: CommitCounter,
    /// 마지막으로 그린 모양. 같으면 손상을 내지 않는다
    drawn: Option<Shape>,
    /// 최소화 번호 (셸이 알린 칸을 찾는다)
    minimized: u32,
    window: Window,
    /// 꺼내는 동안 공간에서 빼는 창의 표면 요소 (update_minimize_effects가 채운다)
    surfaces: Vec<Id>,
    texture: GlesTexture,
    /// 제자리의 창 영역 (화면 좌표)
    rect: Rectangle<i32, Logical>,
    /// 칸 안의 창 그림 자리 (화면 좌표). 셸이 칸을 거둔 뒤에도 마지막 자리를 쓴다
    slot: Rectangle<f64, Logical>,
    effect: Effect,
    motion: Motion,
}

impl Running {
    /// 꺼내는 중이다 (칸에서 제자리로)
    fn restoring(&self) -> bool {
        self.motion.to == 0.0
    }
}

/// 최소화 효과들과 그 설정
#[derive(Default)]
pub struct Effects {
    effect: Effect,
    /// 셸이 알린 칸: 최소화 번호와 Dock 표면 안 논리 좌표
    slots: Vec<(u32, Rectangle<i32, Logical>)>,
    running: Vec<Running>,
    program: Option<GlesTexProgram>,
    /// 셰이더를 만들지 못했다. 효과 없이 바로 내리고 놓는다
    broken: bool,
}

impl Effects {
    /// 효과가 도는 중이다 (렌더 루프가 프레임마다 그린다)
    pub fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    /// 숨은 Dock을 꺼내 둘지: 효과가 도는 동안 칸이 보여야 한다
    pub fn reveals_dock(&self) -> bool {
        self.is_running()
    }

    /// 화면 하나(`output`, 전역 논리 좌표)에 그릴 요소. 먼저 시작한 효과가 아래다
    pub fn elements(
        &mut self,
        output: Rectangle<i32, Logical>,
        scale: Scale<f64>,
    ) -> Vec<OutputRenderElements> {
        let Some(program) = &self.program else {
            return Vec::new();
        };
        let now = Instant::now();
        let mut elements = Vec::new();
        for running in self.running.iter_mut().rev() {
            let shape = shape(
                running.effect,
                running.rect.to_f64(),
                running.slot,
                running.motion.value(now),
            );
            if running.drawn != Some(shape) {
                running.drawn = Some(shape);
                running.commit.increment();
            }
            if let Some(element) = EffectElement::new(
                running.id.clone(),
                running.commit,
                running.texture.clone(),
                program.clone(),
                shape,
                output,
                scale,
            ) {
                elements.push(OutputRenderElements::Minimize(element));
            }
        }
        elements
    }

    /// 꺼내는 중인 창의 요소를 공간의 요소에서 뺀다. 그 자리에는 효과가 그림을 그린다
    pub fn hide_restoring(&self, elements: &mut Vec<SpaceElement>) {
        let hidden: Vec<&Id> = self
            .running
            .iter()
            .filter(|running| running.restoring())
            .flat_map(|running| &running.surfaces)
            .collect();
        if !hidden.is_empty() {
            elements.retain(|element| !hidden.contains(&element.id()));
        }
    }

    /// 그 창의 효과
    fn find(&mut self, window: &Window) -> Option<&mut Running> {
        self.running
            .iter_mut()
            .find(|running| running.window == *window)
    }

    /// 셸이 알린 칸 (Dock 표면 안 좌표)
    fn slot(&self, minimized: u32) -> Option<Rectangle<i32, Logical>> {
        self.slots
            .iter()
            .find(|(id, _)| *id == minimized)
            .map(|(_, rect)| *rect)
    }
}

impl BuntState {
    /// 셸이 시스템 설정의 효과를 알렸다
    pub fn set_minimize_effect(&mut self, effect: Effect) {
        self.minimize_effects.effect = effect;
    }

    /// 셸이 Dock의 최소화 칸 자리(Dock 표면 안 논리 좌표)를 알렸다
    pub fn set_minimized_slots(&mut self, slots: Vec<(u32, Rectangle<i32, Logical>)>) {
        self.minimize_effects.slots = slots;
    }

    /// 창을 내리기 전에 부른다 (windows.rs). 창 그림을 떠서 칸으로 빨려 드는 효과를 시작한다. 그 창이
    /// 칸에서 나오는 중이었으면 그 자리에서 돌아선다
    pub fn start_minimize_effect(&mut self, window: &Window, minimized: u32) {
        let now = Instant::now();
        if let Some(running) = self.minimize_effects.find(window) {
            let full = running.effect.duration();
            running.motion.turn(1.0, now, full);
            running.minimized = minimized;
            self.queue_redraw_all();
            return;
        }
        let Some(rect) = self.space.element_geometry(window) else {
            return;
        };
        self.start_effect(
            window,
            minimized,
            rect,
            Motion::new(0.0, 1.0, now, Duration::ZERO),
        );
    }

    /// 내린 창을 제자리에 놓기 전에 부른다 (windows.rs). 그림이 칸에서 나와 `location`(창 영역의 왼쪽 위)으로
    /// 가는 효과를 시작한다. 칸으로 들어가던 중이었으면 그 자리에서 돌아선다
    pub fn start_restore_effect(
        &mut self,
        window: &Window,
        minimized: u32,
        location: Point<i32, Logical>,
    ) {
        let now = Instant::now();
        if let Some(running) = self.minimize_effects.find(window) {
            let full = running.effect.duration();
            running.motion.turn(0.0, now, full);
            self.queue_redraw_all();
            return;
        }
        let rect = Rectangle::new(location, window.geometry().size);
        self.start_effect(
            window,
            minimized,
            rect,
            Motion::new(1.0, 0.0, now, Duration::ZERO),
        );
    }

    /// 창 그림을 그려 효과를 더한다. `motion`의 시간은 고른 효과의 시간으로 바꾼다
    fn start_effect(
        &mut self,
        window: &Window,
        minimized: u32,
        rect: Rectangle<i32, Logical>,
        motion: Motion,
    ) {
        if self.minimize_effects.broken || rect.is_empty() {
            return;
        }
        let scale = self.effect_scale(rect);
        let renderer = self.backend.renderer();
        if self.minimize_effects.program.is_none() {
            match compile(renderer) {
                Ok(program) => self.minimize_effects.program = Some(program),
                Err(err) => {
                    tracing::warn!("minimize effects disabled: {err}");
                    self.minimize_effects.broken = true;
                    return;
                }
            }
        }
        let texture = match window_texture(renderer, window, scale) {
            Ok(texture) => texture,
            Err(err) => {
                tracing::warn!("cannot draw the window for the minimize effect: {err}");
                return;
            }
        };
        let effect = self.minimize_effects.effect;
        let slot = fit(rect, self.slot_area(minimized, rect));
        self.minimize_effects.running.push(Running {
            id: Id::new(),
            commit: CommitCounter::default(),
            drawn: None,
            minimized,
            window: window.clone(),
            surfaces: window_surfaces(window),
            texture,
            rect,
            slot,
            effect,
            motion: Motion::new(motion.from, motion.to, motion.since, effect.duration()),
        });
        self.queue_redraw_all();
    }

    /// 그리기 전에 부른다 (udev.rs). 칸의 자리를 새로 보고(셸이 알렸거나 Dock이 미끄러졌다), 끝났거나 창이
    /// 사라진 효과를 치운다
    pub fn update_minimize_effects(&mut self) {
        if self.minimize_effects.running.is_empty() {
            return;
        }
        let now = Instant::now();
        let dock = self.dock_area();
        let before = self.minimize_effects.running.len();
        let effects = &mut self.minimize_effects;
        effects.running.retain(|running| {
            !running.motion.done(now) && (!running.restoring() || running.window.alive())
        });
        for index in 0..effects.running.len() {
            let (minimized, rect) = {
                let running = &effects.running[index];
                (running.minimized, running.rect)
            };
            if let (Some(local), Some(dock)) = (effects.slot(minimized), dock) {
                let slot = Rectangle::new(local.loc + dock.loc, local.size);
                effects.running[index].slot = fit(rect, slot.to_f64());
            }
            let running = &mut effects.running[index];
            if running.restoring() {
                running.surfaces = window_surfaces(&running.window);
            }
        }
        if effects.running.len() != before {
            // 끝난 효과가 지운 자리와 다시 그리는 창을 그린다
            self.queue_redraw_all();
        }
    }

    /// 효과의 그림을 그릴 배율: 창 가운데가 든 화면의 배율
    fn effect_scale(&self, rect: Rectangle<i32, Logical>) -> f64 {
        let center = rect.to_f64().loc
            + Point::from((f64::from(rect.size.w) / 2.0, f64::from(rect.size.h) / 2.0));
        self.space
            .output_under(center)
            .next()
            .or_else(|| self.space.outputs().next())
            .map_or(1.0, |output| output.current_scale().fractional_scale())
    }

    /// 최소화 칸 (화면 좌표). 셸이 알린 칸이 먼저고, 아직이면 Dock 오른쪽 끝, Dock이 없으면 창이 있는
    /// 화면의 아래 가운데로 어림한다
    fn slot_area(
        &self,
        minimized: u32,
        window: Rectangle<i32, Logical>,
    ) -> Rectangle<f64, Logical> {
        let dock = self.dock_area();
        if let (Some(local), Some(dock)) = (self.minimize_effects.slot(minimized), dock) {
            return Rectangle::new(local.loc + dock.loc, local.size).to_f64();
        }
        let side = GUESSED_SLOT;
        let guess = match dock {
            Some(dock) => Rectangle::new(
                (
                    dock.loc.x + dock.size.w - DOCK_RIGHT_CHROME - side,
                    dock.loc.y + (dock.size.h - side) / 2,
                )
                    .into(),
                (side, side).into(),
            ),
            None => {
                let output = self
                    .space
                    .outputs()
                    .filter_map(|output| self.space.output_geometry(output))
                    .find(|output| output.overlaps(window))
                    .unwrap_or(window);
                Rectangle::new(
                    (
                        output.loc.x + (output.size.w - side) / 2,
                        output.loc.y + output.size.h - side - BOTTOM_GAP,
                    )
                        .into(),
                    (side, side).into(),
                )
            }
        };
        guess.to_f64()
    }

    /// 셸의 Dock 표면이 그려지는 자리 (화면 좌표). 미끄러지는 중이면 옮긴 자리다
    fn dock_area(&self) -> Option<Rectangle<i32, Logical>> {
        self.space.outputs().find_map(|output| {
            let output_geo = self.space.output_geometry(output)?;
            let (dock, rest) = {
                let map = layer_map_for_output(output);
                let dock = map
                    .layers()
                    .find(|layer| {
                        layer.namespace() == DOCK_NAMESPACE
                            && layer
                                .wl_surface()
                                .client()
                                .and_then(|client| {
                                    client.get_data::<ClientState>().map(|data| data.privileged)
                                })
                                .unwrap_or(false)
                    })?
                    .clone();
                let rest = map.layer_geometry(&dock)?;
                (dock, rest)
            };
            // bar_places도 레이어 맵을 잠그므로 위에서 잠금을 푼 뒤에 부른다
            let at = self
                .bar_places(output)
                .into_iter()
                .find(|place| place.layer == dock)
                .and_then(|place| place.at)
                .unwrap_or(rest.loc);
            Some(Rectangle::new(at + output_geo.loc, rest.size))
        })
    }
}

/// 효과 하나를 화면 하나에 그리는 요소. 창 그림을 효과의 범위에 셰이더로 휘어 그린다
pub struct EffectElement {
    id: Id,
    commit: CommitCounter,
    texture: GlesTexture,
    program: GlesTexProgram,
    /// 효과가 덮는 범위 (화면의 물리 좌표, 바깥으로 반올림)
    geometry: Rectangle<i32, Physical>,
    uniforms: Vec<Uniform<'static>>,
}

impl std::fmt::Debug for EffectElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EffectElement")
            .field("id", &self.id)
            .field("commit", &self.commit)
            .field("geometry", &self.geometry)
            .finish_non_exhaustive()
    }
}

impl EffectElement {
    /// `shape`(화면 좌표)를 화면 하나(`output`)의 요소로. 화면에 닿지 않으면 None
    fn new(
        id: Id,
        commit: CommitCounter,
        texture: GlesTexture,
        program: GlesTexProgram,
        shape: Shape,
        output: Rectangle<i32, Logical>,
        scale: Scale<f64>,
    ) -> Option<Self> {
        let (geometry, uniforms) = placement(shape, output, scale)?;
        Some(Self {
            id,
            commit,
            texture,
            program,
            geometry,
            uniforms,
        })
    }
}

/// 모양을 화면의 물리 좌표로 옮긴 그리는 사각형과, 그 사각형 기준(0..1)의 셰이더 값. 화면에 닿지 않으면 None
fn placement(
    shape: Shape,
    output: Rectangle<i32, Logical>,
    scale: Scale<f64>,
) -> Option<(Rectangle<i32, Physical>, Vec<Uniform<'static>>)> {
    let bounds = shape.bounds();
    let x = |value: f64| (value - f64::from(output.loc.x)) * scale.x;
    let y = |value: f64| (value - f64::from(output.loc.y)) * scale.y;
    let (left, top) = (x(bounds.loc.x).floor(), y(bounds.loc.y).floor());
    let (right, bottom) = (
        x(bounds.loc.x + bounds.size.w).ceil(),
        y(bounds.loc.y + bounds.size.h).ceil(),
    );
    let geometry = Rectangle::<i32, Physical>::new(
        (left as i32, top as i32).into(),
        ((right - left) as i32, (bottom - top) as i32).into(),
    );
    let screen = Rectangle::from_size(output.size.to_physical_precise_round(scale));
    if geometry.is_empty() || !geometry.overlaps(screen) {
        return None;
    }
    let (width, height) = ((right - left).max(1.0), (bottom - top).max(1.0));
    let across = |value: f64| ((x(value) - left) / width) as f32;
    let down = |value: f64| ((y(value) - top) / height) as f32;
    let uniforms = vec![
        Uniform::new(
            "rows",
            [
                down(shape.top),
                down(shape.bottom),
                down(shape.funnel.0),
                down(shape.funnel.1),
            ],
        ),
        Uniform::new(
            "span",
            [
                across(shape.from.0),
                across(shape.from.1),
                across(shape.to.0),
                across(shape.to.1),
            ],
        ),
        Uniform::new("bend", shape.bend as f32),
        Uniform::new("size", [width as f32, height as f32]),
    ];
    Some((geometry, uniforms))
}

impl Element for EffectElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size(self.texture.size().to_f64())
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl RenderElement<GlesRenderer> for EffectElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        frame.render_texture_from_to(
            &self.texture,
            src,
            dst,
            damage,
            &[],
            Transform::Normal,
            1.0,
            Some(&self.program),
            &self.uniforms,
        )
    }
}

/// 창의 표면과 하위 표면, 팝업의 렌더 요소 ID
fn window_surfaces(window: &Window) -> Vec<Id> {
    window
        .toplevel()
        .map(|toplevel| screenshot::surface_ids(toplevel.wl_surface()))
        .unwrap_or_default()
}

/// 그림을 휘는 셰이더를 만든다
fn compile(renderer: &mut GlesRenderer) -> Result<GlesTexProgram, GlesError> {
    renderer.compile_custom_texture_shader(
        SHADER,
        &[
            UniformName::new("rows", UniformType::_4f),
            UniformName::new("span", UniformType::_4f),
            UniformName::new("bend", UniformType::_1f),
            UniformName::new("size", UniformType::_2f),
        ],
    )
}

/// 창 영역(그림자 밖은 뺀다)을 `scale` 배율의 텍스처에 따로 그린다. Dock 칸의 창 그림(windows.rs의
/// thumbnail)과 같은 범위라 효과가 끝나는 모습이 칸의 그림과 맞는다
fn window_texture(
    renderer: &mut GlesRenderer,
    window: &Window,
    scale: f64,
) -> Result<GlesTexture, String> {
    let geometry = window.geometry();
    let size = geometry.size.to_physical_precise_round(scale);
    if size.is_empty() {
        return Err("the window has no size".into());
    }
    let mut texture: GlesTexture = renderer
        .create_buffer(
            Fourcc::Abgr8888,
            size.to_logical(1).to_buffer(1, Transform::Normal),
        )
        .map_err(|err| err.to_string())?;
    // 표면의 원점은 창 영역보다 그림자만큼 왼쪽 위에 있다
    let origin = Point::<i32, Logical>::from((-geometry.loc.x, -geometry.loc.y))
        .to_physical_precise_round(scale);
    let elements: Vec<OutputRenderElements> =
        window.render_elements(renderer, origin, Scale::from(scale), 1.0);
    let mut framebuffer = renderer.bind(&mut texture).map_err(|err| err.to_string())?;
    OutputDamageTracker::new(size, scale, Transform::Normal)
        .render_output(renderer, &mut framebuffer, 0, &elements, [0.0; 4])
        .map_err(|err| format!("{err:?}"))?;
    drop(framebuffer);
    Ok(texture)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::backend::renderer::ImportMem;

    use crate::testing::{self, Frames};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Rectangle<f64, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// 1280×800 화면 위쪽의 창과 아래 Dock의 칸
    fn window() -> Rectangle<f64, Logical> {
        rect(200.0, 100.0, 600.0, 400.0)
    }

    fn slot() -> Rectangle<f64, Logical> {
        rect(900.0, 730.0, 48.0, 32.0)
    }

    #[test]
    fn effects_have_names() {
        assert_eq!(Effect::parse("genie"), Some(Effect::Genie));
        assert_eq!(Effect::parse("scale"), Some(Effect::Scale));
        assert_eq!(Effect::parse("suck"), None);
        // macOS의 기본값
        assert_eq!(Effect::default(), Effect::Genie);
    }

    #[test]
    fn the_effect_starts_on_the_window_and_ends_in_the_slot() {
        for effect in [Effect::Genie, Effect::Scale] {
            let start = shape(effect, window(), slot(), 0.0);
            assert_eq!(start.bounds(), window(), "{effect:?}");
            assert_eq!(start.span(450.0), (200.0, 800.0), "{effect:?}");
            // 맨 왼쪽 위는 그림의 맨 왼쪽 위
            assert_eq!(start.source((200.0, 100.0).into()), Some((0.0, 0.0)));
            let end = shape(effect, window(), slot(), 1.0);
            let bounds = end.bounds();
            assert!(
                close(bounds.loc.x, 900.0) && close(bounds.loc.y, 730.0),
                "{bounds:?}"
            );
            assert!(
                close(bounds.size.w, 48.0) && close(bounds.size.h, 32.0),
                "{bounds:?}"
            );
            // 범위 밖의 진행도 끝에 머문다
            assert_eq!(shape(effect, window(), slot(), 1.5), end);
            assert_eq!(shape(effect, window(), slot(), -1.0), start);
        }
    }

    #[test]
    fn the_genie_bends_the_bottom_into_the_slot_first() {
        // 앞 단계 끝: 위쪽은 그대로이고 아래쪽은 칸에 닿아 칸 너비가 된다
        let bent = shape(Effect::Genie, window(), slot(), BEND_PART);
        assert!(
            close(bent.top, 100.0) && close(bent.bottom, 762.0),
            "{bent:?}"
        );
        let (left, right) = bent.span(100.0);
        assert!(close(left, 200.0) && close(right, 800.0));
        let (left, right) = bent.span(740.0);
        assert!(close(left, 900.0) && close(right, 948.0));
        // 가운데 줄은 그 사이로 좁아지며 칸 쪽으로 기운다
        let (left, right) = bent.span(400.0);
        assert!(left > 200.0 && left < 900.0 && right > 800.0 && right < 948.0);
        assert!(right - left < 600.0);
        // 뒷 단계: 위쪽이 내려가 칸으로 들어간다
        let late = shape(Effect::Genie, window(), slot(), 0.8);
        assert!(late.top > 100.0 && late.top < 730.0);
        assert!(close(late.bottom, 762.0));
        // 그림 밖의 점은 없다: 앞 단계 끝의 아래쪽 왼편은 그림 밖이다
        assert_eq!(bent.source((210.0, 700.0).into()), None);
        assert!(bent.source((924.0, 750.0).into()).is_some());
    }

    #[test]
    fn the_scale_effect_shrinks_the_window_without_bending() {
        let half = shape(Effect::Scale, window(), slot(), 0.5);
        assert_eq!(half.bend, 0.0);
        // 가운데 진행에서 크기와 자리가 반쯤 (부드럽게 시작하고 멈추므로 0.5에서 딱 반이다)
        let bounds = half.bounds();
        assert!(
            close(bounds.loc.x, 550.0) && close(bounds.size.w, 324.0),
            "{bounds:?}"
        );
        assert_eq!(half.span(400.0), half.span(700.0));
    }

    #[test]
    fn a_slot_above_the_window_falls_back_to_scaling() {
        let low_window = rect(200.0, 740.0, 600.0, 300.0);
        let genie = shape(Effect::Genie, low_window, slot(), 0.5);
        assert_eq!(genie, shape(Effect::Scale, low_window, slot(), 0.5));
    }

    #[test]
    fn the_shape_stays_inside_its_bounds_and_maps_back_to_the_picture() {
        for step in 0..=20 {
            let progress = f64::from(step) / 20.0;
            let shape = shape(Effect::Genie, window(), slot(), progress);
            let bounds = shape.bounds();
            assert!(shape.top <= shape.bottom, "{progress}");
            // 줄마다의 가로 범위는 범위 안이다
            for row in 0..=10 {
                let y = lerp(shape.top, shape.bottom, f64::from(row) / 10.0);
                let (left, right) = shape.span(y);
                assert!(
                    left >= bounds.loc.x - 1e-9 && right <= bounds.loc.x + bounds.size.w + 1e-9
                );
                assert!(left <= right);
                // 줄의 가운데 점은 그림의 가로 가운데, 세로는 그 줄의 차례다
                let (column, line) = shape.source(((left + right) / 2.0, y).into()).unwrap();
                assert!(close(column, 0.5), "{progress} {row}");
                assert!(close(line, f64::from(row) / 10.0), "{progress} {row}");
            }
        }
    }

    #[test]
    fn pictures_fit_the_slot_like_the_dock_draws_them() {
        let wide = Rectangle::new((0, 0).into(), (800, 400).into());
        assert_eq!(
            fit(wide, rect(1000.0, 700.0, 48.0, 48.0)),
            rect(1000.0, 712.0, 48.0, 24.0)
        );
        let tall = Rectangle::new((0, 0).into(), (300, 600).into());
        assert_eq!(
            fit(tall, rect(0.0, 0.0, 48.0, 48.0)),
            rect(12.0, 0.0, 24.0, 48.0)
        );
        // 크기가 없는 창도 나누기가 터지지 않는다
        let empty = Rectangle::new((0, 0).into(), (0, 0).into());
        assert_eq!(
            fit(empty, rect(0.0, 0.0, 48.0, 48.0)),
            rect(0.0, 0.0, 48.0, 48.0)
        );
    }

    #[test]
    fn motions_reverse_where_they_are_and_take_the_remaining_time() {
        let start = Instant::now();
        let mut motion = Motion::new(0.0, 1.0, start, GENIE);
        assert_eq!(motion.value(start), 0.0);
        assert!(close(motion.value(start + GENIE / 2), 0.5));
        assert!(!motion.done(start + GENIE / 2));
        // 가운데에서 돌아서면 그 자리에서 시작해 남은 거리(반)만큼만 걸린다
        motion.turn(0.0, start + GENIE / 2, GENIE);
        assert!(close(motion.value(start + GENIE / 2), 0.5));
        assert_eq!(motion.duration, GENIE / 2);
        assert!(close(motion.value(start + GENIE * 3 / 4), 0.25));
        assert!(motion.done(start + GENIE));
        assert_eq!(motion.value(start + GENIE * 2), 0.0);
        // 시간이 없으면 바로 끝이다
        let instant = Motion::new(1.0, 0.0, start, Duration::ZERO);
        assert_eq!(instant.value(start), 0.0);
        assert!(instant.done(start));
    }

    #[test]
    fn placement_moves_the_shape_onto_each_screen() {
        let shape = shape(
            Effect::Scale,
            rect(1300.0, 100.0, 200.0, 100.0),
            slot(),
            0.0,
        );
        // 오른쪽 화면(1280부터, 125%): 화면 안 물리 좌표
        let right = Rectangle::new((1280, 0).into(), (1024, 640).into());
        let (geometry, uniforms) = placement(shape, right, Scale::from(1.25)).unwrap();
        assert_eq!(
            geometry,
            Rectangle::new((25, 125).into(), (250, 125).into())
        );
        // 곧은 사각형은 그리는 사각형 전체다
        assert_eq!(uniforms[0].value, [0.0f32, 1.0, 0.0, 1.0].into());
        assert_eq!(uniforms[1].value, [0.0f32, 1.0, 0.0, 1.0].into());
        assert_eq!(uniforms[3].value, [250.0f32, 125.0].into());
        // 왼쪽 화면에는 닿지 않는다
        let left = Rectangle::new((0, 0).into(), (1280, 800).into());
        assert!(placement(shape, left, Scale::from(1.0)).is_none());
    }

    /// 왼쪽 위 사분면이 빨강, 나머지는 파랑인 RGBA 그림 텍스처
    fn quadrants(renderer: &mut GlesRenderer, width: i32, height: i32) -> GlesTexture {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let red = x < width / 2 && y < height / 2;
                pixels.extend_from_slice(if red {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        renderer
            .import_memory(&pixels, Fourcc::Abgr8888, (width, height).into(), false)
            .expect("import the picture")
    }

    fn draw(
        renderer: &mut GlesRenderer,
        frames: &mut Frames,
        texture: &GlesTexture,
        program: &GlesTexProgram,
        shape: Shape,
        commit: &mut CommitCounter,
    ) -> Vec<u8> {
        let output = Rectangle::new((0, 0).into(), (64, 48).into());
        commit.increment();
        let element = EffectElement::new(
            Id::new(),
            *commit,
            texture.clone(),
            program.clone(),
            shape,
            output,
            Scale::from(1.0),
        )
        .expect("on the screen");
        frames.render(renderer, &[OutputRenderElements::Minimize(element)])
    }

    #[test]
    fn the_shader_draws_the_window_picture_bent_into_the_slot() {
        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let program = compile(&mut renderer).expect("the shader compiles");
        let texture = quadrants(&mut renderer, 32, 16);
        let mut frames = Frames::new(&mut renderer, (64, 48).into(), 1.0);
        let mut commit = CommitCounter::default();
        let window = rect(8.0, 4.0, 32.0, 16.0);
        let slot = rect(48.0, 40.0, 8.0, 4.0);
        let pixel = |pixels: &[u8], x, y| testing::pixel(pixels, 64, x, y);
        // 시작: 창 자리에 그대로. 왼쪽 위 사분면은 빨강, 오른쪽 아래는 파랑, 창 밖은 바탕(검정)
        let start = shape(Effect::Genie, window, slot, 0.0);
        let pixels = draw(
            &mut renderer,
            &mut frames,
            &texture,
            &program,
            start,
            &mut commit,
        );
        assert_eq!(pixel(&pixels, 12, 7), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 35, 17), [0, 0, 255, 255]);
        assert_eq!(pixel(&pixels, 4, 2), [0, 0, 0, 255]);
        // 앞 단계 끝: 아래쪽이 칸으로 휘었다. 창 아래 왼쪽 모서리 자리는 그림 밖이 된다
        let bent = shape(Effect::Genie, window, slot, BEND_PART);
        let pixels = draw(
            &mut renderer,
            &mut frames,
            &texture,
            &program,
            bent,
            &mut commit,
        );
        assert_eq!(pixel(&pixels, 9, 19), [0, 0, 0, 255]);
        // 위쪽 줄은 그대로 창 너비다
        assert_eq!(pixel(&pixels, 9, 4), [255, 0, 0, 255]);
        // 칸 안은 그림의 아래쪽(파랑)이다
        assert_eq!(pixel(&pixels, 52, 42), [0, 0, 255, 255]);
        // 끝: 칸 안에만 그림이 있다
        let end = shape(Effect::Genie, window, slot, 1.0);
        let pixels = draw(
            &mut renderer,
            &mut frames,
            &texture,
            &program,
            end,
            &mut commit,
        );
        assert_eq!(pixel(&pixels, 12, 7), [0, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 49, 40), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, 55, 43), [0, 0, 255, 255]);
    }
}
