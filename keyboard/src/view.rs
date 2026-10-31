// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드의 GTK 위젯 (iPadOS 키보드). 키는 46칸 격자(layout.rs)에 놓고, 누르기는 격자 하나가 받아 손가락마다
//! 따로 따라간다. 빨리 칠 때는 앞 손가락을 떼기 전에 다음 키를 누르기 때문이다(GTK의 제스처는 손가락 하나만 본다).
//!
//! - 글자 키는 뗄 때 보낸다. 누르는 동안 키 위에 글자 미리 보기가 뜨고, 다른 글자 키로 밀면 그 키를 친다(iPadOS).
//! - 지우기와 Shift는 누를 때 한다. 지우기는 누르고 있으면 0.5초 뒤부터 1초에 10번 되풀이한다.
//! - 다른 키(Return, 스페이스, 쪽 바꾸기, 지구본, 키보드 가리기)는 누른 키에서 뗄 때 한다. 밀어서 벗어나면 그만둔다.
//! - 키 사이의 틈과 들여쓴 자리를 눌러도 가장 가까운 키다. 키는 키보드 포커스를 받지 않는다(포커스를 가진 글 칸이
//!   포커스를 지킨다).
//! - 키마다 접근성 역할(단추)과 이름이 있다. 접근성 동작은 없다: 화면 키보드는 사람만 누른다

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use bunt_i18n::{Sources, Translations};
use gtk4::{self as gtk, gdk, glib, graphene, prelude::*};

use crate::{
    KeySink,
    board::{Board, Output},
    layout::{Action, COLUMNS, Face, Icon, Key, Script, Slot},
};

const TRANSLATIONS: &Sources = &[
    ("ko", include_str!("../../i18n/ko/bunt-keyboard.ftl")),
    ("en", include_str!("../../i18n/en/bunt-keyboard.ftl")),
];
/// 키보드 모양. 앱의 CSS(theme/tokens.css 포함)는 앱이 먼저 읽는다
const STYLE: &str = include_str!("../data/keyboard.css");

/// 판의 안쪽 여백과 키 둘레의 여백 (keyboard.css의 var(--space-1))
const PANEL_PADDING: i32 = 4;
const KEY_MARGIN: i32 = 4;
/// 지우기를 누르고 있으면 되풀이하기까지와 그 간격 (iPadOS)
const REPEAT_DELAY: Duration = Duration::from_millis(500);
const REPEAT_INTERVAL: Duration = Duration::from_millis(100);
/// 지우기 되풀이의 상한(20초). 손가락을 뗀 이벤트를 놓치면(창이 사라지는 등) 끝없이 지우지 않게 한다. 시험은 짧게 본다
const MAX_REPEATS: u32 = if cfg!(test) { 5 } else { 200 };
/// 미리 보기 풍선의 크기(키에 견준 배수)와 키 사이의 틈
const PREVIEW_WIDTH: f32 = 1.5;
const PREVIEW_HEIGHT: f32 = 1.2;
const PREVIEW_GAP: f32 = 4.0;
/// 줄 높이: 화면 높이의 이 비율을 이 범위 안에서 쓴다. 가장 작아도 키가 44px이다(HIG의 누를 영역)
const ROW_SHARE: f64 = 0.085;
const MIN_ROW: i32 = 52;
const MAX_ROW: i32 = 80;
/// 기능 키 그림의 크기
const ICON_SIZE: i32 = 22;
/// 미끄러져 나오고 들어가는 시간 (tokens.css의 --duration)
const SLIDE: u32 = 200;

/// 화면 높이(논리 픽셀)에 맞는 키 줄 하나의 높이
pub fn row_height(screen_height: i32) -> i32 {
    ((f64::from(screen_height) * ROW_SHARE).round() as i32).clamp(MIN_ROW, MAX_ROW)
}

/// 판의 높이: 네 줄과 위아래 여백
pub fn panel_height(row: i32) -> i32 {
    4 * row + 2 * PANEL_PADDING
}

/// 맨 윗줄 키의 미리 보기 풍선이 판 위로 나가는 높이
pub fn preview_room(row: i32) -> i32 {
    let key = (row - 2 * KEY_MARGIN) as f32;
    let bubble = (key * PREVIEW_HEIGHT).ceil() + PREVIEW_GAP;
    (bubble as i32 - PANEL_PADDING - KEY_MARGIN).max(0)
}

/// 키보드를 어디에 두는지
pub enum Room {
    /// 키보드만 있는 창(셸의 bunt-osk): 판 위에 미리 보기 풍선이 뜰 투명한 띠를 둔다. 띠는 셸이 창의 입력 영역에서 뺀다
    Window,
    /// 다른 창 안(잠금·로그인 화면): 풍선은 그 창 전체를 덮는 겹치기(overlay)에 띄운다
    Embedded(gtk::Overlay),
}

/// 격자에 놓인 키
struct Placed {
    /// 칸 전체(틈 포함). 누를 자리이고 접근성 단추이며, 그 안에 보이는 키가 있다
    slot: gtk::Box,
    key: Key,
    /// (줄, 시작 칸). 키를 다시 만들어도 같은 자리면 같은 키로 본다
    at: (usize, u8),
}

/// 누르는 중인 손가락 하나 (포인터는 sequence가 None)
struct Touch {
    sequence: Option<gdk::EventSequence>,
    began: (usize, u8),
    /// 지금 손가락 아래에서 칠 키. 처음 키에서 벗어났으면 None (글자 키는 다른 글자 키로 옮겨 간다)
    over: Option<(usize, u8)>,
    /// 지우기 되풀이
    repeat: Option<Rc<Cell<Option<glib::SourceId>>>>,
}

struct Inner {
    tr: Translations,
    sink: Rc<dyn KeySink>,
    board: RefCell<Board>,
    /// 키보드 전체 (Window면 띠와 판, Embedded면 판)
    root: gtk::Widget,
    /// 판 위의 투명한 띠 (Window)
    room: Option<gtk::Box>,
    revealer: gtk::Revealer,
    grid: gtk::Grid,
    /// 미리 보기 풍선을 띄우는 겹치기와 풍선
    host: gtk::Overlay,
    preview: gtk::Label,
    placed: RefCell<Vec<Placed>>,
    touches: RefCell<Vec<Touch>>,
    globe: Cell<bool>,
    row: Cell<i32>,
    on_next_source: RefCell<Option<Rc<dyn Fn()>>>,
    on_hide: RefCell<Option<Rc<dyn Fn()>>>,
    on_concealed: RefCell<Option<Rc<dyn Fn()>>>,
}

/// 화면 키보드 하나
#[derive(Clone)]
pub struct Keyboard(Rc<Inner>);

/// 키보드를 붙들지 않는 손잡이. 키보드가 붙든 처리기(지구본 등)가 다시 키보드를 부를 때 쓴다
#[derive(Clone)]
pub struct WeakKeyboard(std::rc::Weak<Inner>);

impl WeakKeyboard {
    pub fn upgrade(&self) -> Option<Keyboard> {
        self.0.upgrade().map(Keyboard)
    }
}

impl Keyboard {
    pub fn new(sink: Rc<dyn KeySink>, room: Room) -> Self {
        install_style();
        let tr = Translations::load(TRANSLATIONS);
        let grid = gtk::Grid::builder()
            .column_homogeneous(true)
            .row_homogeneous(true)
            .hexpand(true)
            .build();
        let panel = gtk::Box::builder()
            .css_classes(["osk"])
            .hexpand(true)
            .build();
        panel.append(&grid);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(SLIDE)
            .reveal_child(false)
            .valign(gtk::Align::End)
            .child(&panel)
            .build();
        let preview = gtk::Label::builder()
            .css_classes(["osk-preview"])
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .can_target(false)
            .visible(false)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        let (root, room, host) = match room {
            Room::Window => {
                // 창은 크기가 그대로이고(띠와 판), 판은 창의 아래 끝에서 미끄러져 나온다
                revealer.set_vexpand(true);
                let room = gtk::Box::builder().can_target(false).build();
                let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
                column.append(&room);
                column.append(&revealer);
                let host = gtk::Overlay::builder().child(&column).build();
                host.add_overlay(&preview);
                (host.clone().upcast::<gtk::Widget>(), Some(room), host)
            }
            Room::Embedded(host) => {
                host.add_overlay(&preview);
                (revealer.clone().upcast::<gtk::Widget>(), None, host)
            }
        };
        root.update_property(&[gtk::accessible::Property::Label(&tr.get("keyboard-title"))]);
        let keyboard = Self(Rc::new(Inner {
            tr,
            sink,
            board: RefCell::default(),
            root,
            room,
            revealer,
            grid,
            host,
            preview,
            placed: RefCell::default(),
            touches: RefCell::default(),
            globe: Cell::new(false),
            row: Cell::new(MIN_ROW),
            on_next_source: RefCell::default(),
            on_hide: RefCell::default(),
            on_concealed: RefCell::default(),
        }));
        keyboard.listen();
        keyboard.0.rebuild();
        keyboard.0.resize();
        keyboard
    }

    /// 창에 넣을 위젯
    pub fn widget(&self) -> &gtk::Widget {
        &self.0.root
    }

    pub fn downgrade(&self) -> WeakKeyboard {
        WeakKeyboard(Rc::downgrade(&self.0))
    }

    /// 화면 높이(논리 픽셀)에 맞춰 키 높이를 정한다
    pub fn set_screen_height(&self, height: i32) {
        let row = row_height(height);
        if self.0.row.replace(row) != row {
            self.0.resize();
            self.0.rebuild();
        }
    }

    /// (띠의 높이, 판의 높이). 셸은 판의 자리만 창의 입력 영역으로 둔다
    pub fn heights(&self) -> (i32, i32) {
        let row = self.0.row.get();
        let room = if self.0.room.is_some() {
            preview_room(row)
        } else {
            0
        };
        (room, panel_height(row))
    }

    /// 입력 소스가 바뀌었다 (한글이면 2벌식, 그 밖은 QWERTY)
    pub fn set_script(&self, script: Script) {
        if self.0.board.borrow_mut().set_script(script) {
            self.0.rebuild();
        }
    }

    /// 입력 소스를 바꾸는 지구본 키를 둘지 (입력 소스가 둘 이상일 때)
    pub fn set_globe(&self, globe: bool) {
        if self.0.globe.replace(globe) != globe {
            self.0.rebuild();
        }
    }

    /// 배경화면 위(잠금·로그인 화면)에서는 어두운 모양을 쓴다. 셸의 창은 창의 dark 클래스를 따른다
    pub fn set_dark(&self, dark: bool) {
        for widget in [&self.0.root, self.0.preview.upcast_ref()] {
            if dark {
                widget.add_css_class("dark");
            } else {
                widget.remove_css_class("dark");
            }
        }
    }

    /// 지구본 키를 눌렀다
    pub fn connect_next_source(&self, f: impl Fn() + 'static) {
        self.0.on_next_source.replace(Some(Rc::new(f)));
    }

    /// 키보드 가리기 키를 눌렀다
    pub fn connect_hide(&self, f: impl Fn() + 'static) {
        self.0.on_hide.replace(Some(Rc::new(f)));
    }

    /// 다 들어갔다 (conceal 뒤)
    pub fn connect_concealed(&self, f: impl Fn() + 'static) {
        self.0.on_concealed.replace(Some(Rc::new(f)));
    }

    /// 미끄러져 나온다. 숨어 있다 나오면 글자 쪽, Shift 끔으로 시작한다(iPadOS)
    pub fn reveal(&self) {
        if !self.0.revealer.reveals_child() && self.0.board.borrow_mut().reset() {
            self.0.rebuild();
        }
        self.0.revealer.set_reveal_child(true);
    }

    /// 미끄러져 들어간다. 누르던 키는 그만둔다
    pub fn conceal(&self) {
        self.0.cancel_all();
        self.0.revealer.set_reveal_child(false);
    }

    /// 나와 있거나 나오는 중이다
    pub fn is_revealed(&self) -> bool {
        self.0.revealer.reveals_child()
    }

    fn listen(&self) {
        let controller = gtk::EventControllerLegacy::new();
        let weak = Rc::downgrade(&self.0);
        controller.connect_event(move |_, event| match weak.upgrade() {
            Some(inner) => inner.on_event(event),
            None => glib::Propagation::Proceed,
        });
        self.0.grid.add_controller(controller);
        let weak = Rc::downgrade(&self.0);
        self.0
            .revealer
            .connect_child_revealed_notify(move |revealer| {
                if revealer.is_child_revealed() || revealer.reveals_child() {
                    return;
                }
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let concealed = inner.on_concealed.borrow().clone();
                if let Some(concealed) = concealed {
                    concealed();
                }
            });
    }
}

/// 누르기의 단계
enum Phase {
    Begin,
    Move,
    End,
    Cancel,
}

impl Inner {
    /// 격자가 받은 손가락·포인터 이벤트. 키에 쓴 이벤트는 멈춘다
    fn on_event(self: &Rc<Self>, event: &gdk::Event) -> glib::Propagation {
        use gdk::EventType;
        let kind = event.event_type();
        let (sequence, phase) = match kind {
            EventType::TouchBegin => (Some(event.event_sequence()), Phase::Begin),
            EventType::TouchUpdate => (Some(event.event_sequence()), Phase::Move),
            EventType::TouchEnd => (Some(event.event_sequence()), Phase::End),
            EventType::TouchCancel => (Some(event.event_sequence()), Phase::Cancel),
            EventType::ButtonPress | EventType::ButtonRelease => {
                let primary = event
                    .downcast_ref::<gdk::ButtonEvent>()
                    .is_some_and(|button| button.button() == gdk::BUTTON_PRIMARY);
                // 터치를 흉내 낸 포인터 이벤트는 터치로 이미 받았다
                if !primary || event.is_pointer_emulated() {
                    return glib::Propagation::Proceed;
                }
                let phase = if kind == EventType::ButtonPress {
                    Phase::Begin
                } else {
                    Phase::End
                };
                (None, phase)
            }
            EventType::MotionNotify => {
                let pressing = self.touches.borrow().iter().any(|t| t.sequence.is_none());
                if !pressing || event.is_pointer_emulated() {
                    return glib::Propagation::Proceed;
                }
                (None, Phase::Move)
            }
            // 다른 곳이 누르기를 가져갔다. 누르던 키(지우기 되풀이)를 모두 그만둔다
            EventType::GrabBroken => {
                self.cancel_all();
                return glib::Propagation::Proceed;
            }
            _ => return glib::Propagation::Proceed,
        };
        let point = grid_point(&self.grid, event);
        match phase {
            Phase::Begin => self.begin(sequence, point),
            Phase::Move => self.moved(&sequence, point),
            Phase::End => self.end(&sequence, point),
            Phase::Cancel => self.cancel(&sequence),
        }
        glib::Propagation::Stop
    }

    fn begin(
        self: &Rc<Self>,
        sequence: Option<gdk::EventSequence>,
        point: Option<graphene::Point>,
    ) {
        // 같은 손가락의 앞 누르기가 남았으면(끝을 놓쳤다) 버린다
        self.cancel(&sequence);
        let Some(at) = self.slot_at(point) else {
            return;
        };
        let Some(key) = self.key_at(at) else {
            return;
        };
        let repeat = match key.action {
            Action::Delete => {
                self.sink.send(Output::Delete);
                Some(self.repeat_delete())
            }
            _ => None,
        };
        self.touches.borrow_mut().push(Touch {
            sequence,
            began: at,
            over: Some(at),
            repeat,
        });
        if key.action == Action::Shift {
            self.apply(Action::Shift);
        }
        self.show_pressed();
    }

    fn moved(
        self: &Rc<Self>,
        sequence: &Option<gdk::EventSequence>,
        point: Option<graphene::Point>,
    ) {
        let under = self.slot_at(point);
        let mut touches = self.touches.borrow_mut();
        let Some(touch) = touches.iter_mut().find(|touch| touch.sequence == *sequence) else {
            return;
        };
        let typing = |at| {
            self.key_at(at)
                .is_some_and(|key| matches!(key.action, Action::Char(_)))
        };
        let over = match under {
            Some(at) if at == touch.began => Some(at),
            // 글자 키에서는 다른 글자 키로 밀 수 있다 (iPadOS)
            Some(at) if typing(touch.began) && typing(at) => Some(at),
            _ => None,
        };
        if over == touch.over {
            return;
        }
        touch.over = over;
        // 지우기에서 벗어나면 되풀이를 멈춘다. 돌아와도 다시 되풀이하지 않는다
        if over.is_none() {
            stop(touch.repeat.take());
        }
        drop(touches);
        self.show_pressed();
    }

    fn end(self: &Rc<Self>, sequence: &Option<gdk::EventSequence>, point: Option<graphene::Point>) {
        if point.is_some() {
            self.moved(sequence, point);
        }
        let Some(touch) = self.take_touch(sequence) else {
            return;
        };
        stop(touch.repeat);
        self.show_pressed();
        let Some(key) = touch.over.and_then(|at| self.key_at(at)) else {
            return;
        };
        match key.action {
            // 누를 때 했다
            Action::Delete | Action::Shift => {}
            Action::NextSource => call(&self.on_next_source),
            Action::Hide => call(&self.on_hide),
            action => self.apply(action),
        }
    }

    fn cancel(&self, sequence: &Option<gdk::EventSequence>) {
        if let Some(touch) = self.take_touch(sequence) {
            stop(touch.repeat);
            self.show_pressed();
        }
    }

    fn cancel_all(&self) {
        for touch in self.touches.take() {
            stop(touch.repeat);
        }
        self.show_pressed();
    }

    fn take_touch(&self, sequence: &Option<gdk::EventSequence>) -> Option<Touch> {
        let mut touches = self.touches.borrow_mut();
        let index = touches
            .iter()
            .position(|touch| touch.sequence == *sequence)?;
        Some(touches.remove(index))
    }

    /// 키의 일을 하고, 보낼 것이 있으면 보내고, 배열이 바뀌었으면 다시 그린다
    fn apply(self: &Rc<Self>, action: Action) {
        let outcome = self.board.borrow_mut().press(action, Instant::now());
        if let Some(output) = outcome.output {
            self.sink.send(output);
        }
        if outcome.redraw {
            self.rebuild();
        }
    }

    /// 지우기를 누르고 있는 동안 되풀이한다. 돌려준 칸의 원천을 지우면 멈춘다
    fn repeat_delete(self: &Rc<Self>) -> Rc<Cell<Option<glib::SourceId>>> {
        let source = Rc::new(Cell::new(None));
        let (weak, slot) = (Rc::downgrade(self), source.clone());
        let first = glib::timeout_add_local_once(REPEAT_DELAY, move || {
            let Some(inner) = weak.upgrade() else {
                slot.set(None);
                return;
            };
            inner.sink.send(Output::Delete);
            let weak = Rc::downgrade(&inner);
            let (repeated, finished) = (Cell::new(1), slot.clone());
            // 이 원천은 끝나므로 칸에는 되풀이 원천을 둔다
            slot.set(Some(glib::timeout_add_local(REPEAT_INTERVAL, move || {
                let Some(inner) = weak.upgrade().filter(|_| repeated.get() < MAX_REPEATS) else {
                    // 스스로 끝나는 원천이므로 칸을 비운다 (멈출 때 없는 원천을 지우지 않는다)
                    finished.set(None);
                    return glib::ControlFlow::Break;
                };
                repeated.set(repeated.get() + 1);
                inner.sink.send(Output::Delete);
                glib::ControlFlow::Continue
            })));
        });
        source.set(Some(first));
        source
    }

    /// 누르는 손가락 아래의 키를 눌린 모양으로 두고, 마지막에 누른 글자 키의 미리 보기를 띄운다
    fn show_pressed(&self) {
        let touches = self.touches.borrow();
        let placed = self.placed.borrow();
        for key in placed.iter() {
            if touches.iter().any(|touch| touch.over == Some(key.at)) {
                key.slot.add_css_class("pressed");
            } else {
                key.slot.remove_css_class("pressed");
            }
        }
        let typing = touches.iter().rev().find_map(|touch| {
            placed
                .iter()
                .find(|key| Some(key.at) == touch.over && matches!(key.key.action, Action::Char(_)))
        });
        match typing {
            Some(key) => self.show_preview(key),
            None => self.preview.set_visible(false),
        }
    }

    fn show_preview(&self, key: &Placed) {
        // 보이는 키는 칸에서 둘레의 여백(KEY_MARGIN)을 뺀 자리다. 키를 다시 만든 직후에도 맞도록 격자로 셈한다
        let slot = self.slot_rect(key.at, key.key.width);
        let margin = KEY_MARGIN as f32;
        let origin = self.grid.compute_point(
            &self.host,
            &graphene::Point::new(slot.x() + margin, slot.y() + margin),
        );
        let (Face::Text(label, _), Some(origin)) = (&key.key.face, origin) else {
            self.preview.set_visible(false);
            return;
        };
        let (face_width, face_height) = (slot.width() - 2.0 * margin, slot.height() - 2.0 * margin);
        let width = (face_width * PREVIEW_WIDTH).round();
        let height = (face_height * PREVIEW_HEIGHT).round();
        let room = (self.host.width() as f32 - width).max(0.0);
        let x = (origin.x() + (face_width - width) / 2.0).clamp(0.0, room);
        let y = (origin.y() - height - PREVIEW_GAP).max(0.0);
        self.preview.set_label(label);
        self.preview.set_size_request(width as i32, height as i32);
        self.preview.set_margin_start(x as i32);
        self.preview.set_margin_top(y as i32);
        self.preview.set_visible(true);
    }

    /// 격자 좌표에서 그 자리 키의 칸. 격자는 46칸 × 4줄의 같은 크기 칸이므로 셈으로 안다. 키를 다시 만든
    /// 직후(Shift를 누른 뒤 등)에는 새 위젯이 다음 프레임에야 배치되므로, 위젯의 자리를 읽으면 빨리 친 키가 다른 키로 간다
    fn slot_rect(&self, at: (usize, u8), width: u8) -> graphene::Rect {
        let column = self.grid.width() as f32 / f32::from(COLUMNS);
        let row = self.grid.height() as f32 / 4.0;
        graphene::Rect::new(
            f32::from(at.1) * column,
            at.0 as f32 * row,
            f32::from(width) * column,
            row,
        )
    }

    /// 그 점의 키 자리. 먼저 그 점의 줄을 정하고 그 줄에서 가장 가까운 키를 고른다: 둘째 줄의 들여쓴 자리는 a(ㅁ)다.
    /// 판의 여백이나 키 사이의 틈을 눌러도 키다
    fn slot_at(&self, point: Option<graphene::Point>) -> Option<(usize, u8)> {
        let point = point?;
        let row_height = self.grid.height() as f32 / 4.0;
        if row_height <= 0.0 {
            return None;
        }
        let row = (point.y() / row_height).floor().clamp(0.0, 3.0) as usize;
        let mut nearest: Option<((usize, u8), f32)> = None;
        for key in self.placed.borrow().iter().filter(|key| key.at.0 == row) {
            let bounds = self.slot_rect(key.at, key.key.width);
            let distance = (bounds.x() - point.x())
                .max(point.x() - (bounds.x() + bounds.width()))
                .max(0.0);
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((key.at, distance));
            }
        }
        nearest.map(|(at, _)| at)
    }

    fn key_at(&self, at: (usize, u8)) -> Option<Key> {
        self.placed
            .borrow()
            .iter()
            .find(|key| key.at == at)
            .map(|key| key.key.clone())
    }

    /// 띠와 판의 크기를 줄 높이에 맞춘다. 셸의 창은 크기가 그대로여야 미끄러지는 동안 창 크기가 바뀌지 않는다
    fn resize(&self) {
        let row = self.row.get();
        if let Some(room) = &self.room {
            room.set_size_request(-1, preview_room(row));
            self.root
                .set_size_request(-1, preview_room(row) + panel_height(row));
        }
    }

    /// 지금 쪽의 키를 모두 새로 만든다 (쪽, Shift, 입력 소스, 크기가 바뀌었을 때). 누르던 손가락은 자리로 다시 찾는다
    fn rebuild(&self) {
        while let Some(child) = self.grid.first_child() {
            self.grid.remove(&child);
        }
        let rows = self.board.borrow().rows(self.globe.get());
        let mut placed = Vec::new();
        for (row_index, row) in rows.iter().enumerate() {
            let mut column = 0u8;
            for slot in row {
                if let Slot::Key(key) = slot {
                    let slot_widget = self.key_widget(key);
                    self.grid.attach(
                        &slot_widget,
                        i32::from(column),
                        row_index as i32,
                        i32::from(key.width),
                        1,
                    );
                    placed.push(Placed {
                        slot: slot_widget,
                        key: key.clone(),
                        at: (row_index, column),
                    });
                }
                column += slot.width();
            }
        }
        self.placed.replace(placed);
        self.show_pressed();
    }

    /// 칸 하나: 누를 자리이고 접근성 단추인 칸 안에 보이는 키
    fn key_widget(&self, key: &Key) -> gtk::Box {
        let mut classes = vec!["osk-key"];
        if key.function {
            classes.push("function");
        }
        if key.on {
            classes.push("on");
        }
        let face = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .css_classes(classes)
            .build();
        let name = match (&key.face, key.name) {
            (_, Some(name)) => self.tr.get(name.message_id()),
            (Face::Text(label, _), None) => label.clone(),
            _ => String::new(),
        };
        match &key.face {
            Face::Text(label, hint) => {
                if let Some(hint) = hint {
                    face.append(
                        &gtk::Label::builder()
                            .label(hint.to_string())
                            .css_classes(["osk-hint"])
                            .accessible_role(gtk::AccessibleRole::Presentation)
                            .build(),
                    );
                }
                // 키의 이름은 칸(접근성 단추)에 단다. 글자는 접근성 트리에 따로 나오지 않는다
                face.append(
                    &gtk::Label::builder()
                        .label(label)
                        .vexpand(true)
                        .valign(gtk::Align::Center)
                        .accessible_role(gtk::AccessibleRole::Presentation)
                        .build(),
                );
            }
            Face::Icon(icon) => face.append(&icon_widget(*icon)),
            Face::Blank => {}
        }
        let slot = gtk::Box::builder()
            .accessible_role(gtk::AccessibleRole::Button)
            .css_classes(["osk-slot"])
            .hexpand(true)
            .focusable(false)
            .build();
        slot.set_size_request(-1, self.row.get());
        slot.append(&face);
        slot.update_property(&[gtk::accessible::Property::Label(&name)]);
        slot
    }
}

/// 기능 키의 그림. 아이콘 테마에 없으면(개발 중 이미지 밖) 글자로 보인다
fn icon_widget(icon: Icon) -> gtk::Widget {
    let found = gdk::Display::default()
        .is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(icon.icon_name()));
    // 키의 이름은 칸(접근성 단추)에 단다. 그림과 대체 글자는 접근성 트리에 따로 나오지 않는다
    if found {
        gtk::Image::builder()
            .icon_name(icon.icon_name())
            .pixel_size(ICON_SIZE)
            .vexpand(true)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build()
            .upcast()
    } else {
        gtk::Label::builder()
            .label(icon.fallback())
            .vexpand(true)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build()
            .upcast()
    }
}

/// 이벤트 자리를 격자 좌표로. 이벤트는 표면 좌표다
fn grid_point(grid: &gtk::Grid, event: &gdk::Event) -> Option<graphene::Point> {
    let (x, y) = event.position()?;
    let native = grid.native()?;
    let (dx, dy) = native.surface_transform();
    native.upcast_ref::<gtk::Widget>().compute_point(
        grid,
        &graphene::Point::new((x - dx) as f32, (y - dy) as f32),
    )
}

fn stop(repeat: Option<Rc<Cell<Option<glib::SourceId>>>>) {
    if let Some(source) = repeat.and_then(|repeat| repeat.take()) {
        source.remove();
    }
}

/// 처리기를 부른다. 처리기가 키보드를 다시 부를 수 있으므로 빌린 것을 놓고 부른다
fn call(handler: &RefCell<Option<Rc<dyn Fn()>>>) {
    let handler = handler.borrow().clone();
    if let Some(handler) = handler {
        handler();
    }
}

thread_local! {
    static STYLE_INSTALLED: Cell<bool> = const { Cell::new(false) };
}

/// 키보드 CSS를 화면에 한 번 단다. 앱의 CSS(토큰 포함) 다음에 단다
fn install_style() {
    if STYLE_INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_big_enough_to_touch_and_fit_the_screen() {
        // 1280×800(VM): 줄 68px, 판 280px (화면의 35%), 키 60px
        assert_eq!(row_height(800), 68);
        assert_eq!(panel_height(68), 280);
        // 대상 기기(2944×1840, 200%): 1472×920
        assert_eq!(row_height(920), 78);
        // 작은 화면에서도 키(줄 - 위아래 여백)가 44px 이상이고, 큰 화면(세로)에서도 너무 커지지 않는다
        assert_eq!(row_height(400), MIN_ROW);
        const { assert!(MIN_ROW - 2 * KEY_MARGIN >= 44) };
        assert_eq!(row_height(1472), MAX_ROW);
    }

    #[test]
    fn the_preview_room_holds_a_top_row_bubble() {
        for row in MIN_ROW..=MAX_ROW {
            let key = (row - 2 * KEY_MARGIN) as f32;
            let bubble_top = PANEL_PADDING as f32 + KEY_MARGIN as f32
                - PREVIEW_GAP
                - (key * PREVIEW_HEIGHT).round();
            // 띠의 위쪽 끝(-preview_room) 안에 풍선이 든다
            assert!(
                bubble_top >= -(preview_room(row) as f32),
                "row {row}: {bubble_top} < -{}",
                preview_room(row)
            );
        }
        assert_eq!(preview_room(68), 68);
    }

    /// 보낸 키를 적어 둔다
    struct Record(Rc<RefCell<Vec<Output>>>);

    impl KeySink for Record {
        fn send(&self, output: Output) {
            self.0.borrow_mut().push(output);
        }
    }

    /// 실제로 배치한 키 위에서 누르기를 따라간다. GTK가 화면을 열 수 있을 때만 돈다(`xvfb-run cargo test`).
    /// 화면이 없는 곳(CI)에서는 알리고 건너뛴다. GTK를 쓰는 시험은 이것 하나다(GTK는 한 스레드에서만 쓴다)
    #[test]
    fn presses_follow_the_finger_on_laid_out_keys() {
        if gtk::init().is_err() {
            eprintln!("no display; skipping the GTK keyboard test");
            return;
        }
        let sent = Rc::new(RefCell::new(Vec::new()));
        let keyboard = Keyboard::new(Rc::new(Record(sent.clone())), Room::Window);
        keyboard.set_screen_height(800);
        keyboard.set_globe(true);
        let hidden = Rc::new(Cell::new(0));
        let on_hide = hidden.clone();
        keyboard.connect_hide(move || on_hide.set(on_hide.get() + 1));
        let window = gtk::Window::new();
        window.set_default_size(1280, 360);
        window.set_child(Some(keyboard.widget()));
        window.present();
        keyboard.reveal();
        let inner = keyboard.0.clone();
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(10);
        while inner.grid.width() < 1000 || !inner.revealer.is_child_revealed() {
            assert!(Instant::now() < deadline, "the keyboard was never laid out");
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        // 키를 다시 만든 뒤에도 프레임을 돌리지 않고 바로 누른다 (빨리 칠 때와 같다)
        let center = |at: (usize, u8)| {
            let width = inner
                .placed
                .borrow()
                .iter()
                .find(|key| key.at == at)
                .expect("a key there")
                .key
                .width;
            let bounds = inner.slot_rect(at, width);
            Some(graphene::Point::new(
                bounds.x() + bounds.width() / 2.0,
                bounds.y() + bounds.height() / 2.0,
            ))
        };
        let tap = |at| {
            inner.begin(None, center(at));
            inner.end(&None, center(at));
        };
        let take = || std::mem::take(&mut *sent.borrow_mut());
        // 글자 쪽 (QWERTY): q는 (0줄, 0칸), w는 (0, 4), 삭제는 (0, 40), a는 (1, 2), Return은 (1, 38),
        // Shift는 (2, 0), 지구본은 (3, 0), .?123은 (3, 4), 스페이스는 (3, 9), 가리기는 (3, 42)
        tap((0, 0));
        assert_eq!(take(), [Output::Char('q')]);
        // 누르는 동안 미리 보기가 그 글자로 뜨고, 떼면 사라진다
        inner.begin(None, center((0, 4)));
        assert!(inner.preview.is_visible());
        assert_eq!(inner.preview.label(), "w");
        inner.end(&None, center((0, 4)));
        assert!(!inner.preview.is_visible());
        assert_eq!(take(), [Output::Char('w')]);
        // Shift 한 번은 다음 글자 하나만
        tap((2, 0));
        tap((0, 0));
        tap((0, 0));
        assert_eq!(take(), [Output::Char('Q'), Output::Char('q')]);
        // 글자 키에서 다른 글자 키로 밀면 뗀 키를 친다
        inner.begin(None, center((0, 0)));
        inner.moved(&None, center((0, 4)));
        inner.end(&None, center((0, 4)));
        assert_eq!(take(), [Output::Char('w')]);
        // 기능 키(Return)에서 벗어나 떼면 아무것도 하지 않는다
        inner.begin(None, center((1, 38)));
        inner.end(&None, center((0, 36)));
        assert_eq!(take(), []);
        // 둘째 줄의 들여쓴 자리를 누르면 가장 가까운 a다
        let indent = center((1, 2)).map(|point| graphene::Point::new(4.0, point.y()));
        inner.begin(None, indent);
        inner.end(&None, indent);
        assert_eq!(take(), [Output::Char('a')]);
        // 삭제는 누를 때 보낸다. 바로 떼면 되풀이하지 않는다
        inner.begin(None, center((0, 40)));
        assert_eq!(take(), [Output::Delete]);
        inner.end(&None, center((0, 40)));
        assert_eq!(take(), []);
        // 누르고 있으면 0.5초 뒤부터 되풀이하고, 뗀 이벤트를 놓쳐도 상한(시험은 5번)에서 멈춘다
        inner.begin(None, center((0, 40)));
        let until = Instant::now() + REPEAT_DELAY + REPEAT_INTERVAL * (MAX_REPEATS + 5);
        while Instant::now() < until {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(take().len(), 2 + MAX_REPEATS as usize - 1);
        inner.end(&None, center((0, 40)));
        assert_eq!(take(), []);
        // 숫자 쪽에서 스페이스를 치면 글자 쪽으로 돌아온다
        tap((3, 4));
        tap((0, 0));
        tap((3, 9));
        tap((0, 0));
        assert_eq!(
            take(),
            [Output::Char('1'), Output::Char(' '), Output::Char('q')]
        );
        // 한글 배열에서 같은 자리는 2벌식 자모의 라틴 글자다 (ㅂ → q, Shift ㅃ → Q)
        keyboard.set_script(Script::Hangul);
        tap((2, 0));
        tap((0, 0));
        assert_eq!(take(), [Output::Char('Q')]);
        // 키보드 가리기
        tap((3, 42));
        assert_eq!(hidden.get(), 1);
        // 숨기면 누르던 키를 그만둔다
        inner.begin(None, center((0, 0)));
        keyboard.conceal();
        inner.end(&None, center((0, 0)));
        assert_eq!(take(), []);
        window.destroy();
        typing_into_the_focused_field_of_the_window(&context);
    }

    /// 잠금 화면처럼 창 안에 넣은 키보드는 그 창의 포커스를 가진 칸에 바로 넣는다 (EditableKeys). 위 시험에서 이어
    /// 부른다(GTK는 한 스레드에서만 쓴다)
    fn typing_into_the_focused_field_of_the_window(context: &glib::MainContext) {
        let window = gtk::Window::new();
        window.set_default_size(1280, 800);
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let entry = gtk::PasswordEntry::new();
        entry.set_vexpand(true);
        column.append(&entry);
        let overlay = gtk::Overlay::builder().child(&column).build();
        window.set_child(Some(&overlay));
        let keyboard = Keyboard::new(
            Rc::new(crate::EditableKeys::new(&window)),
            Room::Embedded(overlay.clone()),
        );
        keyboard.set_screen_height(800);
        column.append(keyboard.widget());
        let activated = Rc::new(Cell::new(0));
        let on_activate = activated.clone();
        entry.connect_activate(move |_| on_activate.set(on_activate.get() + 1));
        window.present();
        entry.grab_focus();
        keyboard.reveal();
        let inner = keyboard.0.clone();
        let deadline = Instant::now() + Duration::from_secs(10);
        while inner.grid.width() < 1000 || !inner.revealer.is_child_revealed() {
            assert!(Instant::now() < deadline, "the keyboard was never laid out");
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
        let tap = |at: (usize, u8), width: u8| {
            let bounds = inner.slot_rect(at, width);
            let point = Some(graphene::Point::new(
                bounds.x() + bounds.width() / 2.0,
                bounds.y() + bounds.height() / 2.0,
            ));
            inner.begin(None, point);
            inner.end(&None, point);
        };
        // q, w, Shift+e, 삭제(누를 때 지운다), 스페이스
        tap((0, 0), 4);
        tap((0, 4), 4);
        tap((2, 0), 5);
        tap((0, 8), 4);
        assert_eq!(entry.text(), "qwE");
        tap((0, 40), 6);
        assert_eq!(entry.text(), "qw");
        tap((3, 4), 28);
        assert_eq!(entry.text(), "qw ");
        // Return은 칸의 기본 동작(잠금 화면: 암호 확인)
        tap((1, 38), 8);
        assert_eq!(activated.get(), 1);
        // 막아 둔 칸(암호를 확인하는 동안)에는 넣지 않는다
        entry.set_sensitive(false);
        tap((0, 0), 4);
        assert_eq!(entry.text(), "qw ");
        window.destroy();
    }
}
