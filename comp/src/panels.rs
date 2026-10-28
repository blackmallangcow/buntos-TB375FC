// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 셸 막대(메뉴 막대, Dock) 자동 가리기. 시스템 설정의 "자동으로 메뉴 막대 가리기 및 보기"와
//! "자동으로 Dock 가리기"(org.buntos.shell)를 셸이 D-Bus로 알려 준다.
//!
//! macOS의 WindowServer처럼 컴포지터가 막대를 숨긴다. 셸은 막대를 그대로 두고, 컴포지터가 숨은
//! 막대를 그리지 않고 입력도 보내지 않는다. 포인터가 화면 가장자리에 닿으면 막대를 미끄러지듯 꺼내고,
//! 막대를 떠나면 잠시 뒤 다시 넣는다. 최대화·전체 화면 창이 막대 자리까지 채울지도 여기서 정한다.

use std::time::{Duration, Instant};

use smithay::{
    utils::{Logical, Rectangle},
    wayland::shell::wlr_layer::{Anchor, Margins},
};

/// 미끄러져 나오고 들어가는 시간 (theme/tokens.css의 --duration)
pub const SLIDE: Duration = Duration::from_millis(200);
/// 포인터가 꺼낸 막대를 떠난 뒤 다시 숨기까지
pub const HIDE_DELAY: Duration = Duration::from_millis(400);
/// 숨은 막대를 꺼내는 화면 가장자리의 두께 (논리 픽셀)
pub const EDGE: f64 = 2.0;

/// 막대를 자동으로 가리는 때
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autohide {
    /// 늘 보인다. 전체 화면 창도 막대 자리는 비워 둔다
    Never,
    /// 전체 화면 창이 앞에 있을 때
    Fullscreen,
    /// 최대화하거나 전체 화면인 창이 앞에 있을 때
    Maximized,
    /// 늘 숨는다. 보통 창도 막대 자리까지 쓴다
    Always,
}

impl Autohide {
    /// GSettings 값 (never, fullscreen, maximized, always)
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "never" => Some(Self::Never),
            "fullscreen" => Some(Self::Fullscreen),
            "maximized" => Some(Self::Maximized),
            "always" => Some(Self::Always),
            _ => None,
        }
    }

    /// 앞에 있는 창이 이러면 막대가 숨는다
    pub fn hides(self, maximized: bool, fullscreen: bool) -> bool {
        match self {
            Self::Never => false,
            Self::Fullscreen => fullscreen,
            Self::Maximized => maximized || fullscreen,
            Self::Always => true,
        }
    }

    /// 이렇게 놓이는 창이 막대 자리까지 쓰는지
    pub fn covered_by(self, fill: Fill) -> bool {
        match fill {
            Fill::Normal => self == Self::Always,
            Fill::Maximized => matches!(self, Self::Maximized | Self::Always),
            Fill::Fullscreen => self != Self::Never,
        }
    }
}

/// 창이 놓이는 방식
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Normal,
    Maximized,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panels {
    pub menu_bar: Autohide,
    pub dock: Autohide,
    /// 메뉴 막대 배경 보기. 켜면 메뉴 막대 뒤도 흐린다
    pub menu_bar_background: bool,
}

impl Default for Panels {
    /// 셸이 알려 주기 전에는 스키마의 기본값을 쓴다. 메뉴 막대는 macOS와 같고, Dock은 최대화한
    /// 창에서도 숨는다(시스템 설정의 "전체 화면일 때")
    fn default() -> Self {
        Self {
            menu_bar: Autohide::Fullscreen,
            dock: Autohide::Maximized,
            menu_bar_background: false,
        }
    }
}

/// 셸 막대
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    MenuBar,
    Dock,
}

impl Bar {
    pub const ALL: [Bar; 2] = [Bar::MenuBar, Bar::Dock];

    pub fn index(self) -> usize {
        match self {
            Self::MenuBar => 0,
            Self::Dock => 1,
        }
    }
}

/// 막대 하나가 보이는 정도. 1이면 다 보이고 0이면 숨는다. 바뀔 때 SLIDE에 걸쳐 미끄러진다
#[derive(Clone, Copy, Debug)]
pub struct Reveal {
    from: f64,
    shown: bool,
    since: Option<Instant>,
    /// 포인터가 막대를 떠난 때. HIDE_DELAY가 지나면 숨긴다
    pub left_at: Option<Instant>,
    /// 지난번에 앞의 창 때문에 숨어야 했는지. 막 그렇게 됐을 때는 기다리지 않고 숨긴다
    covered: bool,
}

impl Default for Reveal {
    fn default() -> Self {
        Self {
            from: 1.0,
            shown: true,
            since: None,
            left_at: None,
            covered: false,
        }
    }
}

impl Reveal {
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// 지금 보이는 정도 (0..=1). 빨리 시작해 천천히 멈춘다(ease-out)
    pub fn value(&self, now: Instant) -> f64 {
        let target = if self.shown { 1.0 } else { 0.0 };
        let Some(since) = self.since else {
            return target;
        };
        let t = (now.saturating_duration_since(since).as_secs_f64() / SLIDE.as_secs_f64()).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (target - self.from) * eased
    }

    /// 보이게 하거나 숨긴다. 미끄러지는 중이면 지금 자리에서 방향을 바꾼다
    pub fn set(&mut self, shown: bool, now: Instant) {
        if self.shown == shown {
            return;
        }
        self.from = self.value(now);
        self.shown = shown;
        self.since = Some(now);
        self.left_at = None;
    }
}

/// 막대 하나를 두고 이번에 할 일을 정할 때 보는 것
#[derive(Clone, Copy, Debug)]
pub struct BarInput {
    /// 설정과 앞에 있는 창으로 보면 숨어야 하는지
    pub covered: bool,
    /// 포인터가 숨은 막대를 꺼내는 화면 가장자리에 있는지
    pub at_edge: bool,
    /// 포인터가 막대 위에 있는지
    pub inside: bool,
    /// 막대의 메뉴(팝업)가 열려 있는지
    pub menu_open: bool,
}

/// 막대의 보임을 정한다. 다음에 다시 봐야 할 때(숨기기를 기다리는 중)를 돌려준다
pub fn decide(reveal: &mut Reveal, input: BarInput, now: Instant) -> Option<Instant> {
    let newly_covered = input.covered && !reveal.covered;
    reveal.covered = input.covered;
    if !input.covered {
        reveal.set(true, now);
        reveal.left_at = None;
        return None;
    }
    if !reveal.shown() {
        // 가장자리에 닿거나, 숨은 막대의 메뉴를 단축키로 열면 꺼낸다
        if input.at_edge || input.menu_open {
            reveal.set(true, now);
        }
        return None;
    }
    // 나와 있다: 막대 위에 있거나 메뉴가 열려 있으면 그대로 둔다
    if input.inside || input.menu_open {
        reveal.left_at = None;
        return None;
    }
    // 창을 최대화한 것처럼 앞의 창 때문에 막 숨어야 하면 바로, 꺼낸 막대를 떠났으면 잠시 뒤 숨긴다
    if newly_covered {
        reveal.set(false, now);
        return None;
    }
    let left_at = *reveal.left_at.get_or_insert(now);
    if now.saturating_duration_since(left_at) >= HIDE_DELAY {
        reveal.set(false, now);
        None
    } else {
        Some(left_at + HIDE_DELAY)
    }
}

/// 출력의 가장자리
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    /// 레이어 표면이 독점 영역을 두는 가장자리. Smithay의 LayerMap::arrange와 같은 규칙이다
    pub fn of(anchor: Anchor) -> Option<Self> {
        let (top, bottom) = (
            anchor.contains(Anchor::TOP),
            anchor.contains(Anchor::BOTTOM),
        );
        let (left, right) = (
            anchor.contains(Anchor::LEFT),
            anchor.contains(Anchor::RIGHT),
        );
        match () {
            _ if anchor == Anchor::all() => None,
            _ if top && bottom => [(left, Self::Left), (right, Self::Right)]
                .into_iter()
                .find_map(|(on, edge)| on.then_some(edge)),
            _ if left && right => [(top, Self::Top), (bottom, Self::Bottom)]
                .into_iter()
                .find_map(|(on, edge)| on.then_some(edge)),
            _ if left => Some(Self::Left),
            _ if top => Some(Self::Top),
            _ if right => Some(Self::Right),
            _ if bottom => Some(Self::Bottom),
            _ => None,
        }
    }

    /// 그 가장자리 쪽 여백
    pub fn margin(self, margins: &Margins) -> i32 {
        match self {
            Self::Top => margins.top,
            Self::Bottom => margins.bottom,
            Self::Left => margins.left,
            Self::Right => margins.right,
        }
    }
}

/// 영역의 가장자리에서 `amount`만큼 뺀다
pub fn reserve(area: &mut Rectangle<i32, Logical>, edge: Edge, amount: i32) {
    let amount = amount.max(0);
    match edge {
        Edge::Top => {
            let amount = amount.min(area.size.h);
            area.loc.y += amount;
            area.size.h -= amount;
        }
        Edge::Bottom => area.size.h = (area.size.h - amount).max(0),
        Edge::Left => {
            let amount = amount.min(area.size.w);
            area.loc.x += amount;
            area.size.w -= amount;
        }
        Edge::Right => area.size.w = (area.size.w - amount).max(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    fn input(covered: bool, at_edge: bool, inside: bool) -> BarInput {
        BarInput {
            covered,
            at_edge,
            inside,
            menu_open: false,
        }
    }

    #[test]
    fn settings_values_parse() {
        assert_eq!(Autohide::parse("never"), Some(Autohide::Never));
        assert_eq!(Autohide::parse("maximized"), Some(Autohide::Maximized));
        assert_eq!(Autohide::parse("sometimes"), None);
    }

    #[test]
    fn bars_hide_for_the_front_window() {
        // (최대화, 전체 화면)
        assert!(!Autohide::Fullscreen.hides(true, false));
        assert!(Autohide::Fullscreen.hides(false, true));
        assert!(Autohide::Maximized.hides(true, false));
        assert!(Autohide::Always.hides(false, false));
        assert!(!Autohide::Never.hides(true, true));
    }

    #[test]
    fn windows_cover_bars_that_hide_for_them() {
        // 기본값(전체 화면에서만): 최대화한 창은 막대를 피하고 전체 화면 창은 덮는다
        assert!(!Autohide::Fullscreen.covered_by(Fill::Maximized));
        assert!(Autohide::Fullscreen.covered_by(Fill::Fullscreen));
        assert!(Autohide::Maximized.covered_by(Fill::Maximized));
        // 항상: 보통 창도 막대 자리까지 쓴다
        assert!(Autohide::Always.covered_by(Fill::Normal));
        assert!(!Autohide::Maximized.covered_by(Fill::Normal));
        // 안 함: 전체 화면 창도 메뉴 막대 아래에서 시작한다
        assert!(!Autohide::Never.covered_by(Fill::Fullscreen));
    }

    #[test]
    fn bars_slide_and_reverse() {
        let start = Instant::now();
        let mut reveal = Reveal::default();
        assert_eq!(reveal.value(start), 1.0);
        reveal.set(false, start);
        let half = reveal.value(start + SLIDE / 2);
        assert!(half > 0.0 && half < 0.5, "{half}");
        assert_eq!(reveal.value(start + SLIDE), 0.0);
        // 나가는 중에 다시 꺼내면 그 자리에서 돌아온다
        reveal.set(true, start + SLIDE / 2);
        assert!((reveal.value(start + SLIDE / 2) - half).abs() < 1e-9);
        assert_eq!(reveal.value(start + SLIDE * 2), 1.0);
    }

    #[test]
    fn edge_reveals_and_leaving_hides_later() {
        let start = Instant::now();
        let mut reveal = Reveal::default();
        // 창을 최대화하면 기다리지 않고 숨는다
        assert_eq!(decide(&mut reveal, input(true, false, false), start), None);
        assert!(!reveal.shown());
        let at = start + HIDE_DELAY;
        // 가장자리에 닿으면 나온다
        decide(&mut reveal, input(true, true, false), at);
        assert!(reveal.shown());
        // 막대 위에 있는 동안은 그대로
        let later = at + HIDE_DELAY * 3;
        assert_eq!(decide(&mut reveal, input(true, false, true), later), None);
        assert!(reveal.shown());
        // 떠나면 HIDE_DELAY 뒤에 숨는다
        assert_eq!(
            decide(&mut reveal, input(true, false, false), later),
            Some(later + HIDE_DELAY)
        );
        decide(&mut reveal, input(true, false, false), later + HIDE_DELAY);
        assert!(!reveal.shown());
        // 앞의 창이 최대화를 풀면 바로 나온다
        decide(
            &mut reveal,
            input(false, false, false),
            later + HIDE_DELAY * 2,
        );
        assert!(reveal.shown());
    }

    #[test]
    fn open_menus_keep_the_bar() {
        let start = Instant::now();
        let mut reveal = Reveal::default();
        let menu = BarInput {
            menu_open: true,
            ..input(true, false, false)
        };
        // 숨은 막대의 메뉴가 열리면 막대가 나온다
        decide(&mut reveal, input(true, false, false), start);
        assert!(!reveal.shown());
        decide(&mut reveal, menu, start);
        assert!(reveal.shown());
        // 메뉴가 열린 채로 창을 최대화해도, 메뉴가 열려 있는 동안은 숨지 않는다
        decide(&mut reveal, menu, start);
        decide(&mut reveal, menu, start + HIDE_DELAY * 2);
        assert!(reveal.shown());
        // 메뉴를 닫으면 잠시 뒤 숨는다
        let closed = start + HIDE_DELAY * 3;
        assert!(decide(&mut reveal, input(true, false, false), closed).is_some());
        decide(&mut reveal, input(true, false, false), closed + HIDE_DELAY);
        assert!(!reveal.shown());
    }

    #[test]
    fn bars_reserve_their_edge() {
        // 메뉴 막대(위, 왼쪽, 오른쪽에 붙음)와 Dock(아래에만 붙음)
        assert_eq!(
            Edge::of(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT),
            Some(Edge::Top)
        );
        assert_eq!(Edge::of(Anchor::BOTTOM), Some(Edge::Bottom));
        assert_eq!(
            Edge::of(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT),
            Some(Edge::Left)
        );
        assert_eq!(Edge::of(Anchor::all()), None);
        assert_eq!(Edge::of(Anchor::empty()), None);

        let mut area = rect(0, 0, 1280, 800);
        reserve(&mut area, Edge::Top, 28);
        reserve(&mut area, Edge::Bottom, 90);
        assert_eq!(area, rect(0, 28, 1280, 682));
        reserve(&mut area, Edge::Left, 2000);
        assert_eq!(area, rect(1280, 28, 0, 682));
    }
}
