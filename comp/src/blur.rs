// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 셸 유리 뒤 흐림 (macOS의 머티리얼처럼). 특권 셸 클라이언트의 위층 레이어 표면과 그 팝업 아래를
//! 흐리게 그린다. 셸 쪽 프로토콜은 쓰지 않는다.
//!
//! 표면 바로 아래에 흐림 요소(BlurElement)를 끼워 둔다. 화면을 아래 요소부터 그리므로 이 요소가 그려질
//! 차례에 화면 버퍼의 그 자리에는 표면 아래 장면만 있다. 그 부분을 복사해 dual Kawase(축소 3번,
//! 확대 2번)로 흐리고, 표면의 알파가 0.5 이상인 곳(유리)에만 합성한다. 그림자처럼 옅은 곳과 투명한
//! 곳은 그대로 둔다.
//!
//! 아래 장면이 바뀐 프레임에만 다시 흐린다. 그때는 요소의 커밋을 올려 손상 추적이 영역 전체를 다시
//! 그리게 하고(그래야 복사한 영역에 지난 프레임의 유리가 섞이지 않는다), 나머지 프레임은 흐려 둔 결과를
//! 손상된 부분에만 합성한다. 흐릴 표면이 없으면 하는 일이 없다.

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::CStr,
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};

use smithay::{
    backend::renderer::{
        Renderer,
        element::{Element, Id, Kind, RenderElement},
        gles::{GlesError, GlesFrame, GlesRenderer, GlesTexture, ffi},
        utils::{CommitCounter, with_renderer_surface_state},
    },
    reexports::wayland_server::{
        Resource,
        backend::ObjectId,
        protocol::{wl_buffer::WlBuffer, wl_shm, wl_surface::WlSurface},
    },
    utils::{Buffer, Physical, Rectangle, Scale},
    wayland::shm::with_buffer_contents,
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
/// 복사한 텍스처와 표면 텍스처 모두 v=0이 표면의 위다
const COMPOSITE: &str = "#version 100
precision mediump float;
uniform sampler2D tex;
uniform sampler2D mask;
uniform float saturation;
uniform vec2 edges;
uniform vec4 mask_rect;
varying vec2 v;
void main() {
    float alpha = texture2D(mask, mask_rect.xy + v * mask_rect.zw).a;
    float m = smoothstep(edges.x, edges.y, alpha);
    if (m <= 0.0) {
        discard;
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

/// 흐림 상태. 출력마다가 아니라 표면마다 버퍼를 둔다
#[derive(Default)]
pub struct Blur {
    shared: Rc<RefCell<Shared>>,
    surfaces: HashMap<ObjectId, SurfaceBlur>,
}

impl Blur {
    /// 이번 프레임의 요소 목록(위에서 아래 순)에서 `surfaces` 각각의 바로 아래에 흐림 요소를 끼운다.
    /// 하나라도 끼웠으면 true
    pub fn insert(
        &mut self,
        renderer: &mut GlesRenderer,
        elements: &mut Vec<OutputRenderElements>,
        surfaces: &[WlSurface],
        scale: Scale<f64>,
    ) -> bool {
        self.collect_garbage(renderer, surfaces);
        if surfaces.is_empty() || self.shared.borrow().broken {
            return false;
        }
        let mut inserted = false;
        let context = renderer.context_id();
        // 아래 표면부터 끼워, 위 표면의 지문에 아래 흐림 요소의 변화도 들어가게 한다
        let mut targets: Vec<(usize, &WlSurface)> = surfaces
            .iter()
            .filter_map(|surface| {
                let id = Id::from_wayland_resource(surface);
                elements
                    .iter()
                    .position(|element| element.id() == &id)
                    .map(|index| (index, surface))
            })
            .collect();
        targets.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
        for (_, surface) in targets {
            let id = Id::from_wayland_resource(surface);
            let Some(index) = elements.iter().position(|element| element.id() == &id) else {
                continue;
            };
            let surface_geometry = elements[index].geometry(scale);
            if surface_geometry.is_empty() {
                continue;
            }
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
            let entry = self
                .surfaces
                .entry(surface.id())
                .or_insert_with(|| SurfaceBlur {
                    id: Id::new(),
                    commit: CommitCounter::default(),
                    fingerprint: 0,
                    seen: HashMap::new(),
                    targets: Rc::default(),
                    bounds: None,
                });
            // 모든 앱 패널처럼 화면 전체를 덮는 투명한 표면도 있으므로, 유리가 있는 범위만 흐린다
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
            // 흐릴 범위 아래에서 겹치는 요소가 바뀌거나 움직였거나, 그 손상이 흐릴 범위에 닿으면
            // 다시 흐린다. 커밋만 보면 Dock 아래에 걸친 창이 멀리 떨어진 곳을 그릴 때마다(시스템
            // 모니터의 그래프 등) Dock 전체를 다시 흐렸다
            let mut hasher = DefaultHasher::new();
            hash_rectangle(geometry, &mut hasher);
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
                    targets: entry.targets.clone(),
                    shared: self.shared.clone(),
                }),
            );
            inserted = true;
        }
        inserted
    }

    /// 화면에서 사라진 표면의 버퍼를 지운다
    fn collect_garbage(&mut self, renderer: &mut GlesRenderer, surfaces: &[WlSurface]) {
        let alive: Vec<ObjectId> = surfaces.iter().map(Resource::id).collect();
        let gone: Vec<ObjectId> = self
            .surfaces
            .keys()
            .filter(|id| !alive.contains(id))
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

/// 셸 표면 바로 아래에 끼우는 흐림 요소
pub struct BlurElement {
    id: Id,
    commit: CommitCounter,
    /// 흐릴 범위 (표면의 유리 부분)
    geometry: Rectangle<i32, Physical>,
    /// 표면 전체. 표면 텍스처가 덮는 범위다
    surface: Rectangle<i32, Physical>,
    /// 표면의 텍스처. 알파로 흐릴 곳을 정한다
    mask: GlesTexture,
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
        let mask = self.mask.tex_id();
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
                    damage,
                    self.commit,
                )
            }
        })
    }
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

/// 흐림 요소 하나를 그린다. `dst`와 `damage`는 smithay의 물리 좌표다. smithay는 화면 버퍼에 물리 좌표를
/// 그대로 GL 좌표로 써서 그리므로(위아래를 투영 행렬로 뒤집는다) 바꾸지 않고 쓴다
#[allow(clippy::too_many_arguments)]
unsafe fn draw(
    gl: &ffi::Gles2,
    shared: &mut Shared,
    targets: &mut Targets,
    mask: GLuint,
    surface: Rectangle<i32, Physical>,
    dst: Rectangle<i32, Physical>,
    damage: &[Rectangle<i32, Physical>],
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
        let mask_rect = [
            (clip.loc.x - surface.loc.x) as f32 / surface.size.w as f32,
            (clip.loc.y - surface.loc.y) as f32 / surface.size.h as f32,
            clip.size.w as f32 / surface.size.w as f32,
            clip.size.h as f32 / surface.size.h as f32,
        ];

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
        gl.ActiveTexture(ffi::TEXTURE1);
        gl.BindTexture(ffi::TEXTURE_2D, mask);
        gl.Uniform1i(program.mask, 1);
        gl.Uniform1f(program.saturation, SATURATION);
        gl.Uniform2f(program.edges, MASK_LOW, MASK_HIGH);
        let [mask_x, mask_y, mask_w, mask_h] = mask_rect;
        gl.Uniform4f(program.mask_rect, mask_x, mask_y, mask_w, mask_h);
        for rect in damage {
            // damage는 dst 기준이다
            gl.Scissor(
                dst.loc.x + rect.loc.x,
                dst.loc.y + rect.loc.y,
                rect.size.w,
                rect.size.h,
            );
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
