// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 셸 유리와 앱 창 뒤 흐림 (macOS의 머티리얼처럼). 두 가지를 흐린다.
//! - 셸 유리: 특권 셸 클라이언트의 위층 레이어 표면과 그 팝업. 셸 쪽 프로토콜은 쓰지 않고, 표면의 알파가
//!   0.5 이상인 곳(유리)에만 합성한다. 그림자처럼 옅은 곳과 투명한 곳은 그대로 둔다
//! - 앱이 요청한 영역 (ext-background-effect-v1, background_effect.rs): 앱이 정한 곳이므로 알파와
//!   상관없이 영역 전체에 합성한다. 표면 크기 밖은 자른다
//!
//! 표면 바로 아래에 흐림 요소(BlurElement)를 끼워 둔다. 화면을 아래 요소부터 그리므로 이 요소가 그려질
//! 차례에 화면 버퍼의 그 자리에는 표면 아래 장면만 있다. 그 부분을 복사해 dual Kawase(축소 3번,
//! 확대 2번)로 흐려 합성한다.
//!
//! 아래 장면이 바뀐 프레임에만 다시 흐린다. 그때는 요소의 커밋을 올려 손상 추적이 영역 전체를 다시
//! 그리게 하고(그래야 복사한 영역에 지난 프레임의 유리가 섞이지 않는다), 나머지 프레임은 흐려 둔 결과를
//! 손상된 부분에만 합성한다. 흐릴 표면이 없으면 하는 일이 없다.
//!
//! 손상 추적은 불투명한 요소 아래를 그리지 않으므로, 흐림 요소 위의 요소(흐릴 표면 자신 포함)는 흐릴
//! 범위에서 불투명 영역을 빼고 알린다(Uncovered). 흐릴 곳이 위 요소에 다 가려지면 흐림 요소를 넣지 않는다.
//!
//! 셸 유리는 표면 텍스처를 그리기 밖에서 가림(mask)으로 읽는다. smithay는 텍스처의 필터를 그릴 때 정하므로
//! 여기서도 정한다. SHM 텍스처와 dmabuf 텍스처가 같다. dmabuf가 외부 텍스처(GL_TEXTURE_EXTERNAL_OES, YUV
//! 같은 형식)면 sampler2D로 읽을 수 없어 흐리지 않는다

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::CStr,
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
    sync::Arc,
};

use smithay::{
    backend::{
        allocator::Buffer as _,
        renderer::{
            Renderer,
            element::{Element, Id, Kind, RenderElement, UnderlyingStorage},
            gles::{GlesError, GlesFrame, GlesRenderer, GlesTexture, ffi},
            utils::{CommitCounter, DamageSet, OpaqueRegions, with_renderer_surface_state},
        },
    },
    output::{Output, WeakOutput},
    reexports::wayland_server::{
        Resource,
        backend::ObjectId,
        protocol::{wl_buffer::WlBuffer, wl_shm, wl_surface::WlSurface},
    },
    utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Transform},
    wayland::{dmabuf::get_dmabuf, shm::with_buffer_contents},
};

use crate::render::OutputRenderElements;

/// 축소 단계 수. 단계마다 크기가 반이 된다 (1/2, 1/4, 1/8)
const LEVELS: usize = 3;
/// 표본 간격 (클수록 넓게 흐린다)
const OFFSET: f32 = 2.0;
/// 흐린 배경의 채도. macOS의 vibrancy처럼 조금 높인다
const SATURATION: f32 = 1.4;
/// 유리로 보는 알파. 이 사이에서 부드럽게 흐림이 켜진다 (그림자와 가장자리는 덜 흐린다)
const MASK_LOW: f32 = 0.35;
const MASK_HIGH: f32 = 0.5;

const VERTEX: &str = "#version 100
attribute vec2 position;
varying vec2 v;
void main() {
    v = position;
    gl_Position = vec4(position * 2.0 - 1.0, 0.0, 1.0);
}
";

const DOWN: &str = "#version 100
precision mediump float;
uniform sampler2D tex;
uniform vec2 halfpixel;
uniform float offset;
varying vec2 v;
void main() {
    vec4 sum = texture2D(tex, v) * 4.0;
    sum += texture2D(tex, v - halfpixel * offset);
    sum += texture2D(tex, v + halfpixel * offset);
    sum += texture2D(tex, v + vec2(halfpixel.x, -halfpixel.y) * offset);
    sum += texture2D(tex, v - vec2(halfpixel.x, -halfpixel.y) * offset);
    gl_FragColor = sum / 8.0;
}
";

const UP: &str = "#version 100
precision mediump float;
uniform sampler2D tex;
uniform vec2 halfpixel;
uniform float offset;
varying vec2 v;
void main() {
    vec2 h = halfpixel * offset;
    vec4 sum = texture2D(tex, v + vec2(-h.x * 2.0, 0.0));
    sum += texture2D(tex, v + vec2(-h.x, h.y)) * 2.0;
    sum += texture2D(tex, v + vec2(0.0, h.y * 2.0));
    sum += texture2D(tex, v + vec2(h.x, h.y)) * 2.0;
    sum += texture2D(tex, v + vec2(h.x * 2.0, 0.0));
    sum += texture2D(tex, v + vec2(h.x, -h.y)) * 2.0;
    sum += texture2D(tex, v + vec2(0.0, -h.y * 2.0));
    sum += texture2D(tex, v + vec2(-h.x, -h.y)) * 2.0;
    gl_FragColor = sum / 12.0;
}
";

/// 흐린 배경을 표면 알파로 가려 합성한다. smithay는 화면 버퍼에 물리 좌표 그대로(위가 GL의 0) 그리므로
/// 복사한 텍스처와 표면 텍스처 모두 v=0이 표면의 위다 (위아래가 뒤집힌 dmabuf는 mask_rect로 뒤집는다).
/// 앱이 요청한 영역은 가리지 않는다(use_mask 0)
const COMPOSITE: &str = "#version 100
precision mediump float;
uniform sampler2D tex;
uniform sampler2D mask;
uniform float use_mask;
uniform float saturation;
uniform vec2 edges;
uniform vec4 mask_rect;
varying vec2 v;
void main() {
    float m = 1.0;
    if (use_mask > 0.5) {
        float alpha = texture2D(mask, mask_rect.xy + v * mask_rect.zw).a;
        m = smoothstep(edges.x, edges.y, alpha);
        if (m <= 0.0) {
            discard;
        }
    }
    vec3 color = texture2D(tex, v).rgb;
    float luma = dot(color, vec3(0.2126, 0.7152, 0.0722));
    color = clamp(mix(vec3(luma), color, saturation), 0.0, 1.0);
    gl_FragColor = vec4(color * m, m);
}
";

type GLuint = ffi::types::GLuint;
type GLint = ffi::types::GLint;

/// 흐림에 쓰는 GL 프로그램. 컴포지터의 GL 컨텍스트에서 처음 그릴 때 만든다
struct Programs {
    down: Program,
    up: Program,
    composite: Program,
    /// 사각형 정점 (0,0)-(1,1)
    quad: GLuint,
    /// GLES 3 이상: smithay가 남긴 인스턴싱 divisor를 지워야 한다
    divisor: bool,
}

struct Program {
    id: GLuint,
    tex: GLint,
    mask: GLint,
    use_mask: GLint,
    halfpixel: GLint,
    offset: GLint,
    saturation: GLint,
    edges: GLint,
    /// 흐린 영역(화면 안쪽 부분)이 표면 텍스처의 어디인지: 시작 x, y, 너비, 높이 (0~1)
    mask_rect: GLint,
}

/// 흐림 버퍼 한 장 (텍스처와 그것을 그리는 프레임버퍼)
#[derive(Clone, Copy, Default)]
struct Level {
    texture: GLuint,
    framebuffer: GLuint,
    width: i32,
    height: i32,
}

/// 표면 하나의 흐림 버퍼: 복사본과 단계별 축소본
#[derive(Default)]
struct Targets {
    width: i32,
    height: i32,
    copy: Level,
    levels: Vec<Level>,
    /// levels[0]에 흐려 둔 결과가 어느 커밋의 것인지
    blurred: Option<CommitCounter>,
}

#[derive(Default)]
struct Shared {
    programs: Option<Programs>,
    /// 프로그램을 만들지 못했다. 흐림을 끈다
    broken: bool,
    /// 사라진 표면의 버퍼. 다음에 컨텍스트를 쓸 때 지운다
    garbage: Vec<Targets>,
}

struct SurfaceBlur {
    id: Id,
    commit: CommitCounter,
    /// 흐릴 범위와 그 아래에서 겹치는 요소들의 자리
    fingerprint: u64,
    /// 겹치는 요소마다 지난 프레임에 본 커밋. 그 뒤의 손상이 흐릴 범위에 닿을 때만 다시 흐린다
    seen: HashMap<Id, CommitCounter>,
    targets: Rc<RefCell<Targets>>,
    /// 유리 범위를 구한 버퍼 커밋과 그 결과 (glass_bounds)
    bounds: Option<(CommitCounter, Bounds)>,
}

/// 표면 버퍼에서 유리(알파가 MASK_LOW 이상)가 있는 범위
#[derive(Clone, Copy, Debug, PartialEq)]
enum Bounds {
    /// 버퍼를 읽을 수 없다. 표면 전체를 흐린다
    Unknown,
    /// 유리가 없다. 흐리지 않는다
    Empty,
    /// 버퍼 크기에 대한 비율: 왼쪽, 위, 오른쪽, 아래
    Area([f64; 4]),
}

/// 유리 범위를 찾을 때 건너뛰는 픽셀 수 (가로세로). 찾은 범위는 이만큼 넓힌다
const SCAN_STEP: usize = 4;

/// 흐릴 표면과 흐릴 곳
#[derive(Clone, Debug)]
pub struct BlurTarget {
    pub surface: WlSurface,
    pub shape: BlurShape,
}

#[derive(Clone, Debug)]
pub enum BlurShape {
    /// 셸 유리: 표면의 알파가 MASK_LOW 이상인 곳
    Glass,
    /// 앱이 요청한 영역 (표면 기준 논리 좌표, 겹치지 않는 사각형). 알파와 상관없이 다 흐린다
    Region(Arc<[Rectangle<i32, Logical>]>),
}

impl BlurTarget {
    pub fn glass(surface: WlSurface) -> Self {
        Self {
            surface,
            shape: BlurShape::Glass,
        }
    }
}

/// 흐림 상태. 출력마다가 아니라 표면마다 버퍼를 둔다
#[derive(Default)]
pub struct Blur {
    shared: Rc<RefCell<Shared>>,
    /// 표면과 화면마다 따로 둔다. 표면마다 하나만 두었을 때는 두 화면에 걸친 창이 화면마다 다른 자리로
    /// 지문을 바꿔, 두 화면이 그릴 때마다 다시 흐리고 크기가 다르면 버퍼도 새로 만들었다
    surfaces: HashMap<(ObjectId, WeakOutput), SurfaceBlur>,
}

impl Blur {
    /// `output`에 그릴 이번 프레임의 요소 목록(위에서 아래 순)에서 `targets` 각각의 표면 바로 아래에 흐림
    /// 요소를 끼운다. 하나라도 끼웠으면 true. 전에는 셸 유리 표면만 받았다
    pub fn insert(
        &mut self,
        renderer: &mut GlesRenderer,
        elements: &mut Vec<OutputRenderElements>,
        targets: &[BlurTarget],
        output: &Output,
        scale: Scale<f64>,
    ) -> bool {
        self.collect_garbage(renderer, targets);
        if targets.is_empty() || self.shared.borrow().broken {
            return false;
        }
        let mut inserted = false;
        let context = renderer.context_id();
        // 아래 표면부터 끼워, 위 표면의 지문에 아래 흐림 요소의 변화도 들어가게 한다
        let mut targets: Vec<(usize, &BlurTarget)> = targets
            .iter()
            .filter_map(|target| {
                let id = Id::from_wayland_resource(&target.surface);
                elements
                    .iter()
                    .position(|element| element.id() == &id)
                    .map(|index| (index, target))
            })
            .collect();
        targets.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
        for (_, target) in targets {
            let surface = &target.surface;
            let id = Id::from_wayland_resource(surface);
            let Some(index) = elements.iter().position(|element| element.id() == &id) else {
                continue;
            };
            // 앱이 커서 표면에 흐림을 요청해도 커서 아래는 흐리지 않는다 (커서는 커서 평면에 오른다)
            if elements[index].kind() == Kind::Cursor {
                continue;
            }
            let surface_geometry = elements[index].geometry(scale);
            if surface_geometry.is_empty() {
                continue;
            }
            let entry = self
                .surfaces
                .entry((surface.id(), output.downgrade()))
                .or_insert_with(|| SurfaceBlur {
                    id: Id::new(),
                    commit: CommitCounter::default(),
                    fingerprint: 0,
                    seen: HashMap::new(),
                    targets: Rc::default(),
                    bounds: None,
                });
            let (geometry, mask, parts) = match &target.shape {
                BlurShape::Glass => {
                    let Some((mask, buffer_commit, buffer)) =
                        with_renderer_surface_state(surface, |state| {
                            state
                                .texture::<GlesTexture>(context.clone())
                                .cloned()
                                .map(|mask| (mask, state.current_commit(), state.buffer().cloned()))
                        })
                        .flatten()
                    else {
                        continue;
                    };
                    // smithay는 렌더러가 그릴 수 있다고 알린 형식(render formats)이 아닌 dmabuf를 외부
                    // 텍스처로 가져온다. 가림 셰이더의 sampler2D로는 읽을 수 없다
                    if buffer
                        .as_ref()
                        .and_then(|buffer| get_dmabuf(buffer).ok())
                        .is_some_and(|dmabuf| {
                            !renderer
                                .egl_context()
                                .dmabuf_render_formats()
                                .contains(&dmabuf.format())
                        })
                    {
                        continue;
                    }
                    // 모든 앱 패널처럼 화면 전체를 덮는 투명한 표면도 있으므로, 유리가 있는 범위만
                    // 흐린다. dmabuf는 CPU로 읽지 않으므로 표면 전체를 흐릴 범위로 둔다(가림은 그대로다)
                    let bounds = match entry.bounds {
                        Some((commit, bounds)) if commit == buffer_commit => bounds,
                        _ => {
                            let bounds = buffer
                                .as_ref()
                                .map_or(Bounds::Unknown, |buffer| glass_bounds(buffer));
                            entry.bounds = Some((buffer_commit, bounds));
                            bounds
                        }
                    };
                    let geometry = match bounds {
                        Bounds::Empty => continue,
                        Bounds::Unknown => surface_geometry,
                        Bounds::Area([left, top, right, bottom]) => {
                            let size = surface_geometry.size.to_f64();
                            let x0 = (left * size.w).floor() as i32;
                            let y0 = (top * size.h).floor() as i32;
                            let x1 = (right * size.w).ceil() as i32;
                            let y1 = (bottom * size.h).ceil() as i32;
                            Rectangle::new(
                                (surface_geometry.loc.x + x0, surface_geometry.loc.y + y0).into(),
                                ((x1 - x0).max(1), (y1 - y0).max(1)).into(),
                            )
                        }
                    };
                    (geometry, Some(mask), Vec::new())
                }
                BlurShape::Region(rects) => {
                    let parts = region_parts(rects, surface_geometry, scale);
                    let Some(geometry) = parts.iter().copied().reduce(Rectangle::merge) else {
                        continue;
                    };
                    (geometry, None, parts)
                }
            };
            // 흐릴 곳이 위 요소(흐릴 표면 자신 포함)의 불투명 영역에 다 가려지면 흐리지 않는다. 가려진 흐림을
            // 위해 그 아래를 다시 그리지 않고(uncover), 다 가려진 창은 보이지 않는 표면으로 남아 프레임
            // 알림이 줄어든다
            let shown = if parts.is_empty() {
                vec![geometry]
            } else {
                parts.clone()
            };
            if Rectangle::subtract_rects_many_in_place(
                shown,
                opaque_area(&elements[..=index], scale),
            )
            .is_empty()
            {
                continue;
            }
            // 흐릴 범위 아래에서 겹치는 요소가 바뀌거나 움직였거나, 그 손상이 흐릴 범위에 닿으면
            // 다시 흐린다. 커밋만 보면 Dock 아래에 걸친 창이 멀리 떨어진 곳을 그릴 때마다(시스템
            // 모니터의 그래프 등) Dock 전체를 다시 흐렸다. 앱 영역의 모양이 바뀌어도 다시 그린다
            let mut hasher = DefaultHasher::new();
            hash_rectangle(geometry, &mut hasher);
            for part in &parts {
                hash_rectangle(*part, &mut hasher);
            }
            let mut damaged = false;
            let mut seen = HashMap::new();
            for element in &elements[index + 1..] {
                let below = element.geometry(scale);
                if !below.overlaps(geometry) {
                    continue;
                }
                element.id().hash(&mut hasher);
                hash_rectangle(below, &mut hasher);
                let commit = element.current_commit();
                if let Some(&last) = entry.seen.get(element.id())
                    && last != commit
                {
                    // 손상은 요소 기준 좌표다
                    damaged |= element.damage_since(scale, Some(last)).iter().any(|rect| {
                        Rectangle::new(rect.loc + below.loc, rect.size).overlaps(geometry)
                    });
                }
                seen.insert(element.id().clone(), commit);
            }
            entry.seen = seen;
            let fingerprint = hasher.finish();
            if entry.fingerprint != fingerprint
                || damaged
                || entry.commit == CommitCounter::default()
            {
                entry.fingerprint = fingerprint;
                entry.commit.increment();
            }
            elements.insert(
                index + 1,
                OutputRenderElements::Blur(BlurElement {
                    id: entry.id.clone(),
                    commit: entry.commit,
                    geometry,
                    surface: surface_geometry,
                    mask,
                    parts,
                    targets: entry.targets.clone(),
                    shared: self.shared.clone(),
                }),
            );
            inserted = true;
        }
        // 앱 창 흐림을 더하며 고쳤다: 흐릴 범위 위의 불투명한 요소가 그 아래를 손상 추적에서 가리지 않게 한다
        if inserted {
            uncover(elements, scale);
        }
        inserted
    }

    /// 흐림 셰이더를 만들지 못해 흐림을 껐다 (그 뒤로는 흐리지 않는다)
    pub fn broken(&self) -> bool {
        self.shared.borrow().broken
    }

    /// 흐리지 않게 된 표면과 없어진 화면의 버퍼를 지운다
    fn collect_garbage(&mut self, renderer: &mut GlesRenderer, targets: &[BlurTarget]) {
        let alive: Vec<ObjectId> = targets.iter().map(|target| target.surface.id()).collect();
        let gone: Vec<(ObjectId, WeakOutput)> = self
            .surfaces
            .keys()
            .filter(|(id, output)| !alive.contains(id) || !output.is_alive())
            .cloned()
            .collect();
        let mut shared = self.shared.borrow_mut();
        for id in gone {
            if let Some(surface) = self.surfaces.remove(&id)
                && let Ok(targets) = Rc::try_unwrap(surface.targets)
            {
                shared.garbage.push(targets.into_inner());
            }
        }
        if shared.garbage.is_empty() {
            return;
        }
        let garbage = std::mem::take(&mut shared.garbage);
        let result = renderer.with_context(|gl| {
            for targets in &garbage {
                // 안전: 이 컨텍스트에서 만든 이름이고 더는 쓰지 않는다
                unsafe { targets.delete(gl) };
            }
        });
        if let Err(err) = result {
            tracing::warn!("failed to free blur buffers: {err}");
        }
    }
}

/// shm 버퍼에서 알파가 MASK_LOW 이상인 곳의 범위. 몇 픽셀씩 건너뛰며 보고 그만큼 넓힌다
fn glass_bounds(buffer: &WlBuffer) -> Bounds {
    let threshold = (MASK_LOW * 255.0) as u8;
    let result = with_buffer_contents(buffer, |pointer, length, data| {
        if data.format != wl_shm::Format::Argb8888 {
            return Bounds::Unknown;
        }
        let (width, height, stride) = (
            data.width.max(0) as usize,
            data.height.max(0) as usize,
            data.stride.max(0) as usize,
        );
        let offset = data.offset.max(0) as usize;
        if width == 0 || height == 0 || offset + stride * height > length || stride < width * 4 {
            return Bounds::Unknown;
        }
        // 안전: 위에서 버퍼가 풀 안에 있는지 확인했다
        let bytes = unsafe { std::slice::from_raw_parts(pointer.add(offset), stride * height) };
        let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
        for y in (0..height).step_by(SCAN_STEP) {
            let row = &bytes[y * stride..y * stride + width * 4];
            for x in (0..width).step_by(SCAN_STEP) {
                // Argb8888은 리틀 엔디언에서 B, G, R, A 순서다
                if row[x * 4 + 3] >= threshold {
                    left = left.min(x);
                    right = right.max(x);
                    top = top.min(y);
                    bottom = bottom.max(y);
                }
            }
        }
        if right < left {
            return Bounds::Empty;
        }
        let (width, height) = (width as f64, height as f64);
        Bounds::Area([
            left.saturating_sub(SCAN_STEP) as f64 / width,
            top.saturating_sub(SCAN_STEP) as f64 / height,
            ((right + SCAN_STEP + 1) as f64 / width).min(1.0),
            ((bottom + SCAN_STEP + 1) as f64 / height).min(1.0),
        ])
    });
    result.unwrap_or(Bounds::Unknown)
}

fn hash_rectangle(rectangle: Rectangle<i32, Physical>, hasher: &mut impl Hasher) {
    (
        rectangle.loc.x,
        rectangle.loc.y,
        rectangle.size.w,
        rectangle.size.h,
    )
        .hash(hasher);
}

/// 앱이 준 흐림 영역(표면 기준 논리 좌표)을 출력의 물리 좌표로 옮기고 표면(`surface`) 안으로 자른다.
/// 이웃한 사각형 사이에 틈이 생기지 않게 끝점을 따로 반올림한다. 앱이 보낸 아주 크거나 음수인 값도
/// 실수로 계산한 뒤 표면 안으로 잘라 넘치지 않는다
fn region_parts(
    rects: &[Rectangle<i32, Logical>],
    surface: Rectangle<i32, Physical>,
    scale: Scale<f64>,
) -> Vec<Rectangle<i32, Physical>> {
    let (left, top) = (f64::from(surface.loc.x), f64::from(surface.loc.y));
    let right = f64::from(surface.loc.x.saturating_add(surface.size.w));
    let bottom = f64::from(surface.loc.y.saturating_add(surface.size.h));
    let to_x = |x: f64| (left + x * scale.x).round().clamp(left, right) as i32;
    let to_y = |y: f64| (top + y * scale.y).round().clamp(top, bottom) as i32;
    rects
        .iter()
        .filter_map(|rect| {
            let x0 = to_x(f64::from(rect.loc.x));
            let y0 = to_y(f64::from(rect.loc.y));
            let x1 = to_x(f64::from(rect.loc.x) + f64::from(rect.size.w));
            let y1 = to_y(f64::from(rect.loc.y) + f64::from(rect.size.h));
            (x1 > x0 && y1 > y0).then(|| Rectangle::from_extremities((x0, y0), (x1, y1)))
        })
        .collect()
}

/// 흐림을 합성할 사각형 (화면 버퍼의 물리 좌표). `damage`는 `dst` 기준이고, `parts`가 있으면(앱 영역)
/// 그 안으로 자른다. 프레임마다 그리므로 모아 두지 않고 하나씩 낸다
fn scissors<'a>(
    dst: Rectangle<i32, Physical>,
    damage: &'a [Rectangle<i32, Physical>],
    parts: &'a [Rectangle<i32, Physical>],
) -> impl Iterator<Item = Rectangle<i32, Physical>> + 'a {
    damage
        .iter()
        .map(move |rect| Rectangle::new(rect.loc + dst.loc, rect.size))
        .flat_map(move |rect| {
            let whole = parts.is_empty().then_some(rect);
            whole.into_iter().chain(
                parts
                    .iter()
                    .filter_map(move |part| rect.intersection(*part)),
            )
        })
}

/// 가림 텍스처에서 읽을 범위: 시작 x, y, 너비, 높이 (0~1). `clip`은 흐리는 부분, `surface`는 표면
/// 텍스처가 덮는 범위다. 위아래가 뒤집힌 텍스처(y_inverted dmabuf)는 아래에서 위로 읽는다
fn mask_rect(
    clip: Rectangle<i32, Physical>,
    surface: Rectangle<i32, Physical>,
    flipped: bool,
) -> [f32; 4] {
    let x = (clip.loc.x - surface.loc.x) as f32 / surface.size.w as f32;
    let y = (clip.loc.y - surface.loc.y) as f32 / surface.size.h as f32;
    let w = clip.size.w as f32 / surface.size.w as f32;
    let h = clip.size.h as f32 / surface.size.h as f32;
    if flipped {
        [x, 1.0 - y, w, -h]
    } else {
        [x, y, w, h]
    }
}

/// 흐릴 표면 바로 아래에 끼우는 흐림 요소
pub struct BlurElement {
    id: Id,
    commit: CommitCounter,
    /// 흐릴 범위 (셸 표면의 유리 부분, 앱 영역의 바깥 사각형)
    geometry: Rectangle<i32, Physical>,
    /// 표면 전체. 표면 텍스처가 덮는 범위다
    surface: Rectangle<i32, Physical>,
    /// 셸 유리: 표면의 텍스처. 알파로 흐릴 곳을 정한다. 앱 영역은 가리지 않는다(None)
    mask: Option<GlesTexture>,
    /// 앱 영역의 사각형들 (출력의 물리 좌표). 비었으면 흐릴 범위 전체에 합성한다
    parts: Vec<Rectangle<i32, Physical>>,
    targets: Rc<RefCell<Targets>>,
    shared: Rc<RefCell<Shared>>,
}

impl std::fmt::Debug for BlurElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlurElement")
            .field("id", &self.id)
            .field("commit", &self.commit)
            .field("geometry", &self.geometry)
            .finish()
    }
}

impl Element for BlurElement {
    fn id(&self) -> &Id {
        &self.id
    }

    fn current_commit(&self) -> CommitCounter {
        self.commit
    }

    fn src(&self) -> Rectangle<f64, Buffer> {
        Rectangle::from_size(
            (
                f64::from(self.geometry.size.w),
                f64::from(self.geometry.size.h),
            )
                .into(),
        )
    }

    fn geometry(&self, _scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.geometry
    }

    fn kind(&self) -> Kind {
        Kind::Unspecified
    }
}

impl RenderElement<GlesRenderer> for BlurElement {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        _src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        let mut shared = self.shared.borrow_mut();
        if shared.broken {
            return Ok(());
        }
        let mut targets = self.targets.borrow_mut();
        let mask = self
            .mask
            .as_ref()
            .map(|mask| (mask.tex_id(), mask.is_y_inverted()));
        frame.with_context(|gl| {
            // 안전: 컴포지터의 컨텍스트가 현재이고, 바꾼 GL 상태는 모두 되돌린다
            unsafe {
                draw(
                    gl,
                    &mut shared,
                    &mut targets,
                    mask,
                    self.surface,
                    dst,
                    scissors(dst, damage, &self.parts),
                    self.commit,
                )
            }
        })
    }
}

/// 흐림 요소보다 위에 놓여 흐릴 범위를 불투명하다고 알리는 요소. 흐릴 범위 안에서는 불투명 영역을 빼고
/// 알린다. 나머지는 감싼 요소 그대로다
///
/// smithay의 손상 추적은 위 요소가 불투명하다고 알린 곳 아래를 그리지도 지우지도 않는다. 흐림은 흐릴 범위의
/// 화면 버퍼를 통째로 복사하므로, 앱 창 위에 겹친 불투명한 창 아래에 남은 옛 프레임(그 창 자신, 지우지 않은
/// 버퍼)이 창 가장자리 옆으로 번졌고, 창이 비킨 뒤에도 흐려 둔 사본에 남았다. 셸 유리 위에는 불투명한 요소가
/// 거의 없었지만 앱 창 위에는 흔하다. 대신 흐릴 범위 안에서는 가려진 아래 요소도 그린다
pub struct Uncovered {
    element: Box<OutputRenderElements>,
    /// 불투명 영역에서 뺄 흐릴 범위 (출력의 물리 좌표)
    holes: Vec<Rectangle<i32, Physical>>,
}

impl std::fmt::Debug for Uncovered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Uncovered")
            .field("element", &self.element)
            .field("holes", &self.holes)
            .finish()
    }
}

impl Element for Uncovered {
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
        // 불투명 영역은 요소 기준 좌표다 (smithay가 geometry의 위치를 더한다)
        let origin = self.element.geometry(scale).loc;
        let holes = self
            .holes
            .iter()
            .map(|hole| Rectangle::new(hole.loc - origin, hole.size));
        Rectangle::subtract_rects_many_in_place(self.element.opaque_regions(scale).to_vec(), holes)
            .into_iter()
            .collect()
    }

    fn alpha(&self) -> f32 {
        self.element.alpha()
    }

    fn kind(&self) -> Kind {
        self.element.kind()
    }
}

impl RenderElement<GlesRenderer> for Uncovered {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), GlesError> {
        self.element.draw(frame, src, dst, damage, opaque_regions)
    }

    fn underlying_storage(&self, renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        self.element.underlying_storage(renderer)
    }
}

/// 요소들의 불투명 영역 (출력의 물리 좌표)
fn opaque_area(
    elements: &[OutputRenderElements],
    scale: Scale<f64>,
) -> Vec<Rectangle<i32, Physical>> {
    elements
        .iter()
        .flat_map(|element| {
            let origin = element.geometry(scale).loc;
            element
                .opaque_regions(scale)
                .into_iter()
                .map(move |rect| Rectangle::new(rect.loc + origin, rect.size))
        })
        .collect()
}

/// 흐림 요소보다 위에 있으면서 그 흐릴 범위에 불투명 영역이 걸친 요소를 Uncovered로 감싼다 (흐릴 표면
/// 자신도 흐림 요소 바로 위에 있으므로 들어간다). 요소는 위에서 아래 순이다
fn uncover(elements: &mut Vec<OutputRenderElements>, scale: Scale<f64>) {
    let mut blurs: Vec<Rectangle<i32, Physical>> = Vec::new();
    let mut holes: Vec<Option<Vec<Rectangle<i32, Physical>>>> = Vec::with_capacity(elements.len());
    // 아래에서 위로 올라가며 지나온 흐림 요소의 범위를 모은다
    for element in elements.iter().rev() {
        if let OutputRenderElements::Blur(blur) = element {
            blurs.push(blur.geometry);
            holes.push(None);
            continue;
        }
        let geometry = element.geometry(scale);
        let opaque = element.opaque_regions(scale);
        let covered: Vec<_> = blurs
            .iter()
            .copied()
            .filter(|blur| {
                opaque
                    .iter()
                    .any(|rect| Rectangle::new(rect.loc + geometry.loc, rect.size).overlaps(*blur))
            })
            .collect();
        holes.push((!covered.is_empty()).then_some(covered));
    }
    if holes.iter().all(Option::is_none) {
        return;
    }
    holes.reverse();
    *elements = std::mem::take(elements)
        .into_iter()
        .zip(holes)
        .map(|(element, holes)| match holes {
            Some(holes) => OutputRenderElements::Uncovered(Uncovered {
                element: Box::new(element),
                holes,
            }),
            None => element,
        })
        .collect();
}

/// smithay 프레임 도중에 바꾸는 GL 상태. 그리기 전에 적고 끝나면 되돌린다
struct Saved {
    framebuffer: GLint,
    viewport: [GLint; 4],
    scissor: [GLint; 4],
    scissor_enabled: bool,
    blend_enabled: bool,
    blend: [GLint; 4],
    program: GLint,
    active_texture: GLint,
    textures: [GLint; 2],
    array_buffer: GLint,
}

impl Saved {
    unsafe fn save(gl: &ffi::Gles2) -> Self {
        unsafe {
            let mut saved = Self {
                framebuffer: 0,
                viewport: [0; 4],
                scissor: [0; 4],
                scissor_enabled: gl.IsEnabled(ffi::SCISSOR_TEST) == ffi::TRUE,
                blend_enabled: gl.IsEnabled(ffi::BLEND) == ffi::TRUE,
                blend: [0; 4],
                program: 0,
                active_texture: 0,
                textures: [0; 2],
                array_buffer: 0,
            };
            gl.GetIntegerv(ffi::FRAMEBUFFER_BINDING, &mut saved.framebuffer);
            gl.GetIntegerv(ffi::VIEWPORT, saved.viewport.as_mut_ptr());
            gl.GetIntegerv(ffi::SCISSOR_BOX, saved.scissor.as_mut_ptr());
            gl.GetIntegerv(ffi::BLEND_SRC_RGB, &mut saved.blend[0]);
            gl.GetIntegerv(ffi::BLEND_DST_RGB, &mut saved.blend[1]);
            gl.GetIntegerv(ffi::BLEND_SRC_ALPHA, &mut saved.blend[2]);
            gl.GetIntegerv(ffi::BLEND_DST_ALPHA, &mut saved.blend[3]);
            gl.GetIntegerv(ffi::CURRENT_PROGRAM, &mut saved.program);
            gl.GetIntegerv(ffi::ACTIVE_TEXTURE, &mut saved.active_texture);
            for (unit, texture) in saved.textures.iter_mut().enumerate() {
                gl.ActiveTexture(ffi::TEXTURE0 + unit as u32);
                gl.GetIntegerv(ffi::TEXTURE_BINDING_2D, texture);
            }
            gl.GetIntegerv(ffi::ARRAY_BUFFER_BINDING, &mut saved.array_buffer);
            saved
        }
    }

    unsafe fn restore(&self, gl: &ffi::Gles2) {
        unsafe {
            gl.BindFramebuffer(ffi::FRAMEBUFFER, self.framebuffer as GLuint);
            let [x, y, w, h] = self.viewport;
            gl.Viewport(x, y, w, h);
            let [x, y, w, h] = self.scissor;
            gl.Scissor(x, y, w, h);
            if self.scissor_enabled {
                gl.Enable(ffi::SCISSOR_TEST);
            } else {
                gl.Disable(ffi::SCISSOR_TEST);
            }
            if self.blend_enabled {
                gl.Enable(ffi::BLEND);
            } else {
                gl.Disable(ffi::BLEND);
            }
            let [src_rgb, dst_rgb, src_alpha, dst_alpha] = self.blend;
            gl.BlendFuncSeparate(
                src_rgb as u32,
                dst_rgb as u32,
                src_alpha as u32,
                dst_alpha as u32,
            );
            for (unit, texture) in self.textures.iter().enumerate() {
                gl.ActiveTexture(ffi::TEXTURE0 + unit as u32);
                gl.BindTexture(ffi::TEXTURE_2D, *texture as GLuint);
            }
            gl.ActiveTexture(self.active_texture as u32);
            gl.UseProgram(self.program as GLuint);
            gl.BindBuffer(ffi::ARRAY_BUFFER, self.array_buffer as GLuint);
        }
    }
}

/// 흐림 요소 하나를 그린다. `dst`와 `scissors`는 smithay의 물리 좌표다. smithay는 화면 버퍼에 물리 좌표를
/// 그대로 GL 좌표로 써서 그리므로(위아래를 투영 행렬로 뒤집는다) 바꾸지 않고 쓴다. `mask`는 가림 텍스처와
/// 그 텍스처가 위아래로 뒤집혔는지다. 없으면(앱 영역) 가리지 않는다. 전에는 손상(dst 기준)을 받아 여기서
/// 옮겼고, 이제는 화면 버퍼 좌표로 옮기고 앱 영역으로 자른 사각형(scissors)을 받는다
#[allow(clippy::too_many_arguments)]
unsafe fn draw(
    gl: &ffi::Gles2,
    shared: &mut Shared,
    targets: &mut Targets,
    mask: Option<(GLuint, bool)>,
    surface: Rectangle<i32, Physical>,
    dst: Rectangle<i32, Physical>,
    scissors: impl Iterator<Item = Rectangle<i32, Physical>>,
    commit: CommitCounter,
) {
    unsafe {
        if shared.programs.is_none() {
            match Programs::new(gl) {
                Ok(programs) => shared.programs = Some(programs),
                Err(err) => {
                    tracing::warn!("blur disabled: {err}");
                    shared.broken = true;
                    return;
                }
            }
        }
        let Some(programs) = shared.programs.as_ref() else {
            return;
        };
        let saved = Saved::save(gl);
        let [_, _, fb_width, fb_height] = saved.viewport;
        // 팝업은 그림자 여백 때문에 화면 밖으로 조금 나가기도 한다. 화면 안쪽 부분만 흐린다
        let screen = Rectangle::from_size((fb_width, fb_height).into());
        let Some(clip) = dst.intersection(screen).filter(|clip| !clip.is_empty()) else {
            return;
        };
        let (x, y, width, height) = (clip.loc.x, clip.loc.y, clip.size.w, clip.size.h);

        gl.Disable(ffi::SCISSOR_TEST);
        gl.Disable(ffi::BLEND);
        gl.BindBuffer(ffi::ARRAY_BUFFER, programs.quad);
        gl.EnableVertexAttribArray(0);
        gl.VertexAttribPointer(0, 2, ffi::FLOAT, ffi::FALSE, 0, std::ptr::null());
        if programs.divisor {
            gl.VertexAttribDivisor(0, 0);
        }
        gl.ActiveTexture(ffi::TEXTURE0);

        if targets.blurred != Some(commit) || (targets.width, targets.height) != (width, height) {
            targets.ensure(gl, width, height);
            // 지금 화면 버퍼에는 표면 아래 장면만 있다 (손상 추적이 영역 전체를 다시 그렸다).
            // 버퍼를 새로 만들면 그 프레임버퍼가 묶이므로 화면 버퍼를 다시 묶고 복사한다
            gl.BindFramebuffer(ffi::FRAMEBUFFER, saved.framebuffer as GLuint);
            gl.BindTexture(ffi::TEXTURE_2D, targets.copy.texture);
            gl.CopyTexSubImage2D(ffi::TEXTURE_2D, 0, 0, 0, x, y, width, height);
            let mut source = targets.copy;
            for level in targets.levels.clone() {
                pass(gl, &programs.down, source, level);
                source = level;
            }
            for level in targets.levels.clone().into_iter().rev().skip(1) {
                pass(gl, &programs.up, source, level);
                source = level;
            }
            targets.blurred = Some(commit);
            tracing::debug!("blurred {width}x{height}");
        }

        // 화면 버퍼에 합성한다. 손상된 부분만
        gl.BindFramebuffer(ffi::FRAMEBUFFER, saved.framebuffer as GLuint);
        gl.Viewport(x, y, width, height);
        gl.Enable(ffi::BLEND);
        gl.BlendFunc(ffi::ONE, ffi::ONE_MINUS_SRC_ALPHA);
        gl.Enable(ffi::SCISSOR_TEST);
        let program = &programs.composite;
        gl.UseProgram(program.id);
        gl.ActiveTexture(ffi::TEXTURE0);
        gl.BindTexture(ffi::TEXTURE_2D, targets.levels[0].texture);
        gl.Uniform1i(program.tex, 0);
        gl.Uniform1f(program.saturation, SATURATION);
        match mask {
            Some((mask, flipped)) => {
                gl.ActiveTexture(ffi::TEXTURE1);
                gl.BindTexture(ffi::TEXTURE_2D, mask);
                // smithay는 표면 텍스처의 축소 필터를 그 텍스처를 그릴 때마다 정하고, 새로 만든 텍스처
                // (표면 크기가 바뀐 첫 프레임, 처음 받은 dmabuf)에는 GL 기본값(밉맵)이 남아 있다. 밉맵이
                // 없으면 불완전한 텍스처라 알파가 1로 읽혀 유리가 아닌 곳까지 흐림이 덮였고, 그 프레임의
                // 화면 버퍼에만 남아 깜빡였다
                gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, ffi::LINEAR as i32);
                gl.Uniform1i(program.mask, 1);
                gl.Uniform1f(program.use_mask, 1.0);
                gl.Uniform2f(program.edges, MASK_LOW, MASK_HIGH);
                let [mask_x, mask_y, mask_w, mask_h] = mask_rect(clip, surface, flipped);
                gl.Uniform4f(program.mask_rect, mask_x, mask_y, mask_w, mask_h);
            }
            None => gl.Uniform1f(program.use_mask, 0.0),
        }
        for rect in scissors {
            gl.Scissor(rect.loc.x, rect.loc.y, rect.size.w, rect.size.h);
            gl.DrawArrays(ffi::TRIANGLE_STRIP, 0, 4);
        }

        gl.DisableVertexAttribArray(0);
        saved.restore(gl);
    }
}

/// `source`를 흐리며 `target` 크기로 그린다
unsafe fn pass(gl: &ffi::Gles2, program: &Program, source: Level, target: Level) {
    unsafe {
        gl.BindFramebuffer(ffi::FRAMEBUFFER, target.framebuffer);
        gl.Viewport(0, 0, target.width, target.height);
        gl.UseProgram(program.id);
        gl.BindTexture(ffi::TEXTURE_2D, source.texture);
        gl.Uniform1i(program.tex, 0);
        gl.Uniform2f(
            program.halfpixel,
            0.5 / source.width as f32,
            0.5 / source.height as f32,
        );
        gl.Uniform1f(program.offset, OFFSET);
        gl.DrawArrays(ffi::TRIANGLE_STRIP, 0, 4);
    }
}

impl Targets {
    /// 표면 크기에 맞게 버퍼를 만든다 (크기가 같으면 그대로 쓴다)
    unsafe fn ensure(&mut self, gl: &ffi::Gles2, width: i32, height: i32) {
        if (self.width, self.height) == (width, height) && !self.levels.is_empty() {
            return;
        }
        unsafe {
            self.delete(gl);
            self.copy = Level::new(gl, width, height);
            self.levels = (1..=LEVELS)
                .map(|level| {
                    let divisor = 1 << level;
                    Level::new(gl, (width / divisor).max(1), (height / divisor).max(1))
                })
                .collect();
        }
        self.width = width;
        self.height = height;
        self.blurred = None;
    }

    unsafe fn delete(&self, gl: &ffi::Gles2) {
        for level in std::iter::once(&self.copy).chain(&self.levels) {
            if level.texture != 0 {
                unsafe {
                    gl.DeleteFramebuffers(1, &level.framebuffer);
                    gl.DeleteTextures(1, &level.texture);
                }
            }
        }
    }
}

impl Level {
    unsafe fn new(gl: &ffi::Gles2, width: i32, height: i32) -> Self {
        let mut level = Self {
            width,
            height,
            ..Self::default()
        };
        unsafe {
            gl.GenTextures(1, &mut level.texture);
            gl.BindTexture(ffi::TEXTURE_2D, level.texture);
            gl.TexImage2D(
                ffi::TEXTURE_2D,
                0,
                ffi::RGBA as i32,
                width,
                height,
                0,
                ffi::RGBA,
                ffi::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MIN_FILTER, ffi::LINEAR as i32);
            gl.TexParameteri(ffi::TEXTURE_2D, ffi::TEXTURE_MAG_FILTER, ffi::LINEAR as i32);
            gl.TexParameteri(
                ffi::TEXTURE_2D,
                ffi::TEXTURE_WRAP_S,
                ffi::CLAMP_TO_EDGE as i32,
            );
            gl.TexParameteri(
                ffi::TEXTURE_2D,
                ffi::TEXTURE_WRAP_T,
                ffi::CLAMP_TO_EDGE as i32,
            );
            gl.GenFramebuffers(1, &mut level.framebuffer);
            gl.BindFramebuffer(ffi::FRAMEBUFFER, level.framebuffer);
            gl.FramebufferTexture2D(
                ffi::FRAMEBUFFER,
                ffi::COLOR_ATTACHMENT0,
                ffi::TEXTURE_2D,
                level.texture,
                0,
            );
        }
        level
    }
}

impl Programs {
    unsafe fn new(gl: &ffi::Gles2) -> Result<Self, String> {
        unsafe {
            let mut quad = 0;
            gl.GenBuffers(1, &mut quad);
            gl.BindBuffer(ffi::ARRAY_BUFFER, quad);
            let vertices: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            gl.BufferData(
                ffi::ARRAY_BUFFER,
                std::mem::size_of_val(&vertices) as isize,
                vertices.as_ptr().cast(),
                ffi::STATIC_DRAW,
            );
            gl.BindBuffer(ffi::ARRAY_BUFFER, 0);
            let version = gl.GetString(ffi::VERSION);
            let divisor = !version.is_null()
                && CStr::from_ptr(version.cast())
                    .to_string_lossy()
                    .starts_with("OpenGL ES 3");
            Ok(Self {
                down: Program::new(gl, DOWN)?,
                up: Program::new(gl, UP)?,
                composite: Program::new(gl, COMPOSITE)?,
                quad,
                divisor,
            })
        }
    }
}

impl Program {
    unsafe fn new(gl: &ffi::Gles2, fragment: &str) -> Result<Self, String> {
        unsafe {
            let vertex = compile(gl, ffi::VERTEX_SHADER, VERTEX)?;
            let fragment = compile(gl, ffi::FRAGMENT_SHADER, fragment)?;
            let id = gl.CreateProgram();
            gl.AttachShader(id, vertex);
            gl.AttachShader(id, fragment);
            gl.BindAttribLocation(id, 0, c"position".as_ptr());
            gl.LinkProgram(id);
            gl.DeleteShader(vertex);
            gl.DeleteShader(fragment);
            let mut status = 0;
            gl.GetProgramiv(id, ffi::LINK_STATUS, &mut status);
            if status == 0 {
                return Err("failed to link a blur shader".into());
            }
            let location = |name: &CStr| gl.GetUniformLocation(id, name.as_ptr());
            Ok(Self {
                id,
                tex: location(c"tex"),
                mask: location(c"mask"),
                use_mask: location(c"use_mask"),
                halfpixel: location(c"halfpixel"),
                offset: location(c"offset"),
                saturation: location(c"saturation"),
                edges: location(c"edges"),
                mask_rect: location(c"mask_rect"),
            })
        }
    }
}

unsafe fn compile(
    gl: &ffi::Gles2,
    kind: ffi::types::GLenum,
    source: &str,
) -> Result<GLuint, String> {
    unsafe {
        let shader = gl.CreateShader(kind);
        let pointer = source.as_ptr().cast();
        let length = source.len() as GLint;
        gl.ShaderSource(shader, 1, &pointer, &length);
        gl.CompileShader(shader);
        let mut status = 0;
        gl.GetShaderiv(shader, ffi::COMPILE_STATUS, &mut status);
        if status == 0 {
            let mut log = vec![0u8; 1024];
            let mut written = 0;
            gl.GetShaderInfoLog(
                shader,
                log.len() as GLint,
                &mut written,
                log.as_mut_ptr().cast(),
            );
            log.truncate(written.max(0) as usize);
            gl.DeleteShader(shader);
            return Err(format!(
                "failed to compile a blur shader: {}",
                String::from_utf8_lossy(&log)
            ));
        }
        Ok(shader)
    }
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsFd;

    use smithay::{
        backend::{
            allocator::{Format, Fourcc, Modifier},
            renderer::element::memory::MemoryRenderBuffer,
        },
        utils::{Point, Transform},
    };
    use wayland_client::protocol::{
        wl_buffer::WlBuffer as ClientBuffer, wl_compositor::WlCompositor, wl_shm::WlShm,
    };
    use wayland_protocols::wp::linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1::Flags, zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    };

    use super::*;
    use crate::testing::{
        Frames, Server, ServerOptions, TestClient, argb, background, gl_lock, memfd, output, pixel,
        software_renderer, stripes, texture_formats,
    };

    fn physical(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Physical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn logical(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn region_parts_move_onto_the_surface_and_stay_inside_it() {
        let surface = physical(100, 100, 200, 200);
        let one = Scale::from(1.0);
        assert_eq!(
            region_parts(&[logical(10, 20, 30, 40)], surface, one),
            vec![physical(110, 120, 30, 40)]
        );
        // 표면 밖은 자른다. 다 밖이면 없다
        assert_eq!(
            region_parts(&[logical(-50, -50, 100, 100)], surface, one),
            vec![physical(100, 100, 50, 50)]
        );
        assert!(region_parts(&[logical(200, 0, 10, 10)], surface, one).is_empty());
        // 앱이 보낸 아주 큰 값과 작은 값도 넘치지 않는다
        assert_eq!(
            region_parts(
                &[logical(i32::MIN, i32::MIN, i32::MAX, i32::MAX)],
                surface,
                one
            ),
            Vec::new()
        );
        assert_eq!(
            region_parts(&[logical(-10, -10, i32::MAX, i32::MAX)], surface, one),
            vec![surface]
        );
    }

    #[test]
    fn neighboring_parts_keep_no_gap_at_fractional_scales() {
        let surface = physical(0, 0, 100, 100);
        let parts = region_parts(
            &[logical(0, 0, 3, 10), logical(3, 0, 3, 10)],
            surface,
            Scale::from(1.25),
        );
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].loc.x + parts[0].size.w, parts[1].loc.x);
        // 3 × 1.25 = 3.75 → 4, 6 × 1.25 = 7.5 → 8
        assert_eq!(parts[1], physical(4, 0, 4, 13));
    }

    #[test]
    fn scissors_follow_the_damage_and_the_region() {
        let dst = physical(10, 20, 100, 100);
        let damage = [physical(0, 0, 50, 50), physical(60, 60, 40, 40)];
        // 셸 유리: 손상 그대로 화면 버퍼 좌표로
        assert_eq!(
            scissors(dst, &damage, &[]).collect::<Vec<_>>(),
            vec![physical(10, 20, 50, 50), physical(70, 80, 40, 40)]
        );
        // 앱 영역: 영역 안으로 자른다
        let parts = [physical(40, 40, 30, 30), physical(90, 90, 20, 20)];
        assert_eq!(
            scissors(dst, &damage, &parts).collect::<Vec<_>>(),
            vec![physical(40, 40, 20, 30), physical(90, 90, 20, 20)]
        );
        assert_eq!(scissors(dst, &[], &parts).count(), 0);
    }

    #[test]
    fn mask_rect_reads_flipped_textures_from_the_bottom() {
        let surface = physical(0, 0, 200, 100);
        let clip = physical(50, 25, 100, 50);
        assert_eq!(mask_rect(clip, surface, false), [0.25, 0.25, 0.5, 0.5]);
        assert_eq!(mask_rect(clip, surface, true), [0.25, 0.75, 0.5, -0.5]);
    }

    const SIZE: i32 = 64;

    /// 흐림을 켠 시험 서버, 클라이언트와 전역들. GL이 없으면 None
    fn gl_setup() -> Option<(Server, TestClient, WlCompositor, WlShm, ZwpLinuxDmabufV1)> {
        let renderer = software_renderer()?;
        let formats = texture_formats(&renderer);
        let mut server = Server::new(
            Some(renderer),
            ServerOptions {
                dmabuf_formats: Some(formats),
                blur: true,
            },
        );
        let client = server.connect();
        let compositor = client.bind("wl_compositor", 4).unwrap();
        let shm = client.bind("wl_shm", 1).unwrap();
        let dmabuf = client.bind("zwp_linux_dmabuf_v1", 5).unwrap();
        Some((server, client, compositor, shm, dmabuf))
    }

    /// 렌더러가 선형 Argb8888 dmabuf를 보통 텍스처로 가져오는지. Mesa 25.0의 llvmpipe는 선형 dmabuf를
    /// 모두 외부 텍스처로만 가져와 가림으로 쓰지 않는다(25.2는 보통 텍스처)
    fn argb_dmabufs_are_textures(server: &mut Server) -> bool {
        let linear = Format {
            code: Fourcc::Argb8888,
            modifier: Modifier::Linear,
        };
        server
            .renderer()
            .egl_context()
            .dmabuf_render_formats()
            .contains(&linear)
    }

    /// 버퍼 하나를 붙여 커밋한 표면
    fn commit(
        server: &mut Server,
        client: &mut TestClient,
        compositor: &WlCompositor,
        buffer: &ClientBuffer,
    ) {
        let surface = compositor.create_surface(&client.qh, ());
        surface.attach(Some(buffer), 0, 0);
        surface.damage_buffer(0, 0, SIZE, SIZE);
        surface.commit();
        server.pump(client);
        assert_eq!(client.protocol_error(), None);
    }

    /// 줄무늬 배경. 컴포지터의 배경화면처럼 한 번 만들어 계속 쓴다(요소 ID가 같아야 흐린 것을 다시 쓴다)
    fn wallpaper() -> MemoryRenderBuffer {
        MemoryRenderBuffer::from_slice(
            &stripes(SIZE as usize, SIZE as usize, 1),
            Fourcc::Argb8888,
            (SIZE, SIZE),
            1,
            Transform::Normal,
            None,
        )
    }

    /// 줄무늬 배경 위에 커밋한 첫 표면을 놓고 `shape`대로 흐려 그린 화면 (RGBA)
    fn draw(
        server: &mut Server,
        blur: &mut Blur,
        frames: &mut Frames,
        wallpaper: &MemoryRenderBuffer,
        location: Point<i32, Physical>,
        shape: BlurShape,
    ) -> Vec<u8> {
        let scale = Scale::from(1.0);
        let surface = server.state.surfaces[0].clone();
        let mut elements = server.surface_elements(0, location, scale);
        elements.push(background(server.renderer(), wallpaper));
        let target = BlurTarget { surface, shape };
        blur.insert(
            server.renderer(),
            &mut elements,
            &[target],
            &frames.output,
            scale,
        );
        frames.render(server.renderer(), &elements)
    }

    /// 흐리지 않은 줄무늬 그대로인지 (검정이나 흰색)
    fn sharp(px: [u8; 4]) -> bool {
        px[0] == 0 || px[0] == 255
    }

    /// 줄무늬가 섞여 가운데 회색 근처인지
    fn blurred(px: [u8; 4], middle: u8) -> bool {
        px[0].abs_diff(middle) <= 25 && px[0].abs_diff(px[1]) <= 3 && px[1].abs_diff(px[2]) <= 3
    }

    #[test]
    fn app_regions_blur_only_inside_the_region() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, shm, _)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        // 다 투명한 앱 창. 흐린 배경이 그대로 보인다
        let buffer = client.shm_buffer(&shm, SIZE, SIZE, &argb(64, 64, [0, 0, 0, 0]));
        commit(&mut server, &mut client, &compositor, &buffer);
        let mut blur = Blur::default();
        let mut frames = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        let shape = BlurShape::Region(Arc::from(vec![logical(16, 16, 32, 32)]));
        let pixels = draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper(),
            (0, 0).into(),
            shape,
        );
        for y in 0..SIZE as usize {
            for x in 0..SIZE as usize {
                let px = pixel(&pixels, SIZE as usize, x, y);
                if (16..48).contains(&x) && (16..48).contains(&y) {
                    assert!(blurred(px, 128), "({x}, {y}) {px:?} is not blurred");
                } else {
                    assert!(sharp(px), "({x}, {y}) {px:?} is blurred");
                }
            }
        }
        assert!(!blur.broken());
    }

    #[test]
    fn app_regions_are_clipped_to_the_surface() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, shm, _)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let buffer = client.shm_buffer(&shm, 32, 32, &argb(32, 32, [0, 0, 0, 0]));
        commit(&mut server, &mut client, &compositor, &buffer);
        let mut blur = Blur::default();
        let mut frames = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        // 표면보다 훨씬 큰 영역
        let shape = BlurShape::Region(Arc::from(vec![logical(-100, -100, 1000, 1000)]));
        let pixels = draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper(),
            (16, 16).into(),
            shape,
        );
        for (x, y) in [(15, 30), (48, 30), (30, 15), (30, 48), (0, 0), (63, 63)] {
            assert!(sharp(pixel(&pixels, SIZE as usize, x, y)), "({x}, {y})");
        }
        for (x, y) in [(16, 16), (47, 47), (30, 30)] {
            assert!(
                blurred(pixel(&pixels, SIZE as usize, x, y), 128),
                "({x}, {y})"
            );
        }
    }

    /// 왼쪽 절반만 유리(검정 60%)이고 오른쪽 절반은 투명한 그림 (위아래로 나누면 `rows`)
    fn half_glass(rows: bool) -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..SIZE {
            for x in 0..SIZE {
                let glass = if rows { y < SIZE / 2 } else { x < SIZE / 2 };
                // 미리 곱한 알파(premultiplied): 검정이므로 색은 0이다
                pixels.extend_from_slice(&[0, 0, 0, if glass { 153 } else { 0 }]);
            }
        }
        pixels
    }

    /// 왼쪽 절반만 흐렸는지. 유리 아래는 흐린 회색에 검정 60%가 덮여 0.4배다
    fn assert_left_glass(pixels: &[u8]) {
        for y in 0..SIZE as usize {
            for x in 0..SIZE as usize {
                let px = pixel(pixels, SIZE as usize, x, y);
                if x < SIZE as usize / 2 {
                    assert!(
                        blurred(px, 51),
                        "({x}, {y}) {px:?} under the glass is not blurred"
                    );
                } else {
                    assert!(sharp(px), "({x}, {y}) {px:?} beside the glass is blurred");
                }
            }
        }
    }

    #[test]
    fn shm_glass_blurs_where_the_surface_is_opaque_enough() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, shm, _)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let buffer = client.shm_buffer(&shm, SIZE, SIZE, &half_glass(false));
        commit(&mut server, &mut client, &compositor, &buffer);
        let mut blur = Blur::default();
        let mut frames = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        let pixels = draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper(),
            (0, 0).into(),
            BlurShape::Glass,
        );
        assert_left_glass(&pixels);
    }

    #[test]
    fn dmabuf_glass_masks_are_read_on_their_first_frame() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, _, dmabuf)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        if !argb_dmabufs_are_textures(&mut server) {
            eprintln!("skipped: the renderer imports Argb8888 dmabufs only as external textures");
            return;
        }
        let buffer = client.dmabuf_buffer(
            &dmabuf,
            (SIZE, SIZE),
            Fourcc::Argb8888,
            Modifier::Linear.into(),
            &half_glass(false),
            Flags::empty(),
        );
        commit(&mut server, &mut client, &compositor, &buffer);
        let mut blur = Blur::default();
        let mut frames = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        // dmabuf는 CPU로 읽지 않으므로 표면 전체를 흐릴 범위로 두고, 가림이 오른쪽을 지킨다.
        // 처음 그리는 프레임에는 smithay가 아직 텍스처의 필터를 정하지 않았다
        let pixels = draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper(),
            (0, 0).into(),
            BlurShape::Glass,
        );
        assert_left_glass(&pixels);
    }

    #[test]
    fn y_inverted_dmabuf_masks_follow_the_picture() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, _, dmabuf)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        if !argb_dmabufs_are_textures(&mut server) {
            eprintln!("skipped: the renderer imports Argb8888 dmabufs only as external textures");
            return;
        }
        // 메모리의 위쪽 절반이 유리다. 위아래가 뒤집힌 버퍼이므로 화면에서는 아래쪽 절반이 유리다
        let buffer = client.dmabuf_buffer(
            &dmabuf,
            (SIZE, SIZE),
            Fourcc::Argb8888,
            Modifier::Linear.into(),
            &half_glass(true),
            Flags::YInvert,
        );
        commit(&mut server, &mut client, &compositor, &buffer);
        let mut blur = Blur::default();
        let mut frames = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        let pixels = draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper(),
            (0, 0).into(),
            BlurShape::Glass,
        );
        for (x, y) in [(10, 10), (40, 20), (63, 31)] {
            let px = pixel(&pixels, SIZE as usize, x, y);
            assert!(sharp(px), "({x}, {y}) {px:?} above the glass is blurred");
        }
        for (x, y) in [(10, 40), (40, 50), (63, 63)] {
            let px = pixel(&pixels, SIZE as usize, x, y);
            assert!(
                blurred(px, 51),
                "({x}, {y}) {px:?} under the glass is not blurred"
            );
        }
    }

    #[test]
    fn external_dmabufs_are_not_used_as_masks() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, _, dmabuf)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        // NV12(YUV 두 평면)은 외부 텍스처로만 가져온다
        let (width, height) = (SIZE as u32, SIZE as u32);
        let fd = memfd(&vec![128; (width * height * 3 / 2) as usize]);
        let params = dmabuf.create_params(&client.qh, ());
        let linear = u64::from(Modifier::Linear);
        params.add(
            fd.as_fd(),
            0,
            0,
            width,
            (linear >> 32) as u32,
            linear as u32,
        );
        params.add(
            fd.as_fd(),
            1,
            width * height,
            width,
            (linear >> 32) as u32,
            linear as u32,
        );
        let buffer = params.create_immed(
            SIZE,
            SIZE,
            Fourcc::Nv12 as u32,
            Flags::empty(),
            &client.qh,
            (),
        );
        // Mesa 25.0의 llvmpipe는 NV12를 알리기만 하고 가져오지 못한다(25.2는 가져온다)
        server.pump(&mut client);
        if client.protocol_error().is_some() {
            eprintln!("skipped: the renderer could not import NV12");
            return;
        }
        commit(&mut server, &mut client, &compositor, &buffer);
        let surface = server.state.surfaces[0].clone();
        let scale = Scale::from(1.0);
        let mut elements = server.surface_elements(0, (0, 0).into(), scale);
        assert_eq!(elements.len(), 1, "the NV12 buffer was imported");
        let mut blur = Blur::default();
        let output = output("test");
        let glass = BlurTarget::glass(surface.clone());
        assert!(!blur.insert(server.renderer(), &mut elements, &[glass], &output, scale));
        // 앱 영역은 가림을 쓰지 않지만, NV12에는 알파가 없어 표면 자신이 영역을 다 가린다(불투명 영역).
        // 보이지 않는 흐림은 넣지 않는다. 흐림 위 불투명 요소를 고치며 바꿨다
        let region = BlurTarget {
            surface,
            shape: BlurShape::Region(Arc::from(vec![logical(0, 0, 8, 8)])),
        };
        assert!(!blur.insert(server.renderer(), &mut elements, &[region], &output, scale));
        assert_eq!(elements.len(), 1);
    }

    #[test]
    fn the_region_shape_and_the_scene_below_trigger_a_new_blur() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, shm, _)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let buffer = client.shm_buffer(&shm, SIZE, SIZE, &argb(64, 64, [0, 0, 0, 0]));
        commit(&mut server, &mut client, &compositor, &buffer);
        let mut blur = Blur::default();
        let mut frames = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        let region = |rects: Vec<Rectangle<i32, Logical>>| BlurShape::Region(Arc::from(rects));
        let commit_of = |blur: &Blur| {
            let entry = blur.surfaces.values().next().unwrap();
            entry.commit
        };
        let wallpaper = wallpaper();
        let square = || region(vec![logical(0, 0, 32, 32)]);
        draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper,
            (0, 0).into(),
            square(),
        );
        let first = commit_of(&blur);
        // 같은 장면은 흐려 둔 것을 쓴다
        draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper,
            (0, 0).into(),
            square(),
        );
        assert_eq!(commit_of(&blur), first);
        // 영역의 모양이 바뀌면(바깥 사각형이 같아도) 다시 흐린다
        let pixels = draw(
            &mut server,
            &mut blur,
            &mut frames,
            &wallpaper,
            (0, 0).into(),
            region(vec![logical(0, 0, 32, 16), logical(0, 16, 16, 16)]),
        );
        assert_ne!(commit_of(&blur), first);
        // 빠진 오른쪽 아래 조각은 다시 줄무늬다 (손상 추적이 그 자리를 다시 그렸다)
        assert!(sharp(pixel(&pixels, SIZE as usize, 24, 24)));
        assert!(blurred(pixel(&pixels, SIZE as usize, 8, 24), 128));
    }

    /// 표면 전체를 흐리는 첫 표면(앱 창) 위에 둘째 표면(불투명한 창)을 x = `window_x`에 겹쳐 그린 화면
    fn draw_under_window(
        server: &mut Server,
        blur: &mut Blur,
        frames: &mut Frames,
        wallpaper: &MemoryRenderBuffer,
        window_x: i32,
    ) -> Vec<u8> {
        let scale = Scale::from(1.0);
        let surface = server.state.surfaces[0].clone();
        let mut elements = server.surface_elements(1, (window_x, 0).into(), scale);
        elements.extend(server.surface_elements(0, (0, 0).into(), scale));
        elements.push(background(server.renderer(), wallpaper));
        let target = BlurTarget {
            surface,
            shape: BlurShape::Region(Arc::from(vec![logical(0, 0, SIZE, SIZE)])),
        };
        blur.insert(
            server.renderer(),
            &mut elements,
            &[target],
            &frames.output,
            scale,
        );
        frames.render(server.renderer(), &elements)
    }

    #[test]
    fn windows_above_do_not_show_through_the_blur() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, shm, _)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let buffer = client.shm_buffer(&shm, SIZE, SIZE, &argb(64, 64, [0, 0, 0, 0]));
        commit(&mut server, &mut client, &compositor, &buffer);
        // 앱 창 위의 불투명한 빨간 창 (24×64, 불투명 영역을 알린다)
        let red = client.shm_buffer(&shm, 24, SIZE, &argb(24, 64, [255, 0, 0, 255]));
        let window = compositor.create_surface(&client.qh, ());
        let opaque = compositor.create_region(&client.qh, ());
        opaque.add(0, 0, 24, SIZE);
        window.set_opaque_region(Some(&opaque));
        window.attach(Some(&red), 0, 0);
        window.damage_buffer(0, 0, 24, SIZE);
        window.commit();
        server.pump(&mut client);
        assert_eq!(client.protocol_error(), None);

        let mut blur = Blur::default();
        let mut frames = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        let fine = wallpaper();
        let coarse = MemoryRenderBuffer::from_slice(
            &stripes(SIZE as usize, SIZE as usize, 2),
            Fourcc::Argb8888,
            (SIZE, SIZE),
            1,
            Transform::Normal,
            None,
        );
        // 첫 프레임(지우지 않은 버퍼)과 아래 장면이 바뀌어 다시 흐린 프레임: 창 옆에 창의 빨강이나 옛 프레임이
        // 번지지 않는다
        for wallpaper in [&fine, &coarse] {
            let pixels = draw_under_window(&mut server, &mut blur, &mut frames, wallpaper, 40);
            for x in [20, 32, 36, 39] {
                let px = pixel(&pixels, SIZE as usize, x, 32);
                assert!(blurred(px, 128), "({x}, 32) {px:?} beside the window");
            }
            assert_eq!(pixel(&pixels, SIZE as usize, 50, 32), [255, 0, 0, 255]);
        }
        // 창이 비키면 그 자리도 흐린 배경이다 (창이 흐려 둔 사본에 남지 않았다)
        let pixels = draw_under_window(&mut server, &mut blur, &mut frames, &coarse, SIZE);
        for x in [40, 48, 56, 63] {
            let px = pixel(&pixels, SIZE as usize, x, 32);
            assert!(blurred(px, 128), "({x}, 32) {px:?} where the window was");
        }
    }

    #[test]
    fn a_window_on_two_screens_is_blurred_once_on_each() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, shm, _)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let buffer = client.shm_buffer(&shm, SIZE, SIZE, &argb(64, 64, [0, 0, 0, 0]));
        commit(&mut server, &mut client, &compositor, &buffer);
        let mut blur = Blur::default();
        let mut left = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        let mut right = Frames::new(server.renderer(), (SIZE, SIZE).into(), 1.0);
        let wallpaper = wallpaper();
        let region = || BlurShape::Region(Arc::from(vec![logical(0, 0, SIZE, SIZE)]));
        let commits = |blur: &Blur| {
            let mut commits: Vec<CommitCounter> =
                blur.surfaces.values().map(|entry| entry.commit).collect();
            commits.sort();
            commits
        };
        // 두 화면에 걸친 창: 왼쪽 화면에서는 x = 32, 오른쪽 화면에서는 x = -32. 두 화면이 번갈아 그린다
        let mut seen = Vec::new();
        for frame in 0..6 {
            let (frames, location) = if frame % 2 == 0 {
                (&mut left, (32, 0))
            } else {
                (&mut right, (-32, 0))
            };
            let pixels = draw(
                &mut server,
                &mut blur,
                frames,
                &wallpaper,
                location.into(),
                region(),
            );
            let x = if frame % 2 == 0 { 48 } else { 16 };
            assert!(blurred(pixel(&pixels, SIZE as usize, x, 32), 128));
            seen.push(commits(&blur));
        }
        // 화면마다 처음 한 번만 흐린다 (표면마다 하나만 두었을 때는 프레임마다 다시 흐렸다)
        assert!(
            seen[2..].iter().all(|commits| *commits == seen[1]),
            "{seen:?}"
        );
        assert_eq!(blur.surfaces.len(), 2);
    }

    #[test]
    fn a_blur_hidden_under_an_opaque_window_is_left_out() {
        let _gl = gl_lock();
        let Some((mut server, mut client, compositor, shm, _)) = gl_setup() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let buffer = client.shm_buffer(&shm, SIZE, SIZE, &argb(64, 64, [0, 0, 0, 0]));
        commit(&mut server, &mut client, &compositor, &buffer);
        // 앱 창과 같은 크기의 불투명한 창
        let gray = client.shm_buffer(&shm, SIZE, SIZE, &argb(64, 64, [90, 90, 90, 255]));
        let window = compositor.create_surface(&client.qh, ());
        let opaque = compositor.create_region(&client.qh, ());
        opaque.add(0, 0, SIZE, SIZE);
        window.set_opaque_region(Some(&opaque));
        window.attach(Some(&gray), 0, 0);
        window.damage_buffer(0, 0, SIZE, SIZE);
        window.commit();
        server.pump(&mut client);
        assert_eq!(client.protocol_error(), None);

        let scale = Scale::from(1.0);
        let wallpaper = wallpaper();
        let target = BlurTarget {
            surface: server.state.surfaces[0].clone(),
            shape: BlurShape::Region(Arc::from(vec![logical(0, 0, SIZE, SIZE)])),
        };
        let mut blur = Blur::default();
        let output = output("test");
        let mut scene = |server: &mut Server, window_x: i32| {
            let mut elements = server.surface_elements(1, (window_x, 0).into(), scale);
            elements.extend(server.surface_elements(0, (0, 0).into(), scale));
            elements.push(background(server.renderer(), &wallpaper));
            let blurred = blur.insert(
                server.renderer(),
                &mut elements,
                std::slice::from_ref(&target),
                &output,
                scale,
            );
            let uncovered = elements
                .iter()
                .filter(|element| matches!(element, OutputRenderElements::Uncovered(_)))
                .count();
            (blurred, uncovered)
        };
        // 다 가려지면 흐림 요소를 넣지 않고 아래를 다시 그리지도 않는다 (가려진 창은 보이지 않는 표면이다)
        assert_eq!(scene(&mut server, 0), (false, 0));
        // 조금이라도 보이면 흐리고, 덮은 창은 흐릴 범위에서 불투명하지 않다고 알린다
        assert_eq!(scene(&mut server, 8), (true, 1));
    }
}
