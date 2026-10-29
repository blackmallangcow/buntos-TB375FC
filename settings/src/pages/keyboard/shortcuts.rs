// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 키보드 단축키 시트 (macOS 26의 키보드 > 키보드 단축키…). 왼쪽에 배지가 붙은 분류, 오른쪽에 고른
//! 분류의 단축키 목록(체크 상자, 이름, 오른쪽 끝 단축키 기호), 맨 아래 왼쪽에 기본값으로 복원(고른
//! 분류만), 오른쪽에 완료.
//!
//! 단축키는 컴포지터가 가진다 (org.buntos.Compositor1의 GetShortcuts, SetShortcut, ResetShortcuts와
//! ShortcutsChanged 신호). 줄을 두 번 누르거나 Enter를 누르면 단축키 칸이 기록 칸으로 바뀌고 다음에
//! 누른 조합을 받는다. Esc는 그만두고 ⌫는 단축키를 지운다. 기록하는 동안 시트 창이 시스템 단축키를
//! 막으므로(zwp_keyboard_shortcuts_inhibit) 컴포지터는 ⌘⇥ 같은 키를 가로채지 않고 그대로 보낸다.
//! 키는 조합 키 없이 읽은 1단계 keysym으로 적는다: ⇧3은 numbersign이 아니라 `<Shift><Super>3`이다

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

use super::{
    COMPOSITOR, COMPOSITOR_INTERFACE, COMPOSITOR_PATH, ICON,
    accelerator::{self, KeyNames, Modifiers, Recorded},
};
use crate::{
    dbus::{self, Debounce, Props, Remote},
    pages::Context,
    widgets::{self, BadgeSize, Tint, badge},
};

/// 시트 크기. 시스템 설정 창(900×640, 최소 너비 720) 안에 들어간다
const SHEET_WIDTH: i32 = 680;
const SHEET_HEIGHT: i32 = 480;
/// 왼쪽 분류 목록의 너비
const SIDEBAR_WIDTH: i32 = 180;
/// 단축키 칸의 최소 너비 (글자 수). 기호 길이가 달라도 오른쪽 열이 고르게 선다
const KEYS_WIDTH: i32 = 9;
const REFRESH_DELAY: Duration = Duration::from_millis(150);
/// GetShortcuts의 답 형식
const REPLY_TYPE: &str = "(aa{sv})";
/// 텍스트 편집 분류. 여기 단축키에는 ⇧를 쓰지 않는다 (⇧를 더하면 선택 영역을 넓힌다)
const TEXT_CATEGORY: &str = "text";
/// 입력 소스 분류. 여기 단축키는 조합 키만(⌃⇧)이나 ⇧스페이스도 된다 (Windows의 한/영 전환)
const INPUT_CATEGORY: &str = "input";
/// 입력 소스 전환. 키보드 페이지의 "입력 소스 바꾸기" 행이 이 단축키를 보인다
pub(super) const SWITCH_SOURCE: &str = "input.switch-source";

/// 분류: ID, 배지 기호, 배지 색. 사이드바는 이 순서이고, 모르는 분류(새 컴포지터)는 맨 뒤에 회색
/// 키보드 배지로 붙인다
const CATEGORIES: [(&str, &str, Tint); 6] = [
    ("launcher", "view-app-grid-symbolic", Tint::Graphite),
    ("windows", "focus-windows-symbolic", Tint::Blue),
    ("input", "preferences-desktop-locale-symbolic", Tint::Indigo),
    ("screenshots", "camera-photo-symbolic", Tint::Gray),
    (TEXT_CATEGORY, "insert-text-symbolic", Tint::Gray),
    (
        "app",
        "preferences-desktop-keyboard-shortcuts-symbolic",
        Tint::Blue,
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// 키 조합이 있는 단축키
    Key,
    /// 켜고 끄기만 하는 항목 (예: 그 밖의 ⌘ 단축키 보내기). 단축키 칸이 없다
    Switch,
}

/// 컴포지터가 아는 단축키 하나 (GetShortcuts의 사전 하나)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Shortcut {
    pub(super) id: String,
    category: String,
    kind: Kind,
    /// 지금 키. 비었으면 키가 없다
    pub(super) accelerator: String,
    default: String,
    pub(super) enabled: bool,
    default_enabled: bool,
}

impl Shortcut {
    fn from_props(props: &Props) -> Self {
        Self {
            id: props.string("id"),
            category: props.string("category"),
            kind: if props.string("kind") == "switch" {
                Kind::Switch
            } else {
                Kind::Key
            },
            accelerator: props.string("accelerator"),
            default: props.string("default"),
            enabled: props.bool("enabled"),
            default_enabled: props.get("default-enabled").unwrap_or(true),
        }
    }

    fn is_default(&self) -> bool {
        self.accelerator == self.default && self.enabled == self.default_enabled
    }
}

/// 컴포지터의 단축키 (보일 순서). 단축키 API가 없는 컴포지터면 오류다
pub(super) async fn fetch(compositor: &Remote) -> Result<Vec<Shortcut>, glib::Error> {
    let reply = compositor
        .call(COMPOSITOR_INTERFACE, "GetShortcuts", None)
        .await?;
    if reply.type_().as_str() != REPLY_TYPE {
        return Err(glib::Error::new(
            gio::IOErrorEnum::InvalidData,
            "unexpected GetShortcuts reply",
        ));
    }
    Ok(reply
        .child_value(0)
        .iter()
        .map(|dict| Shortcut::from_props(&Props::new(dict)))
        .filter(|shortcut| !shortcut.id.is_empty())
        .collect())
}

pub(super) fn key_names(tr: &Translations) -> KeyNames {
    KeyNames {
        space: tr.get("shortcuts-key-space"),
        hangul: tr.get("shortcuts-key-hangul"),
    }
}

/// 메시지. 없으면 대신할 글을 쓴다 (새 컴포지터가 보낸 모르는 단축키는 ID를 보인다)
fn message_or(tr: &Translations, id: &str, fallback: &str) -> String {
    let text = tr.get(id);
    if text == id {
        fallback.to_owned()
    } else {
        text
    }
}

/// 단축키 이름. 메시지 ID는 단축키 ID의 .을 -로 바꾼 것이다 (app.copy → shortcut-app-copy)
fn shortcut_label(tr: &Translations, id: &str) -> String {
    message_or(tr, &format!("shortcut-{}", id.replace('.', "-")), id)
}

fn category_label(tr: &Translations, category: &str) -> String {
    message_or(tr, &format!("shortcut-category-{category}"), category)
}

/// 목록 아래 설명. 메시지가 있는 분류만
fn category_note(tr: &Translations, category: &str) -> Option<String> {
    let id = format!("shortcuts-note-{category}");
    let text = tr.get(&id);
    (text != id).then_some(text)
}

fn category_badge(category: &str) -> gtk::Box {
    let (icon, tint) = CATEGORIES
        .iter()
        .find(|(id, ..)| *id == category)
        .map_or((ICON, Tint::Gray), |(_, icon, tint)| (*icon, *tint));
    badge(icon, tint, BadgeSize::Small)
}

/// 사이드바의 분류: 아는 분류는 정한 순서로, 모르는 분류는 그 뒤에 처음 나온 순서로
fn category_order(shortcuts: &[Shortcut]) -> Vec<String> {
    let mut order: Vec<String> = CATEGORIES
        .iter()
        .map(|(id, ..)| *id)
        .filter(|id| shortcuts.iter().any(|shortcut| shortcut.category == *id))
        .map(str::to_owned)
        .collect();
    for shortcut in shortcuts {
        if !order.contains(&shortcut.category) {
            order.push(shortcut.category.clone());
        }
    }
    order
}

/// SetShortcut 오류("… already used by <ID>")에서 그 키를 쓰는 단축키의 ID
fn conflict(message: &str) -> Option<&str> {
    let (_, rest) = message.split_once("already used by ")?;
    let rest = rest.trim_start_matches(['\'', '"', '‘', '“', '`']);
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '-' | '_')))
        .unwrap_or(rest.len());
    let id = rest[..end].trim_end_matches('.');
    (!id.is_empty()).then_some(id)
}

/// 이벤트의 조합 키 (⌘는 Super). Meta와 Hyper는 보지 않는다: Alt를 누르면 Meta도 켜지는 배치가 있다
fn modifiers(state: gdk::ModifierType) -> Modifiers {
    Modifiers {
        control: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        command: state.contains(gdk::ModifierType::SUPER_MASK),
    }
}

/// 조합 키 자체. 누르는 이벤트의 상태에는 아직 그 키가 없다
fn modifier_key(key: gdk::Key) -> Modifiers {
    let mut modifiers = Modifiers::default();
    match key {
        gdk::Key::Control_L | gdk::Key::Control_R => modifiers.control = true,
        // ⇧와 함께 누른 Alt는 Meta_L로 온다
        gdk::Key::Alt_L | gdk::Key::Alt_R | gdk::Key::Meta_L | gdk::Key::Meta_R => {
            modifiers.alt = true
        }
        gdk::Key::Shift_L | gdk::Key::Shift_R => modifiers.shift = true,
        gdk::Key::Super_L | gdk::Key::Super_R => modifiers.command = true,
        _ => {}
    }
    modifiers
}

/// 누른 하드웨어 키를 조합 키 없이 읽는다 (1단계 keysym, 글자는 소문자). ⇧3도 3이다
fn base_key(display: &gdk::Display, code: u32, layout: u32) -> Option<gdk::Key> {
    display
        .translate_key(
            code,
            gdk::ModifierType::empty(),
            i32::try_from(layout).unwrap_or(0),
        )
        .map(|(key, ..)| key.to_lower())
}

fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

/// 시트를 띄우고 컴포지터에서 단축키를 읽는다
pub(super) fn present(context: &Rc<Context>) {
    let tr = &context.tr;
    let window = sheet_window(context.window().as_ref(), &tr.get("shortcuts-title"));

    // 왼쪽: 분류와 조합 키 안내
    let sidebar = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Browse)
        .vexpand(true)
        .css_classes(["shortcut-categories"])
        .build();
    sidebar.update_property(&[gtk::accessible::Property::Label(&tr.get("shortcuts-title"))]);
    let side = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .width_request(SIDEBAR_WIDTH)
        .build();
    side.append(&sidebar);
    side.append(&legend(tr));

    // 오른쪽: 묶음 상자 안의 단축키 목록, 실패 안내, 분류 설명
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::Single)
        .activate_on_single_click(false)
        .css_classes(["shortcut-list"])
        .build();
    let frame = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .css_classes(["file-types-frame"])
        .build();
    let message = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .visible(false)
        .css_classes(["shortcut-message"])
        .build();
    let note = widgets::description_label("");
    note.set_visible(false);
    let detail = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .hexpand(true)
        .build();
    detail.append(&frame);
    detail.append(&message);
    detail.append(&note);
    let split = gtk::Box::builder().spacing(16).build();
    split.append(&side);
    split.append(&detail);

    // 컴포지터가 단축키 API를 주지 않으면 빈 목록 대신 까닭을 보인다
    let stack = gtk::Stack::builder().vexpand(true).build();
    stack.add_named(&split, Some("shortcuts"));
    stack.add_named(
        &widgets::placeholder(
            ICON,
            Tint::Gray,
            &tr.get("shortcuts-unavailable-title"),
            &tr.get("shortcuts-unavailable-description"),
        ),
        Some("unavailable"),
    );

    // 아래: 왼쪽에 기본값으로 복원, 오른쪽에 완료(기본 단추)
    let restore = gtk::Button::builder()
        .label(tr.get("shortcuts-restore"))
        .halign(gtk::Align::Start)
        .hexpand(true)
        .sensitive(false)
        .build();
    let done = gtk::Button::builder()
        .label(tr.get("done"))
        .css_classes(["suggested-action"])
        .build();
    let buttons = gtk::Box::builder().spacing(8).build();
    buttons.append(&restore);
    buttons.append(&done);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .css_classes(["sheet-body"])
        .build();
    body.append(&stack);
    body.append(&buttons);
    window.set_child(Some(&body));
    window.set_default_widget(Some(&done));

    let sheet = Rc::new(ShortcutsSheet {
        context: context.clone(),
        window: window.clone(),
        compositor: RefCell::new(None),
        shortcuts: RefCell::default(),
        categories: RefCell::default(),
        category: RefCell::default(),
        stack,
        sidebar,
        list,
        frame,
        rows: RefCell::default(),
        message,
        note,
        restore,
        recording: RefCell::new(None),
        held: Cell::default(),
        peak: Cell::default(),
        combined: Cell::new(false),
        swallow: Cell::new(None),
        stale: Cell::new(false),
        updating: Cell::new(false),
        debounce: Debounce::default(),
        subscription: RefCell::new(None),
    });
    sheet.connect(&done);
    widgets::keep_alive(&window, sheet.clone());
    window.present();
    glib::spawn_future_local(sheet.start());
}

/// 시트 창. widgets::Sheet와 같은 모양과 동작(부모 창 위의 모달, 제목 표시줄 없음, 어둡게)이고 크기만
/// 크다. macOS의 키보드 단축키 시트처럼 머리글 없이 목록과 아래 단추 줄만 둔다. 제목은 화면 읽기
/// 프로그램과 창 목록에만 쓰인다
fn sheet_window(parent: Option<&gtk::Window>, title: &str) -> gtk::Window {
    let window = gtk::Window::builder()
        .modal(true)
        .resizable(false)
        .default_width(SHEET_WIDTH)
        .default_height(SHEET_HEIGHT)
        .title(title)
        .css_classes(["sheet"])
        .build();
    let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    titlebar.set_visible(false);
    window.set_titlebar(Some(&titlebar));
    if let Some(parent) = parent {
        window.set_transient_for(Some(parent));
        window.set_application(parent.application().as_ref());
        widgets::follow_dark(&window, parent);
    }
    window
}

/// 조합 키 기호가 PC 키보드의 어느 키인지 (⌘는 Super, ⌥는 Alt)
fn legend(tr: &Translations) -> gtk::Grid {
    let grid = gtk::Grid::builder()
        .row_spacing(2)
        .column_spacing(6)
        .css_classes(["shortcut-legend"])
        .build();
    for (row, (symbol, id)) in [
        ("⌃", "shortcuts-legend-control"),
        ("⌥", "shortcuts-legend-option"),
        ("⇧", "shortcuts-legend-shift"),
        ("⌘", "shortcuts-legend-command"),
    ]
    .into_iter()
    .enumerate()
    {
        let row = row as i32;
        grid.attach(&gtk::Label::new(Some(symbol)), 0, row, 1, 1);
        grid.attach(
            &gtk::Label::builder().label(tr.get(id)).xalign(0.0).build(),
            1,
            row,
            1,
            1,
        );
    }
    grid
}

/// 단축키 줄 하나
struct Row {
    id: String,
    row: gtk::ListBoxRow,
    check: gtk::CheckButton,
    /// 단축키 기호. 켜고 끄기만 하는 항목에는 없다
    keys: Option<gtk::Label>,
}

struct ShortcutsSheet {
    context: Rc<Context>,
    window: gtk::Window,
    compositor: RefCell<Option<Remote>>,
    /// 컴포지터가 마지막으로 알려 준 단축키
    shortcuts: RefCell<Vec<Shortcut>>,
    /// 사이드바의 분류 (줄 순서)
    categories: RefCell<Vec<String>>,
    /// 오른쪽에 보이는 분류
    category: RefCell<String>,
    stack: gtk::Stack,
    sidebar: gtk::ListBox,
    list: gtk::ListBox,
    frame: gtk::ScrolledWindow,
    rows: RefCell<Vec<Row>>,
    message: gtk::Label,
    note: gtk::Label,
    restore: gtk::Button,
    /// 기록 중인 단축키 ID
    recording: RefCell<Option<String>>,
    /// 기록하는 동안 누르고 있는 조합 키. 기록 칸에 보인다
    held: Cell<Modifiers>,
    /// 조합 키를 누르기 시작해 모두 뗄 때까지 함께 누른 조합 키 (조합 키만인 단축키)
    peak: Cell<Modifiers>,
    /// 그사이 조합 키가 아닌 키를 눌렀다
    combined: Cell<bool>,
    /// 기록을 시작하거나 끝낸 키(하드웨어 키 코드). 뗄 때까지 되풀이 입력을 버린다. 길게 누른 Esc가
    /// 시트를 닫거나 기록한 ⌃Q가 되풀이되어 앱을 끝내지 않게 한다
    swallow: Cell<Option<u32>>,
    /// 기록하는 동안 단축키가 바뀌었다는 신호가 왔다. 기록이 끝나면 다시 읽는다
    stale: Cell<bool>,
    /// 줄을 모델에 맞추는 동안에는 체크 상자 신호를 무시한다
    updating: Cell<bool>,
    debounce: Debounce,
    subscription: RefCell<Option<gio::SignalSubscription>>,
}

impl ShortcutsSheet {
    fn connect(self: &Rc<Self>, done: &gtk::Button) {
        let window = self.window.downgrade();
        done.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        });
        // Esc로 닫는다. 기록하는 동안의 Esc는 아래 키 처리기가 먼저 받아 기록만 그만둔다
        let escape = gtk::ShortcutController::new();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::CallbackAction::new(|widget, _| {
                if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                    window.close();
                }
                glib::Propagation::Stop
            })),
        ));
        self.window.add_controller(escape);

        // 기록: 창의 키를 가장 먼저(capture) 받는다. 나중에 붙인 컨트롤러가 먼저 돌므로 앱
        // 단축키(Ctrl+Q)보다도 앞선다
        let keys = gtk::EventControllerKey::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |controller, key, code, state| {
            weak.upgrade().map_or(glib::Propagation::Proceed, |sheet| {
                sheet.key_pressed(controller, key, code, state)
            })
        });
        let weak = Rc::downgrade(self);
        keys.connect_key_released(move |_, key, code, state| {
            if let Some(sheet) = weak.upgrade() {
                sheet.key_released(key, code, state);
            }
        });
        self.window.add_controller(keys);

        // 다른 곳을 누르거나(포커스가 옮겨 감) 창이 뒤로 가거나 닫히면 기록을 그만둔다
        let weak = Rc::downgrade(self);
        self.window.connect_focus_widget_notify(move |window| {
            let Some(sheet) = weak.upgrade() else {
                return;
            };
            let focus = GtkWindowExt::focus(window);
            if sheet.recording.borrow().is_some() && !sheet.focus_on_recording(focus.as_ref()) {
                sheet.stop_recording();
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_is_active_notify(move |window| {
            if let Some(sheet) = weak.upgrade().filter(|_| !window.is_active()) {
                // 창이 뒤로 가면 키를 뗀 것을 받지 못할 수 있다
                sheet.swallow.set(None);
                sheet.stop_recording();
            }
        });
        let weak = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(sheet) = weak.upgrade() {
                sheet.stop_recording();
            }
            glib::Propagation::Proceed
        });

        let weak = Rc::downgrade(self);
        self.sidebar.connect_row_selected(move |_, row| {
            let (Some(sheet), Some(row)) = (weak.upgrade(), row) else {
                return;
            };
            let category = usize::try_from(row.index())
                .ok()
                .and_then(|index| sheet.categories.borrow().get(index).cloned());
            if let Some(category) = category {
                sheet.show_category(&category);
            }
        });

        // 목록: Space는 체크 상자, Enter와 두 번 누르기(row-activated)는 기록
        let list_keys = gtk::EventControllerKey::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = Rc::downgrade(self);
        list_keys.connect_key_pressed(move |_, key, code, state| {
            let Some(sheet) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if modifiers(state) != Modifiers::default() {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::space | gdk::Key::KP_Space => sheet.toggle_focused(),
                gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                    let propagation = sheet.activate_focused();
                    // 기록을 시작한 Enter가 되풀이되어 기록 칸에 들어가지 않게 한다
                    if sheet.recording.borrow().is_some() {
                        sheet.swallow.set(Some(code));
                    }
                    propagation
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.list.add_controller(list_keys);
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            let Some(sheet) = weak.upgrade() else {
                return;
            };
            let id = usize::try_from(row.index())
                .ok()
                .and_then(|index| sheet.rows.borrow().get(index).map(|row| row.id.clone()));
            if let Some(id) = id {
                sheet.activate(&id);
            }
        });

        let weak = Rc::downgrade(self);
        self.restore.connect_clicked(move |_| {
            if let Some(sheet) = weak.upgrade() {
                sheet.restore_defaults();
            }
        });
    }

    async fn start(self: Rc<Self>) {
        let Ok(bus) = dbus::session().await else {
            self.show_unavailable();
            return;
        };
        let weak = Rc::downgrade(&self);
        let subscription = dbus::subscribe(
            &bus,
            Some(COMPOSITOR),
            Some(COMPOSITOR_INTERFACE),
            Some("ShortcutsChanged"),
            Some(COMPOSITOR_PATH),
            move |_| {
                if let Some(sheet) = weak.upgrade() {
                    sheet.schedule_refresh();
                }
            },
        );
        self.subscription.replace(Some(subscription));
        self.compositor
            .replace(Some(Remote::new(&bus, COMPOSITOR, COMPOSITOR_PATH)));
        self.reload().await;
    }

    /// ShortcutsChanged: 잠시 뒤 다시 읽는다. 기록하는 동안에는 기록이 끝난 뒤로 미룬다
    fn schedule_refresh(self: &Rc<Self>) {
        if self.recording.borrow().is_some() {
            self.stale.set(true);
            return;
        }
        let weak = Rc::downgrade(self);
        self.debounce.run(REFRESH_DELAY, move || {
            if let Some(sheet) = weak.upgrade() {
                glib::spawn_future_local(async move { sheet.reload().await });
            }
        });
    }

    /// 컴포지터에서 다시 읽는다. 분류와 줄이 그대로면 줄만 고치므로 포커스와 스크롤이 남는다
    async fn reload(self: &Rc<Self>) {
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        let shortcuts = match fetch(&compositor).await {
            Ok(shortcuts) if !shortcuts.is_empty() => shortcuts,
            _ => {
                self.show_unavailable();
                return;
            }
        };
        let categories = category_order(&shortcuts);
        let category = self.category.borrow().clone();
        let same_categories = *self.categories.borrow() == categories;
        let same_rows = self
            .rows
            .borrow()
            .iter()
            .map(|row| row.id.as_str())
            .eq(shortcuts
                .iter()
                .filter(|shortcut| shortcut.category == category)
                .map(|shortcut| shortcut.id.as_str()));
        self.shortcuts.replace(shortcuts);
        self.stack.set_visible_child_name("shortcuts");
        if !same_categories {
            self.stop_recording();
            self.categories.replace(categories);
            self.render_sidebar();
        } else if !same_rows {
            self.show_category(&category);
        } else {
            self.update_rows();
        }
    }

    fn show_unavailable(self: &Rc<Self>) {
        self.stop_recording();
        self.stack.set_visible_child_name("unavailable");
        self.restore.set_sensitive(false);
    }

    /// 분류 목록을 다시 만들고 보던 분류(없으면 첫 분류)를 고른다. 고르면 오른쪽 목록이 채워진다
    fn render_sidebar(self: &Rc<Self>) {
        let tr = &self.context.tr;
        let categories = self.categories.borrow().clone();
        let current = self.category.borrow().clone();
        widgets::clear(&self.sidebar);
        for category in &categories {
            let label = category_label(tr, category);
            let content = gtk::Box::builder().spacing(8).build();
            content.append(&category_badge(category));
            content.append(
                &gtk::Label::builder()
                    .label(&label)
                    .xalign(0.0)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build(),
            );
            let row = gtk::ListBoxRow::builder().child(&content).build();
            row.update_property(&[gtk::accessible::Property::Label(&label)]);
            self.sidebar.append(&row);
        }
        let index = categories
            .iter()
            .position(|category| *category == current)
            .unwrap_or(0);
        let row = i32::try_from(index)
            .ok()
            .and_then(|index| self.sidebar.row_at_index(index));
        self.sidebar.select_row(row.as_ref());
        // 처음 열 때는 분류에 포커스를 둔다 (Tab으로 목록, 단추 순서로 간다)
        if current.is_empty()
            && let Some(row) = row
        {
            row.grab_focus();
        }
    }

    /// 오른쪽 목록을 분류의 단축키로 채운다
    fn show_category(self: &Rc<Self>, category: &str) {
        self.stop_recording();
        self.hide_message();
        self.category.replace(category.to_owned());
        let tr = &self.context.tr;
        let shortcuts: Vec<Shortcut> = self
            .shortcuts
            .borrow()
            .iter()
            .filter(|shortcut| shortcut.category == category)
            .cloned()
            .collect();
        widgets::clear(&self.list);
        let rows: Vec<Row> = shortcuts
            .iter()
            .map(|shortcut| self.shortcut_row(shortcut))
            .collect();
        for row in &rows {
            self.list.append(&row.row);
        }
        self.rows.replace(rows);
        self.frame.vadjustment().set_value(0.0);
        self.list
            .update_property(&[gtk::accessible::Property::Label(&category_label(
                tr, category,
            ))]);
        match category_note(tr, category) {
            Some(note) => {
                self.note.set_text(&note);
                self.note.set_visible(true);
            }
            None => self.note.set_visible(false),
        }
        self.update_rows();
    }

    /// 줄: 체크 상자(켬), 이름, 오른쪽 끝 단축키 칸. 체크 상자는 포커스를 받지 않고 줄이 받는다
    /// (Space가 체크 상자를 바꾼다). 이름은 체크 상자와 줄의 접근성 이름이다
    fn shortcut_row(self: &Rc<Self>, shortcut: &Shortcut) -> Row {
        let label = shortcut_label(&self.context.tr, &shortcut.id);
        let check = gtk::CheckButton::builder()
            .focusable(false)
            .valign(gtk::Align::Center)
            .build();
        check.update_property(&[gtk::accessible::Property::Label(&label)]);
        let content = gtk::Box::builder().spacing(8).build();
        content.append(&check);
        content.append(
            &gtk::Label::builder()
                .label(&label)
                .xalign(0.0)
                .hexpand(true)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .css_classes(["row-title"])
                .build(),
        );
        let keys = (shortcut.kind == Kind::Key).then(|| {
            let keys = gtk::Label::builder()
                .xalign(1.0)
                .width_chars(KEYS_WIDTH)
                .valign(gtk::Align::Center)
                .css_classes(["shortcut-keys"])
                .build();
            content.append(&keys);
            keys
        });
        let row = gtk::ListBoxRow::builder()
            .child(&content)
            .activatable(true)
            .build();
        row.update_property(&[gtk::accessible::Property::Label(&label)]);
        let weak = Rc::downgrade(self);
        let id = shortcut.id.clone();
        check.connect_toggled(move |check| {
            if let Some(sheet) = weak.upgrade() {
                sheet.toggled(&id, check.is_active());
            }
        });
        Row {
            id: shortcut.id.clone(),
            row,
            check,
            keys,
        }
    }

    /// 줄을 모델에 맞춘다. 기록 중인 칸은 그대로 둔다
    fn update_rows(&self) {
        let tr = &self.context.tr;
        let names = key_names(tr);
        let recording = self.recording.borrow().clone();
        let shortcuts = self.shortcuts.borrow();
        self.updating.set(true);
        for row in self.rows.borrow().iter() {
            let Some(shortcut) = shortcuts.iter().find(|shortcut| shortcut.id == row.id) else {
                continue;
            };
            row.check.set_active(shortcut.enabled);
            let Some(keys) = &row.keys else {
                continue;
            };
            let symbols = accelerator::display(&shortcut.accelerator, &names);
            let text = if symbols.is_empty() {
                tr.get("shortcuts-none")
            } else {
                symbols.clone()
            };
            for widget in [row.row.upcast_ref::<gtk::Widget>(), row.check.upcast_ref()] {
                widget.update_property(&[gtk::accessible::Property::Description(&text)]);
            }
            if recording.as_deref() == Some(row.id.as_str()) {
                continue;
            }
            keys.set_text(&text);
            set_class(keys, "none", symbols.is_empty());
            set_class(keys, "off", !shortcut.enabled);
        }
        self.updating.set(false);
        let category = self.category.borrow();
        self.restore.set_sensitive(
            shortcuts
                .iter()
                .any(|shortcut| shortcut.category == *category && !shortcut.is_default()),
        );
    }

    fn find(&self, id: &str) -> Option<Shortcut> {
        self.shortcuts
            .borrow()
            .iter()
            .find(|shortcut| shortcut.id == id)
            .cloned()
    }

    fn keys_of(&self, id: &str) -> Option<gtk::Label> {
        self.rows
            .borrow()
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| row.keys.clone())
    }

    /// 포커스가 있는 줄의 번호
    fn focused_row(&self) -> Option<usize> {
        let row = self.list.focus_child().and_downcast::<gtk::ListBoxRow>()?;
        usize::try_from(row.index()).ok()
    }

    /// 포커스가 기록 중인 줄(이나 그 안)에 있는지
    fn focus_on_recording(&self, focus: Option<&gtk::Widget>) -> bool {
        let Some(id) = self.recording.borrow().clone() else {
            return false;
        };
        let rows = self.rows.borrow();
        let Some(row) = rows.iter().find(|row| row.id == id) else {
            return false;
        };
        focus.is_some_and(|focus| {
            focus == row.row.upcast_ref::<gtk::Widget>() || focus.is_ancestor(&row.row)
        })
    }

    /// Space: 포커스가 있는 줄의 체크 상자를 바꾼다
    fn toggle_focused(&self) -> glib::Propagation {
        let check = self
            .focused_row()
            .and_then(|index| self.rows.borrow().get(index).map(|row| row.check.clone()));
        match check {
            Some(check) => {
                check.set_active(!check.is_active());
                glib::Propagation::Stop
            }
            None => glib::Propagation::Proceed,
        }
    }

    /// Enter: 포커스가 있는 줄을 연다 (activate)
    fn activate_focused(self: &Rc<Self>) -> glib::Propagation {
        let id = self
            .focused_row()
            .and_then(|index| self.rows.borrow().get(index).map(|row| row.id.clone()));
        match id {
            Some(id) => {
                self.activate(&id);
                glib::Propagation::Stop
            }
            None => glib::Propagation::Proceed,
        }
    }

    /// 줄을 두 번 누르거나 Enter: 단축키를 기록한다. 켜고 끄기만 하는 항목은 체크 상자를 바꾼다
    fn activate(self: &Rc<Self>, id: &str) {
        let Some(shortcut) = self.find(id) else {
            return;
        };
        match shortcut.kind {
            Kind::Key => self.start_recording(id),
            Kind::Switch => {
                let check = self
                    .rows
                    .borrow()
                    .iter()
                    .find(|row| row.id == id)
                    .map(|row| row.check.clone());
                if let Some(check) = check {
                    check.set_active(!check.is_active());
                }
            }
        }
    }

    /// 체크 상자: 단축키를 켜고 끈다. 키는 그대로 둔다
    fn toggled(self: &Rc<Self>, id: &str, enabled: bool) {
        if self.updating.get() {
            return;
        }
        // 컴포지터가 받지 않으면 다시 읽을 때 되돌아간다
        let accelerator = {
            let mut shortcuts = self.shortcuts.borrow_mut();
            let Some(shortcut) = shortcuts.iter_mut().find(|shortcut| shortcut.id == id) else {
                return;
            };
            shortcut.enabled = enabled;
            match shortcut.kind {
                Kind::Key => shortcut.accelerator.clone(),
                Kind::Switch => String::new(),
            }
        };
        self.stop_recording();
        self.set(id, accelerator, enabled);
    }

    /// 줄의 단축키 칸을 기록 칸으로 바꾸고 다음 키 조합을 기다린다
    fn start_recording(self: &Rc<Self>, id: &str) {
        let target = self
            .rows
            .borrow()
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| Some((row.row.clone(), row.keys.clone()?)));
        let Some((row, keys)) = target else {
            return;
        };
        self.stop_recording();
        self.hide_message();
        // 포커스를 먼저 옮긴다. 기록 중에 포커스가 옮겨 가면 기록을 그만두기 때문이다
        row.grab_focus();
        self.list.select_row(Some(&row));
        self.recording.replace(Some(id.to_owned()));
        self.held.set(Modifiers::default());
        self.peak.set(Modifiers::default());
        self.combined.set(false);
        keys.add_css_class("recording");
        keys.set_xalign(0.5);
        self.show_recording();
        // 컴포지터가 단축키를 가로채지 않고 이 창에 보내게 한다
        if let Some(toplevel) = self.toplevel() {
            toplevel.inhibit_system_shortcuts(None::<gdk::Event>);
        }
    }

    /// 기록을 그만두고 단축키 칸을 되돌린다. 막았던 시스템 단축키도 되살린다
    fn stop_recording(self: &Rc<Self>) {
        let Some(id) = self.recording.take() else {
            return;
        };
        self.held.set(Modifiers::default());
        self.peak.set(Modifiers::default());
        self.combined.set(false);
        if let Some(toplevel) = self.toplevel() {
            toplevel.restore_system_shortcuts();
        }
        if let Some(keys) = self.keys_of(&id) {
            keys.remove_css_class("recording");
            keys.remove_css_class("waiting");
            keys.set_xalign(1.0);
        }
        self.update_rows();
        if self.stale.take() {
            self.schedule_refresh();
        }
    }

    fn toplevel(&self) -> Option<gdk::Toplevel> {
        self.window.surface().and_downcast::<gdk::Toplevel>()
    }

    /// 기록 칸: 누르고 있는 조합 키, 아무것도 누르지 않았으면 "단축키 입력"
    fn show_recording(&self) {
        let Some(id) = self.recording.borrow().clone() else {
            return;
        };
        let Some(keys) = self.keys_of(&id) else {
            return;
        };
        let held = self.held.get();
        let waiting = held == Modifiers::default();
        keys.set_text(&if waiting {
            self.context.tr.get("shortcuts-recording")
        } else {
            held.symbols()
        });
        set_class(&keys, "waiting", waiting);
    }

    /// 기록하는 동안 창의 모든 키를 받는다. 기록하지 않을 때는 그대로 흘려보낸다
    fn key_pressed(
        self: &Rc<Self>,
        controller: &gtk::EventControllerKey,
        key: gdk::Key,
        code: u32,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        if self.swallow.get() == Some(code) {
            return glib::Propagation::Stop;
        }
        let Some(id) = self.recording.borrow().clone() else {
            return glib::Propagation::Proceed;
        };
        let event = controller
            .current_event()
            .and_then(|event| event.downcast::<gdk::KeyEvent>().ok());
        let held = modifiers(state);
        let pressed = modifier_key(key);
        if pressed != Modifiers::default() || event.as_ref().is_some_and(gdk::KeyEvent::is_modifier)
        {
            self.held.set(held.union(pressed));
            self.peak.set(self.peak.get().union(held.union(pressed)));
            self.show_recording();
            return glib::Propagation::Stop;
        }
        self.combined.set(true);
        let layout = event
            .as_ref()
            .map_or_else(|| controller.group(), gdk::KeyEvent::layout);
        let display = WidgetExt::display(&self.window);
        let base = base_key(&display, code, layout).unwrap_or_else(|| key.to_lower());
        let Some(name) = base.name() else {
            return glib::Propagation::Stop;
        };
        let category = self
            .find(&id)
            .map(|shortcut| shortcut.category)
            .unwrap_or_default();
        let allow_shift = category != TEXT_CATEGORY;
        let input = category == INPUT_CATEGORY;
        let tr = &self.context.tr;
        match accelerator::record(held, &accelerator::keysym_name(&name), allow_shift, input) {
            Recorded::Cancel => {
                self.swallow.set(Some(code));
                self.stop_recording();
            }
            Recorded::Clear => {
                self.swallow.set(Some(code));
                self.commit(&id, String::new());
            }
            Recorded::Set(accelerator) => {
                self.swallow.set(Some(code));
                self.commit(&id, accelerator.to_string());
            }
            Recorded::NeedsModifier => self.show_message(&tr.get("shortcuts-needs-modifier")),
            Recorded::ShiftNotAllowed => self.show_message(&tr.get("shortcuts-no-shift")),
            Recorded::NeedsKey => self.show_message(&tr.get("shortcuts-needs-key")),
        }
        glib::Propagation::Stop
    }

    fn key_released(self: &Rc<Self>, key: gdk::Key, code: u32, state: gdk::ModifierType) {
        if self.swallow.get() == Some(code) {
            self.swallow.set(None);
        }
        let Some(id) = self.recording.borrow().clone() else {
            return;
        };
        // 떼는 이벤트의 상태에는 뗀 키가 아직 들어 있다
        let held = modifiers(state).without(modifier_key(key));
        self.held.set(held);
        self.show_recording();
        if held != Modifiers::default() {
            return;
        }
        // 모두 뗐다. 다른 키 없이 조합 키만 눌렀으면 조합 키만인 단축키(⌃⇧)이거나, 다른 키가 있어야
        // 한다고 알린다
        let peak = self.peak.replace(Modifiers::default());
        if self.combined.replace(false) {
            return;
        }
        let input = self
            .find(&id)
            .is_some_and(|shortcut| shortcut.category == INPUT_CATEGORY);
        match accelerator::record_modifiers(peak, input) {
            Some(Recorded::Set(accelerator)) => self.commit(&id, accelerator.to_string()),
            Some(Recorded::NeedsKey) => {
                self.show_message(&self.context.tr.get("shortcuts-needs-key"));
            }
            _ => {}
        }
    }

    /// 기록한 키를 컴포지터에 보낸다. 새 키를 정하면 단축키를 켜고, 지우면 켬·끔은 그대로 둔다.
    /// 답을 기다리는 동안 칸에는 새 키를 보인다
    fn commit(self: &Rc<Self>, id: &str, accelerator: String) {
        self.stop_recording();
        let enabled = if accelerator.is_empty() {
            self.find(id).is_none_or(|shortcut| shortcut.enabled)
        } else {
            true
        };
        if let Some(keys) = self.keys_of(id) {
            let symbols = accelerator::display(&accelerator, &key_names(&self.context.tr));
            let text = if symbols.is_empty() {
                self.context.tr.get("shortcuts-none")
            } else {
                symbols.clone()
            };
            keys.set_text(&text);
            set_class(&keys, "none", symbols.is_empty());
            set_class(&keys, "off", !enabled);
        }
        self.set(id, accelerator, enabled);
    }

    /// SetShortcut. 실패하면 까닭을 시트 안에 보인다. 어느 쪽이든 컴포지터에서 다시 읽는다
    fn set(self: &Rc<Self>, id: &str, accelerator: String, enabled: bool) {
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        let sheet = self.clone();
        let id = id.to_owned();
        glib::spawn_future_local(async move {
            let args = (id.as_str(), accelerator.as_str(), enabled).to_variant();
            match compositor
                .call(COMPOSITOR_INTERFACE, "SetShortcut", Some(args))
                .await
            {
                Ok(_) => sheet.hide_message(),
                Err(err) => sheet.show_failure(&dbus::message(&err)),
            }
            sheet.reload().await;
        });
    }

    /// 기본값으로 복원: 보고 있는 분류만 되돌린다
    fn restore_defaults(self: &Rc<Self>) {
        self.stop_recording();
        self.hide_message();
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        let category = self.category.borrow().clone();
        let sheet = self.clone();
        glib::spawn_future_local(async move {
            let args = (category.as_str(),).to_variant();
            if let Err(err) = compositor
                .call(COMPOSITOR_INTERFACE, "ResetShortcuts", Some(args))
                .await
            {
                sheet.show_failure(&dbus::message(&err));
            }
            sheet.reload().await;
        });
    }

    /// 컴포지터가 받지 않은 까닭. 다른 단축키가 쓰는 키면 그 이름을 댄다
    fn show_failure(&self, error: &str) {
        let tr = &self.context.tr;
        let mut args = FluentArgs::new();
        let text = match conflict(error) {
            Some(other) => {
                args.set("name", shortcut_label(tr, other));
                tr.get_with("shortcuts-in-use", &args)
            }
            None => {
                args.set("error", error.to_owned());
                tr.get_with("shortcuts-error", &args)
            }
        };
        self.show_message(&text);
    }

    fn show_message(&self, text: &str) {
        self.message.set_text(text);
        self.message.set_visible(true);
    }

    fn hide_message(&self) {
        self.message.set_visible(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 컴포지터가 처음 내놓는 단축키 (분류마다 보이는 순서)
    const KNOWN: [&str; 53] = [
        "launcher.show",
        "launcher.dock-autohide",
        "windows.next-app",
        "windows.previous-app",
        "windows.next-window",
        "windows.previous-window",
        "windows.quit-app",
        "windows.minimize",
        "windows.hide-app",
        "windows.hide-others",
        "windows.fullscreen",
        "windows.close",
        "windows.terminal",
        "input.switch-source",
        "screenshot.screen",
        "screenshot.screen-clipboard",
        "screenshot.area",
        "screenshot.area-clipboard",
        "text.line-start",
        "text.line-end",
        "text.document-start",
        "text.document-end",
        "text.word-left",
        "text.word-right",
        "text.paragraph-up",
        "text.paragraph-down",
        "text.delete-word",
        "text.delete-word-forward",
        "text.delete-line",
        "app.cut",
        "app.copy",
        "app.paste",
        "app.undo",
        "app.redo",
        "app.select-all",
        "app.find",
        "app.find-next",
        "app.find-previous",
        "app.new",
        "app.new-tab",
        "app.open",
        "app.save",
        "app.save-as",
        "app.print",
        "app.close",
        "app.settings",
        "app.back",
        "app.forward",
        "app.reload",
        "app.zoom-in",
        "app.zoom-out",
        "app.zoom-reset",
        "app.other-command-keys",
    ];

    fn shortcut(id: &str, category: &str) -> Shortcut {
        Shortcut {
            id: id.into(),
            category: category.into(),
            kind: Kind::Key,
            accelerator: String::new(),
            default: String::new(),
            enabled: true,
            default_enabled: true,
        }
    }

    #[test]
    fn every_known_shortcut_has_a_label() {
        for (language, source) in crate::TRANSLATIONS {
            let tr = Translations::from_source(language, source);
            for id in KNOWN {
                assert_ne!(shortcut_label(&tr, id), id, "{language}: {id}");
            }
            for (category, ..) in CATEGORIES {
                assert_ne!(category_label(&tr, category), category, "{language}");
            }
            for category in ["input", "text", "app"] {
                assert!(category_note(&tr, category).is_some(), "{language}");
            }
            assert_eq!(category_note(&tr, "launcher"), None);
        }
    }

    #[test]
    fn unknown_shortcuts_show_their_id() {
        let (language, source) = crate::TRANSLATIONS[0];
        let tr = Translations::from_source(language, source);
        assert_eq!(shortcut_label(&tr, "dock.magnify"), "dock.magnify");
        assert_eq!(category_label(&tr, "dock"), "dock");
    }

    #[test]
    fn categories_keep_the_macos_order_and_append_unknown_ones() {
        let shortcuts = [
            shortcut("app.copy", "app"),
            shortcut("new.one", "new"),
            shortcut("launcher.show", "launcher"),
            shortcut("text.line-start", "text"),
            shortcut("app.paste", "app"),
            shortcut("other.one", "other"),
        ];
        assert_eq!(
            category_order(&shortcuts),
            ["launcher", "text", "app", "new", "other"]
        );
    }

    #[test]
    fn restore_is_needed_only_for_changed_shortcuts() {
        let mut copy = shortcut("app.copy", "app");
        copy.accelerator = "<Super>c".into();
        copy.default = "<Super>c".into();
        assert!(copy.is_default());
        copy.enabled = false;
        assert!(!copy.is_default());
        copy.enabled = true;
        copy.accelerator.clear();
        assert!(!copy.is_default());
    }

    #[test]
    fn conflicts_name_the_other_shortcut() {
        assert_eq!(conflict("already used by app.copy"), Some("app.copy"));
        assert_eq!(
            conflict("<Super>c is already used by app.copy."),
            Some("app.copy")
        );
        assert_eq!(
            conflict("accelerator already used by 'windows.close' (enabled)"),
            Some("windows.close")
        );
        assert_eq!(conflict("unknown shortcut"), None);
        assert_eq!(conflict("invalid accelerator"), None);
        assert_eq!(conflict("already used by "), None);
    }
}
