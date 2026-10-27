// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 모든 앱 (macOS 26의 응용 프로그램 창). Dock의 앱 단추를 누르면 화면 가운데에 유리 패널로
//! 뜬다. 위에서부터 검색 칸, 분류 칩, 최근에 쓴 앱, 모든 앱 격자다. 이름으로 찾고(Enter는 첫
//! 결과), 분류로 거르고, 눌러서 실행한다. 이미 실행 중인 앱은 창을 앞으로 가져온다.
//! 패널 바깥을 누르거나 Esc를 누르면 닫힌다(검색어가 있으면 Esc는 먼저 검색어를 지운다).

use std::{cell::RefCell, rc::Rc, time::Duration};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{
    dock::{activate_or_launch, app_button_content},
    shell_settings,
    toplevels::Toplevels,
};

/// Dock 단추의 아이콘 (shell/data/icons)
const ICON: &str = "org.buntos.Apps";
const TILE_ICON_SIZE: i32 = 64;
const COLUMNS: u32 = 7;
/// 최근에 쓴 앱 줄에 놓는 수 (한 줄)
const RECENT_COUNT: usize = 7;
/// 화면이 충분히 크면 쓰는 패널 크기. 작은 화면에서는 화면에 맞춘다
const PANEL_WIDTH: i32 = 860;
const PANEL_HEIGHT: i32 = 620;
/// 닫힐 때 사라지는 시간 (style.css의 launcher-panel transition과 같다)
const CLOSE_DELAY: Duration = Duration::from_millis(140);
/// org.buntos.shell 키: 최근에 실행한 앱의 .desktop ID, 최근 것이 먼저
const RECENT_KEY: &str = "recent-apps";

/// 앱 분류 (macOS 응용 프로그램 창의 분류 칩). .desktop 파일의 Categories에서 정한다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Utilities,
    Developer,
    Productivity,
    Creativity,
    Internet,
    Entertainment,
    Games,
    Education,
    Other,
}

impl Category {
    /// 칩을 놓는 순서
    const ALL: [Category; 9] = [
        Category::Utilities,
        Category::Developer,
        Category::Productivity,
        Category::Creativity,
        Category::Internet,
        Category::Entertainment,
        Category::Games,
        Category::Education,
        Category::Other,
    ];

    /// freedesktop 메뉴 명세의 주 분류에서 고른다. 여럿이면 앞의 규칙이 이긴다
    fn of(categories: &str) -> Self {
        let has = |name: &str| categories.split(';').any(|category| category == name);
        if has("Game") {
            Self::Games
        } else if has("Development") {
            Self::Developer
        } else if has("Graphics") {
            Self::Creativity
        } else if has("AudioVideo") || has("Audio") || has("Video") {
            Self::Entertainment
        } else if has("Office") {
            Self::Productivity
        } else if has("Network") {
            Self::Internet
        } else if has("Education") || has("Science") {
            Self::Education
        } else if has("Utility") || has("System") || has("Settings") {
            Self::Utilities
        } else {
            Self::Other
        }
    }

    fn label_id(self) -> &'static str {
        match self {
            Self::Utilities => "launcher-category-utilities",
            Self::Developer => "launcher-category-developer",
            Self::Productivity => "launcher-category-productivity",
            Self::Creativity => "launcher-category-creativity",
            Self::Internet => "launcher-category-internet",
            Self::Entertainment => "launcher-category-entertainment",
            Self::Games => "launcher-category-games",
            Self::Education => "launcher-category-education",
            Self::Other => "launcher-category-other",
        }
    }
}

/// 목록에 보이는 앱 하나
struct App {
    info: gio::AppInfo,
    /// .desktop 파일 ID
    id: String,
    category: Category,
    /// 검색에 쓰는 글자: 이름, 영문 이름, 키워드, 실행 파일 (소문자)
    key: String,
}

/// 설치된 앱을 이름순으로
fn installed_apps() -> Vec<App> {
    let mut apps: Vec<App> = gio::AppInfo::all()
        .into_iter()
        .filter(|info| info.should_show())
        .filter_map(|info| {
            let id = info.id()?.to_string();
            let desktop = info.downcast_ref::<gio_unix::DesktopAppInfo>();
            let category = Category::of(
                &desktop
                    .and_then(|desktop| desktop.categories())
                    .unwrap_or_default(),
            );
            let key = search_key(&info, desktop);
            Some(App {
                info,
                id,
                category,
                key,
            })
        })
        .collect();
    apps.sort_by_cached_key(|app| app.info.display_name().to_lowercase());
    apps
}

fn search_key(info: &gio::AppInfo, desktop: Option<&gio_unix::DesktopAppInfo>) -> String {
    let mut key = format!("{} {}", info.display_name(), info.name());
    if let Some(desktop) = desktop {
        for keyword in desktop.keywords() {
            key.push(' ');
            key.push_str(&keyword);
        }
    }
    key.push(' ');
    key.push_str(&info.executable().to_string_lossy());
    key.to_lowercase()
}

/// 실행한 앱을 최근 목록 맨 앞에 둔다. Dock에서 실행한 것도 센다
pub fn remember(info: &impl IsA<gio::AppInfo>) {
    let (Some(settings), Some(id)) = (shell_settings(), info.as_ref().id()) else {
        return;
    };
    let mut recent: Vec<String> = settings
        .strv(RECENT_KEY)
        .iter()
        .map(|id| id.to_string())
        .collect();
    recent.retain(|other| other.as_str() != id.as_str());
    recent.insert(0, id.to_string());
    recent.truncate(RECENT_COUNT * 2);
    if let Err(err) = settings.set_strv(RECENT_KEY, recent) {
        eprintln!("bunt-shell: failed to save recent apps: {err}");
    }
}

fn recent_ids() -> Vec<String> {
    shell_settings()
        .map(|settings| {
            settings
                .strv(RECENT_KEY)
                .iter()
                .map(|id| id.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// 화면 전체를 덮는 투명한 오버레이 창과 그 가운데의 패널. 복제해도 같은 창을 가리킨다
#[derive(Clone)]
pub struct Launcher(Rc<Inner>);

struct Inner {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    tr: Rc<Translations>,
    toplevels: Option<Toplevels>,
    /// 지금 패널의 검색 칸 (열 때마다 새로 만든다)
    search: RefCell<Option<gtk::SearchEntry>>,
}

impl Launcher {
    pub fn new(
        app: &gtk::Application,
        tr: &Rc<Translations>,
        toplevels: Option<&Toplevels>,
    ) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .css_classes(["bunt-launcher"])
            .build();
        window.init_layer_shell();
        window.set_namespace(Some("bunt-launcher"));
        window.set_layer(Layer::Overlay);
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            window.set_anchor(edge, true);
        }
        // 메뉴바와 Dock까지 덮어, 패널 바깥 어디를 눌러도 닫히게 한다
        window.set_exclusive_zone(-1);
        window.set_keyboard_mode(KeyboardMode::Exclusive);

        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["launcher-panel"])
            .build();
        window.set_child(Some(&panel));

        let launcher = Self(Rc::new(Inner {
            window,
            panel,
            tr: tr.clone(),
            toplevels: toplevels.cloned(),
            search: RefCell::new(None),
        }));

        // 패널 바깥을 누르면 닫는다. 안쪽 누르기는 그대로 패널로 간다
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&launcher.0);
        click.connect_pressed(move |gesture, _, x, y| {
            let (Some(inner), Some(root)) = (weak.upgrade(), gesture.widget()) else {
                return;
            };
            let inside = root
                .pick(x, y, gtk::PickFlags::DEFAULT)
                .is_some_and(|picked| {
                    &picked == inner.panel.upcast_ref::<gtk::Widget>()
                        || picked.is_ancestor(&inner.panel)
                });
            if !inside {
                Launcher(inner).hide();
            }
        });
        launcher.0.window.add_controller(click);

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&launcher.0);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            let search = inner.search.borrow().clone();
            match search {
                Some(search) if !search.text().is_empty() => search.set_text(""),
                _ => Launcher(inner).hide(),
            }
            glib::Propagation::Stop
        });
        launcher.0.window.add_controller(keys);
        launcher
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    pub fn toggle(&self) {
        if self.0.window.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    pub fn show(&self) {
        // 열 때마다 앱 목록을 새로 읽는다 (그사이 설치·삭제된 앱 반영)
        self.fill();
        self.fit_to_monitor();
        self.0.window.present();
        if let Some(search) = self.0.search.borrow().as_ref() {
            search.grab_focus();
        }
        // 그려진 다음에 클래스를 붙여야 나타나는 전환이 보인다
        let panel = self.0.panel.clone();
        glib::timeout_add_local_once(Duration::from_millis(16), move || {
            panel.add_css_class("shown");
        });
    }

    pub fn hide(&self) {
        if !self.0.window.is_visible() || !self.0.panel.has_css_class("shown") {
            return;
        }
        self.0.panel.remove_css_class("shown");
        let window = self.0.window.clone();
        glib::timeout_add_local_once(CLOSE_DELAY, move || window.set_visible(false));
    }

    /// 화면이 작으면 패널을 화면에 맞춘다 (가운데에 두어도 메뉴바와 Dock에 닿지 않게 남긴다)
    fn fit_to_monitor(&self) {
        let monitor = WidgetExt::display(&self.0.window)
            .monitors()
            .item(0)
            .and_downcast::<gdk::Monitor>();
        let (width, height) = match monitor {
            Some(monitor) => {
                let geometry = monitor.geometry();
                (
                    PANEL_WIDTH.min(geometry.width() - 80),
                    PANEL_HEIGHT.min(geometry.height() - 240),
                )
            }
            None => (PANEL_WIDTH, PANEL_HEIGHT),
        };
        self.0.panel.set_size_request(width, height);
    }

    /// 앱을 연다: 실행 중이면 창을 앞으로, 아니면 실행
    fn open(&self, app: &App) {
        activate_or_launch(&app.info, self.0.toplevels.as_ref(), &self.0.window);
        self.hide();
    }

    fn fill(&self) {
        let inner = &self.0;
        let tr = &inner.tr;
        while let Some(child) = inner.panel.first_child() {
            inner.panel.remove(&child);
        }
        let apps = Rc::new(installed_apps());

        // 검색 칸: 앱 아이콘과 큰 글씨 (macOS 응용 프로그램 창의 머리)
        let search = gtk::SearchEntry::builder()
            .placeholder_text(tr.get("launcher-search"))
            .hexpand(true)
            .css_classes(["launcher-search"])
            .build();
        // 패널 어디에 포커스가 있든 글자를 치면 검색 칸으로 간다
        search.set_key_capture_widget(Some(&inner.window));
        let header = gtk::Box::builder()
            .spacing(10)
            .css_classes(["launcher-header"])
            .build();
        let icon = gtk::Image::from_icon_name(ICON);
        icon.set_pixel_size(24);
        header.append(&icon);
        header.append(&search);
        inner.panel.append(&header);
        inner.panel.append(&separator());

        // 분류 칩: 앱이 있는 분류만. 고른 칩을 다시 누르면 모두 보인다
        let category: Rc<RefCell<Option<Category>>> = Rc::default();
        let chips = gtk::Box::builder()
            .spacing(6)
            .css_classes(["launcher-chips"])
            .build();
        let mut chip_buttons = Vec::new();
        for kind in Category::ALL {
            if !apps.iter().any(|app| app.category == kind) {
                continue;
            }
            let chip = gtk::ToggleButton::builder()
                .label(tr.get(kind.label_id()))
                .css_classes(["launcher-chip"])
                .build();
            chips.append(&chip);
            chip_buttons.push((kind, chip));
        }
        let chip_scroller = gtk::ScrolledWindow::builder()
            .child(&chips)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::External)
            .build();
        inner.panel.append(&chip_scroller);
        inner.panel.append(&separator());

        // 최근에 쓴 앱 한 줄과 모든 앱
        let recent: Vec<usize> = recent_ids()
            .iter()
            .filter_map(|id| apps.iter().position(|app| &app.id == id))
            .take(RECENT_COUNT)
            .collect();
        let recent_grid = grid();
        for &index in &recent {
            recent_grid.append(&tile(&apps[index]));
        }
        let recent_line = separator();
        let all_grid = grid();
        for app in apps.iter() {
            all_grid.append(&tile(app));
        }
        let empty = gtk::Label::builder()
            .label(tr.get("launcher-empty"))
            .vexpand(true)
            .visible(false)
            .css_classes(["launcher-empty"])
            .build();
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        body.append(&recent_grid);
        body.append(&recent_line);
        body.append(&all_grid);
        body.append(&empty);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&body)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .css_classes(["launcher-scroller"])
            .build();
        inner.panel.append(&scroller);

        // 거르기: 검색어와 고른 분류
        let query: Rc<RefCell<String>> = Rc::default();
        let matches = {
            let (apps, query, category) = (apps.clone(), query.clone(), category.clone());
            Rc::new(move |index: usize| {
                let Some(app) = apps.get(index) else {
                    return false;
                };
                let query = query.borrow();
                category.borrow().is_none_or(|kind| app.category == kind)
                    && app.key.contains(query.as_str())
            })
        };
        let filter = matches.clone();
        all_grid.set_filter_func(move |child| {
            usize::try_from(child.index()).is_ok_and(|index| filter(index))
        });
        let refresh = {
            let (apps, query, category, matches) = (
                apps.clone(),
                query.clone(),
                category.clone(),
                matches.clone(),
            );
            let (all_grid, recent_grid, recent_line, empty) = (
                all_grid.clone(),
                recent_grid.clone(),
                recent_line.clone(),
                empty.clone(),
            );
            let has_recent = !recent.is_empty();
            Rc::new(move || {
                all_grid.invalidate_filter();
                // 최근 줄은 거르지 않을 때만 보인다
                let browsing = query.borrow().is_empty() && category.borrow().is_none();
                recent_grid.set_visible(browsing && has_recent);
                recent_line.set_visible(browsing && has_recent);
                let any = (0..apps.len()).any(|index| matches(index));
                all_grid.set_visible(any);
                empty.set_visible(!any);
            })
        };
        refresh();

        let changed = refresh.clone();
        let typed = query.clone();
        search.connect_search_changed(move |search| {
            *typed.borrow_mut() = search.text().trim().to_lowercase();
            changed();
        });
        // 칩은 하나만 고른다. 고른 칩을 다시 누르면 고르지 않은 상태로 돌아간다
        let all_chips = chips.downgrade();
        for (kind, chip) in chip_buttons {
            let (category, refresh, all_chips) =
                (category.clone(), refresh.clone(), all_chips.clone());
            chip.connect_toggled(move |chip| {
                if chip.is_active() {
                    category.replace(Some(kind));
                    let mut other = all_chips.upgrade().and_then(|chips| chips.first_child());
                    while let Some(widget) = other {
                        other = widget.next_sibling();
                        if let Some(button) = widget.downcast_ref::<gtk::ToggleButton>()
                            && button != chip
                        {
                            button.set_active(false);
                        }
                    }
                } else if *category.borrow() == Some(kind) {
                    category.replace(None);
                }
                refresh();
            });
        }

        // 누르기와 Enter: 격자 칸의 순서가 apps의 순서다
        let this = self.clone();
        let opened = apps.clone();
        all_grid.connect_child_activated(move |_, child| {
            if let Some(app) = usize::try_from(child.index())
                .ok()
                .and_then(|index| opened.get(index))
            {
                this.open(app);
            }
        });
        let this = self.clone();
        let opened = apps.clone();
        recent_grid.connect_child_activated(move |_, child| {
            if let Some(app) = usize::try_from(child.index())
                .ok()
                .and_then(|position| recent.get(position))
                .and_then(|&index| opened.get(index))
            {
                this.open(app);
            }
        });
        // 검색 칸의 Enter는 보이는 첫 앱을 연다
        let this = self.clone();
        search.connect_activate(move |_| {
            if let Some(app) = (0..apps.len())
                .find(|&index| matches(index))
                .and_then(|index| apps.get(index))
            {
                this.open(app);
            }
        });
        inner.search.replace(Some(search));
    }
}

fn separator() -> gtk::Separator {
    gtk::Separator::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["launcher-separator"])
        .build()
}

fn grid() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(COLUMNS)
        .max_children_per_line(COLUMNS)
        .row_spacing(4)
        .column_spacing(4)
        .valign(gtk::Align::Start)
        .css_classes(["launcher-grid"])
        .build()
}

/// 앱 칸: 아이콘과 두 줄까지의 이름
fn tile(app: &App) -> gtk::FlowBoxChild {
    let icon = match app.info.icon() {
        Some(icon) => gtk::Image::from_gicon(&icon),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    icon.set_pixel_size(TILE_ICON_SIZE);
    let label = gtk::Label::builder()
        .label(app.info.display_name())
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .justify(gtk::Justification::Center)
        .max_width_chars(12)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    content.append(&icon);
    content.append(&label);
    gtk::FlowBoxChild::builder()
        .child(&content)
        .tooltip_text(app.info.display_name())
        .css_classes(["launcher-tile"])
        .build()
}

/// Dock의 앱 단추. 누르면 모든 앱 패널을 열고 닫는다
pub fn dock_button(launcher: &Launcher, tooltip: &str, icon_size: i32) -> gtk::Button {
    let image = gtk::Image::from_icon_name(ICON);
    image.set_pixel_size(icon_size);
    // 실행 중 표시 점 자리를 비워 다른 Dock 단추와 높이를 맞춘다
    let (content, _dot) = app_button_content(&image);
    let button = gtk::Button::builder()
        .child(&content)
        .tooltip_text(tooltip)
        .css_classes(["dock-item"])
        .build();
    let launcher = launcher.clone();
    button.connect_clicked(move |_| launcher.toggle());
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_follow_the_main_category() {
        assert_eq!(
            Category::of("GNOME;GTK;Utility;TextEditor;"),
            Category::Utilities
        );
        assert_eq!(
            Category::of("System;TerminalEmulator;"),
            Category::Utilities
        );
        assert_eq!(
            Category::of("GNOME;GTK;Graphics;Viewer;"),
            Category::Creativity
        );
        assert_eq!(
            Category::of("GNOME;GTK;Office;Viewer;"),
            Category::Productivity
        );
        assert_eq!(Category::of("Development;IDE;"), Category::Developer);
        assert_eq!(Category::of("Game;Utility;"), Category::Games);
        assert_eq!(Category::of(""), Category::Other);
    }
}
