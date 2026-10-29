// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 앱 창 뒤 흐림 요청 (ext-background-effect-v1, 디자인 스펙 2.2절).
//!
//! 앱은 표면의 어느 곳 뒤를 흐릴지 영역(wl_region)으로 알리고, 컴포지터는 그 영역 뒤를 셸 유리와 같은
//! 흐림(blur.rs)으로 그린다. 셸 유리는 표면 알파로 흐릴 곳을 찾지만, 앱의 영역은 앱이 정한 곳이므로
//! 알파와 상관없이 다 흐린다.
//! - 영역은 표면 기준 논리 좌표이고 wl_surface.commit에 함께 바뀐다(smithay의 cached_state). 표면 크기
//!   밖은 그릴 때 자른다
//! - 영역은 받을 때 겹치지 않는 사각형으로 펴 둔다. 사각형이 MAX_RECTS보다 많으면 더하는 사각형 전체를
//!   감싸는 사각형 하나로 흐린다(그리는 횟수와 펴는 계산이 사각형 수를 따라 늘기 때문이다)
//! - 셸 유리로 흐리는 셸 표면(udev.rs)은 셸 유리가 먼저다
//! - 흐림을 끈 설정(로그인 화면, `[effects] blur = false`)에서는 전역을 열지 않는다. 흐림 셰이더를
//!   만들지 못하면 흐림 기능이 없어졌다고 알린다(capabilities 0)

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use smithay::{
    reexports::{
        wayland_protocols::ext::background_effect::v1::server::{
            ext_background_effect_manager_v1::{self, Capability, ExtBackgroundEffectManagerV1},
            ext_background_effect_surface_v1::{self, ExtBackgroundEffectSurfaceV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, Weak,
            backend::GlobalId, protocol::wl_surface::WlSurface,
        },
    },
    utils::{Logical, Rectangle},
    wayland::compositor::{
        Cacheable, RectangleKind, RegionAttributes, get_region_attributes, with_states,
    },
};

use crate::{blur::BlurTarget, state::BuntState};

/// 펴서 둘 사각형의 최대 수. 넘으면 감싸는 사각형 하나로 흐린다
const MAX_RECTS: usize = 64;

/// 표면의 흐림 영역 (커밋으로 바뀌는 상태)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlurRegionState {
    /// 겹치지 않는 사각형 (표면 기준 논리 좌표). None이면 흐리지 않는다
    rects: Option<Arc<[Rectangle<i32, Logical>]>>,
}

impl Cacheable for BlurRegionState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        self.clone()
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// 표면에 효과 객체가 있다 (한 표면에 하나만 만들 수 있다)
#[derive(Default)]
struct HasEffect(AtomicBool);

/// 효과 객체의 데이터: 그 표면
pub struct EffectData {
    surface: Weak<WlSurface>,
}

pub trait BackgroundEffectHandler {
    fn background_effect_state(&mut self) -> &mut BackgroundEffectState;
}

/// 전역과 효과 객체를 만든 표면들
pub struct BackgroundEffectState {
    global: Option<GlobalId>,
    /// 흐림을 할 수 있다 (capabilities)
    blur: bool,
    managers: Vec<Weak<ExtBackgroundEffectManagerV1>>,
    /// 효과 객체를 만든 적이 있는 표면. 그릴 때 이 표면들만 본다
    surfaces: Vec<Weak<WlSurface>>,
}

impl BackgroundEffectState {
    /// `enabled`가 거짓이면 전역을 만들지 않는다 (흐림을 끈 설정)
    pub fn new<D>(display: &DisplayHandle, enabled: bool) -> Self
    where
        D: GlobalDispatch<ExtBackgroundEffectManagerV1, ()>
            + Dispatch<ExtBackgroundEffectManagerV1, ()>
            + Dispatch<ExtBackgroundEffectSurfaceV1, EffectData>
            + BackgroundEffectHandler
            + 'static,
    {
        Self {
            global: enabled
                .then(|| display.create_global::<D, ExtBackgroundEffectManagerV1, _>(1, ())),
            blur: enabled,
            managers: Vec::new(),
            surfaces: Vec::new(),
        }
    }

    /// 흐림을 할 수 있는지가 바뀌면 앱에 다시 알린다
    pub fn set_blur(&mut self, blur: bool) {
        if self.global.is_none() || self.blur == blur {
            return;
        }
        self.blur = blur;
        self.managers.retain(|manager| manager.upgrade().is_ok());
        for manager in self
            .managers
            .iter()
            .filter_map(|manager| manager.upgrade().ok())
        {
            manager.capabilities(capabilities(blur));
        }
        tracing::info!(
            "background blur for apps: {}",
            if blur { "on" } else { "off" }
        );
    }

    /// 지금 흐림 영역이 있는 표면. 사라진 표면과, 효과 객체를 없애고 영역도 비운 표면은 목록에서 뺀다
    pub fn targets(&mut self) -> Vec<BlurTarget> {
        if !self.blur || self.surfaces.is_empty() {
            return Vec::new();
        }
        let mut targets = Vec::new();
        self.surfaces.retain(|surface| {
            let Ok(surface) = surface.upgrade() else {
                return false;
            };
            let (rects, attached) = with_states(&surface, |states| {
                let rects = states
                    .cached_state
                    .get::<BlurRegionState>()
                    .current()
                    .rects
                    .clone();
                let attached = states
                    .data_map
                    .get::<HasEffect>()
                    .is_some_and(|has| has.0.load(Ordering::Relaxed));
                (rects, attached)
            });
            if let Some(rects) = rects.filter(|rects| !rects.is_empty()) {
                targets.push(BlurTarget {
                    surface,
                    shape: crate::blur::BlurShape::Region(rects),
                });
                return true;
            }
            attached
        });
        targets
    }

    fn track(&mut self, surface: &WlSurface) {
        if !self.surfaces.iter().any(|known| known == surface) {
            self.surfaces.push(surface.downgrade());
        }
    }
}

fn capabilities(blur: bool) -> Capability {
    if blur {
        Capability::Blur
    } else {
        Capability::empty()
    }
}

/// wl_region을 겹치지 않는 사각형으로 편다. 빼는 사각형은 앞에서 더한 곳에서만 뺀다(wl_region의 뜻).
/// 너비나 높이가 0 이하인 사각형은 빈 곳이다
pub fn flatten_region(region: &RegionAttributes) -> Vec<Rectangle<i32, Logical>> {
    let valid = |rect: &Rectangle<i32, Logical>| rect.size.w > 0 && rect.size.h > 0;
    if region.rects.len() > MAX_RECTS {
        return region
            .rects
            .iter()
            .filter(|(kind, rect)| matches!(kind, RectangleKind::Add) && valid(rect))
            .map(|(_, rect)| *rect)
            .reduce(Rectangle::merge)
            .into_iter()
            .collect();
    }
    let mut rects: Vec<Rectangle<i32, Logical>> = Vec::new();
    for (kind, rect) in &region.rects {
        if !valid(rect) {
            continue;
        }
        match kind {
            RectangleKind::Add => {
                let new = rect.subtract_rects(rects.iter().copied());
                rects.extend(new);
            }
            RectangleKind::Subtract => {
                rects = Rectangle::subtract_rects_many_in_place(rects, [*rect]);
            }
        }
    }
    if rects.len() > MAX_RECTS {
        // 빼기로 조각이 많아졌다
        return rects
            .into_iter()
            .reduce(Rectangle::merge)
            .into_iter()
            .collect();
    }
    rects
}

impl<D> GlobalDispatch<ExtBackgroundEffectManagerV1, (), D> for BackgroundEffectState
where
    D: GlobalDispatch<ExtBackgroundEffectManagerV1, ()>
        + Dispatch<ExtBackgroundEffectManagerV1, ()>
        + Dispatch<ExtBackgroundEffectSurfaceV1, EffectData>
        + BackgroundEffectHandler
        + 'static,
{
    fn bind(
        state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<ExtBackgroundEffectManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        let manager = data_init.init(resource, ());
        let effects = state.background_effect_state();
        manager.capabilities(capabilities(effects.blur));
        effects.managers.retain(|manager| manager.upgrade().is_ok());
        effects.managers.push(manager.downgrade());
    }
}

impl<D> Dispatch<ExtBackgroundEffectManagerV1, (), D> for BackgroundEffectState
where
    D: GlobalDispatch<ExtBackgroundEffectManagerV1, ()>
        + Dispatch<ExtBackgroundEffectManagerV1, ()>
        + Dispatch<ExtBackgroundEffectSurfaceV1, EffectData>
        + BackgroundEffectHandler
        + 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        resource: &ExtBackgroundEffectManagerV1,
        request: ext_background_effect_manager_v1::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            ext_background_effect_manager_v1::Request::GetBackgroundEffect { id, surface } => {
                let exists = with_states(&surface, |states| {
                    states
                        .data_map
                        .insert_if_missing_threadsafe(HasEffect::default);
                    states
                        .data_map
                        .get::<HasEffect>()
                        .is_some_and(|has| has.0.swap(true, Ordering::Relaxed))
                });
                // 오류를 보낸 앱은 끊기므로 새 객체를 만들지 않아도 된다 (wayland-backend가 허용한다)
                if exists {
                    resource.post_error(
                        ext_background_effect_manager_v1::Error::BackgroundEffectExists,
                        "the surface already has a background effect object",
                    );
                    return;
                }
                data_init.init(
                    id,
                    EffectData {
                        surface: surface.downgrade(),
                    },
                );
                state.background_effect_state().track(&surface);
            }
            ext_background_effect_manager_v1::Request::Destroy => {}
            _ => unreachable!(),
        }
    }
}

impl<D> Dispatch<ExtBackgroundEffectSurfaceV1, EffectData, D> for BackgroundEffectState
where
    D: Dispatch<ExtBackgroundEffectSurfaceV1, EffectData> + BackgroundEffectHandler + 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        resource: &ExtBackgroundEffectSurfaceV1,
        request: ext_background_effect_surface_v1::Request,
        data: &EffectData,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            ext_background_effect_surface_v1::Request::SetBlurRegion { region } => {
                let Ok(surface) = data.surface.upgrade() else {
                    resource.post_error(
                        ext_background_effect_surface_v1::Error::SurfaceDestroyed,
                        "the surface of this background effect object was destroyed",
                    );
                    return;
                };
                let rects =
                    region.map(|region| Arc::from(flatten_region(&get_region_attributes(&region))));
                with_states(&surface, |states| {
                    states.cached_state.get::<BlurRegionState>().pending().rects = rects;
                });
            }
            // 영역은 다음 커밋에 없어진다. 같은 표면에 효과 객체를 다시 만들 수 있다
            ext_background_effect_surface_v1::Request::Destroy => {
                if let Ok(surface) = data.surface.upgrade() {
                    with_states(&surface, |states| {
                        states.cached_state.get::<BlurRegionState>().pending().rects = None;
                        if let Some(has) = states.data_map.get::<HasEffect>() {
                            has.0.store(false, Ordering::Relaxed);
                        }
                    });
                }
            }
            _ => unreachable!(),
        }
    }
}

/// 상태 타입에 이 프로토콜의 처리를 붙인다 (smithay의 delegate_* 매크로와 같다)
macro_rules! delegate_background_effect {
    ($ty: ty) => {
        smithay::reexports::wayland_server::delegate_global_dispatch!($ty: [
            smithay::reexports::wayland_protocols::ext::background_effect::v1::server::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1: ()
        ] => $crate::background_effect::BackgroundEffectState);
        smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            smithay::reexports::wayland_protocols::ext::background_effect::v1::server::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1: ()
        ] => $crate::background_effect::BackgroundEffectState);
        smithay::reexports::wayland_server::delegate_dispatch!($ty: [
            smithay::reexports::wayland_protocols::ext::background_effect::v1::server::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1: $crate::background_effect::EffectData
        ] => $crate::background_effect::BackgroundEffectState);
    };
}
// 시험의 상태 타입에도 붙인다
#[cfg(test)]
pub(crate) use delegate_background_effect;

impl BackgroundEffectHandler for BuntState {
    fn background_effect_state(&mut self) -> &mut BackgroundEffectState {
        &mut self.backend.background_effects
    }
}
delegate_background_effect!(BuntState);

#[cfg(test)]
mod tests {
    use wayland_client::protocol::{
        wl_compositor::WlCompositor, wl_surface::WlSurface as ClientSurface,
    };
    use wayland_protocols::ext::background_effect::v1::client::{
        ext_background_effect_manager_v1::{self as manager, ExtBackgroundEffectManagerV1},
        ext_background_effect_surface_v1::{self as effect, ExtBackgroundEffectSurfaceV1},
    };

    use super::*;
    use crate::{
        blur::BlurShape,
        testing::{Server, ServerOptions, TestClient},
    };

    const MANAGER: &str = "ext_background_effect_manager_v1";

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn region(ops: &[(RectangleKind, Rectangle<i32, Logical>)]) -> RegionAttributes {
        RegionAttributes {
            rects: ops.to_vec(),
        }
    }

    fn area(rects: &[Rectangle<i32, Logical>]) -> i64 {
        rects
            .iter()
            .map(|rect| i64::from(rect.size.w) * i64::from(rect.size.h))
            .sum()
    }

    /// 펴 놓은 사각형이 서로 겹치지 않는지
    fn disjoint(rects: &[Rectangle<i32, Logical>]) -> bool {
        rects
            .iter()
            .enumerate()
            .all(|(index, a)| rects[index + 1..].iter().all(|b| !a.overlaps(*b)))
    }

    #[test]
    fn overlapping_rectangles_flatten_without_overlap() {
        use RectangleKind::Add;
        let rects = flatten_region(&region(&[
            (Add, rect(0, 0, 100, 100)),
            (Add, rect(50, 50, 100, 100)),
        ]));
        assert!(disjoint(&rects));
        // 두 사각형의 합집합: 100² + 100² - 50²
        assert_eq!(area(&rects), 17_500);
    }

    #[test]
    fn subtracting_cuts_a_hole_only_where_added_before() {
        use RectangleKind::{Add, Subtract};
        // 앞에 더한 곳이 없으면 빼도 아무것도 없다
        assert!(flatten_region(&region(&[(Subtract, rect(0, 0, 10, 10))])).is_empty());
        let rects = flatten_region(&region(&[
            (Add, rect(0, 0, 100, 100)),
            (Subtract, rect(25, 25, 50, 50)),
        ]));
        assert!(disjoint(&rects));
        assert_eq!(area(&rects), 100 * 100 - 50 * 50);
        assert!(!rects.iter().any(|r| r.contains((50, 50))));
        // 뺀 뒤에 다시 더한 곳은 들어간다
        let rects = flatten_region(&region(&[
            (Add, rect(0, 0, 100, 100)),
            (Subtract, rect(0, 0, 100, 100)),
            (Add, rect(10, 10, 5, 5)),
        ]));
        assert_eq!(rects, vec![rect(10, 10, 5, 5)]);
    }

    /// 음수 크기의 사각형. smithay의 Size::new는 디버그 빌드에서 음수를 막으므로 필드를 직접 쓴다.
    /// 릴리스 빌드의 wl_region.add는 앱이 보낸 음수를 그대로 담는다
    fn negative(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        let mut rect = rect(x, y, 0, 0);
        (rect.size.w, rect.size.h) = (w, h);
        rect
    }

    #[test]
    fn empty_and_negative_rectangles_are_ignored() {
        use RectangleKind::{Add, Subtract};
        let rects = flatten_region(&region(&[
            (Add, rect(0, 0, 0, 10)),
            (Add, negative(0, 0, -5, 10)),
            (Add, negative(0, 0, 10, i32::MIN)),
        ]));
        assert!(rects.is_empty());
        assert!(flatten_region(&RegionAttributes::default()).is_empty());
        // 음수 크기를 빼도 더한 곳은 그대로다
        let rects = flatten_region(&region(&[
            (Add, rect(0, 0, 10, 10)),
            (Subtract, negative(5, 5, -100, -100)),
        ]));
        assert_eq!(rects, vec![rect(0, 0, 10, 10)]);
    }

    #[test]
    fn extreme_coordinates_do_not_overflow() {
        use RectangleKind::{Add, Subtract};
        let rects = flatten_region(&region(&[
            (Add, rect(i32::MAX - 10, i32::MAX - 10, i32::MAX, i32::MAX)),
            (Add, rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX)),
            (Subtract, rect(-5, -5, 10, 10)),
        ]));
        assert!(!rects.is_empty());
        assert!(disjoint(&rects));
        // 사각형이 많아 감싸는 사각형으로 바꿀 때도 넘치지 않는다
        let mut ops: Vec<_> = (0..MAX_RECTS as i32)
            .map(|index| (Add, rect(index * 10, 0, 5, 5)))
            .collect();
        ops.push((Add, rect(i32::MIN, i32::MIN, i32::MAX, i32::MAX)));
        ops.push((Add, rect(i32::MAX - 1, i32::MAX - 1, i32::MAX, i32::MAX)));
        let rects = flatten_region(&region(&ops));
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].loc, (i32::MIN, i32::MIN).into());
    }

    #[test]
    fn too_many_rectangles_become_their_bounding_box() {
        use RectangleKind::Add;
        // 앱이 보낸 사각형이 많으면 다 펴지 않고 감싸는 사각형 하나로 흐린다
        let ops: Vec<_> = (0..MAX_RECTS as i32 + 1)
            .map(|index| (Add, rect(index * 10, 0, 5, 5)))
            .collect();
        assert_eq!(
            flatten_region(&region(&ops)),
            vec![rect(0, 0, MAX_RECTS as i32 * 10 + 5, 5)]
        );
        // 빼기로 조각이 많아져도 같다
        let mut ops = vec![(Add, rect(0, 0, 1000, 1000))];
        ops.extend((0..30).map(|index| {
            (
                RectangleKind::Subtract,
                rect(index * 30 + 5, index * 30 + 5, 10, 10),
            )
        }));
        let rects = flatten_region(&region(&ops));
        assert_eq!(rects, vec![rect(0, 0, 1000, 1000)]);
    }

    /// 흐림을 켠 시험 서버, 클라이언트, 그 클라이언트의 표면 하나와 효과 객체
    fn effect_setup() -> (
        Server,
        TestClient,
        ClientSurface,
        ExtBackgroundEffectManagerV1,
        ExtBackgroundEffectSurfaceV1,
    ) {
        let mut server = Server::new(
            None,
            ServerOptions {
                dmabuf_formats: None,
                blur: true,
            },
        );
        let mut client = server.connect();
        let compositor: WlCompositor = client.bind("wl_compositor", 4).unwrap();
        let manager: ExtBackgroundEffectManagerV1 = client.bind(MANAGER, 1).unwrap();
        let surface = compositor.create_surface(&client.qh, ());
        let effect = manager.get_background_effect(&surface, &client.qh, ());
        server.pump(&mut client);
        (server, client, surface, manager, effect)
    }

    fn region_of(server: &mut Server) -> Option<Vec<Rectangle<i32, Logical>>> {
        let targets = server.state.effects.targets();
        assert!(targets.len() <= 1);
        targets.into_iter().next().map(|target| match target.shape {
            BlurShape::Region(rects) => rects.to_vec(),
            BlurShape::Glass => panic!("an app region became glass"),
        })
    }

    #[test]
    fn capabilities_tell_whether_blur_is_available() {
        let (mut server, mut client, ..) = effect_setup();
        assert_eq!(
            client.data.capabilities,
            vec![manager::Capability::Blur.bits()]
        );
        // 흐림 셰이더를 만들지 못하면 다시 알린다. 같은 값은 다시 보내지 않는다
        server.state.effects.set_blur(false);
        server.state.effects.set_blur(false);
        server.pump(&mut client);
        assert_eq!(client.data.capabilities, vec![1, 0]);
        assert!(region_of(&mut server).is_none());
    }

    #[test]
    fn no_global_when_blur_is_off() {
        let mut server = Server::new(
            None,
            ServerOptions {
                dmabuf_formats: None,
                blur: false,
            },
        );
        let client = server.connect();
        assert_eq!(client.global_version(MANAGER), None);
    }

    #[test]
    fn the_region_changes_with_the_surface_commit() {
        let (mut server, mut client, surface, _manager, effect) = effect_setup();
        let compositor: WlCompositor = client.bind("wl_compositor", 4).unwrap();
        let region = compositor.create_region(&client.qh, ());
        region.add(10, 20, 30, 40);
        effect.set_blur_region(Some(&region));
        // 복사해 두므로 바로 없애도 된다
        region.destroy();
        server.pump(&mut client);
        assert_eq!(region_of(&mut server), None, "pending until the commit");
        surface.commit();
        server.pump(&mut client);
        assert_eq!(region_of(&mut server), Some(vec![rect(10, 20, 30, 40)]));
        // 다른 상태만 커밋해도 영역은 남는다
        surface.commit();
        server.pump(&mut client);
        assert_eq!(region_of(&mut server), Some(vec![rect(10, 20, 30, 40)]));
        // NULL이면 흐리지 않는다
        effect.set_blur_region(None);
        surface.commit();
        server.pump(&mut client);
        assert_eq!(region_of(&mut server), None);
        assert_eq!(client.protocol_error(), None);
    }

    #[test]
    fn destroying_the_effect_clears_the_region_on_the_next_commit() {
        let (mut server, mut client, surface, manager, effect) = effect_setup();
        let compositor: WlCompositor = client.bind("wl_compositor", 4).unwrap();
        let region = compositor.create_region(&client.qh, ());
        region.add(0, 0, 50, 50);
        effect.set_blur_region(Some(&region));
        surface.commit();
        server.pump(&mut client);
        assert!(region_of(&mut server).is_some());
        effect.destroy();
        server.pump(&mut client);
        assert!(
            region_of(&mut server).is_some(),
            "still there until the commit"
        );
        surface.commit();
        server.pump(&mut client);
        assert_eq!(region_of(&mut server), None);
        // 표면을 더 보지 않는다
        assert!(server.state.effects.surfaces.is_empty());
        // 같은 표면에 효과 객체를 다시 만들 수 있다
        let _again = manager.get_background_effect(&surface, &client.qh, ());
        server.pump(&mut client);
        assert_eq!(client.protocol_error(), None);
    }

    #[test]
    fn a_second_effect_for_a_surface_is_a_protocol_error() {
        let (mut server, mut client, surface, manager, _effect) = effect_setup();
        let _second = manager.get_background_effect(&surface, &client.qh, ());
        server.pump(&mut client);
        assert_eq!(
            client.protocol_error(),
            Some((
                manager::Error::BackgroundEffectExists as u32,
                MANAGER.into()
            ))
        );
    }

    #[test]
    fn using_the_effect_after_the_surface_is_gone_is_a_protocol_error() {
        let (mut server, mut client, surface, _manager, effect) = effect_setup();
        surface.destroy();
        server.pump(&mut client);
        // 표면이 사라진 표면은 그릴 대상에서도 빠진다
        assert!(region_of(&mut server).is_none());
        assert!(server.state.effects.surfaces.is_empty());
        effect.set_blur_region(None);
        server.pump(&mut client);
        assert_eq!(
            client.protocol_error(),
            Some((
                effect::Error::SurfaceDestroyed as u32,
                "ext_background_effect_surface_v1".into()
            ))
        );
    }
}
