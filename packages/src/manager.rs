// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 앱을 그냥 열면 뜨는 창: 사용자가 설치한 것의 목록. 줄마다 [열기]와 [제거…]가 있고, 도구 막대에 설치 파일 열기와 앱
//! 스토어가 있다. 파일을 창에 끌어다 놓아도 설치 창이 뜬다. 다른 곳(앱 스토어, 터미널)에서 설치한 것도 창이 앞으로 올
//! 때 다시 읽어 보여 준다

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

use crate::{
    installed::{self, Item, RemovalPlan, RemoveError, Source},
    installer, ui,
};

/// 앱 스토어(GNOME Software)의 데스크톱 ID
const STORE_ID: &str = "org.gnome.Software.desktop";
/// 알림이 보이는 시간 (초). 시스템 설정의 알림과 같다
const NOTICE_SECONDS: u32 = 4;

pub struct Manager {
    tr: Rc<Translations>,
    /// 지금 떠 있는 창. 닫으면 없어지고 창이 모두 닫히면 앱도 끝난다
    state: Rc<RefCell<Option<Rc<Window>>>>,
}

struct Window {
    tr: Rc<Translations>,
    window: gtk::ApplicationWindow,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    stack: gtk::Stack,
    tools: gtk::Box,
    notice: gtk::Label,
    notice_bar: gtk::Revealer,
    /// 알림을 띄운 차례. 새 알림이 뜨면 앞 알림의 타이머는 알림을 닫지 않는다
    notice_serial: Cell<u32>,
    items: RefCell<Vec<Item>>,
    /// 지금 읽는 중이면 더 부르지 않는다
    loading: Cell<bool>,
    /// 읽는 동안 또 불렸다
    reload_again: Cell<bool>,
}

impl Manager {
    pub fn new(tr: &Rc<Translations>) -> Self {
        Self {
            tr: tr.clone(),
            state: Rc::new(RefCell::new(None)),
        }
    }

    pub fn present(&self, app: &gtk::Application) {
        let mut state = self.state.borrow_mut();
        let window = state.get_or_insert_with(|| {
            let window = Window::new(app, &self.tr);
            let holder = self.state.clone();
            window.window.connect_destroy(move |_| {
                holder.borrow_mut().take();
            });
            window
        });
        window.window.present();
        window.reload();
    }
}

impl Window {
    fn new(app: &gtk::Application, tr: &Rc<Translations>) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(tr.get("app-name"))
            .default_width(720)
            .default_height(520)
            .width_request(560)
            .height_request(360)
            .css_classes(["pm-window"])
            .build();

        let open_file = gtk::Button::builder()
            .label(tr.get("manager-open-file"))
            .build();
        let store = gtk::Button::builder()
            .label(tr.get("manager-store"))
            .build();
        store.set_visible(gio_unix::DesktopAppInfo::new(STORE_ID).is_some());
        let tools = gtk::Box::builder()
            .spacing(8)
            .valign(gtk::Align::Center)
            .css_classes(["pm-tools"])
            .build();
        tools.append(&open_file);
        tools.append(&store);
        ui::title_bar(&window, &tr.get("app-name"), Some(&tools));
        ui::follow_color_scheme(&window);
        ui::close_on_escape(&window);

        let search = gtk::SearchEntry::builder()
            .placeholder_text(tr.get("manager-search"))
            .hexpand(true)
            .css_classes(["pm-search"])
            .build();

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["pm-group"])
            .build();
        let footnote = gtk::Label::builder()
            .label(tr.get("manager-footnote"))
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .css_classes(["pm-footnote"])
            .build();
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["pm-list-column"])
            .build();
        column.append(&search);
        column.append(&list);
        column.append(&footnote);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&column)
            .build();

        // 설치한 것이 없을 때
        let empty_icon = gtk::Image::from_icon_name("org.buntos.PackageManager");
        empty_icon.set_pixel_size(96);
        let empty_open = gtk::Button::builder()
            .label(tr.get("manager-open-file"))
            .css_classes(["suggested-action"])
            .build();
        let empty_store = gtk::Button::builder()
            .label(tr.get("manager-store"))
            .build();
        empty_store.set_visible(gio_unix::DesktopAppInfo::new(STORE_ID).is_some());
        let empty_buttons = gtk::Box::builder()
            .spacing(8)
            .halign(gtk::Align::Center)
            .margin_top(8)
            .build();
        empty_buttons.append(&empty_store);
        empty_buttons.append(&empty_open);
        let empty = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .css_classes(["pm-empty"])
            .build();
        empty.append(&empty_icon);
        empty.append(
            &gtk::Label::builder()
                .label(tr.get("manager-empty-title"))
                .css_classes(["pm-result-title"])
                .build(),
        );
        empty.append(
            &gtk::Label::builder()
                .label(tr.get("manager-empty-details"))
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .justify(gtk::Justification::Center)
                .max_width_chars(52)
                .css_classes(["pm-secondary"])
                .build(),
        );
        empty.append(&empty_buttons);

        let spinner = gtk::Spinner::builder()
            .spinning(true)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&spinner, Some("loading"));
        stack.add_named(&scroller, Some("list"));
        stack.add_named(&empty, Some("empty"));

        // 결과 알림: 본문 아래에 잠깐 뜬다 (시스템 설정과 같다)
        let notice = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .css_classes(["pm-notice-bar"])
            .build();
        let notice_bar = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .child(&notice)
            .build();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.append(&stack);
        body.append(&notice_bar);
        window.set_child(Some(&body));

        let this = Rc::new(Self {
            tr: tr.clone(),
            window,
            search,
            list,
            stack,
            tools,
            notice,
            notice_bar,
            notice_serial: Cell::new(0),
            items: RefCell::new(Vec::new()),
            loading: Cell::new(false),
            reload_again: Cell::new(false),
        });

        for button in [&open_file, &empty_open] {
            let weak = Rc::downgrade(&this);
            button.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.choose_file();
                }
            });
        }
        for button in [&store, &empty_store] {
            let weak = Rc::downgrade(&this);
            button.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.open_store();
                }
            });
        }
        let weak = Rc::downgrade(&this);
        this.search.connect_search_changed(move |_| {
            if let Some(this) = weak.upgrade() {
                this.show_items();
            }
        });
        // 다른 곳에서 설치하고 돌아오면 다시 읽는다
        let weak = Rc::downgrade(&this);
        this.window.connect_is_active_notify(move |window| {
            if window.is_active()
                && let Some(this) = weak.upgrade()
            {
                this.reload();
            }
        });
        this.accept_dropped_files();
        this
    }

    fn accept_dropped_files(self: &Rc<Self>) {
        let target = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let weak: Weak<Self> = Rc::downgrade(self);
        target.connect_drop(move |_, value, _, _| {
            let (Some(this), Ok(files)) = (weak.upgrade(), value.get::<gdk::FileList>()) else {
                return false;
            };
            for file in files.files() {
                this.open_installer(&file);
            }
            true
        });
        self.window.add_controller(target);
    }

    fn open_installer(&self, file: &gio::File) {
        if let Some(app) = self.window.application() {
            installer::open(&app, &self.tr, file);
        }
    }

    fn choose_file(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&self.tr.get("manager-file-filter")));
        for pattern in [
            "*.deb",
            "*.AppImage",
            "*.appimage",
            "*.flatpak",
            "*.flatpakref",
            "*.flatpakrepo",
        ] {
            filter.add_pattern(pattern);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(self.tr.get("manager-open-file"))
            .filters(&filters)
            .default_filter(&filter)
            .modal(true)
            .build();
        // 받은 파일은 대개 다운로드 폴더에 있다
        let downloads = glib::user_special_dir(glib::UserDirectory::Downloads)
            .unwrap_or_else(|| glib::home_dir().join("Downloads"));
        if downloads.is_dir() {
            dialog.set_initial_folder(Some(&gio::File::for_path(downloads)));
        }
        let weak = Rc::downgrade(self);
        dialog.open(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if let (Some(this), Ok(file)) = (weak.upgrade(), result) {
                this.open_installer(&file);
            }
        });
    }

    fn open_store(&self) {
        let Some(info) = gio_unix::DesktopAppInfo::new(STORE_ID) else {
            return;
        };
        ui::launch_app(&info, &self.window, |err| {
            eprintln!("bunt-packages: cannot open the store: {err}");
        });
    }

    fn reload(self: &Rc<Self>) {
        if self.loading.replace(true) {
            self.reload_again.set(true);
            return;
        }
        if self.items.borrow().is_empty() {
            self.stack.set_visible_child_name("loading");
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let items = gio::spawn_blocking(installed::list)
                .await
                .unwrap_or_default();
            this.loading.set(false);
            let changed = *this.items.borrow() != items;
            let first = this.stack.visible_child_name().as_deref() == Some("loading");
            *this.items.borrow_mut() = items;
            if changed || first {
                this.show_items();
            }
            if this.reload_again.replace(false) {
                this.reload();
            }
        });
    }

    fn show_items(self: &Rc<Self>) {
        while let Some(row) = self.list.row_at_index(0) {
            self.list.remove(&row);
        }
        let items = self.items.borrow();
        // 빈 화면에는 같은 단추가 가운데에 있다
        self.tools.set_visible(!items.is_empty());
        if items.is_empty() {
            self.stack.set_visible_child_name("empty");
            return;
        }
        self.stack.set_visible_child_name("list");
        let query = self.search.text().to_lowercase();
        let mut shown = 0;
        for item in items.iter() {
            if !query.is_empty() && !item.title.to_lowercase().contains(&query) {
                continue;
            }
            self.list.append(&self.item_row(item));
            shown += 1;
        }
        if shown == 0 {
            let row = gtk::Label::builder()
                .label(self.tr.get("manager-no-match"))
                .css_classes(["pm-secondary", "pm-no-match"])
                .build();
            self.list.append(&row);
        }
    }

    fn item_row(self: &Rc<Self>, item: &Item) -> gtk::ListBoxRow {
        let icon = ui::app_image(item.desktop.as_deref(), 32);
        let title = gtk::Label::builder()
            .label(&item.title)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["pm-row-title"])
            .build();
        let subtitle = gtk::Label::builder()
            .label(self.subtitle(item))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["pm-row-subtitle"])
            .build();
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        text.append(&title);
        text.append(&subtitle);
        let line = gtk::Box::builder()
            .spacing(10)
            .css_classes(["pm-row"])
            .build();
        line.append(&icon);
        line.append(&text);
        if let Some(desktop) = &item.desktop {
            let open = gtk::Button::with_label(&self.tr.get("manager-row-open"));
            let desktop = desktop.clone();
            let weak = Rc::downgrade(self);
            open.connect_clicked(move |button| {
                let weak = weak.clone();
                ui::launch(&desktop, button, move |err| {
                    if let Some(this) = weak.upgrade() {
                        this.show_notice(&err.to_string());
                    }
                });
            });
            open.set_valign(gtk::Align::Center);
            line.append(&open);
        }
        let remove = gtk::Button::with_label(&self.tr.get("manager-row-remove"));
        remove.set_valign(gtk::Align::Center);
        let weak = Rc::downgrade(self);
        let target = item.clone();
        remove.connect_clicked(move |button| {
            if let Some(this) = weak.upgrade() {
                let target = target.clone();
                let button = button.clone();
                glib::spawn_future_local(async move { this.ask_remove(&target, &button).await });
            }
        });
        line.append(&remove);
        gtk::ListBoxRow::builder()
            .activatable(false)
            .child(&line)
            .build()
    }

    fn subtitle(&self, item: &Item) -> String {
        let mut parts = vec![self.tr.get(match item.source {
            Source::Deb { .. } => "kind-deb",
            Source::Flatpak { .. } => "kind-flatpak",
            Source::AppImage(_) => "kind-appimage",
        })];
        if !item.version.is_empty() {
            parts.push(item.version.clone());
        }
        if let Some(size) = &item.size {
            parts.push(size.clone());
        }
        parts.join(" · ")
    }

    async fn ask_remove(self: Rc<Self>, item: &Item, button: &gtk::Button) {
        self.hide_notice();
        button.set_sensitive(false);
        let plan = match &item.source {
            Source::Deb { name } => match installed::plan_deb_removal(name).await {
                Ok(plan) => plan,
                Err(failure) => {
                    button.set_sensitive(true);
                    self.show_notice(&installer::failure_message(&self.tr, &failure));
                    return;
                }
            },
            _ => RemovalPlan::default(),
        };
        button.set_sensitive(true);
        let this = self.clone();
        let target = item.clone();
        let button = button.clone();
        let confirmed_plan = plan.clone();
        installer::ask_to_remove(
            &self.window,
            &self.tr,
            &item.title,
            &item.source,
            &plan,
            move || {
                let this = this.clone();
                let target = target.clone();
                let plan = confirmed_plan.clone();
                let button = button.clone();
                glib::spawn_future_local(async move {
                    this.remove(&target, &plan, &button).await;
                });
            },
        );
    }

    async fn remove(self: Rc<Self>, item: &Item, plan: &RemovalPlan, button: &gtk::Button) {
        button.set_sensitive(false);
        button.set_label(&self.tr.get("manager-row-removing"));
        let result = installed::remove(&item.source, plan).await;
        match result {
            Ok(()) => {
                let mut args = FluentArgs::new();
                args.set("name", item.title.clone());
                self.show_notice(&self.tr.get_with("manager-removed", &args));
            }
            Err(RemoveError::PackageKit(failure)) if failure.is_cancelled() => {}
            Err(RemoveError::PackageKit(failure)) => {
                self.show_notice(&installer::failure_message(&self.tr, &failure));
            }
            Err(RemoveError::Other(details)) => {
                let mut args = FluentArgs::new();
                args.set("error", details);
                self.show_notice(&self.tr.get_with("installer-failed-details", &args));
            }
        }
        button.set_sensitive(true);
        button.set_label(&self.tr.get("manager-row-remove"));
        self.reload();
    }

    fn show_notice(self: &Rc<Self>, message: &str) {
        self.notice.set_label(message);
        self.notice_bar.set_reveal_child(true);
        let serial = self.notice_serial.get().wrapping_add(1);
        self.notice_serial.set(serial);
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local_once(NOTICE_SECONDS, move || {
            if let Some(this) = weak.upgrade()
                && this.notice_serial.get() == serial
            {
                this.notice_bar.set_reveal_child(false);
            }
        });
    }

    fn hide_notice(&self) {
        self.notice_bar.set_reveal_child(false);
    }
}
