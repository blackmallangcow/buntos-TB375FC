// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! ⇧⌘5 도구 막대의 "선택 부분"이 띄우는 고칠 수 있는 선택 영역 (macOS의 스크린샷 선택 영역).
//! 안쪽을 끌면 옮기고, 테두리와 모서리(손잡이 여덟 개)를 끌면 그 변을 옮겨 크기를 바꾸고, 바깥을 끌면
//! 새로 고른다. 영역은 놓인 화면 밖으로 나가지 않는다. 바깥은 어둡게 덮고 고른 곳은 그대로 보인다.
//! 찍기와 입력은 screenshot.rs가 하고, 여기에는 BuntState와 상관없는 계산과 그리기만 둔다

use smithay::{
    backend::allocator::Fourcc,
    backend::renderer::{
        element::{
            Id, Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            solid::SolidColorRenderElement,
        },
        gles::GlesRenderer,
        utils::CommitCounter,
    },
    input::pointer::CursorIcon,
    utils::{Logical, Physical, Point, Rectangle, Scale, Size, Transform},
};

use crate::render::OutputRenderElements;

/// 바깥을 덮는 색 (미리 곱한 RGBA)
const DIM: [f32; 4] = [0.0, 0.0, 0.0, 0.4];
/// 테두리 색 (⇧⌘4의 고르는 영역과 같다)
const BORDER: [f32; 4] = [0.9, 0.9, 0.9, 0.9];
/// 손잡이 지름 (논리 픽셀)
const HANDLE: i32 = 9;
/// 손잡이 둘레의 어두운 테두리 (논리 픽셀). 흰 바탕 위에서도 보이게 한다
const HANDLE_RING: f64 = 1.0;
const HANDLE_RING_ALPHA: f64 = 0.35;
/// 변과 모서리를 잡는 거리 (논리 픽셀). 영역 바깥으로는 이만큼, 안으로는 작은 영역도 옮길 수 있게
/// 변 길이의 3분의 1까지만 받는다
const REACH: f64 = 8.0;
/// 가장 작은 변 (논리 픽셀). 이보다 좁게 끌면 이만큼 남긴다
pub const MIN_SIDE: i32 = 4;
/// 기억한 영역이 없을 때 띄우는 영역: 화면 가운데, 화면 크기의 이만큼
const FIRST: f64 = 0.5;

/// 변의 어느 쪽인가. 가로로는 왼쪽이 Start, 세로로는 위가 Start다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Start,
    End,
}

/// 누른 곳에 따라 끌기가 하는 일
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grip {
    /// 안쪽: 영역을 옮긴다
    Inside,
    /// 테두리·모서리: 잡은 변을 옮긴다. `x`는 왼쪽·오른쪽 변, `y`는 위·아래 변이고 모서리는 둘 다다
    Edge { x: Option<Side>, y: Option<Side> },
    /// 바깥: 새 영역을 끌어 고른다
    Outside,
}

impl Grip {
    /// 포인터 모양: 옮기기, 변 방향의 크기 조절, 바깥은 십자선
    pub fn cursor(self) -> CursorIcon {
        match self {
            Self::Inside => CursorIcon::Move,
            Self::Outside => CursorIcon::Crosshair,
            // 왼쪽 위와 오른쪽 아래 모서리
            Self::Edge {
                x: Some(x),
                y: Some(y),
            } if x == y => CursorIcon::NwseResize,
            Self::Edge {
                x: Some(_),
                y: Some(_),
            } => CursorIcon::NeswResize,
            Self::Edge {
                x: Some(_),
                y: None,
            } => CursorIcon::EwResize,
            Self::Edge {
                x: None,
                y: Some(_),
            } => CursorIcon::NsResize,
            Self::Edge { x: None, y: None } => CursorIcon::Default,
        }
    }
}

/// `pos`를 누르면 `rect`의 무엇을 잡는가
pub fn grip_at(rect: Rectangle<i32, Logical>, pos: Point<f64, Logical>) -> Grip {
    let (left, top) = (f64::from(rect.loc.x), f64::from(rect.loc.y));
    let (right, bottom) = (left + f64::from(rect.size.w), top + f64::from(rect.size.h));
    let within_x = pos.x >= left - REACH && pos.x <= right + REACH;
    let within_y = pos.y >= top - REACH && pos.y <= bottom + REACH;
    if within_x && within_y {
        let x = near_side(pos.x, left, right);
        let y = near_side(pos.y, top, bottom);
        if x.is_some() || y.is_some() {
            return Grip::Edge { x, y };
        }
    }
    if rect.to_f64().contains(pos) {
        Grip::Inside
    } else {
        Grip::Outside
    }
}

/// `value`가 [start, end]의 어느 끝 가까이 있는지. 두 끝 모두 가까우면(작은 영역) 더 가까운 쪽이다
fn near_side(value: f64, start: f64, end: f64) -> Option<Side> {
    let inward = REACH.min((end - start) / 3.0);
    let at_start = value >= start - REACH && value <= start + inward;
    let at_end = value >= end - inward && value <= end + REACH;
    match (at_start, at_end) {
        (true, true) if (value - start).abs() <= (value - end).abs() => Some(Side::Start),
        (true, false) => Some(Side::Start),
        (_, true) => Some(Side::End),
        (false, false) => None,
    }
}

/// 누른 채 끄는 중: 잡은 것, 누른 자리, 누를 때의 영역
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub grip: Grip,
    from: Point<f64, Logical>,
    rect: Rectangle<i32, Logical>,
}

impl Drag {
    pub fn new(grip: Grip, from: Point<f64, Logical>, rect: Rectangle<i32, Logical>) -> Self {
        Self { grip, from, rect }
    }

    /// 누를 때의 영역
    pub fn started_with(&self) -> Rectangle<i32, Logical> {
        self.rect
    }

    /// 포인터가 `to`에 왔을 때의 영역. `bounds`(영역이 놓인 화면) 밖으로 나가지 않는다. 바깥에서 새로
    /// 고르는 중에 아직 가장 작은 변보다 작으면 누를 때의 영역 그대로다(누르기만 하면 바뀌지 않는다)
    pub fn rect_at(
        &self,
        to: Point<f64, Logical>,
        bounds: Rectangle<i32, Logical>,
    ) -> Rectangle<i32, Logical> {
        match self.grip {
            Grip::Inside => moved(self.rect, to - self.from, bounds),
            Grip::Edge { x, y } => {
                let delta = to - self.from;
                let (left, width) = resized_axis(
                    (self.rect.loc.x, self.rect.size.w),
                    x,
                    delta.x,
                    (bounds.loc.x, bounds.loc.x + bounds.size.w),
                );
                let (top, height) = resized_axis(
                    (self.rect.loc.y, self.rect.size.h),
                    y,
                    delta.y,
                    (bounds.loc.y, bounds.loc.y + bounds.size.h),
                );
                Rectangle::new((left, top).into(), (width, height).into())
            }
            Grip::Outside => spanned(self.from, to, bounds)
                .filter(|rect| rect.size.w >= MIN_SIDE && rect.size.h >= MIN_SIDE)
                .unwrap_or(self.rect),
        }
    }
}

/// 크기를 지킨 채 옮긴다. 화면보다 크면 화면 크기로 줄인다
fn moved(
    rect: Rectangle<i32, Logical>,
    delta: Point<f64, Logical>,
    bounds: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let size: Size<i32, Logical> = (
        rect.size.w.min(bounds.size.w),
        rect.size.h.min(bounds.size.h),
    )
        .into();
    let place = |start: i32, delta: f64, low: i32, span: i32, len: i32| {
        ((f64::from(start) + delta).round() as i32).clamp(low, low + span - len)
    };
    Rectangle::new(
        (
            place(rect.loc.x, delta.x, bounds.loc.x, bounds.size.w, size.w),
            place(rect.loc.y, delta.y, bounds.loc.y, bounds.size.h, size.h),
        )
            .into(),
        size,
    )
}

/// 한 축의 (시작, 길이)에서 잡은 변(`side`)을 `delta`만큼 옮긴다. 다른 변은 그대로다. 잡은 변이 반대쪽
/// 변을 넘어가면 뒤집히고, 가장 작은 변보다 좁아지면 고정한 변에서 그만큼 뻗는다. [low, high] 밖으로
/// 나가지 않는다
fn resized_axis(
    (start, len): (i32, i32),
    side: Option<Side>,
    delta: f64,
    (low, high): (i32, i32),
) -> (i32, i32) {
    let Some(side) = side else {
        return (start, len);
    };
    let end = start + len;
    let (fixed, moving) = match side {
        Side::Start => (end, start),
        Side::End => (start, end),
    };
    let moving = ((f64::from(moving) + delta).round() as i32).clamp(low, high);
    if (moving - fixed).abs() >= MIN_SIDE {
        return (fixed.min(moving), (moving - fixed).abs());
    }
    let toward_end = moving > fixed || (moving == fixed && side == Side::End);
    if toward_end {
        (fixed.min(high - MIN_SIDE), MIN_SIDE)
    } else {
        (fixed.max(low + MIN_SIDE) - MIN_SIDE, MIN_SIDE)
    }
}

/// 두 점이 맞모서리인 영역 (화면 안으로 자른다). 넓이가 없으면 None
fn spanned(
    from: Point<f64, Logical>,
    to: Point<f64, Logical>,
    bounds: Rectangle<i32, Logical>,
) -> Option<Rectangle<i32, Logical>> {
    let rect = Rectangle::<f64, Logical>::bounding_box([from, to]);
    let (left, top) = (rect.loc.x.floor() as i32, rect.loc.y.floor() as i32);
    let (right, bottom) = (
        (rect.loc.x + rect.size.w).ceil() as i32,
        (rect.loc.y + rect.size.h).ceil() as i32,
    );
    Rectangle::new((left, top).into(), (right - left, bottom - top).into())
        .intersection(bounds)
        .filter(|rect| !rect.is_empty())
}

/// 선택 영역을 띄울 자리와 그 화면. 기억한 영역이 걸친 화면이 있으면 그 화면 안으로 들인 영역이고(가운데가
/// 든 화면이 먼저), 아니면 `pointer`가 있는 화면 가운데의 영역이다. 화면이 없으면 None
pub fn initial(
    remembered: Option<Rectangle<i32, Logical>>,
    outputs: &[Rectangle<i32, Logical>],
    pointer: Point<f64, Logical>,
) -> Option<(Rectangle<i32, Logical>, Rectangle<i32, Logical>)> {
    if let Some(rect) = remembered {
        let center = Point::<f64, Logical>::from((
            f64::from(rect.loc.x) + f64::from(rect.size.w) / 2.0,
            f64::from(rect.loc.y) + f64::from(rect.size.h) / 2.0,
        ));
        let bounds = outputs
            .iter()
            .find(|output| output.to_f64().contains(center))
            .or_else(|| outputs.iter().find(|output| output.overlaps(rect)));
        if let Some(bounds) = bounds {
            return Some((fit_inside(rect, *bounds), *bounds));
        }
    }
    let bounds = *outputs
        .iter()
        .find(|output| output.to_f64().contains(pointer))
        .or(outputs.first())?;
    let size: Size<i32, Logical> = (
        (f64::from(bounds.size.w) * FIRST).round() as i32,
        (f64::from(bounds.size.h) * FIRST).round() as i32,
    )
        .into();
    let loc =
        bounds.loc + Point::from(((bounds.size.w - size.w) / 2, (bounds.size.h - size.h) / 2));
    Some((Rectangle::new(loc, size), bounds))
}

/// 화면 안으로 들인 영역. 화면보다 크면 줄이고, 가장 작은 변보다 작으면 늘인다
fn fit_inside(
    rect: Rectangle<i32, Logical>,
    bounds: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let size: Size<i32, Logical> = (
        rect.size.w.clamp(MIN_SIDE, bounds.size.w.max(MIN_SIDE)),
        rect.size.h.clamp(MIN_SIDE, bounds.size.h.max(MIN_SIDE)),
    )
        .into();
    let resized = Rectangle::new(rect.loc, size);
    moved(resized, Point::default(), bounds)
}

/// 고칠 수 있는 선택 영역의 모양: 바깥 어둡게, 테두리, 손잡이. 요소 ID는 늘 같게 두어 손상 추적이 같은
/// 요소가 움직인 것으로 본다
pub struct Overlay {
    /// 위, 아래, 왼쪽, 오른쪽의 어두운 띠와 테두리
    dim: [Id; 4],
    border: [Id; 4],
    /// 손잡이 그림을 버퍼 배율(정수)마다 하나씩. 메모리 버퍼의 요소 ID는 버퍼에서 오므로 손잡이마다 따로 둔다
    handles: Vec<(i32, Vec<MemoryRenderBuffer>)>,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            dim: std::array::from_fn(|_| Id::new()),
            border: std::array::from_fn(|_| Id::new()),
            handles: Vec::new(),
        }
    }
}

impl Overlay {
    /// 화면 하나(`output`, 전역 논리 좌표)에 그릴 요소. 앞의 것부터 손잡이, 테두리, 어두운 띠다
    pub fn elements(
        &mut self,
        renderer: &mut GlesRenderer,
        rect: Rectangle<i32, Logical>,
        output: Rectangle<i32, Logical>,
        scale: Scale<f64>,
    ) -> Vec<OutputRenderElements> {
        let screen = output.size.to_physical_precise_round(scale);
        let edges = Edges::of(rect, output, scale, screen);
        let mut elements = Vec::new();
        // 영역이 없는 화면은 어둡게만 덮는다. 전에는 화면 끝으로 자른 변에 손잡이와 테두리를 그려, 영역이 없는
        // 옆 화면 가장자리에 반쪽 손잡이가 보였다
        if !rect.overlaps(output) {
            elements.push(solid(&self.dim[0], Rectangle::from_size(screen), DIM));
            return elements;
        }
        let buffer_scale = scale.x.max(scale.y).ceil().max(1.0) as i32;
        let half = f64::from(HANDLE) * scale.x / 2.0;
        let handle_size = Size::from((HANDLE, HANDLE));
        for (buffer, center) in self
            .handles(buffer_scale)
            .iter()
            .zip(edges.handle_centers())
        {
            let location = Point::<f64, Physical>::from((center.x - half, center.y - half));
            match MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                location,
                buffer,
                None,
                None,
                Some(handle_size),
                Kind::Unspecified,
            ) {
                Ok(element) => elements.push(OutputRenderElements::Memory(element)),
                Err(err) => tracing::warn!("failed to draw a selection handle: {err}"),
            }
        }
        let thickness = scale.x.round().max(1.0) as i32;
        let border = self.border.iter().zip(edges.border(thickness));
        let dim = self.dim.iter().zip(edges.outside(screen));
        for ((id, part), color) in border
            .map(|part| (part, BORDER))
            .chain(dim.map(|part| (part, DIM)))
        {
            // 넓이가 없는 띠(화면 끝에 붙은 영역)는 넣지 않는다. 손상 추적이 지난 자리를 지운다
            if !part.is_empty() {
                elements.push(solid(id, part, color));
            }
        }
        elements
    }

    /// 버퍼 배율 `scale`의 손잡이 그림 여덟 개
    fn handles(&mut self, scale: i32) -> &[MemoryRenderBuffer] {
        let index = match self.handles.iter().position(|(other, _)| *other == scale) {
            Some(index) => index,
            None => {
                let size = HANDLE * scale;
                let pixels = handle_pixels(size, HANDLE_RING * f64::from(scale));
                let buffers = (0..8)
                    .map(|_| {
                        MemoryRenderBuffer::from_slice(
                            &pixels,
                            Fourcc::Argb8888,
                            (size, size),
                            scale,
                            Transform::Normal,
                            None,
                        )
                    })
                    .collect();
                self.handles.push((scale, buffers));
                self.handles.len() - 1
            }
        };
        &self.handles[index].1
    }
}

fn solid(id: &Id, geometry: Rectangle<i32, Physical>, color: [f32; 4]) -> OutputRenderElements {
    OutputRenderElements::Solid(SolidColorRenderElement::new(
        id.clone(),
        geometry,
        CommitCounter::default(),
        color,
        Kind::Unspecified,
    ))
}

/// 선택 영역의 변을 그 화면의 물리 좌표로 옮긴 것. 위치와 크기를 따로 반올림하면 소수 배율에서 띠와
/// 테두리 사이에 틈이 생기므로 변을 하나씩 옮긴다. 화면 밖으로 나간 변은 화면 끝으로 자른다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edges {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl Edges {
    fn of(
        rect: Rectangle<i32, Logical>,
        output: Rectangle<i32, Logical>,
        scale: Scale<f64>,
        screen: Size<i32, Physical>,
    ) -> Self {
        let x = |value: i32| (f64::from(value - output.loc.x) * scale.x).round() as i32;
        let y = |value: i32| (f64::from(value - output.loc.y) * scale.y).round() as i32;
        Self {
            left: x(rect.loc.x).clamp(0, screen.w),
            top: y(rect.loc.y).clamp(0, screen.h),
            right: x(rect.loc.x + rect.size.w).clamp(0, screen.w),
            bottom: y(rect.loc.y + rect.size.h).clamp(0, screen.h),
        }
    }

    /// 손잡이 가운데: 왼쪽 위부터 시계 방향으로 모서리와 변 가운데
    fn handle_centers(self) -> [Point<f64, Physical>; 8] {
        let (left, top) = (f64::from(self.left), f64::from(self.top));
        let (right, bottom) = (f64::from(self.right), f64::from(self.bottom));
        let (middle, center) = ((left + right) / 2.0, (top + bottom) / 2.0);
        [
            (left, top),
            (middle, top),
            (right, top),
            (right, center),
            (right, bottom),
            (middle, bottom),
            (left, bottom),
            (left, center),
        ]
        .map(Point::from)
    }

    /// 위, 아래, 왼쪽, 오른쪽 테두리 (영역 안쪽에 `thickness` 두께로)
    fn border(self, thickness: i32) -> [Rectangle<i32, Physical>; 4] {
        let (width, height) = (self.right - self.left, self.bottom - self.top);
        let thick_x = thickness.min(width);
        let thick_y = thickness.min(height);
        [
            physical_rect(self.left, self.top, width, thick_y),
            physical_rect(self.left, self.bottom - thick_y, width, thick_y),
            physical_rect(self.left, self.top, thick_x, height),
            physical_rect(self.right - thick_x, self.top, thick_x, height),
        ]
    }

    /// 영역 바깥을 덮는 띠: 위, 아래(화면 너비 전체), 왼쪽, 오른쪽(영역 높이만큼)
    fn outside(self, screen: Size<i32, Physical>) -> [Rectangle<i32, Physical>; 4] {
        let height = self.bottom - self.top;
        [
            physical_rect(0, 0, screen.w, self.top),
            physical_rect(0, self.bottom, screen.w, screen.h - self.bottom),
            physical_rect(0, self.top, self.left, height),
            physical_rect(self.right, self.top, screen.w - self.right, height),
        ]
    }
}

fn physical_rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Physical> {
    Rectangle::new((x, y).into(), (w.max(0), h.max(0)).into())
}

/// 손잡이 그림: 흰 동그라미에 옅은 검은 테두리. Argb8888(리틀 엔디언 B, G, R, A)이고 알파를 미리 곱했다.
/// 가장자리는 픽셀 가운데까지의 거리로 덮는 정도를 매겨 부드럽게 한다
fn handle_pixels(size: i32, ring: f64) -> Vec<u8> {
    let size = size.max(1);
    let outer = f64::from(size) / 2.0;
    let inner = (outer - ring).max(0.0);
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (f64::from(x) + 0.5 - outer, f64::from(y) + 0.5 - outer);
            let distance = (dx * dx + dy * dy).sqrt();
            let cover = |radius: f64| (radius - distance + 0.5).clamp(0.0, 1.0);
            let fill = cover(inner);
            let alpha = fill + (cover(outer) - fill) * HANDLE_RING_ALPHA;
            let white = (fill * 255.0).round() as u8;
            pixels.extend_from_slice(&[white, white, white, (alpha * 255.0).round() as u8]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::backend::renderer::element::Element;

    use crate::testing::{self, Frames};

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn point(x: f64, y: f64) -> Point<f64, Logical> {
        (x, y).into()
    }

    /// 1280×800 화면 하나
    fn screen() -> Rectangle<i32, Logical> {
        rect(0, 0, 1280, 800)
    }

    #[test]
    fn presses_grab_the_inside_the_edges_the_corners_or_the_outside() {
        let area = rect(100, 100, 400, 300);
        assert_eq!(grip_at(area, point(300.0, 250.0)), Grip::Inside);
        assert_eq!(grip_at(area, point(50.0, 250.0)), Grip::Outside);
        assert_eq!(grip_at(area, point(300.0, 420.0)), Grip::Outside);
        // 변은 바깥으로 8px, 안으로 8px까지 잡힌다
        let left = Grip::Edge {
            x: Some(Side::Start),
            y: None,
        };
        assert_eq!(grip_at(area, point(93.0, 250.0)), left);
        assert_eq!(grip_at(area, point(107.0, 250.0)), left);
        assert_eq!(grip_at(area, point(109.0, 250.0)), Grip::Inside);
        assert_eq!(
            grip_at(area, point(300.0, 401.0)),
            Grip::Edge {
                x: None,
                y: Some(Side::End)
            }
        );
        // 모서리는 두 변을 함께 잡는다
        assert_eq!(
            grip_at(area, point(503.0, 96.0)),
            Grip::Edge {
                x: Some(Side::End),
                y: Some(Side::Start)
            }
        );
        // 변을 따라 영역 밖으로 멀리 가면 잡지 않는다
        assert_eq!(grip_at(area, point(93.0, 450.0)), Grip::Outside);
    }

    #[test]
    fn small_areas_can_still_be_moved_and_take_the_nearer_edge() {
        let small = rect(100, 100, 12, 12);
        // 안쪽으로는 변 길이의 3분의 1(4px)까지만 변이다
        assert_eq!(grip_at(small, point(106.0, 106.0)), Grip::Inside);
        assert_eq!(
            grip_at(small, point(103.0, 106.0)),
            Grip::Edge {
                x: Some(Side::Start),
                y: None
            }
        );
        assert_eq!(
            grip_at(small, point(110.0, 106.0)),
            Grip::Edge {
                x: Some(Side::End),
                y: None
            }
        );
        // 넓이가 없는 영역도 더 가까운 변을 잡는다
        let thin = rect(100, 100, 0, 50);
        assert_eq!(
            grip_at(thin, point(101.0, 120.0)),
            Grip::Edge {
                x: Some(Side::End),
                y: None
            }
        );
    }

    #[test]
    fn grips_pick_macos_cursors() {
        let corner = |x, y| Grip::Edge {
            x: Some(x),
            y: Some(y),
        };
        assert_eq!(Grip::Inside.cursor(), CursorIcon::Move);
        assert_eq!(Grip::Outside.cursor(), CursorIcon::Crosshair);
        assert_eq!(
            corner(Side::Start, Side::Start).cursor(),
            CursorIcon::NwseResize
        );
        assert_eq!(
            corner(Side::End, Side::End).cursor(),
            CursorIcon::NwseResize
        );
        assert_eq!(
            corner(Side::End, Side::Start).cursor(),
            CursorIcon::NeswResize
        );
        assert_eq!(
            Grip::Edge {
                x: Some(Side::End),
                y: None
            }
            .cursor(),
            CursorIcon::EwResize
        );
        assert_eq!(
            Grip::Edge {
                x: None,
                y: Some(Side::Start)
            }
            .cursor(),
            CursorIcon::NsResize
        );
    }

    #[test]
    fn dragging_inside_moves_the_area_and_keeps_it_on_the_screen() {
        let area = rect(100, 100, 400, 300);
        let drag = Drag::new(Grip::Inside, point(300.0, 250.0), area);
        assert_eq!(
            drag.rect_at(point(350.4, 190.0), screen()),
            rect(150, 40, 400, 300)
        );
        // 화면 끝에서 멈춘다. 크기는 그대로다
        assert_eq!(
            drag.rect_at(point(-500.0, 250.0), screen()),
            rect(0, 100, 400, 300)
        );
        assert_eq!(
            drag.rect_at(point(3000.0, 3000.0), screen()),
            rect(880, 500, 400, 300)
        );
        // 화면보다 큰 영역은 화면 크기가 된다
        let huge = Drag::new(Grip::Inside, point(0.0, 0.0), rect(-10, -10, 2000, 1000));
        assert_eq!(huge.rect_at(point(5.0, 5.0), screen()), screen());
    }

    #[test]
    fn dragging_an_edge_or_a_corner_resizes_from_the_other_side() {
        let area = rect(100, 100, 400, 300);
        let right = Drag::new(
            Grip::Edge {
                x: Some(Side::End),
                y: None,
            },
            point(500.0, 200.0),
            area,
        );
        assert_eq!(
            right.rect_at(point(600.0, 900.0), screen()),
            rect(100, 100, 500, 300)
        );
        assert_eq!(
            right.rect_at(point(200.0, 200.0), screen()),
            rect(100, 100, 100, 300)
        );
        // 반대쪽 변을 넘어가면 뒤집힌다
        assert_eq!(
            right.rect_at(point(40.0, 200.0), screen()),
            rect(40, 100, 60, 300)
        );
        // 화면 밖으로는 늘지 않는다
        assert_eq!(
            right.rect_at(point(5000.0, 200.0), screen()),
            rect(100, 100, 1180, 300)
        );
        let top_left = Drag::new(
            Grip::Edge {
                x: Some(Side::Start),
                y: Some(Side::Start),
            },
            point(100.0, 100.0),
            area,
        );
        assert_eq!(
            top_left.rect_at(point(80.0, 60.0), screen()),
            rect(80, 60, 420, 340)
        );
        assert_eq!(
            top_left.rect_at(point(-30.0, -30.0), screen()),
            rect(0, 0, 500, 400)
        );
    }

    #[test]
    fn areas_never_get_thinner_than_the_smallest_side() {
        let area = rect(100, 100, 400, 300);
        let left = Drag::new(
            Grip::Edge {
                x: Some(Side::Start),
                y: None,
            },
            point(100.0, 200.0),
            area,
        );
        // 반대쪽 변 바로 앞에 멈추면 가장 작은 변만큼 남긴다
        assert_eq!(
            left.rect_at(point(499.0, 200.0), screen()),
            rect(496, 100, 4, 300)
        );
        assert_eq!(
            left.rect_at(point(500.0, 200.0), screen()),
            rect(496, 100, 4, 300)
        );
        assert_eq!(
            left.rect_at(point(502.0, 200.0), screen()),
            rect(500, 100, 4, 300)
        );
        // 화면 끝에 붙은 영역은 화면 안쪽으로 남긴다
        let edge = Drag::new(
            Grip::Edge {
                x: Some(Side::Start),
                y: None,
            },
            point(1180.0, 10.0),
            rect(1180, 0, 100, 100),
        );
        assert_eq!(
            edge.rect_at(point(1290.0, 10.0), screen()),
            rect(1276, 0, 4, 100)
        );
    }

    #[test]
    fn dragging_outside_draws_a_new_area_only_once_it_is_big_enough() {
        let area = rect(100, 100, 400, 300);
        let new = Drag::new(Grip::Outside, point(700.5, 500.5), area);
        assert_eq!(
            new.rect_at(point(902.2, 610.0), screen()),
            rect(700, 500, 203, 110)
        );
        // 거꾸로 끌어도 같다
        assert_eq!(
            new.rect_at(point(600.0, 450.0), screen()),
            rect(600, 450, 101, 51)
        );
        // 누르기만 했거나 아직 작으면 앞의 영역 그대로
        assert_eq!(new.rect_at(point(700.5, 500.5), screen()), area);
        assert_eq!(new.rect_at(point(702.0, 520.0), screen()), area);
        // 화면 밖으로 끌면 화면 끝에서 자른다
        assert_eq!(
            new.rect_at(point(2000.0, -50.0), screen()),
            rect(700, 0, 580, 501)
        );
    }

    #[test]
    fn the_first_area_is_centered_and_remembered_areas_come_back_inside_a_screen() {
        let left = rect(0, 0, 1280, 800);
        let right = rect(1280, 0, 1920, 1080);
        let outputs = [left, right];
        // 처음에는 포인터가 있는 화면 가운데, 화면의 반
        assert_eq!(
            initial(None, &outputs, point(1500.0, 10.0)),
            Some((rect(1760, 270, 960, 540), right))
        );
        // 기억한 영역은 그대로
        let remembered = rect(200, 150, 300, 200);
        assert_eq!(
            initial(Some(remembered), &outputs, point(1500.0, 10.0)),
            Some((remembered, left))
        );
        // 화면 경계에 걸치면 가운데가 든 화면 안으로 들인다
        assert_eq!(
            initial(Some(rect(1200, 100, 300, 200)), &outputs, point(0.0, 0.0)),
            Some((rect(1280, 100, 300, 200), right))
        );
        // 해상도를 줄여 화면 밖에 남은 영역은 버리고 가운데에 새로 띄운다
        assert_eq!(
            initial(Some(rect(4000, 3000, 300, 200)), &[left], point(10.0, 10.0)),
            Some((rect(320, 200, 640, 400), left))
        );
        // 화면보다 크면 화면에 맞춘다
        assert_eq!(
            initial(
                Some(rect(-100, -100, 3000, 3000)),
                &[left],
                point(10.0, 10.0)
            ),
            Some((left, left))
        );
        assert_eq!(initial(None, &[], point(0.0, 0.0)), None);
    }

    #[test]
    fn edges_meet_without_gaps_at_fractional_scales() {
        let scale = Scale::from(1.25);
        let output = rect(0, 0, 1024, 640);
        let screen = output.size.to_physical_precise_round(scale);
        let edges = Edges::of(rect(101, 33, 333, 211), output, scale, screen);
        let [top, bottom, left, right] = edges.outside(screen);
        // 띠가 영역과 맞닿고 화면을 덮는다
        assert_eq!(top.loc.y + top.size.h, edges.top);
        assert_eq!(bottom.loc.y, edges.bottom);
        assert_eq!(left.loc.x + left.size.w, edges.left);
        assert_eq!(right.loc.x, edges.right);
        assert_eq!(bottom.loc.y + bottom.size.h, screen.h);
        assert_eq!(right.loc.x + right.size.w, screen.w);
        // 화면 밖으로 나간 영역은 화면 끝까지만
        let clipped = Edges::of(rect(-50, 600, 100, 100), output, scale, screen);
        assert_eq!((clipped.left, clipped.bottom), (0, screen.h));
        assert!(
            clipped
                .outside(screen)
                .iter()
                .all(|band| band.size.w >= 0 && band.size.h >= 0)
        );
    }

    #[test]
    fn screens_without_the_selection_are_only_dimmed() {
        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let mut overlay = Overlay::default();
        // 영역은 왼쪽 화면에 있고 오른쪽 화면(1280부터)에는 손잡이도 테두리도 없이 한 장으로 덮는다
        let right = rect(1280, 0, 1024, 768);
        let elements = overlay.elements(
            &mut renderer,
            rect(100, 100, 400, 300),
            right,
            Scale::from(1.0),
        );
        assert_eq!(elements.len(), 1);
        assert!(matches!(elements[0], OutputRenderElements::Solid(_)));
        assert_eq!(
            elements[0].geometry(Scale::from(1.0)),
            Rectangle::from_size((1024, 768).into())
        );
        // 맞닿기만 한 화면도 같다
        let touching = overlay.elements(
            &mut renderer,
            rect(1180, 100, 100, 100),
            right,
            Scale::from(1.0),
        );
        assert_eq!(touching.len(), 1);
    }

    #[test]
    fn handles_are_white_discs_with_a_soft_ring() {
        let size = 9;
        let pixels = handle_pixels(size, 1.0);
        let at = |x: i32, y: i32| {
            let index = ((y * size + x) * 4) as usize;
            [
                pixels[index],
                pixels[index + 1],
                pixels[index + 2],
                pixels[index + 3],
            ]
        };
        assert_eq!(at(4, 4), [255, 255, 255, 255]);
        // 모서리는 비어 있고, 테두리는 어둡고 반투명하다
        assert_eq!(at(0, 0)[3], 0);
        let ring = at(4, 0);
        assert!(ring[3] > 0 && ring[3] < 200, "{ring:?}");
        assert!(ring[0] < 128, "{ring:?}");
        // 미리 곱한 색은 알파보다 크지 않다
        for pixel in pixels.chunks(4) {
            assert!(pixel[0] <= pixel[3], "{pixel:?}");
        }
    }

    #[test]
    fn the_overlay_dims_the_outside_and_draws_the_border_and_handles() {
        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let (width, height) = (64, 48);
        let mut frames = Frames::new(&mut renderer, (width, height).into(), 1.0);
        let background = MemoryRenderBuffer::from_slice(
            &testing::argb(width as usize, height as usize, [200, 120, 40, 255]),
            Fourcc::Argb8888,
            (width, height),
            1,
            Transform::Normal,
            None,
        );
        let mut overlay = Overlay::default();
        let mut elements = overlay.elements(
            &mut renderer,
            rect(16, 12, 32, 24),
            rect(0, 0, width, height),
            Scale::from(1.0),
        );
        elements.push(testing::background(&mut renderer, &background));
        let pixels = frames.render(&mut renderer, &elements);
        let pixel = |x, y| testing::pixel(&pixels, width as usize, x, y);
        // 안쪽은 그대로, 바깥은 어둡다 (0.6배)
        assert_eq!(pixel(32, 24), [200, 120, 40, 255]);
        let dimmed = pixel(4, 40);
        assert!(
            (118..=122).contains(&dimmed[0]) && (70..=74).contains(&dimmed[1]),
            "{dimmed:?}"
        );
        // 테두리 (위 변의 가운데는 손잡이라 옆에서 본다)
        let border = pixel(24, 12);
        assert!(border[0] > 220 && border[2] > 220, "{border:?}");
        // 손잡이는 모서리와 변 가운데에 흰 동그라미로
        for (x, y) in [(16, 12), (32, 12), (48, 36), (16, 24)] {
            let handle = pixel(x, y);
            assert!(
                handle.iter().take(3).all(|&c| c > 230),
                "({x}, {y}): {handle:?}"
            );
        }
        // 손잡이에서 떨어진 바깥 띠에는 손잡이가 없다
        assert_eq!(pixel(32, 4), dimmed);
    }
}
