// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설정 창. macOS 26 시스템 설정처럼 왼쪽 사이드바(검색, 사용자, 묶음별 페이지 목록)와 오른쪽
//! 본문(뒤로·앞으로 단추, 제목, 페이지)으로 나눈다. 제목 표시줄은 따로 두지 않고 두 칸의 맨 위가
//! 창을 끄는 손잡이와 창 단추를 맡는다. 페이지는 처음 열 때 만든다

pub mod account;

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::Rc,
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    pages::{Context, Page, interface_settings},
    widgets::{BadgeSize, Notice, badge, keep_alive},
};

use self::account::Account;

const SIDEBAR_WIDTH: i32 = 230;

pub struct SettingsWindow {
    window: gtk::ApplicationWindow,
    navigator: Rc<Navigator>,
}

struct Navigator {
    context: Rc<Context>,
    stack: gtk::Stack,
    sidebar: gtk::ListBox,
    rows: Vec<(Page, gtk::ListBoxRow)>,
    title: gtk::Label,
    back: gtk::Button,
    forward: gtk::Button,
    /// 뒤로 가기로 돌아갈 페이지들 (마지막이 바로 앞)
    history: RefCell<Vec<Page>>,
    /// 앞으로 가기로 갈 페이지들
    future: RefCell<Vec<Page>>,
    current: Cell<Option<Page>>,
    built: RefCell<HashSet<Page>>,
    /// 코드에서 사이드바 항목을 고를 때 선택 처리기가 다시 이동하지 않게 한다
    selecting: Cell<bool>,
}

impl SettingsWindow {
    pub fn new(app: &gtk::Application, tr: &Rc<Translations>) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(tr.get("app-title"))
            .default_width(900)
            .default_height(640)
            .width_request(720)
            .height_request(480)
            .build();
        window.add_css_class("settings-window");
        // 제목 표시줄 없이 사이드바와 본문 머리가 그 자리를 맡는다. 숨긴 위젯을 제목 표시줄로 두면
        // GTK가 기본 막대를 만들지 않고 창 그림자와 크기 조절은 그대로 둔다
        let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        titlebar.set_visible(false);
        window.set_titlebar(Some(&titlebar));

        let notice = Notice::new();
        let context = Rc::new(Context::new(
            tr.clone(),
            notice.clone(),
            window.upcast_ref(),
        ));

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(120)
            .hexpand(true)
            .vexpand(true)
            .build();
        let content = gtk::Overlay::builder().child(&stack).build();
        content.add_overlay(notice.widget());

        let sidebar = gtk::ListBox::builder()
            .css_classes(["sidebar-list"])
            .build();
        let mut rows = Vec::new();
        for page in Page::ALL {
            let row = sidebar_row(tr, page);
            sidebar.append(&row);
            rows.push((page, row));
        }

        let back = nav_button("go-previous-symbolic", &tr.get("nav-back"));
        let forward = nav_button("go-next-symbolic", &tr.get("nav-forward"));
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["content-title"])
            .build();

        let navigator = Rc::new(Navigator {
            context: context.clone(),
            stack: stack.clone(),
            sidebar: sidebar.clone(),
            rows,
            title: title.clone(),
            back: back.clone(),
            forward: forward.clone(),
            history: RefCell::default(),
            future: RefCell::default(),
            current: Cell::new(None),
            built: RefCell::default(),
            selecting: Cell::new(false),
        });

        let weak = Rc::downgrade(&navigator);
        context.set_navigate(move |page| {
            if let Some(navigator) = weak.upgrade() {
                navigator.show(page, true);
            }
        });
        let weak = Rc::downgrade(&navigator);
        sidebar.connect_row_selected(move |_, row| {
            let (Some(navigator), Some(row)) = (weak.upgrade(), row) else {
                return;
            };
            if navigator.selecting.get() {
                return;
            }
            if let Some(page) = navigator.page_of(row) {
                navigator.show(page, true);
            }
        });
        let weak = Rc::downgrade(&navigator);
        back.connect_clicked(move |_| {
            if let Some(navigator) = weak.upgrade() {
                navigator.go_back();
            }
        });
        let weak = Rc::downgrade(&navigator);
        forward.connect_clicked(move |_| {
            if let Some(navigator) = weak.upgrade() {
                navigator.go_forward();
            }
        });

        let search = gtk::SearchEntry::builder()
            .placeholder_text(tr.get("sidebar-search"))
            .css_classes(["sidebar-search"])
            .build();
        setup_search(&search, &navigator, tr);
        setup_headers(&sidebar, &navigator);

        // 사이드바: 창 단추 자리, 검색, 사용자, 페이지 목록
        let sidebar_pane = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(SIDEBAR_WIDTH)
            .css_classes(["sidebar-pane"])
            .build();
        let sidebar_top = gtk::Box::builder().css_classes(["sidebar-top"]).build();
        sidebar_top.append(&gtk::WindowControls::new(gtk::PackType::Start));
        sidebar_pane.append(&gtk::WindowHandle::builder().child(&sidebar_top).build());
        sidebar_pane.append(&search);
        sidebar_pane.append(&account_header(tr, &navigator));
        sidebar_pane.append(
            &gtk::ScrolledWindow::builder()
                .child(&sidebar)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .build(),
        );

        // 본문: 뒤로·앞으로, 제목, 창 단추
        let header = gtk::Box::builder()
            .spacing(10)
            .css_classes(["content-header"])
            .build();
        let arrows = gtk::Box::builder()
            .css_classes(["nav-buttons"])
            .valign(gtk::Align::Center)
            .build();
        arrows.append(&back);
        arrows.append(&forward);
        header.append(&arrows);
        title.set_hexpand(true);
        header.append(&title);
        header.append(&gtk::WindowControls::new(gtk::PackType::End));
        let content_pane = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .css_classes(["content-pane"])
            .build();
        content_pane.append(&gtk::WindowHandle::builder().child(&header).build());
        content_pane.append(&content);

        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&sidebar_pane);
        body.append(&content_pane);
        window.set_child(Some(&body));

        add_shortcuts(&window, &search, &navigator);
        follow_color_scheme(&window);
        Self { window, navigator }
    }

    /// 창을 앞으로 가져온다. 페이지를 주면 그 페이지로, 없으면 처음이면 Wi-Fi로 간다
    pub fn show(&self, page: Option<Page>) {
        match page {
            Some(page) => self.navigator.show(page, true),
            None if self.navigator.current.get().is_none() => {
                self.navigator.show(Page::ALL[0], false);
            }
            None => {}
        }
        self.window.present();
        // macOS처럼 검색 칸이 아니라 고른 페이지 항목에 포커스를 둔다
        if let Some(row) = self
            .navigator
            .current
            .get()
            .and_then(|page| self.navigator.row_of(page))
        {
            row.grab_focus();
        }
    }
}

impl Navigator {
    fn page_of(&self, row: &gtk::ListBoxRow) -> Option<Page> {
        self.rows
            .iter()
            .find(|(_, candidate)| candidate == row)
            .map(|(page, _)| *page)
    }

    fn row_of(&self, page: Page) -> Option<&gtk::ListBoxRow> {
        self.rows
            .iter()
            .find(|(candidate, _)| *candidate == page)
            .map(|(_, row)| row)
    }

    /// 페이지를 연다. record면 지금 페이지를 뒤로 가기 목록에 넣는다
    fn show(&self, page: Page, record: bool) {
        if self.current.get() == Some(page) {
            return;
        }
        if record {
            if let Some(current) = self.current.get() {
                self.history.borrow_mut().push(current);
            }
            self.future.borrow_mut().clear();
        }
        if self.built.borrow_mut().insert(page) {
            self.stack
                .add_named(&page.build(&self.context), Some(page.name()));
        }
        self.stack.set_visible_child_name(page.name());
        self.title.set_text(&self.context.tr.get(page.title_id()));
        self.current.set(Some(page));

        self.selecting.set(true);
        self.sidebar.select_row(self.row_of(page));
        self.selecting.set(false);
        self.back.set_sensitive(!self.history.borrow().is_empty());
        self.forward.set_sensitive(!self.future.borrow().is_empty());
    }

    fn go_back(&self) {
        let Some(page) = self.history.borrow_mut().pop() else {
            return;
        };
        if let Some(current) = self.current.get() {
            self.future.borrow_mut().push(current);
        }
        self.show(page, false);
    }

    fn go_forward(&self) {
        let Some(page) = self.future.borrow_mut().pop() else {
            return;
        };
        if let Some(current) = self.current.get() {
            self.history.borrow_mut().push(current);
        }
        self.show(page, false);
    }
}

fn sidebar_row(tr: &Translations, page: Page) -> gtk::ListBoxRow {
    let content = gtk::Box::builder().spacing(8).build();
    content.append(&badge(page.icon(), page.tint(), BadgeSize::Small));
    content.append(
        &gtk::Label::builder()
            .label(tr.get(page.title_id()))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    gtk::ListBoxRow::builder().child(&content).build()
}

fn nav_button(icon: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .sensitive(false)
        .build()
}

/// 사이드바 맨 위 계정 카드의 사진 크기
const ACCOUNT_AVATAR_SIZE: i32 = 36;

/// 사이드바 맨 위의 사용자 (macOS의 계정 자리). 사진과 별칭(없으면 사용자 이름)을
/// AccountsService에서 읽고, 시스템 설정이나 다른 곳에서 바꾸면 따라간다. 누르면 사용자 및 그룹을 연다
fn account_header(tr: &Translations, navigator: &Rc<Navigator>) -> gtk::Button {
    let real_name = glib::real_name().to_string_lossy().into_owned();
    let name = if real_name.is_empty() || real_name == "Unknown" {
        glib::user_name().to_string_lossy().into_owned()
    } else {
        real_name
    };
    let avatar = account::avatar(&name, None, ACCOUNT_AVATAR_SIZE);
    let name_label = gtk::Label::builder()
        .label(&name)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(["account-name"])
        .build();
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .build();
    text.append(&name_label);
    text.append(
        &gtk::Label::builder()
            .label(tr.get("sidebar-account"))
            .xalign(0.0)
            .css_classes(["account-subtitle"])
            .build(),
    );
    let header = gtk::Box::builder()
        .spacing(10)
        .css_classes(["account-header"])
        .build();
    header.append(&avatar);
    header.append(&text);
    let button = gtk::Button::builder()
        .child(&header)
        .tooltip_text(tr.get("page-users"))
        .css_classes(["account-button"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&name)]);
    let weak = Rc::downgrade(navigator);
    button.connect_clicked(move |_| {
        if let Some(navigator) = weak.upgrade() {
            navigator.show(Page::Users, true);
        }
    });

    // 계정 정보를 읽어 바꾸고, 바뀌었다는 신호(Changed)를 받으면 다시 읽는다
    let show = {
        let (avatar, name_label, button) = (
            avatar.downgrade(),
            name_label.downgrade(),
            button.downgrade(),
        );
        move |account: &Account| {
            let (Some(avatar), Some(name_label), Some(button)) =
                (avatar.upgrade(), name_label.upgrade(), button.upgrade())
            else {
                return;
            };
            let texture = account
                .icon_file
                .as_ref()
                .and_then(|path| gtk::gdk::Texture::from_filename(path).ok());
            account::set_avatar(
                &avatar,
                account.display_name(),
                texture.as_ref(),
                ACCOUNT_AVATAR_SIZE,
            );
            name_label.set_text(account.display_name());
            button.update_property(&[gtk::accessible::Property::Label(account.display_name())]);
        }
    };
    let show = Rc::new(show);
    let holder = button.downgrade();
    glib::spawn_future_local(async move {
        let account = match Account::current().await {
            Ok(account) => account,
            Err(err) => {
                eprintln!("bunt-settings: account: {}", err.message());
                return;
            }
        };
        show(&account);
        let Some(button) = holder.upgrade() else {
            return;
        };
        let watched = account.clone();
        let subscription = account.watch(move || {
            let (account, show) = (watched.clone(), show.clone());
            glib::spawn_future_local(async move {
                if let Ok(account) = account.reload().await {
                    show(&account);
                }
            });
        });
        keep_alive(&button, subscription);
    });
    button
}

/// 검색 칸에 쓴 글자가 페이지 이름이나 낱말에 들어 있는 페이지만 보인다. Enter는 첫 페이지를 연다
fn setup_search(search: &gtk::SearchEntry, navigator: &Rc<Navigator>, tr: &Translations) {
    let keys: Vec<(Page, String)> = Page::ALL
        .iter()
        .map(|page| {
            let key = format!(
                "{} {} {}",
                tr.get(page.title_id()),
                tr.get(&page.keywords_id()),
                page.name()
            );
            (*page, key.to_lowercase())
        })
        .collect();
    let keys = Rc::new(keys);
    let query = Rc::new(RefCell::new(String::new()));

    let matches = {
        let (keys, query) = (keys.clone(), query.clone());
        move |page: Page| {
            let query = query.borrow();
            query.is_empty()
                || keys.iter().any(|(candidate, key)| {
                    *candidate == page && query.split_whitespace().all(|word| key.contains(word))
                })
        }
    };
    let matches = Rc::new(matches);

    let weak = Rc::downgrade(navigator);
    let filter = matches.clone();
    navigator.sidebar.set_filter_func(move |row| {
        weak.upgrade()
            .and_then(|navigator| navigator.page_of(row))
            .is_none_or(|page| filter(page))
    });

    let weak = Rc::downgrade(navigator);
    search.connect_search_changed(move |search| {
        *query.borrow_mut() = search.text().trim().to_lowercase();
        if let Some(navigator) = weak.upgrade() {
            navigator.sidebar.invalidate_filter();
            navigator.sidebar.invalidate_headers();
        }
    });
    let weak = Rc::downgrade(navigator);
    search.connect_activate(move |_| {
        let Some(navigator) = weak.upgrade() else {
            return;
        };
        if let Some(page) = Page::ALL.into_iter().find(|page| matches(*page)) {
            navigator.show(page, true);
        }
    });
}

/// 사이드바 묶음 사이에 간격을 둔다. 검색으로 가려진 행은 건너뛴다
fn setup_headers(sidebar: &gtk::ListBox, navigator: &Rc<Navigator>) {
    let weak = Rc::downgrade(navigator);
    sidebar.set_header_func(move |row, before| {
        let Some(navigator) = weak.upgrade() else {
            return;
        };
        let section = |row: &gtk::ListBoxRow| navigator.page_of(row).map(Page::section);
        let gap = before.is_some_and(|before| section(before) != section(row));
        if gap {
            if row.header().is_none() {
                row.set_header(Some(
                    &gtk::Box::builder().css_classes(["sidebar-gap"]).build(),
                ));
            }
        } else {
            row.set_header(None::<&gtk::Widget>);
        }
    });
}

/// Ctrl+F 검색, Alt+←/→ 뒤로·앞으로, Ctrl+W 창 닫기
fn add_shortcuts(
    window: &gtk::ApplicationWindow,
    search: &gtk::SearchEntry,
    navigator: &Rc<Navigator>,
) {
    let controller = gtk::ShortcutController::new();
    controller.set_scope(gtk::ShortcutScope::Managed);
    let add = |trigger: &str, action: gtk::CallbackAction| {
        controller.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(trigger),
            Some(action),
        ));
    };
    add(
        "<Control>f",
        gtk::CallbackAction::new(glib::clone!(
            #[weak]
            search,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _| {
                search.grab_focus();
                glib::Propagation::Stop
            }
        )),
    );
    let weak = Rc::downgrade(navigator);
    add(
        "<Alt>Left",
        gtk::CallbackAction::new(move |_, _| {
            if let Some(navigator) = weak.upgrade() {
                navigator.go_back();
            }
            glib::Propagation::Stop
        }),
    );
    let weak = Rc::downgrade(navigator);
    add(
        "<Alt>Right",
        gtk::CallbackAction::new(move |_, _| {
            if let Some(navigator) = weak.upgrade() {
                navigator.go_forward();
            }
            glib::Propagation::Stop
        }),
    );
    add(
        "<Control>w",
        gtk::CallbackAction::new(|widget, _| {
            if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                window.close();
            }
            glib::Propagation::Stop
        }),
    );
    window.add_controller(controller);
}

/// 밝게·어둡게 설정을 따른다. GTK 기본 테마는 prefer-dark로, 자체 CSS는 dark 클래스로 바꾼다
fn follow_color_scheme(window: &gtk::ApplicationWindow) {
    let Some(settings) = interface_settings() else {
        return;
    };
    let apply = |settings: &gio::Settings, window: &gtk::ApplicationWindow| {
        let dark = settings.string("color-scheme") == "prefer-dark";
        if let Some(gtk_settings) = gtk::Settings::default() {
            gtk_settings.set_gtk_application_prefer_dark_theme(dark);
        }
        if dark {
            window.add_css_class("dark");
        } else {
            window.remove_css_class("dark");
        }
    };
    apply(&settings, window);
    let keep_alive = settings.clone();
    settings.connect_changed(
        Some("color-scheme"),
        glib::clone!(
            #[weak]
            window,
            move |settings, _| {
                // 변경 알림은 설정 객체가 살아 있을 때만 온다. 처리기가 객체를 붙든다
                let _ = &keep_alive;
                apply(settings, &window);
            }
        ),
    );
}
