// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 클립보드 기록 화면 (macOS 26 Spotlight의 클립보드: ⌘스페이스 뒤 ⌘4). 모든 앱 패널과 같은 자리, 같은 모양의 패널이다.
//! 위에서부터 검색 칸과 단추(모든 앱, 클립보드, 더 보기), 복사한 항목이다. 항목을 누르거나 Enter, ⌘C를 누르면 그 내용이
//! 클립보드에 들어가고 패널이 닫혀 앞의 앱에 ⌘V로 붙일 수 있다. 처음에는 macOS처럼 허용할지 묻는다. 내용은 clipboard.rs에 있다.
//!
//! 모든 앱 패널과 다른 창(`bunt-clipboard`)인 까닭: 에이전트가 기록을 읽거나 항목을 골라 앱에 붙이지 못해야 하기 때문이다.
//! 이 이름은 사람만 답하는 창이어서(comp/src/agent.rs의 AUTH_NAMESPACES, agent/src/atspi.rs의 HUMAN_ONLY_WINDOWS) 에이전트의
//! 목록과 스냅샷에 들지 않고, 떠 있는 동안 에이전트의 입력을 받지 않으며, 창을 찍지도 못한다.
//!
//! 항목의 글은 라벨이 아니라 [`PrivateText`]로 그린다: 접근성 버스는 같은 사용자의 프로그램이면 모두 읽으므로, 라벨에 넣으면
//! 패널이 열린 동안 다른 앱이 기록을 읽을 수 있다. 그 값으로 화면 읽기 프로그램도 항목의 글을 읽지 못하고 "클립보드 항목 3"처럼
//! 차례만 듣는다(DESIGN-SPEC의 macOS와 다른 것).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, SystemTime},
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, glib, prelude::*};

use crate::{
    clipboard::ClipboardHistory,
    clipboard_history::{Age, Item, Kind, age},
    launcher::{self, Mode},
};

/// 사람만 답하는 창의 이름. 바꾸면 comp/src/agent.rs와 agent/src/atspi.rs도 함께 고친다
const NAMESPACE: &str = "bunt-clipboard";
pub const CLIPBOARD_ICON: &str = "edit-paste-symbolic";
const TEXT_ICON: &str = "text-x-generic-symbolic";
const FILES_ICON: &str = "folder-symbolic";
const COPY_ICON: &str = "edit-copy-symbolic";
const MORE_ICON: &str = "view-more-symbolic";
/// 항목 앞의 아이콘과 미리 보기 (논리 픽셀). 미리 보기는 44px 줄 안에 든다
const ROW_ICON_SIZE: i32 = 20;
const THUMBNAIL_SIZE: i32 = 36;
/// 허용을 묻는 화면의 아이콘
const ALLOW_ICON_SIZE: i32 = 48;

/// 접근성 트리에 글을 내놓지 않고 한 줄로 그리는 위젯. 넘치는 글은 줄임표로 줄인다
mod private_text {
    use std::cell::RefCell;

    use gtk4::{self as gtk, glib, graphene, pango, prelude::*, subclass::prelude::*};

    #[derive(Default)]
    pub struct Imp {
        pub text: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Imp {
        const NAME: &'static str = "BuntPrivateText";
        type Type = super::PrivateText;
        type ParentType = gtk::Widget;

        fn class_init(class: &mut Self::Class) {
            class.set_css_name("private-text");
            class.set_accessible_role(gtk::AccessibleRole::Presentation);
        }
    }

    impl ObjectImpl for Imp {}

    impl WidgetImpl for Imp {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            if orientation == gtk::Orientation::Horizontal {
                return (0, 1, -1, -1);
            }
            let (_, height) = self.layout(-1).pixel_size();
            (height, height, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let widget = self.obj();
            let layout = self.layout(widget.width());
            let (_, height) = layout.pixel_size();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                0.0,
                ((widget.height() - height) / 2) as f32,
            ));
            snapshot.append_layout(&layout, &widget.color());
            snapshot.restore();
        }
    }

    impl Imp {
        fn layout(&self, width: i32) -> pango::Layout {
            let layout = self.obj().create_pango_layout(Some(&self.text.borrow()));
            layout.set_single_paragraph_mode(true);
            layout.set_ellipsize(pango::EllipsizeMode::End);
            if width > 0 {
                layout.set_width(width * pango::SCALE);
            }
            layout
        }
    }
}

glib::wrapper! {
    pub struct PrivateText(ObjectSubclass<private_text::Imp>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PrivateText {
    fn new(text: &str) -> Self {
        let widget: Self = glib::Object::new();
        use gtk::subclass::prelude::ObjectSubclassIsExt;
        widget.imp().text.replace(text.to_owned());
        widget.set_hexpand(true);
        widget.set_valign(gtk::Align::Center);
        widget
    }
}

/// 복제해도 같은 창을 가리킨다
#[derive(Clone)]
pub struct ClipboardPanel(Rc<Inner>);

type Back = Rc<dyn Fn()>;

struct Inner {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    tr: Rc<Translations>,
    history: ClipboardHistory,
    /// 모든 앱으로 돌아가는 길 (⌘1, 모든 앱 단추). 모든 앱 패널이 단다
    back: RefCell<Option<Back>>,
    /// 지금 패널의 위젯 (열 때마다 새로 만든다). 허용을 묻는 화면에서는 없다
    view: RefCell<Option<View>>,
    /// 허용을 묻는 화면의 단추
    allow: RefCell<Option<gtk::Button>>,
    /// 사라지는 전환 중이다. 그동안에는 줄을 다시 그리지 않는다: 고른 항목이 맨 위로 올라가는 것이 보인다
    closing: Cell<bool>,
}

struct View {
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    empty: gtk::Label,
    more: gtk::Popover,
    /// 목록의 줄 차례와 같다
    items: Rc<RefCell<Vec<Rc<Item>>>>,
    /// 소문자 검색어
    query: Rc<RefCell<String>>,
}

impl ClipboardPanel {
    pub fn new(app: &gtk::Application, tr: &Rc<Translations>, history: &ClipboardHistory) -> Self {
        let window = launcher::overlay_window(app, NAMESPACE);
        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["launcher-panel"])
            .build();
        window.set_child(Some(&panel));
        let clipboard = Self(Rc::new(Inner {
            window,
            panel,
            tr: tr.clone(),
            history: history.clone(),
            back: RefCell::new(None),
            view: RefCell::new(None),
            allow: RefCell::new(None),
            closing: Cell::new(false),
        }));

        let weak = Rc::downgrade(&clipboard.0);
        launcher::close_on_outside_press(&clipboard.0.window, &clipboard.0.panel, move || {
            if let Some(inner) = weak.upgrade() {
                ClipboardPanel(inner).hide();
            }
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&clipboard.0);
        keys.connect_key_pressed(move |_, key, _, state| {
            weak.upgrade().map_or(glib::Propagation::Proceed, |inner| {
                ClipboardPanel(inner).key_pressed(key, state)
            })
        });
        clipboard.0.window.add_controller(keys);

        let weak = Rc::downgrade(&clipboard.0);
        history.subscribe(move || {
            if let Some(inner) = weak.upgrade() {
                ClipboardPanel(inner).history_changed();
            }
        });
        clipboard
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    pub fn set_back(&self, back: impl Fn() + 'static) {
        self.0.back.replace(Some(Rc::new(back)));
    }

    pub fn is_open(&self) -> bool {
        self.0.window.is_visible() && !self.0.closing.get()
    }

    /// 연다. `fade`면 모든 앱처럼 나타나는 전환을 보인다. 모든 앱에서 넘어올 때는 전환 없이 같은 자리에 뜬다.
    /// `shown`은 이 창이 그려진 뒤에 부른다: 모든 앱 창을 그때 내려야 패널이 깜빡이지 않는다
    pub fn show(&self, fade: bool, shown: impl FnOnce() + 'static) {
        if self.0.history.locked() {
            return;
        }
        let inner = &self.0;
        inner.closing.set(false);
        self.fill();
        launcher::fit_panel(&inner.window, &inner.panel);
        if !fade {
            inner.panel.add_css_class("shown");
        }
        inner.window.present();
        self.focus_first();
        if fade {
            // 그려진 다음에 클래스를 붙여야 나타나는 전환이 보인다
            let panel = inner.panel.clone();
            glib::timeout_add_local_once(Duration::from_millis(16), move || {
                panel.add_css_class("shown");
            });
        }
        let shown = RefCell::new(Some(shown));
        let frames = Cell::new(0);
        inner.window.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            if let Some(shown) = shown.take() {
                shown();
            }
            glib::ControlFlow::Break
        });
    }

    /// 사라지는 전환 뒤에 닫는다
    pub fn hide(&self) {
        if !self.is_open() {
            return;
        }
        self.0.closing.set(true);
        self.0.panel.remove_css_class("shown");
        let this = self.clone();
        glib::timeout_add_local_once(launcher::CLOSE_DELAY, move || {
            // 그사이 다시 열었으면 그대로 둔다
            if this.0.closing.get() {
                this.hide_now();
            }
        });
    }

    /// 바로 닫고 줄을 없앤다
    pub fn hide_now(&self) {
        let inner = &self.0;
        inner.closing.set(false);
        inner.panel.remove_css_class("shown");
        inner.window.set_visible(false);
        inner.view.replace(None);
        inner.allow.replace(None);
        launcher::clear(&inner.panel);
    }

    /// 처음 포커스. 창을 띄운 바로 뒤에 부른다: 띄울 때 GTK가 첫 칸에 준 포커스를 창이 키보드를 받기 전에 옮긴다
    fn focus_first(&self) {
        if let Some(view) = self.0.view.borrow().as_ref() {
            launcher::focus_search(&self.0.window, &view.search);
        } else if let Some(allow) = self.0.allow.borrow().as_ref() {
            allow.grab_focus();
        }
    }

    fn history_changed(&self) {
        let inner = &self.0;
        // 잠그면 바로 닫는다. 풀린 뒤에도 저절로 다시 뜨지 않는다
        if inner.history.locked() {
            if inner.window.is_visible() {
                self.hide_now();
            }
            return;
        }
        if !self.is_open() {
            return;
        }
        let listing = inner.view.borrow().is_some();
        if listing == self.can_list() {
            self.refresh();
        } else {
            // 허용했거나 시스템 설정에서 껐다
            self.fill();
            self.focus_first();
        }
    }

    fn can_list(&self) -> bool {
        self.0.history.enabled() && !self.0.history.unavailable()
    }

    /// 패널을 새로 만든다
    fn fill(&self) {
        let inner = &self.0;
        launcher::clear(&inner.panel);
        inner.view.replace(None);
        inner.allow.replace(None);
        if self.can_list() {
            self.build_list();
        } else {
            self.build_message();
        }
    }

    fn mode_buttons(&self) -> gtk::Box {
        let this = self.clone();
        launcher::mode_buttons(&self.0.tr, Mode::Clipboard, move |mode| {
            if mode == Mode::Apps {
                this.go_back();
            }
        })
    }

    fn go_back(&self) {
        let back = self.0.back.borrow().clone();
        if let Some(back) = back {
            back();
        }
    }

    fn header_icon() -> gtk::Image {
        let icon = gtk::Image::from_icon_name(CLIPBOARD_ICON);
        icon.set_pixel_size(24);
        icon
    }

    /// 허용을 묻는 화면, 또는 쓸 수 없다는 알림
    fn build_message(&self) {
        let inner = &self.0;
        let tr = &inner.tr;
        let header = gtk::Box::builder()
            .spacing(10)
            .css_classes(["launcher-header"])
            .build();
        header.append(&Self::header_icon());
        header.append(
            &gtk::Label::builder()
                .label(tr.get("clipboard-title"))
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["clipboard-title"])
                .build(),
        );
        header.append(&self.mode_buttons());
        inner.panel.append(&header);
        inner.panel.append(&launcher::separator());

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .css_classes(["clipboard-message"])
            .build();
        let icon = gtk::Image::from_icon_name(CLIPBOARD_ICON);
        icon.set_pixel_size(ALLOW_ICON_SIZE);
        body.append(&icon);
        if inner.history.unavailable() {
            body.append(
                &gtk::Label::builder()
                    .label(tr.get("clipboard-unavailable"))
                    .css_classes(["launcher-empty"])
                    .build(),
            );
        } else {
            body.append(
                &gtk::Label::builder()
                    .label(tr.get("clipboard-allow-title"))
                    .css_classes(["clipboard-message-title"])
                    .build(),
            );
            body.append(
                &gtk::Label::builder()
                    .label(tr.get("clipboard-allow-description"))
                    .wrap(true)
                    .justify(gtk::Justification::Center)
                    .max_width_chars(44)
                    .css_classes(["clipboard-message-description"])
                    .build(),
            );
            let allow = gtk::Button::builder()
                .label(tr.get("clipboard-allow"))
                .halign(gtk::Align::Center)
                .css_classes(["clipboard-allow"])
                .build();
            let history = inner.history.clone();
            allow.connect_clicked(move |_| history.allow());
            body.append(&allow);
            inner.allow.replace(Some(allow));
        }
        inner.panel.append(&body);
    }

    fn build_list(&self) {
        let inner = &self.0;
        let tr = &inner.tr;
        let search = gtk::SearchEntry::builder()
            .placeholder_text(tr.get("clipboard-search"))
            .hexpand(true)
            .css_classes(["launcher-search"])
            .build();
        launcher::prepare_search(&inner.window, &search);

        // 더 보기: 기록 지우기. 사람이 잘못 누를 만한 자리가 아니고 메뉴를 한 번 거치므로 다시 묻지 않는다(macOS와 같다)
        let clear = gtk::Button::builder()
            .label(tr.get("clipboard-clear"))
            .css_classes(["extra-row", "clipboard-clear"])
            .build();
        let more = gtk::Popover::builder()
            .child(&clear)
            .has_arrow(false)
            .css_classes(["extra-popover"])
            .build();
        let more_button = gtk::MenuButton::builder()
            .icon_name(MORE_ICON)
            .popover(&more)
            .tooltip_text(tr.get("clipboard-more"))
            .valign(gtk::Align::Center)
            .css_classes(["launcher-mode"])
            .build();
        more_button.update_property(&[gtk::accessible::Property::Label(&tr.get("clipboard-more"))]);
        crate::popups::guard(&more_button);
        let (history, popover) = (inner.history.clone(), more.clone());
        clear.connect_clicked(move |_| {
            popover.popdown();
            history.clear();
        });

        let header = gtk::Box::builder()
            .spacing(10)
            .css_classes(["launcher-header"])
            .build();
        header.append(&Self::header_icon());
        header.append(&search);
        header.append(&self.mode_buttons());
        header.append(&more_button);
        inner.panel.append(&header);
        inner.panel.append(&launcher::separator());

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["clipboard-list"])
            .build();
        let empty = gtk::Label::builder()
            .vexpand(true)
            .visible(false)
            .css_classes(["launcher-empty"])
            .build();
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        body.append(&list);
        body.append(&empty);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&body)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .css_classes(["launcher-scroller"])
            .build();
        inner.panel.append(&scroller);

        let items: Rc<RefCell<Vec<Rc<Item>>>> = Rc::default();
        let query: Rc<RefCell<String>> = Rc::default();
        let (shown, typed) = (items.clone(), query.clone());
        list.set_filter_func(move |row| {
            usize::try_from(row.index()).is_ok_and(|index| {
                shown
                    .borrow()
                    .get(index)
                    .is_some_and(|item| item.matches(&typed.borrow()))
            })
        });
        let this = self.clone();
        list.connect_row_activated(move |_, row| this.pick_row(row.index()));
        let this = self.clone();
        let typed = query.clone();
        search.connect_search_changed(move |search| {
            typed.replace(search.text().trim().to_lowercase());
            this.filter_changed();
        });
        // 검색 칸의 Enter는 고른 항목을 복사한다. 눌린 키는 key_pressed가 먼저 받지만 입력기를 거친 Enter는 여기로 온다
        let this = self.clone();
        search.connect_activate(move |_| this.pick_selected());

        inner.view.replace(Some(View {
            search,
            list,
            scroller,
            empty,
            more,
            items,
            query,
        }));
        self.refresh();
    }

    /// 기록을 다시 읽어 줄을 만든다. 고른 항목과 검색어는 그대로 둔다
    fn refresh(&self) {
        let inner = &self.0;
        let view = inner.view.borrow();
        let Some(view) = view.as_ref() else {
            return;
        };
        let selected = view
            .list
            .selected_row()
            .and_then(|row| view.items.borrow().get(row.index() as usize).map(|i| i.id));
        let items = inner.history.items();
        view.list.remove_all();
        for (position, item) in items.iter().enumerate() {
            view.list.append(&self.row(item, position));
        }
        view.items.replace(items);
        view.list.invalidate_filter();
        self.reselect(selected);
    }

    fn filter_changed(&self) {
        {
            let view = self.0.view.borrow();
            let Some(view) = view.as_ref() else {
                return;
            };
            view.list.invalidate_filter();
        }
        self.reselect(None);
    }

    /// 빈 화면 문구를 맞추고 항목을 고른다: 그 항목이 보이면 그것을, 아니면 보이는 첫 항목을
    fn reselect(&self, wanted: Option<u64>) {
        let view = self.0.view.borrow();
        let Some(view) = view.as_ref() else {
            return;
        };
        let items = view.items.borrow();
        let query = view.query.borrow();
        let visible: Vec<usize> = (0..items.len())
            .filter(|&index| items[index].matches(&query))
            .collect();
        let target = wanted
            .and_then(|id| visible.iter().copied().find(|&index| items[index].id == id))
            .or_else(|| visible.first().copied());
        view.list.select_row(
            target
                .and_then(|index| view.list.row_at_index(index as i32))
                .as_ref(),
        );
        view.list.set_visible(!visible.is_empty());
        view.empty.set_visible(visible.is_empty());
        view.empty.set_label(&self.0.tr.get(if items.is_empty() {
            "clipboard-empty"
        } else {
            "clipboard-no-match"
        }));
    }

    fn row(&self, item: &Rc<Item>, position: usize) -> gtk::ListBoxRow {
        let tr = &self.0.tr;
        let line = gtk::Box::builder()
            .spacing(10)
            .css_classes(["clipboard-row"])
            .build();
        line.append(&leading(item));
        let text = match item.kind {
            Kind::Image => format!("{} {}", tr.get("clipboard-image"), item.preview),
            Kind::Text | Kind::Files => item.preview.clone(),
        };
        line.append(&PrivateText::new(&text));
        line.append(
            &gtk::Label::builder()
                .label(age_label(tr, age(item.copied_at, SystemTime::now())))
                .css_classes(["clipboard-time"])
                .build(),
        );
        // 항목 오른쪽의 복사 아이콘 (macOS와 같다). 줄을 눌러도 같다
        let copy = gtk::Button::builder()
            .icon_name(COPY_ICON)
            .tooltip_text(tr.get("clipboard-copy"))
            .valign(gtk::Align::Center)
            .css_classes(["clipboard-copy"])
            .build();
        copy.update_property(&[gtk::accessible::Property::Label(&tr.get("clipboard-copy"))]);
        let (this, id) = (self.clone(), item.id);
        copy.connect_clicked(move |_| this.pick(id));
        line.append(&copy);

        let row = gtk::ListBoxRow::builder()
            .child(&line)
            .css_classes(["clipboard-item"])
            .build();
        let mut args = FluentArgs::new();
        args.set("position", position + 1);
        row.update_property(&[gtk::accessible::Property::Label(
            &tr.get_with("clipboard-item", &args),
        )]);
        row
    }

    fn pick_row(&self, index: i32) {
        let id = {
            let view = self.0.view.borrow();
            view.as_ref().and_then(|view| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| view.items.borrow().get(index).map(|item| item.id))
            })
        };
        if let Some(id) = id {
            self.pick(id);
        }
    }

    fn pick_selected(&self) {
        let selected = {
            let view = self.0.view.borrow();
            view.as_ref()
                .and_then(|view| view.list.selected_row())
                .map(|row| row.index())
        };
        if let Some(index) = selected {
            self.pick_row(index);
        }
    }

    /// 항목을 클립보드에 넣고 닫는다. 닫으면 앞의 앱이 키보드를 돌려받아 ⌘V로 붙일 수 있다
    fn pick(&self, id: u64) {
        self.hide();
        self.0.history.pick(id);
    }

    /// 고른 항목을 위나 아래의 보이는 항목으로 옮긴다
    fn move_selection(&self, down: bool) {
        let view = self.0.view.borrow();
        let Some(view) = view.as_ref() else {
            return;
        };
        let items = view.items.borrow();
        let query = view.query.borrow();
        let current = view.list.selected_row().map_or(-1, |row| row.index());
        let mut visible =
            (0..items.len() as i32).filter(|&index| items[index as usize].matches(&query));
        let next = if down {
            visible.find(|&index| index > current)
        } else {
            visible.rfind(|&index| index < current)
        };
        let Some(row) = next.and_then(|index| view.list.row_at_index(index)) else {
            return;
        };
        view.list.select_row(Some(&row));
        // 포커스는 검색 칸에 둔다. 고른 줄이 보이게 스크롤만 한다
        if let Some(bounds) = row.compute_bounds(&view.list) {
            view.scroller.vadjustment().clamp_page(
                f64::from(bounds.y()),
                f64::from(bounds.y() + bounds.height()),
            );
        }
    }

    fn key_pressed(&self, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        // ⌘글자 키는 컴포지터가 Ctrl로 옮겨 보낸다. 시스템 설정에서 옮기기를 끄면 그대로 온다
        let command = state.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::SUPER_MASK
                | gdk::ModifierType::META_MASK,
        );
        let listing = self.0.view.borrow().as_ref().map(|view| {
            (
                !view.search.text().is_empty(),
                view.more.is_visible(),
                GtkWindowExt::focus(&self.0.window).is_some_and(|focus| {
                    focus.is_ancestor(&view.search)
                        || &focus == view.search.upcast_ref::<gtk::Widget>()
                }),
            )
        });
        let Some((searching, menu_open, in_search)) = listing else {
            return self.key_without_list(key, command);
        };
        // 열린 메뉴는 Esc와 화살표를 스스로 받는다
        if menu_open {
            return glib::Propagation::Proceed;
        }
        match key {
            gdk::Key::Escape => {
                if searching {
                    if let Some(view) = self.0.view.borrow().as_ref() {
                        view.search.set_text("");
                    }
                } else {
                    self.hide();
                }
            }
            // 포커스가 검색 칸에 있어야 항목을 다룬다. 단추와 줄은 이 키를 스스로 받는다(⇥로 옮긴 때)
            gdk::Key::Down if in_search => self.move_selection(true),
            gdk::Key::Up if in_search => self.move_selection(false),
            gdk::Key::Return | gdk::Key::KP_Enter if in_search => self.pick_selected(),
            gdk::Key::c | gdk::Key::C if command && !self.search_has_selection() => {
                self.pick_selected();
            }
            gdk::Key::_1 | gdk::Key::_4 if command => self.go_back(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// 허용을 묻는 화면의 키. Enter와 스페이스는 포커스를 가진 단추가 받는다
    fn key_without_list(&self, key: gdk::Key, command: bool) -> glib::Propagation {
        match key {
            gdk::Key::Escape => self.hide(),
            gdk::Key::_1 | gdk::Key::_4 if command => self.go_back(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    }

    /// 검색 칸의 글을 골라 두었으면 ⌘C는 그 글을 복사한다
    fn search_has_selection(&self) -> bool {
        self.0
            .view
            .borrow()
            .as_ref()
            .is_some_and(|view| view.search.selection_bounds().is_some())
    }
}

fn leading(item: &Item) -> gtk::Widget {
    if let Some(thumbnail) = &item.thumbnail {
        let texture = gdk::MemoryTexture::new(
            thumbnail.width as i32,
            thumbnail.height as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &thumbnail.pixels,
            thumbnail.width as usize * 4,
        );
        // 그림 전체가 보이게 줄인다. 줄 높이를 늘리지 않는다
        let picture = gtk::Image::from_paintable(Some(&texture));
        picture.set_pixel_size(THUMBNAIL_SIZE);
        picture.add_css_class("clipboard-thumbnail");
        return picture.upcast();
    }
    let icon = gtk::Image::from_icon_name(if item.kind == Kind::Files {
        FILES_ICON
    } else {
        TEXT_ICON
    });
    icon.set_pixel_size(ROW_ICON_SIZE);
    icon.add_css_class("clipboard-icon");
    icon.upcast()
}

fn age_label(tr: &Translations, age: Age) -> String {
    let with_count = |id: &str, count: u64| {
        let mut args = FluentArgs::new();
        args.set("count", count);
        tr.get_with(id, &args)
    };
    match age {
        Age::JustNow => tr.get("clipboard-time-now"),
        Age::Minutes(count) => with_count("clipboard-time-minutes", count),
        Age::Hours(count) => with_count("clipboard-time-hours", count),
        Age::Days(count) => with_count("clipboard-time-days", count),
    }
}
