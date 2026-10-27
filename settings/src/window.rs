// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설정 창: 왼쪽 목록에서 고른 페이지를 오른쪽에 보여 준다

use std::rc::Rc;

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    pages::{Context, Page, interface_settings},
    widgets::Notice,
};

pub struct SettingsWindow {
    window: gtk::ApplicationWindow,
    sidebar: gtk::ListBox,
}

impl SettingsWindow {
    pub fn new(app: &gtk::Application, tr: &Rc<Translations>) -> Self {
        let notice = Notice::new();
        let context = Rc::new(Context {
            tr: tr.clone(),
            notice: notice.clone(),
        });

        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        let sidebar = gtk::ListBox::builder()
            .css_classes(["navigation-sidebar"])
            .build();
        for page in Page::ALL {
            stack.add_named(&page.build(&context), Some(page.name()));
            sidebar.append(&sidebar_row(tr, page));
        }
        sidebar.connect_row_selected(glib::clone!(
            #[weak]
            stack,
            move |_, row| {
                if let Some(page) = row.and_then(|row| Page::ALL.get(row.index() as usize)) {
                    stack.set_visible_child_name(page.name());
                }
            }
        ));
        sidebar.select_row(sidebar.row_at_index(0).as_ref());

        let content = gtk::Overlay::builder().child(&stack).build();
        content.add_overlay(notice.widget());
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(
            &gtk::ScrolledWindow::builder()
                .child(&sidebar)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .width_request(210)
                .css_classes(["settings-sidebar"])
                .build(),
        );
        body.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        body.append(&content);

        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(tr.get("app-title"))
            .default_width(860)
            .default_height(600)
            .child(&body)
            .build();
        window.set_titlebar(Some(&gtk::HeaderBar::new()));
        follow_color_scheme(&window);
        Self { window, sidebar }
    }

    pub fn show(&self, page: Option<Page>) {
        if let Some(page) = page {
            let index = Page::ALL.iter().position(|p| *p == page).unwrap_or(0);
            self.sidebar
                .select_row(self.sidebar.row_at_index(index as i32).as_ref());
        }
        self.window.present();
    }
}

fn sidebar_row(tr: &Translations, page: Page) -> gtk::ListBoxRow {
    let content = gtk::Box::builder().spacing(10).build();
    content.append(&gtk::Image::from_icon_name(page.icon()));
    content.append(&gtk::Label::new(Some(&tr.get(page.title_id()))));
    gtk::ListBoxRow::builder().child(&content).build()
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
