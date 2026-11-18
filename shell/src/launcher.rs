// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 모든 앱 (macOS 26의 응용 프로그램 창). Dock의 앱 단추를 누르면 화면 가운데에 유리 패널로
//! 뜬다. 위에서부터 검색 칸, 분류 칩, 최근에 쓴 앱, 모든 앱 격자다. 이름으로 찾고(Enter는 첫
//! 결과), 분류로 거르고, 눌러서 실행한다. 이미 실행 중인 앱은 창을 앞으로 가져온다.
//! 패널 바깥을 누르거나 Esc를 누르면 닫힌다(검색어가 있으면 Esc는 먼저 검색어를 지운다).
//! 검색 칸 오른쪽의 단추나 ⌘4는 클립보드 기록(clipboard_panel.rs)으로 넘어간다. Spotlight의 클립보드 단추와 같다.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{
    clipboard::ClipboardHistory,
    clipboard_panel::{CLIPBOARD_ICON, ClipboardPanel},
    dock::{activate_or_launch, app_button_content},
    input_mode::TOUCH_CLASS,
    shell_settings,
    toplevels::Toplevels,
};

/// Dock 단추의 아이콘 (shell/data/icons)
const ICON: &str = "org.buntos.Apps";
/// 패널 머리의 모든 앱 단추
const APPS_MODE_ICON: &str = "view-app-grid-symbolic";
const TILE_ICON_SIZE: i32 = 64;
const COLUMNS: u32 = 7;
/// 최근에 쓴 앱 줄에 놓는 수 (한 줄)
const RECENT_COUNT: usize = 7;
/// 화면이 충분히 크면 쓰는 패널 크기. 작은 화면에서는 화면에 맞춘다
const PANEL_WIDTH: i32 = 860;
const PANEL_HEIGHT: i32 = 620;
/// 닫힐 때 사라지는 시간 (style.css의 launcher-panel transition과 같다)
pub const CLOSE_DELAY: Duration = Duration::from_millis(140);
/// 셸이 뜬 뒤 패널을 미리 만들어 두기까지. 로그인 직후의 바쁜 때를 피한다
const PREBUILD_AFTER: Duration = Duration::from_secs(3);
/// 앱이 설치·삭제된 뒤 패널을 다시 만들기까지. 꾸러미 하나가 .desktop 파일을 여럿 건드리는 동안 기다린다
const REBUILD_AFTER: Duration = Duration::from_secs(1);
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
    /// 지금 패널의 검색 칸 (패널을 만들 때마다 새로 만든다)
    search: RefCell<Option<gtk::SearchEntry>>,
    /// 만들어 둔 패널. 열 때마다 만들면 누른 뒤 한참 있다가 뜨고 첫 프레임이 무겁다(TB375FC에서 손을 뗀 뒤 약
    /// 0.3초). 그래서 한 번 만들어 두고, 열 때는 고르던 것만 처음으로 되돌린다
    built: RefCell<Option<Built>>,
    /// 설치된 앱이 바뀐 횟수. 패널을 만든 때와 다르면 다시 만든다
    apps_version: Rc<Cell<u64>>,
    /// 앱이 설치·삭제되는 것을 알리는 감시. 쥐고 있어야 신호가 온다
    _monitor: gio::AppInfoMonitor,
    /// 클립보드 기록 창
    clipboard: ClipboardPanel,
}

/// 만들어 둔 패널에서 열 때마다 되돌릴 것
struct Built {
    /// 만들 때의 것. 달라지면 다시 만든다: 설치된 앱, 입력 모드(검색 칸이 키를 받는 법이 다르다, `prepare_search`)
    apps_version: u64,
    touch: bool,
    apps: Rc<Vec<App>>,
    search: gtk::SearchEntry,
    query: Rc<RefCell<String>>,
    chips: Vec<gtk::ToggleButton>,
    /// 분류 칩 줄과 앱 격자의 스크롤
    scrollers: [gtk::ScrolledWindow; 2],
    recent_grid: gtk::FlowBox,
    /// 최근에 쓴 앱 줄에 놓은 앱 (apps의 자리)
    recent: Rc<RefCell<Vec<usize>>>,
    /// 거른 결과를 다시 보인다
    refresh: Rc<dyn Fn()>,
}

/// 최근에 쓴 앱 줄에 놓을 앱 (apps의 자리)
fn recent_apps(apps: &[App]) -> Vec<usize> {
    recent_ids()
        .iter()
        .filter_map(|id| apps.iter().position(|app| &app.id == id))
        .take(RECENT_COUNT)
        .collect()
}

impl Launcher {
    pub fn new(
        app: &gtk::Application,
        tr: &Rc<Translations>,
        toplevels: Option<&Toplevels>,
        history: &ClipboardHistory,
    ) -> Self {
        let window = overlay_window(app, "bunt-launcher");

        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .css_classes(["launcher-panel"])
            .build();
        window.set_child(Some(&panel));

        let monitor = gio::AppInfoMonitor::get();
        let launcher = Self(Rc::new(Inner {
            window,
            panel,
            tr: tr.clone(),
            toplevels: toplevels.cloned(),
            search: RefCell::new(None),
            built: RefCell::new(None),
            apps_version: Rc::default(),
            _monitor: monitor.clone(),
            clipboard: ClipboardPanel::new(app, tr, history),
        }));

        // 패널을 미리 만들어 둔다: 셸이 뜬 뒤 한 번, 그리고 앱이 설치·삭제될 때마다. 패널이 떠 있는 동안에는 고르던
        // 것을 잃지 않게 그대로 두고 다음에 열 때 만든다 (`prepare`)
        let weak = Rc::downgrade(&launcher.0);
        let prebuild = move || {
            if let Some(inner) = weak.upgrade() {
                Launcher(inner).prebuild();
            }
        };
        glib::timeout_add_local_once(PREBUILD_AFTER, prebuild.clone());
        let version = launcher.0.apps_version.clone();
        let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
        monitor.connect_changed(move |_| {
            version.set(version.get() + 1);
            if let Some(source) = pending.take() {
                source.remove();
            }
            let (pending_done, prebuild) = (pending.clone(), prebuild.clone());
            pending.set(Some(glib::timeout_add_local_once(
                REBUILD_AFTER,
                move || {
                    pending_done.set(None);
                    prebuild();
                },
            )));
        });

        let weak = Rc::downgrade(&launcher.0);
        launcher.0.clipboard.set_back(move || {
            if let Some(inner) = weak.upgrade() {
                Launcher(inner).back_from_clipboard();
            }
        });

        let weak = Rc::downgrade(&launcher.0);
        close_on_outside_press(&launcher.0.window, &launcher.0.panel, move || {
            if let Some(inner) = weak.upgrade() {
                Launcher(inner).hide();
            }
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&launcher.0);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            // ⌘4는 컴포지터가 Ctrl+4로 옮겨 보낸다. 시스템 설정에서 옮기기를 끄면 그대로 온다
            if key == gdk::Key::_4
                && state.intersects(
                    gdk::ModifierType::CONTROL_MASK
                        | gdk::ModifierType::SUPER_MASK
                        | gdk::ModifierType::META_MASK,
                )
            {
                Launcher(inner).show_clipboard();
                return glib::Propagation::Stop;
            }
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

    /// 클립보드 기록 창 (밝게·어둡게를 따르게 하려고 내놓는다)
    pub fn clipboard_window(&self) -> &gtk::ApplicationWindow {
        self.0.clipboard.window()
    }

    pub fn toggle(&self) {
        if self.0.clipboard.is_open() {
            self.0.clipboard.hide();
        } else if self.0.window.is_visible() {
            self.hide();
        } else {
            self.show();
        }
    }

    /// 클립보드 기록을 연다. 모든 앱 패널이 떠 있으면 같은 자리에서 바뀐 것처럼 보이게 새 창이 그려진 뒤 내린다
    pub fn show_clipboard(&self) {
        let (window, panel) = (self.0.window.clone(), self.0.panel.clone());
        let from_apps = window.is_visible();
        self.0.clipboard.show(!from_apps, move || {
            if from_apps {
                panel.remove_css_class("shown");
                window.set_visible(false);
            }
        });
    }

    fn focus_search(&self) {
        if let Some(search) = self.0.search.borrow().as_ref() {
            focus_search(&self.0.window, search);
        }
    }

    /// 클립보드 기록에서 모든 앱으로 돌아온다
    fn back_from_clipboard(&self) {
        self.prepare();
        fit_panel(&self.0.window, &self.0.panel);
        self.0.panel.add_css_class("shown");
        self.0.window.present();
        self.focus_search();
        let clipboard = self.0.clipboard.clone();
        let frames = std::cell::Cell::new(0);
        self.0.window.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            clipboard.hide_now();
            glib::ControlFlow::Break
        });
    }

    pub fn show(&self) {
        self.prepare();
        fit_panel(&self.0.window, &self.0.panel);
        self.0.window.present();
        self.focus_search();
        // 그려진 다음에 클래스를 붙여야 나타나는 전환이 보인다
        let panel = self.0.panel.clone();
        glib::timeout_add_local_once(Duration::from_millis(16), move || {
            panel.add_css_class("shown");
        });
    }

    /// 만들어 둔 패널이 지금의 앱 목록과 입력 모드에 맞는지
    fn is_fresh(&self) -> bool {
        let touch = self.0.window.has_css_class(TOUCH_CLASS);
        let version = self.0.apps_version.get();
        self.0
            .built
            .borrow()
            .as_ref()
            .is_some_and(|built| built.apps_version == version && built.touch == touch)
    }

    /// 패널을 띄우기 전에 부른다. 만들어 둔 패널이 맞으면 고르던 것만 처음으로 되돌리고, 아니면 다시 만든다
    fn prepare(&self) {
        if self.is_fresh() {
            self.reset();
        } else {
            self.fill();
        }
    }

    /// 패널이 떠 있지 않은 동안 미리 만들어 둔다
    fn prebuild(&self) {
        if !self.0.window.is_visible() && !self.is_fresh() {
            self.fill();
        }
    }

    /// 만들어 둔 패널을 막 만든 것처럼 되돌린다: 검색어와 고른 분류를 지우고, 맨 위로 올리고, 최근에 쓴 앱 줄을
    /// 고친다
    fn reset(&self) {
        let built = self.0.built.borrow();
        let Some(built) = built.as_ref() else {
            return;
        };
        let recent = recent_apps(&built.apps);
        if *built.recent.borrow() != recent {
            while let Some(child) = built.recent_grid.first_child() {
                built.recent_grid.remove(&child);
            }
            for &index in &recent {
                built.recent_grid.append(&tile(&built.apps[index]));
            }
            built.recent.replace(recent);
        }
        for chip in &built.chips {
            chip.set_active(false);
        }
        // 검색 칸은 바뀐 글을 조금 늦게 알리므로(search-changed) 거르는 글은 여기서 바로 지운다
        built.search.set_text("");
        built.query.borrow_mut().clear();
        if built.touch {
            // 터치 모드의 칸은 눌러야 포커스를 받는다 (`prepare_search`)
            built.search.set_can_focus(false);
        }
        (built.refresh)();
        for scroller in &built.scrollers {
            scroller.hadjustment().set_value(0.0);
            scroller.vadjustment().set_value(0.0);
        }
        // 지난번에 누른 칸에 포커스가 남지 않게 한다. 띄울 때 GTK가 새 패널에서처럼 다시 정한다
        gtk::prelude::GtkWindowExt::set_focus(&self.0.window, None::<&gtk::Widget>);
    }

    pub fn hide(&self) {
        if !self.0.window.is_visible() || !self.0.panel.has_css_class("shown") {
            return;
        }
        self.0.panel.remove_css_class("shown");
        let window = self.0.window.clone();
        glib::timeout_add_local_once(CLOSE_DELAY, move || window.set_visible(false));
    }

    /// 앱을 연다: 실행 중이면 창을 앞으로, 아니면 실행
    fn open(&self, app: &App) {
        activate_or_launch(&app.info, self.0.toplevels.as_ref(), &self.0.window);
        self.hide();
    }

    fn fill(&self) {
        let inner = &self.0;
        let tr = &inner.tr;
        clear(&inner.panel);
        let apps_version = inner.apps_version.get();
        let touch = inner.window.has_css_class(TOUCH_CLASS);
        let apps = Rc::new(installed_apps());

        // 검색 칸: 앱 아이콘과 큰 글씨 (macOS 응용 프로그램 창의 머리)
        let search = gtk::SearchEntry::builder()
            .placeholder_text(tr.get("launcher-search"))
            .hexpand(true)
            .css_classes(["launcher-search"])
            .build();
        prepare_search(&inner.window, &search);
        let header = gtk::Box::builder()
            .spacing(10)
            .css_classes(["launcher-header"])
            .build();
        let icon = gtk::Image::from_icon_name(ICON);
        icon.set_pixel_size(24);
        header.append(&icon);
        header.append(&search);
        let this = self.clone();
        header.append(&mode_buttons(tr, Mode::Apps, move |mode| {
            if mode == Mode::Clipboard {
                this.show_clipboard();
            }
        }));
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
        let recent = Rc::new(RefCell::new(recent_apps(&apps)));
        let recent_grid = grid();
        for &index in recent.borrow().iter() {
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
            let recent = recent.clone();
            Rc::new(move || {
                all_grid.invalidate_filter();
                // 최근 줄은 거르지 않을 때만 보인다
                let browsing = query.borrow().is_empty() && category.borrow().is_none();
                let has_recent = !recent.borrow().is_empty();
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
        let chip_widgets: Vec<gtk::ToggleButton> =
            chip_buttons.iter().map(|(_, chip)| chip.clone()).collect();
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
        let placed = recent.clone();
        recent_grid.connect_child_activated(move |_, child| {
            let index = usize::try_from(child.index())
                .ok()
                .and_then(|position| placed.borrow().get(position).copied());
            if let Some(app) = index.and_then(|index| opened.get(index)) {
                this.open(app);
            }
        });
        // 검색 칸의 Enter는 보이는 첫 앱을 연다
        let this = self.clone();
        let listed = apps.clone();
        search.connect_activate(move |_| {
            if let Some(app) = (0..listed.len())
                .find(|&index| matches(index))
                .and_then(|index| listed.get(index))
            {
                this.open(app);
            }
        });
        inner.search.replace(Some(search.clone()));
        let refresh: Rc<dyn Fn()> = refresh;
        inner.built.replace(Some(Built {
            apps_version,
            touch,
            apps,
            search,
            query,
            chips: chip_widgets,
            scrollers: [chip_scroller, scroller],
            recent_grid,
            recent,
            refresh,
        }));
    }
}

/// 화면 전체를 덮는 투명한 overlay 창. 메뉴바와 Dock까지 덮어 패널 바깥 어디를 눌러도 닫히게 하고, 검색 칸에 치는 동안
/// 화면 키보드 위까지만 덮는다(키보드보다 늦게 떠도 키보드를 가리지 않는다). 모든 앱과 클립보드 기록이 쓴다
pub fn overlay_window(app: &gtk::Application, name: &str) -> gtk::ApplicationWindow {
    // 제목은 보이지 않지만 접근성 이름이 된다. 에이전트는 이것으로 레이어 표면을 찾는다 (layer_window)
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(name)
        .css_classes([name])
        .build();
    window.init_layer_shell();
    window.set_namespace(Some(name));
    window.set_layer(Layer::Overlay);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::Exclusive);
    crate::osk::avoid_keyboard(&window, crate::osk::Avoid::Fill);
    window
}

/// 패널 바깥을 누르면 `close`를 부른다. 안쪽 누르기는 그대로 패널로 간다
pub fn close_on_outside_press(
    window: &gtk::ApplicationWindow,
    panel: &gtk::Box,
    close: impl Fn() + 'static,
) {
    let click = gtk::GestureClick::new();
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    let panel = panel.clone();
    click.connect_pressed(move |gesture, _, x, y| {
        let Some(root) = gesture.widget() else {
            return;
        };
        let inside = root
            .pick(x, y, gtk::PickFlags::DEFAULT)
            .is_some_and(|picked| {
                &picked == panel.upcast_ref::<gtk::Widget>() || picked.is_ancestor(&panel)
            });
        if !inside {
            close();
        }
    });
    window.add_controller(click);
}

/// 화면이 작으면 패널을 화면에 맞춘다 (가운데에 두어도 메뉴바와 Dock에 닿지 않게 남긴다)
pub fn fit_panel(window: &gtk::ApplicationWindow, panel: &gtk::Box) {
    let monitor = WidgetExt::display(window)
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
    panel.set_size_request(width, height);
}

pub fn clear(panel: &gtk::Box) {
    while let Some(child) = panel.first_child() {
        panel.remove(&child);
    }
}

/// 패널이 지금 보이는 것 (Spotlight의 ⌘1, ⌘4 단추)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Apps,
    Clipboard,
}

/// 패널 머리의 단추 둘. 지금 보이는 쪽은 강조색이고 누르면 다른 쪽으로 간다
pub fn mode_buttons(tr: &Translations, active: Mode, open: impl Fn(Mode) + 'static) -> gtk::Box {
    let open = Rc::new(open);
    let buttons = gtk::Box::builder().spacing(6).build();
    for (mode, icon, label) in [
        (Mode::Apps, APPS_MODE_ICON, "launcher-apps"),
        (Mode::Clipboard, CLIPBOARD_ICON, "launcher-clipboard"),
    ] {
        let label = tr.get(label);
        let button = gtk::Button::builder()
            .icon_name(icon)
            .tooltip_text(&label)
            .valign(gtk::Align::Center)
            .css_classes(["launcher-mode"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(&label)]);
        if mode == active {
            button.add_css_class("active");
        }
        let open = open.clone();
        button.connect_clicked(move |_| open(mode));
        buttons.append(&button);
    }
    buttons
}

/// 패널을 띄운 바로 뒤의 포커스. 포인터 모드는 검색 칸이다. 터치 모드는 칸이 포커스를 받지 못하므로(`prepare_search`) GTK가 정한
/// 단추에 그대로 두고 포커스 표시만 끈다
pub fn focus_search(window: &gtk::ApplicationWindow, search: &gtk::SearchEntry) {
    if window.has_css_class(TOUCH_CLASS) {
        window.set_focus_visible(false);
    } else {
        search.grab_focus();
    }
}

/// 검색 칸이 키를 받는 법을 입력 모드에 맞춘다.
/// - 포인터 모드: 패널 어디에 포커스가 있든 글자를 치면 검색 칸으로 간다.
/// - 터치 모드: 칸을 눌러야 포커스를 받고, 패널에 키를 모으지 않는다. 칸에 포커스가 있거나 키를 모으면 GTK가 패널을 띄울 때부터 글 입력을
///   켜 둔다. 그러면 칸과 같은 줄의 단추를 누른 손가락이 칸을 누른 것으로 쳐져(앱이 글 입력을 다시 켠 것과 글 커서 자리로 가린다,
///   comp/src/osk.rs) 화면 키보드가 패널을 가린다. 터치 모드에는 물리 키보드가 없으므로 키를 모을 일도 없다
pub fn prepare_search(window: &gtk::ApplicationWindow, search: &gtk::SearchEntry) {
    if !window.has_css_class(TOUCH_CLASS) {
        search.set_key_capture_widget(Some(window));
        return;
    }
    search.set_can_focus(false);
    let press = gtk::GestureClick::new();
    press.set_propagation_phase(gtk::PropagationPhase::Capture);
    press.connect_pressed(|press, _, _, _| {
        if let Some(search) = press.widget() {
            search.set_can_focus(true);
            search.grab_focus();
        }
    });
    search.add_controller(press);
}

pub fn separator() -> gtk::Separator {
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
