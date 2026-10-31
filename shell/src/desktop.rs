// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 데스크탑 항목(macOS의 데스크탑). 데스크탑 폴더(xdg-user-dirs의 DESKTOP, 없으면 ~/Desktop)의 파일과
//! 폴더를 배경화면 위 층(bottom)의 창에, macOS처럼 화면 오른쪽 위부터 위에서 아래로 칸에 맞춰 놓는다.
//! - 누르면 고르고(⌘·⇧를 누른 채 누르면 더하거나 뺀다), 두 번 누르거나 ⌘O·⌘↓로 기본 앱으로 연다
//! - 빈 곳에서 드래그하면 반투명 사각형이 나타나고 그 안에 든 항목을 고른다
//! - 항목을 드래그하면 다른 앱에 파일로 건네고, 데스크탑 안에 놓으면 그 칸으로 옮긴다
//! - Return은 이름 변경, ⌘⌫는 휴지통으로 이동이다(영구 삭제는 하지 않는다). 단축 메뉴는 오른쪽 단추나 손가락으로
//!   길게 눌러 연다
//!
//! 칸 자리는 이름마다 상태 폴더의 `buntos/desktop-positions`에 적어 다음 로그인에도 쓴다. 창은 누르면
//! 키보드를 받는다(on-demand). 컴포지터는 창이 없는 곳을 누르면 이 창에 키보드를 준다. 시스템 설정의
//! 데스크탑 및 Dock 페이지에서 끄면(`desktop-show-items`) 창을 숨긴다.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gdk, gio, glib, graphene, pango, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{menu_names, menubar, shell_settings};

const NAMESPACE: &str = "bunt-desktop";
/// 아이콘 크기 (macOS 데스크탑의 기본 64pt)
const ICON_SIZE: i32 = 64;
/// 한 칸의 크기. 아이콘과 두 줄 이름이 들어간다
const SLOT_SIZE: (i32, i32) = (100, 104);
/// 창 가장자리와 띄우는 거리. 창은 메뉴 막대와 Dock의 배타 영역 밖에 놓인다(셸이 두 막대 다음에 띄운다)
const MARGIN_TOP: i32 = 8;
const MARGIN_RIGHT: i32 = 12;
const MARGIN_BOTTOM: i32 = 8;
/// 미리 보기를 만들 그림 파일의 크기 상한
const THUMBNAIL_MAX_BYTES: i64 = 30 * 1024 * 1024;
/// 폴더가 바뀐 알림을 모아 한 번에 다시 읽는 간격
const REFRESH_DELAY: Duration = Duration::from_millis(120);
const QUERY_ATTRIBUTES: &str = "standard::name,standard::display-name,standard::icon,\
standard::is-hidden,standard::is-backup,standard::content-type,standard::size,standard::type";

/// 칸 (열, 행). 열은 오른쪽 끝에서부터, 행은 위에서부터 센다
type Slot = (u32, u32);

/// 화면에 놓인 항목 하나
struct Item {
    /// 파일 이름 (폴더 안에서 겹치지 않는다. 칸 자리와 고르기의 열쇠)
    name: String,
    file: gio::File,
    widget: gtk::Box,
    label: gtk::Label,
}

/// 빈 곳에서 시작한 드래그해 여러 항목 선택
struct Marquee {
    start: (f64, f64),
    /// 시작할 때 이미 골라 둔 것. ⌘·⇧를 누른 채 시작하면 사각형 안의 항목을 이것에 더하거나 뺀다
    base: BTreeSet<String>,
}

struct Inner {
    tr: Rc<Translations>,
    window: gtk::ApplicationWindow,
    area: gtk::Fixed,
    marquee: gtk::Box,
    dir: gio::File,
    positions_path: PathBuf,
    items: RefCell<Vec<Item>>,
    slots: RefCell<HashMap<String, Slot>>,
    selected: RefCell<BTreeSet<String>>,
    size: Cell<(i32, i32)>,
    dragging_marquee: RefCell<Option<Marquee>>,
    /// 이 창의 항목을 끄는 중이면 끈 항목(앞)과 함께 끄는 항목들. 데스크탑 안에 놓으면 옮긴다
    dragging_items: RefCell<Option<(String, Vec<String>)>>,
    /// 이미 골라 둔 항목을 눌렀다. 끌지 않고 떼면 그 항목만 고른다(macOS)
    pressed_selected: RefCell<Option<String>>,
    renaming: RefCell<Option<(String, gtk::Entry)>>,
    refresh_pending: Cell<bool>,
    monitor: RefCell<Option<gio::FileMonitor>>,
}

#[derive(Clone)]
pub struct Desktop(Rc<Inner>);

/// 데스크탑 항목 창을 만든다. 데스크탑 폴더를 만들 수 없으면 None
pub fn build(app: &gtk::Application, tr: &Rc<Translations>) -> Option<Desktop> {
    let dir_path = desktop_dir();
    if let Err(err) = std::fs::create_dir_all(&dir_path) {
        eprintln!("bunt-shell: desktop folder {}: {err}", dir_path.display());
        return None;
    }
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        // 제목은 보이지 않지만 접근성 이름이 된다(셸 레이어 창 관례: 레이어 이름과 같다)
        .title(NAMESPACE)
        .css_classes([NAMESPACE])
        .build();
    window.init_layer_shell();
    window.set_namespace(Some(NAMESPACE));
    // 배경화면(background) 위, 창 아래
    window.set_layer(Layer::Bottom);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    // 메뉴 막대와 Dock의 배타 영역 밖에 놓는다
    window.set_exclusive_zone(0);
    // 이름 변경과 ⌘⌫ 같은 키를 받는다. 컴포지터는 누를 때만 준다
    window.set_keyboard_mode(KeyboardMode::OnDemand);

    let area = gtk::Fixed::builder()
        .accessible_role(gtk::AccessibleRole::List)
        .hexpand(true)
        .vexpand(true)
        .focusable(true)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(&tr.get("desktop-title"))]);
    let marquee = gtk::Box::builder()
        .css_classes(["desktop-marquee"])
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .can_target(false)
        .visible(false)
        .build();
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&marquee);
    window.set_child(Some(&overlay));

    let desktop = Desktop(Rc::new(Inner {
        tr: tr.clone(),
        window,
        area,
        marquee,
        dir: gio::File::for_path(&dir_path),
        positions_path: positions_path(),
        items: RefCell::new(Vec::new()),
        slots: RefCell::new(HashMap::new()),
        selected: RefCell::new(BTreeSet::new()),
        size: Cell::new((0, 0)),
        dragging_marquee: RefCell::new(None),
        dragging_items: RefCell::new(None),
        pressed_selected: RefCell::new(None),
        renaming: RefCell::new(None),
        refresh_pending: Cell::new(false),
        monitor: RefCell::new(None),
    }));
    let saved = std::fs::read_to_string(&desktop.0.positions_path)
        .map(|text| parse_positions(&text))
        .unwrap_or_default();
    desktop.0.slots.replace(saved);
    desktop.follow_size();
    desktop.add_gestures();
    desktop.add_keys();
    desktop.watch_folder();
    desktop.follow_activity();
    desktop.refresh();
    desktop.follow_setting();
    // 처리기들은 약한 참조를 든다. 창의 처리기가 강한 참조를 들어 셸이 끝날 때까지 산다
    let keep_alive = desktop.clone();
    desktop.0.window.connect_destroy(move |_| {
        let _ = &keep_alive;
    });
    Some(desktop)
}

impl Desktop {
    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    /// 창 크기가 바뀌면(화면 해상도, 배율, 메뉴 막대) 칸을 다시 놓는다
    fn follow_size(&self) {
        let weak = Rc::downgrade(&self.0);
        self.0.window.connect_realize(move |window| {
            let Some(surface) = window.surface() else {
                return;
            };
            let weak = weak.clone();
            surface.connect_layout(move |_, width, height| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if inner.size.replace((width, height)) != (width, height) {
                    Desktop(inner).place_items();
                }
            });
        });
    }

    /// 창을 보이는지: 시스템 설정의 "항목 보기: 데스크탑에서"
    fn follow_setting(&self) {
        let Some(settings) = shell_settings() else {
            self.0.window.present();
            return;
        };
        let apply = {
            let window = self.0.window.clone();
            move |settings: &gio::Settings| {
                if settings.boolean("desktop-show-items") {
                    window.present();
                } else {
                    window.set_visible(false);
                }
            }
        };
        apply(&settings);
        // 변경 알림은 설정 객체가 살아 있을 때만 온다. 처리기가 객체를 붙든다
        let keep = settings.clone();
        settings.connect_changed(Some("desktop-show-items"), move |settings, _| {
            let _ = &keep;
            apply(settings);
        });
    }

    /// 창이 키보드를 잃으면 고른 항목을 회색으로 보인다(macOS의 활성이 아닌 Finder)
    fn follow_activity(&self) {
        let area = self.0.area.clone();
        self.0.window.connect_is_active_notify(move |window| {
            if window.is_active() {
                area.remove_css_class("inactive");
            } else {
                area.add_css_class("inactive");
            }
        });
    }

    fn watch_folder(&self) {
        let monitor = match self
            .0
            .dir
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            Ok(monitor) => monitor,
            Err(err) => {
                eprintln!("bunt-shell: cannot watch the desktop folder: {err}");
                return;
            }
        };
        let weak = Rc::downgrade(&self.0);
        monitor.connect_changed(move |_, file, other, event| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // 이름을 바꾸면 칸 자리를 새 이름으로 옮긴다 (다른 앱에서 바꿔도)
            if event == gio::FileMonitorEvent::Renamed
                && let (Some(old), Some(new)) =
                    (file.basename(), other.and_then(|other| other.basename()))
            {
                let (old, new) = (
                    old.to_string_lossy().into_owned(),
                    new.to_string_lossy().into_owned(),
                );
                let mut slots = inner.slots.borrow_mut();
                if let Some(slot) = slots.remove(&old) {
                    slots.insert(new, slot);
                    drop(slots);
                    Desktop(inner.clone()).save_positions();
                }
            }
            Desktop(inner).refresh_soon();
        });
        self.0.monitor.replace(Some(monitor));
    }

    /// 폴더의 바뀜 알림은 여러 개가 잇달아 오므로 모아서 한 번 읽는다
    fn refresh_soon(&self) {
        if self.0.refresh_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        glib::timeout_add_local_once(REFRESH_DELAY, move || {
            if let Some(inner) = weak.upgrade() {
                inner.refresh_pending.set(false);
                Desktop(inner).refresh();
            }
        });
    }

    /// 데스크탑 폴더를 다시 읽어 항목을 맞춘다. 있던 항목은 그대로 두고 새 항목만 만든다
    fn refresh(&self) {
        let inner = &self.0;
        let entries = list_folder(&inner.dir);
        let names: HashSet<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        // 사라진 항목을 거둔다 (이름을 바꾸는 중인 항목이 사라지면 바꾸기를 그만둔다)
        let mut items = inner.items.borrow_mut();
        items.retain(|item| {
            let keep = names.contains(item.name.as_str());
            if !keep {
                inner.area.remove(&item.widget);
            }
            keep
        });
        let renaming_gone = inner
            .renaming
            .borrow()
            .as_ref()
            .is_some_and(|(name, _)| !names.contains(name.as_str()));
        if renaming_gone {
            drop(items);
            self.end_rename(false);
            items = inner.items.borrow_mut();
        }
        inner
            .selected
            .borrow_mut()
            .retain(|name| names.contains(name.as_str()));
        let known: HashSet<String> = items.iter().map(|item| item.name.clone()).collect();
        for entry in entries {
            if known.contains(&entry.name) {
                continue;
            }
            let item = self.make_item(entry);
            inner.area.put(&item.widget, 0.0, 0.0);
            items.push(item);
        }
        items.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        drop(items);
        self.place_items();
        self.show_selection();
    }

    /// 항목마다 칸을 정해 그 자리에 놓는다. 자리가 바뀌었으면 적어 둔다
    fn place_items(&self) {
        let inner = &self.0;
        let (width, height) = inner.size.get();
        if width <= 0 || height <= 0 {
            return;
        }
        let items = inner.items.borrow();
        let names: Vec<String> = items.iter().map(|item| item.name.clone()).collect();
        let slots = assign_slots(&names, &inner.slots.borrow(), rows_for(height));
        for item in items.iter() {
            if let Some(&slot) = slots.get(&item.name) {
                let (x, y) = slot_origin(slot, width);
                inner.area.move_(&item.widget, f64::from(x), f64::from(y));
            }
        }
        // 접근성 트리(에이전트, 화면 읽기 프로그램)가 화면 차례(오른쪽 열부터 위에서 아래로)로 읽게
        // 자식 차례를 맞춘다
        let mut order: Vec<&Item> = items.iter().collect();
        order.sort_by_key(|item| {
            slots
                .get(&item.name)
                .copied()
                .unwrap_or((u32::MAX, u32::MAX))
        });
        let mut previous: Option<&gtk::Widget> = None;
        for item in order {
            item.widget.insert_after(&inner.area, previous);
            previous = Some(item.widget.upcast_ref());
        }
        drop(items);
        if *inner.slots.borrow() != slots {
            inner.slots.replace(slots);
            self.save_positions();
        }
    }

    fn save_positions(&self) {
        let inner = &self.0;
        if let Some(parent) = inner.positions_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::write(
            &inner.positions_path,
            format_positions(&inner.slots.borrow()),
        ) {
            eprintln!("bunt-shell: {}: {err}", inner.positions_path.display());
        }
    }

    fn make_item(&self, entry: Entry) -> Item {
        let widget = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .width_request(SLOT_SIZE.0)
            .css_classes(["desktop-item"])
            .accessible_role(gtk::AccessibleRole::ListItem)
            .build();
        widget.update_property(&[gtk::accessible::Property::Label(&entry.display_name)]);
        let image = gtk::Image::builder()
            .pixel_size(ICON_SIZE)
            .halign(gtk::Align::Center)
            .css_classes(["desktop-icon"])
            .build();
        match &entry.icon {
            Some(icon) => image.set_from_gicon(icon),
            None => image.set_icon_name(Some("text-x-generic")),
        }
        let label = gtk::Label::builder()
            .label(&entry.display_name)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(pango::EllipsizeMode::Middle)
            .justify(gtk::Justification::Center)
            .max_width_chars(14)
            .halign(gtk::Align::Center)
            .css_classes(["desktop-name"])
            .build();
        widget.append(&image);
        widget.append(&label);
        let file = self.0.dir.child(&entry.name);
        if entry.thumbnail {
            load_thumbnail(&file, &image);
        }
        self.add_drag_source(&widget, &entry.name);
        Item {
            name: entry.name,
            file,
            widget,
            label,
        }
    }

    /// 고른 항목에 `selected` 클래스와 접근성 상태를 단다
    fn show_selection(&self) {
        let selected = self.0.selected.borrow();
        for item in self.0.items.borrow().iter() {
            let on = selected.contains(&item.name);
            if on {
                item.widget.add_css_class("selected");
            } else {
                item.widget.remove_css_class("selected");
            }
            item.widget
                .update_state(&[gtk::accessible::State::Selected(Some(on))]);
        }
    }

    fn select(&self, names: impl IntoIterator<Item = String>) {
        self.0.selected.replace(names.into_iter().collect());
        self.show_selection();
    }

    /// 그 점에 있는 항목의 이름
    fn item_at(&self, x: f64, y: f64) -> Option<String> {
        let point = graphene::Point::new(x as f32, y as f32);
        self.0
            .items
            .borrow()
            .iter()
            .find(|item| {
                item.widget
                    .compute_bounds(&self.0.area)
                    .is_some_and(|bounds| bounds.contains_point(&point))
            })
            .map(|item| item.name.clone())
    }

    fn add_gestures(&self) {
        let area = &self.0.area;
        // 누르기: 고르기, 두 번 눌러 열기, 단축 메뉴
        let click = gtk::GestureClick::builder().button(0).build();
        let weak = Rc::downgrade(&self.0);
        click.connect_pressed(move |gesture, presses, x, y| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let desktop = Desktop(inner);
            desktop.0.area.grab_focus();
            let button = gesture.current_button();
            let state = gesture.current_event_state();
            let adding = state.intersects(
                gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::META_MASK
                    | gdk::ModifierType::SHIFT_MASK,
            );
            let hit = desktop.item_at(x, y);
            desktop.0.pressed_selected.replace(None);
            if button == gdk::BUTTON_SECONDARY {
                desktop.open_menu_at(x, y);
                return;
            }
            if button != gdk::BUTTON_PRIMARY {
                return;
            }
            match hit {
                Some(name) if presses >= 2 => {
                    desktop.select([name]);
                    desktop.open_selected();
                }
                Some(name) if adding => {
                    let mut selected = desktop.0.selected.borrow_mut();
                    if !selected.remove(&name) {
                        selected.insert(name);
                    }
                    drop(selected);
                    desktop.show_selection();
                }
                Some(name) => {
                    if desktop.0.selected.borrow().contains(&name) {
                        // 함께 끌 수 있게 그대로 두고, 끌지 않고 떼면 이 항목만 고른다
                        desktop.0.pressed_selected.replace(Some(name));
                    } else {
                        desktop.select([name]);
                    }
                }
                None if !adding => desktop.select([]),
                None => {}
            }
        });
        let weak = Rc::downgrade(&self.0);
        click.connect_released(move |_, _, _, _| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if let Some(name) = inner.pressed_selected.take() {
                Desktop(inner).select([name]);
            }
        });
        area.add_controller(click);

        // 손가락으로 길게 누르면 단축 메뉴를 연다. 터치 화면에는 오른쪽 단추가 없다(iPadOS처럼)
        let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
        let weak = Rc::downgrade(&self.0);
        long_press.connect_pressed(move |gesture, x, y| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // 누르는 동안 시작한 드래그해 여러 항목 선택은 그만둔다
            gesture.set_state(gtk::EventSequenceState::Claimed);
            inner.dragging_marquee.replace(None);
            inner.marquee.set_visible(false);
            inner.pressed_selected.replace(None);
            Desktop(inner).open_menu_at(x, y);
        });
        area.add_controller(long_press);

        // 빈 곳에서 드래그해 여러 항목 선택. 항목 위에서 시작한 드래그는 항목의 끌어 놓기에 넘긴다
        let drag = gtk::GestureDrag::new();
        let weak = Rc::downgrade(&self.0);
        drag.connect_drag_begin(move |gesture, x, y| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let desktop = Desktop(inner);
            if desktop.item_at(x, y).is_some() {
                gesture.set_state(gtk::EventSequenceState::Denied);
                return;
            }
            let adding = gesture.current_event_state().intersects(
                gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::META_MASK
                    | gdk::ModifierType::SHIFT_MASK,
            );
            let base = if adding {
                desktop.0.selected.borrow().clone()
            } else {
                BTreeSet::new()
            };
            desktop.0.dragging_marquee.replace(Some(Marquee {
                start: (x, y),
                base,
            }));
        });
        let weak = Rc::downgrade(&self.0);
        drag.connect_drag_update(move |_, dx, dy| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            Desktop(inner).update_marquee(dx, dy);
        });
        let weak = Rc::downgrade(&self.0);
        drag.connect_drag_end(move |_, _, _| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            inner.dragging_marquee.replace(None);
            inner.marquee.set_visible(false);
        });
        area.add_controller(drag);

        // 이 창의 항목을 데스크탑 안에 놓으면 그 칸으로 옮긴다. 다른 앱에서 끌어 온 파일은 아직 받지
        // 않는다(파일을 옮기거나 복사하는 일이라 따로 정한다)
        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let weak = Rc::downgrade(&self.0);
        drop.connect_accept(move |_, _| {
            weak.upgrade()
                .is_some_and(|inner| inner.dragging_items.borrow().is_some())
        });
        let weak = Rc::downgrade(&self.0);
        drop.connect_drop(move |_, _, x, y| {
            let Some(inner) = weak.upgrade() else {
                return false;
            };
            let Some((lead, moving)) = inner.dragging_items.take() else {
                return false;
            };
            let (width, height) = inner.size.get();
            let rows = rows_for(height);
            let target = slot_at((x, y), width, rows);
            let moved = move_slots(&inner.slots.borrow(), &moving, &lead, target, rows);
            inner.slots.replace(moved);
            let desktop = Desktop(inner);
            desktop.save_positions();
            desktop.place_items();
            true
        });
        area.add_controller(drop);
    }

    fn update_marquee(&self, dx: f64, dy: f64) {
        let inner = &self.0;
        let marquee = inner.dragging_marquee.borrow();
        let Some(Marquee { start, base }) = marquee.as_ref() else {
            return;
        };
        let rect = Rect::spanning(*start, (start.0 + dx, start.1 + dy));
        inner.marquee.set_margin_start(rect.x as i32);
        inner.marquee.set_margin_top(rect.y as i32);
        inner
            .marquee
            .set_size_request(rect.width.max(1.0) as i32, rect.height.max(1.0) as i32);
        inner.marquee.set_visible(true);
        let boxes: Vec<(String, Rect)> = inner
            .items
            .borrow()
            .iter()
            .filter_map(|item| {
                let bounds = item.widget.compute_bounds(&inner.area)?;
                Some((item.name.clone(), Rect::from(bounds)))
            })
            .collect();
        let selection = marquee_selection(base, &hits(rect, &boxes));
        drop(marquee);
        self.select(selection);
    }

    /// 항목을 끌면 고른 항목들을 파일 목록으로 건넨다. 끄는 그림은 그 항목이다
    fn add_drag_source(&self, widget: &gtk::Box, name: &str) {
        let source = gtk::DragSource::new();
        source.set_actions(gdk::DragAction::COPY);
        let weak = Rc::downgrade(&self.0);
        let name = name.to_owned();
        source.connect_prepare(move |source, x, y| {
            let inner = weak.upgrade()?;
            let desktop = Desktop(inner);
            desktop.0.pressed_selected.replace(None);
            if !desktop.0.selected.borrow().contains(&name) {
                desktop.select([name.clone()]);
            }
            let moving: Vec<String> = desktop.0.selected.borrow().iter().cloned().collect();
            let items = desktop.0.items.borrow();
            let files: Vec<gio::File> = items
                .iter()
                .filter(|item| moving.contains(&item.name))
                .map(|item| item.file.clone())
                .collect();
            if let Some(item) = items.iter().find(|item| item.name == name) {
                let paintable = gtk::WidgetPaintable::new(Some(&item.widget));
                source.set_icon(Some(&paintable), x as i32, y as i32);
            }
            drop(items);
            desktop
                .0
                .dragging_items
                .replace(Some((name.clone(), moving)));
            Some(gdk::ContentProvider::for_value(
                &gdk::FileList::from_array(&files).to_value(),
            ))
        });
        let weak = Rc::downgrade(&self.0);
        source.connect_drag_end(move |_, _, _| {
            if let Some(inner) = weak.upgrade() {
                inner.dragging_items.replace(None);
            }
        });
        widget.add_controller(source);
    }

    fn add_keys(&self) {
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&self.0);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if inner.renaming.borrow().is_some() {
                return glib::Propagation::Proceed;
            }
            let desktop = Desktop(inner);
            // 컴포지터는 글을 치지 않는 곳의 ⌘⌫·⌘↓를 그대로 보내고, ⌘A·⌘O·⇧⌘N은 앱처럼 Ctrl 단축키로 보낸다
            let command =
                state.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK);
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            match key {
                gdk::Key::Return | gdk::Key::KP_Enter if !command && !ctrl => {
                    desktop.rename_selected();
                }
                gdk::Key::BackSpace | gdk::Key::Delete if command => desktop.trash_selected(),
                gdk::Key::Down if command => desktop.open_selected(),
                gdk::Key::o | gdk::Key::O if ctrl => desktop.open_selected(),
                gdk::Key::a | gdk::Key::A if ctrl => {
                    let all: Vec<String> = desktop
                        .0
                        .items
                        .borrow()
                        .iter()
                        .map(|item| item.name.clone())
                        .collect();
                    desktop.select(all);
                }
                gdk::Key::n | gdk::Key::N if ctrl && shift => desktop.new_folder(),
                gdk::Key::Escape => desktop.select([]),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.0.window.add_controller(keys);
    }

    fn selected_items(&self) -> Vec<(String, gio::File)> {
        let selected = self.0.selected.borrow();
        self.0
            .items
            .borrow()
            .iter()
            .filter(|item| selected.contains(&item.name))
            .map(|item| (item.name.clone(), item.file.clone()))
            .collect()
    }

    /// 고른 항목을 기본 앱으로 연다 (폴더는 파일 앱)
    fn open_selected(&self) {
        for (_, file) in self.selected_items() {
            if let Err(err) =
                gio::AppInfo::launch_default_for_uri(&file.uri(), None::<&gio::AppLaunchContext>)
            {
                eprintln!("bunt-shell: cannot open {}: {err}", file.uri());
            }
        }
    }

    /// 고른 항목을 휴지통으로 옮긴다. 영구 삭제는 하지 않는다
    fn trash_selected(&self) {
        for (name, file) in self.selected_items() {
            if let Err(err) = file.trash(gio::Cancellable::NONE) {
                eprintln!("bunt-shell: cannot move {name} to the trash: {err}");
            }
        }
        self.refresh_soon();
    }

    /// 새로운 폴더를 만들고(이름은 "무제 폴더", 있으면 뒤에 수를 단다) 이름을 바꾸게 한다
    fn new_folder(&self) {
        let base = self.0.tr.get("desktop-untitled-folder");
        let name = (1..)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base} {n}")
                }
            })
            .find(|name| !self.0.dir.child(name).query_exists(gio::Cancellable::NONE))
            .unwrap_or(base);
        if let Err(err) = self
            .0
            .dir
            .child(&name)
            .make_directory(gio::Cancellable::NONE)
        {
            eprintln!("bunt-shell: cannot make the folder {name}: {err}");
            return;
        }
        self.refresh();
        self.select([name.clone()]);
        self.start_rename(&name);
    }

    /// 이름순으로 다시 놓는다 (macOS 단축 메뉴의 정리 기준 > 이름)
    fn sort_by_name(&self) {
        self.0.slots.borrow_mut().clear();
        self.place_items();
        self.save_positions();
    }

    fn rename_selected(&self) {
        let selected: Vec<String> = self.0.selected.borrow().iter().cloned().collect();
        if let [name] = selected.as_slice() {
            self.start_rename(name);
        }
    }

    /// 이름을 그 자리에서 고친다. Return·다른 곳 누르기는 바꾸고 Esc는 그만둔다. 처음에는 확장자를 뺀
    /// 이름을 고른다(macOS)
    fn start_rename(&self, name: &str) {
        self.end_rename(true);
        let items = self.0.items.borrow();
        let Some(item) = items.iter().find(|item| item.name == name) else {
            return;
        };
        let text = item.label.text().to_string();
        let entry = gtk::Entry::builder()
            .text(&text)
            .width_chars(12)
            .max_width_chars(16)
            .halign(gtk::Align::Center)
            .css_classes(["desktop-rename"])
            .build();
        entry.update_property(&[gtk::accessible::Property::Label(
            &self.0.tr.get("desktop-rename"),
        )]);
        item.label.set_visible(false);
        item.widget.append(&entry);
        drop(items);
        let weak = Rc::downgrade(&self.0);
        entry.connect_activate(move |_| {
            if let Some(inner) = weak.upgrade() {
                Desktop(inner).end_rename(true);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&self.0);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if let Some(inner) = weak.upgrade() {
                Desktop(inner).end_rename(false);
            }
            glib::Propagation::Stop
        });
        entry.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&self.0);
        focus.connect_leave(move |_| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // 포커스를 잃은 신호 안에서 위젯을 떼지 않는다
            glib::idle_add_local_once(move || Desktop(inner).end_rename(true));
        });
        entry.add_controller(focus);
        entry.grab_focus();
        let stem = stem_length(&text);
        entry.select_region(0, stem as i32);
        self.0.renaming.replace(Some((name.to_owned(), entry)));
    }

    /// 이름 바꾸기를 끝낸다. `commit`이면 고친 이름으로 바꾼다
    fn end_rename(&self, commit: bool) {
        let Some((name, entry)) = self.0.renaming.take() else {
            return;
        };
        let new_name = entry.text().trim().to_owned();
        let items = self.0.items.borrow();
        if let Some(item) = items.iter().find(|item| item.name == name) {
            item.widget.remove(&entry);
            item.label.set_visible(true);
            if commit && valid_new_name(&new_name, &item.label.text()) {
                match item
                    .file
                    .set_display_name(&new_name, gio::Cancellable::NONE)
                {
                    Ok(_) => {
                        let mut slots = self.0.slots.borrow_mut();
                        if let Some(slot) = slots.remove(&name) {
                            slots.insert(new_name.clone(), slot);
                        }
                        drop(slots);
                        self.save_positions();
                        let mut selected = self.0.selected.borrow_mut();
                        if selected.remove(&name) {
                            selected.insert(new_name);
                        }
                    }
                    Err(err) => eprintln!("bunt-shell: cannot rename {name}: {err}"),
                }
            }
        }
        drop(items);
        self.0.area.grab_focus();
        self.refresh_soon();
    }

    /// 그 자리의 단축 메뉴를 연다(오른쪽 단추, 길게 누르기). 고르지 않은 항목이면 그 항목만 고르고, 빈 곳이면 고른
    /// 것을 푼다
    fn open_menu_at(&self, x: f64, y: f64) {
        let hit = self.item_at(x, y);
        if let Some(name) = &hit
            && !self.0.selected.borrow().contains(name)
        {
            self.select([name.clone()]);
        } else if hit.is_none() {
            self.select([]);
        }
        self.show_menu(hit.is_some(), x, y);
    }

    /// 단축 메뉴. 항목 위면 그 항목들의 메뉴, 빈 곳이면 데스크탑 메뉴다. 동작 묶음은 메뉴에 단다(창에
    /// 달면 GTK가 창의 접근성 동작으로 내보낸다)
    fn show_menu(&self, on_item: bool, x: f64, y: f64) {
        let tr = &self.0.tr;
        let menu = gio::Menu::new();
        if on_item {
            menu.append(Some(&tr.get("desktop-open")), Some("desktop.open"));
            let edit = gio::Menu::new();
            edit.append(Some(&tr.get("desktop-rename")), Some("desktop.rename"));
            menu.append_section(None, &edit);
            let trash = gio::Menu::new();
            trash.append(
                Some(&tr.get("desktop-move-to-trash")),
                Some("desktop.trash"),
            );
            menu.append_section(None, &trash);
        } else {
            menu.append(
                Some(&tr.get("desktop-new-folder")),
                Some("desktop.new-folder"),
            );
            let arrange = gio::Menu::new();
            let by = gio::Menu::new();
            by.append(
                Some(&tr.get("desktop-sort-name")),
                Some("desktop.sort-by-name"),
            );
            arrange.append_submenu(Some(&tr.get("desktop-clean-up-by")), &by);
            menu.append_section(None, &arrange);
            let wallpaper = gio::Menu::new();
            wallpaper.append(
                Some(&tr.get("desktop-change-wallpaper")),
                Some("desktop.change-wallpaper"),
            );
            menu.append_section(None, &wallpaper);
        }
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.insert_action_group("desktop", Some(&self.actions()));
        popover.set_parent(&self.0.area);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        menu_names::follow(&popover);
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        popover.popup();
    }

    fn actions(&self) -> gio::SimpleActionGroup {
        let group = gio::SimpleActionGroup::new();
        let add = |name: &str, run: fn(&Desktop)| {
            let action = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(&self.0);
            action.connect_activate(move |_, _| {
                if let Some(inner) = weak.upgrade() {
                    run(&Desktop(inner));
                }
            });
            group.add_action(&action);
        };
        add("open", Desktop::open_selected);
        add("rename", Desktop::rename_selected);
        add("trash", Desktop::trash_selected);
        add("new-folder", Desktop::new_folder);
        add("sort-by-name", Desktop::sort_by_name);
        add("change-wallpaper", |_| {
            menubar::open_settings(Some("wallpaper"))
        });
        group
    }
}

/// 폴더의 항목 하나 (숨은 파일과 백업 파일은 뺀다)
struct Entry {
    name: String,
    display_name: String,
    icon: Option<gio::Icon>,
    /// 그림 파일이면 아이콘 대신 미리 보기를 만든다
    thumbnail: bool,
}

fn list_folder(dir: &gio::File) -> Vec<Entry> {
    let enumerator = match dir.enumerate_children(
        QUERY_ATTRIBUTES,
        gio::FileQueryInfoFlags::NONE,
        gio::Cancellable::NONE,
    ) {
        Ok(enumerator) => enumerator,
        Err(err) => {
            eprintln!("bunt-shell: cannot read the desktop folder: {err}");
            return Vec::new();
        }
    };
    let mut entries = Vec::new();
    while let Ok(Some(info)) = enumerator.next_file(gio::Cancellable::NONE) {
        if info.is_hidden() || info.is_backup() {
            continue;
        }
        let name = info.name().to_string_lossy().into_owned();
        let content_type = info.content_type().map(|kind| kind.to_string());
        let thumbnail = content_type
            .as_deref()
            .is_some_and(|kind| kind.starts_with("image/"))
            && info.size() <= THUMBNAIL_MAX_BYTES;
        entries.push(Entry {
            display_name: info.display_name().to_string(),
            icon: info.icon(),
            thumbnail,
            name,
        });
    }
    entries
}

/// 그림 파일의 미리 보기를 다른 스레드에서 줄여 읽어 아이콘 자리에 넣는다
fn load_thumbnail(file: &gio::File, image: &gtk::Image) {
    let Some(path) = file.path() else {
        return;
    };
    let image = image.downgrade();
    glib::spawn_future_local(async move {
        let size = ICON_SIZE * 2;
        // 풀어 줄인 픽셀만 넘긴다(그림 객체는 스레드를 건너지 못한다)
        let loaded = gio::spawn_blocking(move || {
            gdk::gdk_pixbuf::Pixbuf::from_file_at_scale(&path, size, size, true)
                .map(|pixbuf| {
                    (
                        pixbuf.width(),
                        pixbuf.height(),
                        pixbuf.rowstride(),
                        pixbuf.has_alpha(),
                        pixbuf.read_pixel_bytes(),
                    )
                })
                .map_err(|err| err.to_string())
        })
        .await;
        let Some(image) = image.upgrade() else {
            return;
        };
        match loaded {
            Ok(Ok((width, height, stride, alpha, pixels))) => {
                let format = if alpha {
                    gdk::MemoryFormat::R8g8b8a8
                } else {
                    gdk::MemoryFormat::R8g8b8
                };
                let texture =
                    gdk::MemoryTexture::new(width, height, format, &pixels, stride as usize);
                image.set_paintable(Some(&texture));
            }
            Ok(Err(err)) => eprintln!("bunt-shell: desktop thumbnail: {err}"),
            Err(_) => {}
        }
    });
}

/// 데스크탑 폴더: xdg-user-dirs의 DESKTOP(컴포지터가 스크린샷을 저장하는 곳과 같다), 없으면 ~/Desktop
fn desktop_dir() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Desktop)
        .unwrap_or_else(|| glib::home_dir().join("Desktop"))
}

/// 칸 자리를 적는 파일: 상태 폴더(XDG_STATE_HOME, 없으면 ~/.local/state)의 buntos/desktop-positions
fn positions_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".local/state"))
        .join("buntos/desktop-positions")
}

/// 화면 높이에 드는 행 수 (적어도 하나)
fn rows_for(height: i32) -> u32 {
    ((height - MARGIN_TOP - MARGIN_BOTTOM) / SLOT_SIZE.1).max(1) as u32
}

/// 칸의 왼쪽 위 자리 (창 좌표)
fn slot_origin((column, row): Slot, width: i32) -> (i32, i32) {
    (
        width - MARGIN_RIGHT - (column as i32 + 1) * SLOT_SIZE.0,
        MARGIN_TOP + row as i32 * SLOT_SIZE.1,
    )
}

/// 그 점이 든 칸. 화면 밖이면 가장 가까운 칸이다
fn slot_at((x, y): (f64, f64), width: i32, rows: u32) -> Slot {
    let column = ((f64::from(width - MARGIN_RIGHT) - x) / f64::from(SLOT_SIZE.0))
        .floor()
        .max(0.0) as u32;
    let row = ((y - f64::from(MARGIN_TOP)) / f64::from(SLOT_SIZE.1))
        .floor()
        .clamp(0.0, f64::from(rows.saturating_sub(1))) as u32;
    (column, row)
}

/// 이름마다 칸을 정한다. 적어 둔 칸이 화면 안이고 아직 비어 있으면 그대로 쓰고, 나머지는 오른쪽 열부터
/// 위에서 아래로 빈 칸을 차례로 채운다(macOS가 새 항목을 놓는 차례). `names`의 차례가 채우는 차례다
fn assign_slots(
    names: &[String],
    saved: &HashMap<String, Slot>,
    rows: u32,
) -> HashMap<String, Slot> {
    let mut taken = HashSet::new();
    let mut slots = HashMap::new();
    for name in names {
        if let Some(&slot) = saved.get(name)
            && slot.1 < rows
            && taken.insert(slot)
        {
            slots.insert(name.clone(), slot);
        }
    }
    let mut free = free_slots(rows, taken.clone());
    for name in names {
        if !slots.contains_key(name)
            && let Some(slot) = free.next()
        {
            taken.insert(slot);
            slots.insert(name.clone(), slot);
        }
    }
    slots
}

/// 오른쪽 열부터 위에서 아래로, `taken`에 없는 칸
fn free_slots(rows: u32, taken: HashSet<Slot>) -> impl Iterator<Item = Slot> {
    (0..u32::MAX)
        .flat_map(move |column| (0..rows).map(move |row| (column, row)))
        .filter(move |slot| !taken.contains(slot))
}

/// 끈 항목들을 놓을 칸. 끈 항목(`lead`)이 `target`에 오도록 함께 끈 항목도 같은 만큼 옮기고, 화면 밖이거나
/// 다른 항목이 있는 칸이면 가장 가까운 빈 칸에 놓는다
fn move_slots(
    slots: &HashMap<String, Slot>,
    moving: &[String],
    lead: &str,
    target: Slot,
    rows: u32,
) -> HashMap<String, Slot> {
    let Some(&from) = slots.get(lead) else {
        return slots.clone();
    };
    let shift = (
        i64::from(target.0) - i64::from(from.0),
        i64::from(target.1) - i64::from(from.1),
    );
    let mut placed: HashMap<String, Slot> = slots
        .iter()
        .filter(|(name, _)| !moving.contains(name))
        .map(|(name, slot)| (name.clone(), *slot))
        .collect();
    let mut taken: HashSet<Slot> = placed.values().copied().collect();
    // 끈 항목을 먼저 놓아 원하는 칸을 먼저 갖게 한다
    let order =
        std::iter::once(lead.to_owned()).chain(moving.iter().filter(|name| *name != lead).cloned());
    for name in order {
        let Some(&slot) = slots.get(&name) else {
            continue;
        };
        let wanted = (
            (i64::from(slot.0) + shift.0).max(0) as u32,
            (i64::from(slot.1) + shift.1).clamp(0, i64::from(rows.saturating_sub(1))) as u32,
        );
        let slot = nearest_free(wanted, &taken, rows);
        taken.insert(slot);
        placed.insert(name, slot);
    }
    placed
}

/// `wanted`에서 가장 가까운 빈 칸 (가로·세로 거리의 합으로 재고, 같으면 위·오른쪽이 먼저)
fn nearest_free(wanted: Slot, taken: &HashSet<Slot>, rows: u32) -> Slot {
    if !taken.contains(&wanted) {
        return wanted;
    }
    let mut best: Option<(u64, Slot)> = None;
    let reach = taken.len() as u32 + 2;
    for column in wanted.0.saturating_sub(reach)..=wanted.0 + reach {
        for row in 0..rows {
            let slot = (column, row);
            if taken.contains(&slot) {
                continue;
            }
            let distance = u64::from(column.abs_diff(wanted.0)) + u64::from(row.abs_diff(wanted.1));
            if best.is_none_or(|(d, s)| (distance, slot) < (d, s)) {
                best = Some((distance, slot));
            }
        }
    }
    best.map_or(wanted, |(_, slot)| slot)
}

/// 창 좌표의 사각형
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    /// 두 점을 꼭짓점으로 하는 사각형 (드래그한 방향과 상관없다)
    fn spanning(a: (f64, f64), b: (f64, f64)) -> Self {
        Self {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            width: (a.0 - b.0).abs(),
            height: (a.1 - b.1).abs(),
        }
    }

    fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }
}

impl From<graphene::Rect> for Rect {
    fn from(rect: graphene::Rect) -> Self {
        Self {
            x: f64::from(rect.x()),
            y: f64::from(rect.y()),
            width: f64::from(rect.width()),
            height: f64::from(rect.height()),
        }
    }
}

/// 사각형에 걸친 항목의 이름
fn hits(rect: Rect, boxes: &[(String, Rect)]) -> BTreeSet<String> {
    boxes
        .iter()
        .filter(|(_, item)| rect.intersects(item))
        .map(|(name, _)| name.clone())
        .collect()
}

/// 드래그해 여러 항목 선택의 결과: 시작 때 골라 둔 것에서 사각형 안의 항목을 뒤집는다(⌘·⇧로 시작하지
/// 않았으면 시작 때 골라 둔 것이 비어 있어 사각형 안의 항목만 남는다)
fn marquee_selection(base: &BTreeSet<String>, inside: &BTreeSet<String>) -> BTreeSet<String> {
    base.symmetric_difference(inside).cloned().collect()
}

/// 이름 바꾸기에서 처음 고를 길이: 확장자 앞까지(폴더나 확장자가 없으면 전부). 글자 수로 센다
fn stem_length(name: &str) -> usize {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].chars().count(),
        _ => name.chars().count(),
    }
}

/// 고친 이름으로 바꿀지: 비지 않고, 바뀌었고, 경로 구분자가 없다
fn valid_new_name(new: &str, old: &str) -> bool {
    !new.is_empty() && new != old && !new.contains('/') && new != "." && new != ".."
}

/// 칸 자리 파일: 한 줄에 "열\t행\t이름"
fn parse_positions(text: &str) -> HashMap<String, Slot> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let column = fields.next()?.parse().ok()?;
            let row = fields.next()?.parse().ok()?;
            let name = fields.next().filter(|name| !name.is_empty())?;
            Some((name.to_owned(), (column, row)))
        })
        .collect()
}

fn format_positions(slots: &HashMap<String, Slot>) -> String {
    let mut lines: Vec<String> = slots
        .iter()
        .filter(|(name, _)| !name.contains('\n'))
        .map(|(name, (column, row))| format!("{column}\t{row}\t{name}"))
        .collect();
    lines.sort();
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// 이름 차례: 대소문자를 가리지 않고, 수는 크기대로 (Finder처럼 "2"가 "10" 앞이다)
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn chunks(text: &str) -> Vec<(bool, String)> {
        let mut chunks: Vec<(bool, String)> = Vec::new();
        for ch in text.chars() {
            let digit = ch.is_ascii_digit();
            match chunks.last_mut() {
                Some((was_digit, chunk)) if *was_digit == digit => chunk.push(ch),
                _ => chunks.push((digit, ch.to_string())),
            }
        }
        chunks
    }
    let (left, right) = (chunks(a), chunks(b));
    for ((a_digit, a_chunk), (b_digit, b_chunk)) in left.iter().zip(right.iter()) {
        let order = if *a_digit && *b_digit {
            let (a_trim, b_trim) = (
                a_chunk.trim_start_matches('0'),
                b_chunk.trim_start_matches('0'),
            );
            a_trim
                .len()
                .cmp(&b_trim.len())
                .then_with(|| a_trim.cmp(b_trim))
        } else {
            a_chunk.to_lowercase().cmp(&b_chunk.to_lowercase())
        };
        if order != std::cmp::Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn slots_fill_columns_from_the_top_right() {
        let slots = assign_slots(&names(&["a", "b", "c"]), &HashMap::new(), 2);
        assert_eq!(slots["a"], (0, 0));
        assert_eq!(slots["b"], (0, 1));
        // 두 행이면 셋째는 다음(왼쪽) 열의 맨 위
        assert_eq!(slots["c"], (1, 0));
        // 오른쪽 위 칸의 자리: 오른쪽 여백 안쪽 끝에서 한 칸 너비만큼
        assert_eq!(
            slot_origin((0, 0), 1280),
            (1280 - MARGIN_RIGHT - SLOT_SIZE.0, MARGIN_TOP)
        );
        assert_eq!(
            slot_origin((1, 1), 1280).0,
            1280 - MARGIN_RIGHT - 2 * SLOT_SIZE.0
        );
    }

    #[test]
    fn saved_slots_stay_and_new_items_take_free_ones() {
        let saved = HashMap::from([("b".to_owned(), (0, 0)), ("gone".to_owned(), (3, 3))]);
        let slots = assign_slots(&names(&["a", "b"]), &saved, 4);
        assert_eq!(slots["b"], (0, 0));
        assert_eq!(slots["a"], (0, 1));
        assert!(!slots.contains_key("gone"));
        // 화면이 낮아져 적어 둔 행이 없으면 빈 칸으로 옮긴다
        let saved = HashMap::from([("a".to_owned(), (0, 5))]);
        assert_eq!(assign_slots(&names(&["a"]), &saved, 3)["a"], (0, 0));
        // 같은 칸을 둘이 적었으면 앞의 것이 갖는다
        let saved = HashMap::from([("a".to_owned(), (0, 0)), ("b".to_owned(), (0, 0))]);
        let slots = assign_slots(&names(&["a", "b"]), &saved, 3);
        assert_eq!((slots["a"], slots["b"]), ((0, 0), (0, 1)));
    }

    #[test]
    fn rows_and_slot_under_a_point() {
        assert_eq!(
            rows_for(800),
            ((800 - MARGIN_TOP - MARGIN_BOTTOM) / SLOT_SIZE.1) as u32
        );
        assert_eq!(rows_for(10), 1);
        let width = 1280;
        let (x, y) = slot_origin((2, 3), width);
        assert_eq!(
            slot_at((f64::from(x) + 5.0, f64::from(y) + 5.0), width, 6),
            (2, 3)
        );
        // 화면 밖은 가장 가까운 칸
        assert_eq!(slot_at((5000.0, -50.0), width, 6), (0, 0));
        assert_eq!(slot_at((f64::from(x), 99999.0), width, 6).1, 5);
    }

    #[test]
    fn moving_keeps_the_group_shape_and_avoids_others() {
        let slots = HashMap::from([
            ("a".to_owned(), (0, 0)),
            ("b".to_owned(), (0, 1)),
            ("c".to_owned(), (2, 2)),
        ]);
        // a와 b를 함께 끌어 a를 (1, 1)에 놓으면 b는 (1, 2)
        let moved = move_slots(&slots, &names(&["a", "b"]), "a", (1, 1), 5);
        assert_eq!(
            (moved["a"], moved["b"], moved["c"]),
            ((1, 1), (1, 2), (2, 2))
        );
        // 다른 항목이 있는 칸에 놓으면 가장 가까운 빈 칸
        let moved = move_slots(&slots, &names(&["a"]), "a", (2, 2), 5);
        assert_eq!(moved["c"], (2, 2));
        let distance = moved["a"].0.abs_diff(2) + moved["a"].1.abs_diff(2);
        assert_eq!(distance, 1);
    }

    #[test]
    fn marquee_picks_items_it_touches() {
        let boxes = vec![
            (
                "a".to_owned(),
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            ),
            (
                "b".to_owned(),
                Rect {
                    x: 0.0,
                    y: 200.0,
                    width: 100.0,
                    height: 100.0,
                },
            ),
        ];
        // 아래에서 위로 끌어도 같은 사각형
        let rect = Rect::spanning((150.0, 150.0), (50.0, 50.0));
        assert_eq!(hits(rect, &boxes), BTreeSet::from(["a".to_owned()]));
        // ⌘로 시작하면 사각형 안의 것을 뒤집는다
        let base = BTreeSet::from(["a".to_owned(), "b".to_owned()]);
        let inside = BTreeSet::from(["a".to_owned()]);
        assert_eq!(
            marquee_selection(&base, &inside),
            BTreeSet::from(["b".to_owned()])
        );
        assert_eq!(marquee_selection(&BTreeSet::new(), &inside), inside);
    }

    #[test]
    fn rename_selects_the_name_before_the_extension() {
        assert_eq!(
            stem_length("스크린샷 2026-10-31.png"),
            "스크린샷 2026-10-31".chars().count()
        );
        assert_eq!(stem_length("무제 폴더"), 5);
        assert_eq!(stem_length(".bashrc"), 7);
        assert!(valid_new_name("새 이름", "옛 이름"));
        assert!(!valid_new_name("", "a"));
        assert!(!valid_new_name("a", "a"));
        assert!(!valid_new_name("a/b", "a"));
    }

    #[test]
    fn positions_round_trip() {
        let slots = HashMap::from([
            ("메모 1.txt".to_owned(), (0, 2)),
            ("a\tb".to_owned(), (3, 0)),
        ]);
        assert_eq!(parse_positions(&format_positions(&slots)), slots);
        assert!(parse_positions("x\t1\tname\n1\t2\t\n").is_empty());
    }

    #[test]
    fn names_sort_like_finder() {
        let mut list = names(&["파일 10", "파일 2", "B", "a", "파일 1"]);
        list.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(list, names(&["a", "B", "파일 1", "파일 2", "파일 10"]));
    }
}
