// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AI 에이전트: 이 컴퓨터를 제어할 수 있는 AI와 AI가 접근할 수 없는 앱.
//!
//! 묶음은 macOS 개인정보 보호 및 보안의 앱 목록(기기 제어 및 데이터 접근)을 따른다: 상자 맨 위에 설명,
//! 줄마다 아이콘과 이름(AI는 오른쪽에 스위치), 아래 왼쪽에 [+][−]. 줄을 골라 [−]나 Delete로 뺀다.
//!
//! 정책(/etc/buntos/agent-access.toml)은 root만 쓴다. 켜기·끄기·더하기·빼기는
//! `pkexec bunt-access-helper …`로 하므로 셸의 인증 창이 계정 암호를 묻는다(polkit 동작
//! org.buntos.agent.access.change, 바꿀 때마다 묻는다). [+]는 접근을 요청했던 AI(에이전트 데몬의 요청
//! 기록)와 "다른 프로그램…"(실행 파일 고르기), 설치된 앱 목록을 보여 준다.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    rc::Rc,
};

use bunt_access::{HELPER_PATH, Policy, ProgramError, Request};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

use super::Context;
use crate::widgets::{self, BadgeSize, Sheet, Tint, badge};

const ICON: &str = "org.buntos.Agent-symbolic";
/// .desktop 앱이 아닌 프로그램(CLI)의 아이콘
const PROGRAM_ICON: &str = "utilities-terminal-symbolic";
/// 설치되지 않은 앱의 아이콘
const APP_ICON: &str = "application-x-executable-symbolic";
/// 줄의 아이콘 크기 (macOS 목록의 작은 앱 아이콘)
const ROW_ICON: i32 = 20;

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Pink, BadgeSize::Medium),
        &tr.get("page-agent"),
        &tr.get("agent-description"),
        None,
    ));
    let clients = Section::new(
        &column,
        &tr.get("agent-clients-title"),
        &tr.get("agent-clients-description"),
        &tr.get("agent-empty"),
        (&tr.get("agent-add-client"), &tr.get("agent-remove-client")),
    );
    let apps = Section::new(
        &column,
        &tr.get("agent-apps-title"),
        &tr.get("agent-apps-description"),
        &tr.get("agent-empty"),
        (&tr.get("agent-add-app"), &tr.get("agent-remove-app")),
    );
    column.append(&widgets::description_label(&tr.get("agent-limit-note")));

    let page = Rc::new(AgentPage {
        context: context.clone(),
        clients,
        apps,
        busy: Cell::new(false),
        policy: RefCell::default(),
    });
    page.connect();
    // 다른 곳(다른 창, 명령줄)에서 바꾼 정책과 새 요청이 보이도록 페이지가 보일 때마다 읽는다
    let shown = page.clone();
    widgets::while_mapped(&scroller, move || shown.reload(), || {});
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

/// 묶음 하나: 설명, 줄 목록, 비었을 때의 글, 아래 [+][−]
struct Section {
    frame: gtk::Box,
    list: gtk::ListBox,
    empty: gtk::Label,
    add: gtk::Button,
    remove: gtk::Button,
}

impl Section {
    fn new(
        column: &gtk::Box,
        title: &str,
        description: &str,
        empty: &str,
        (add_label, remove_label): (&str, &str),
    ) -> Section {
        let section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        section.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .css_classes(["settings-group-title"])
                .build(),
        );
        let frame = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["settings-group", "agent-box"])
            .build();
        frame.append(
            &gtk::Label::builder()
                .label(description)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .css_classes(["agent-box-description"])
                .build(),
        );
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["agent-list"])
            .build();
        list.update_property(&[gtk::accessible::Property::Label(title)]);
        frame.append(&list);
        let empty = gtk::Label::builder()
            .label(empty)
            .css_classes(["agent-empty"])
            .build();
        frame.append(&empty);
        let bar = gtk::Box::builder().css_classes(["agent-box-bar"]).build();
        let button = |icon: &str, label: &str| {
            let button = gtk::Button::builder()
                .icon_name(icon)
                .tooltip_text(label)
                .css_classes(["agent-bar-button"])
                .build();
            button.update_property(&[gtk::accessible::Property::Label(label)]);
            button
        };
        let add = button("list-add-symbolic", add_label);
        let remove = button("list-remove-symbolic", remove_label);
        bar.append(&add);
        bar.append(&remove);
        frame.append(&bar);
        section.append(&frame);
        column.append(&section);
        Section {
            frame,
            list,
            empty,
            add,
            remove,
        }
    }

    /// 고른 줄의 번호
    fn selected(&self) -> Option<usize> {
        self.list
            .selected_row()
            .and_then(|row| usize::try_from(row.index()).ok())
    }

    fn select(&self, index: Option<usize>) {
        let row = index
            .and_then(|index| i32::try_from(index).ok())
            .and_then(|index| self.list.row_at_index(index));
        self.list.select_row(row.as_ref());
    }

    fn update_buttons(&self, busy: bool) {
        self.add.set_sensitive(!busy);
        self.remove
            .set_sensitive(!busy && self.list.selected_row().is_some());
    }
}

struct AgentPage {
    context: Rc<Context>,
    clients: Section,
    apps: Section,
    /// 도우미(인증 창)가 도는 동안 다른 바꾸기를 막는다
    busy: Cell<bool>,
    /// 지금 보이는 정책. 줄 번호가 clients·blocked_apps의 번호다
    policy: RefCell<Policy>,
}

impl AgentPage {
    fn connect(self: &Rc<Self>) {
        for section in [&self.clients, &self.apps] {
            let page = Rc::downgrade(self);
            section.list.connect_selected_rows_changed(move |_| {
                if let Some(page) = page.upgrade() {
                    page.update_buttons();
                }
            });
        }
        let page = Rc::downgrade(self);
        self.clients.add.connect_clicked(move |_| {
            if let Some(page) = page.upgrade() {
                page.show_add_client();
            }
        });
        let page = Rc::downgrade(self);
        self.apps.add.connect_clicked(move |_| {
            if let Some(page) = page.upgrade() {
                page.show_add_apps();
            }
        });
        let page = Rc::downgrade(self);
        self.clients.remove.connect_clicked(move |_| {
            if let Some(page) = page.upgrade() {
                page.remove_client();
            }
        });
        let page = Rc::downgrade(self);
        self.apps.remove.connect_clicked(move |_| {
            if let Some(page) = page.upgrade() {
                page.unblock_app();
            }
        });
        // macOS 목록처럼 Delete와 Backspace로도 뺀다
        for (section, clients) in [(&self.clients, true), (&self.apps, false)] {
            let keys = gtk::EventControllerKey::new();
            let page = Rc::downgrade(self);
            keys.connect_key_pressed(move |_, key, _, _| {
                if !matches!(key, gdk::Key::Delete | gdk::Key::BackSpace) {
                    return glib::Propagation::Proceed;
                }
                if let Some(page) = page.upgrade() {
                    if clients {
                        page.remove_client();
                    } else {
                        page.unblock_app();
                    }
                }
                glib::Propagation::Stop
            });
            section.list.add_controller(keys);
        }
    }

    /// 정책 파일을 다시 읽어 줄을 만든다. 고른 줄은 같은 항목에 남긴다
    fn reload(self: &Rc<Self>) {
        let policy = Policy::load();
        let apps = gio::AppInfo::all();
        let (old_client, old_app) = {
            let old = self.policy.borrow();
            (
                self.clients
                    .selected()
                    .and_then(|index| old.clients.get(index).cloned()),
                self.apps
                    .selected()
                    .and_then(|index| old.blocked_apps.get(index).cloned()),
            )
        };

        widgets::clear(&self.clients.list);
        for client in &policy.clients {
            let row = self.client_row(client, &apps);
            self.clients.list.append(&row);
        }
        self.clients.empty.set_visible(policy.clients.is_empty());
        self.clients.select(old_client.and_then(|old| {
            policy
                .clients
                .iter()
                .position(|client| client.exe == old.exe && client.script == old.script)
        }));

        widgets::clear(&self.apps.list);
        for id in &policy.blocked_apps {
            self.apps.list.append(&app_row(id, &apps));
        }
        self.apps.empty.set_visible(policy.blocked_apps.is_empty());
        self.apps
            .select(old_app.and_then(|old| policy.blocked_apps.iter().position(|id| *id == old)));

        self.policy.replace(policy);
        self.update_buttons();
    }

    fn update_buttons(&self) {
        let busy = self.busy.get();
        self.clients.update_buttons(busy);
        self.apps.update_buttons(busy);
    }

    /// AI 줄: 아이콘, 이름, 스위치. 도움말은 실행하는 명령이다
    fn client_row(
        self: &Rc<Self>,
        client: &bunt_access::AllowedClient,
        apps: &[gio::AppInfo],
    ) -> gtk::ListBoxRow {
        let icon = program_icon(&client.exe, client.script.as_deref(), apps);
        let (row, accessories) = widgets::rich_row(Some(&icon), &client.name, None);
        row.set_tooltip_text(Some(&command_line(&client.exe, client.script.as_deref())));
        let switch = gtk::Switch::builder()
            .active(client.enabled)
            .valign(gtk::Align::Center)
            .build();
        switch.update_property(&[gtk::accessible::Property::Label(&client.name)]);
        let page = Rc::downgrade(self);
        let (exe, script) = (client.exe.clone(), client.script.clone());
        switch.connect_active_notify(move |switch| {
            let Some(page) = page.upgrade() else {
                return;
            };
            let command = if switch.is_active() {
                "enable-client"
            } else {
                "disable-client"
            };
            page.change(client_arguments(command, None, &exe, script.as_deref()));
        });
        accessories.append(&switch);
        row
    }

    fn remove_client(self: &Rc<Self>) {
        let client = self
            .clients
            .selected()
            .and_then(|index| self.policy.borrow().clients.get(index).cloned());
        if let Some(client) = client {
            self.change(client_arguments(
                "remove-client",
                None,
                &client.exe,
                client.script.as_deref(),
            ));
        }
    }

    fn unblock_app(self: &Rc<Self>) {
        let app = self
            .apps
            .selected()
            .and_then(|index| self.policy.borrow().blocked_apps.get(index).cloned());
        if let Some(app) = app {
            self.change(vec!["unblock-app".into(), app]);
        }
    }

    /// 특권 도우미로 정책을 바꾼다. 인증 창이 암호를 묻는 동안 묶음을 잠그고, 끝나면 파일을 다시 읽는다
    /// (취소하거나 실패하면 스위치가 원래대로 돌아간다)
    fn change(self: &Rc<Self>, arguments: Vec<String>) {
        if self.busy.replace(true) {
            return;
        }
        self.set_locked(true);
        let page = self.clone();
        glib::spawn_future_local(async move {
            let outcome = run_helper(&arguments).await;
            page.busy.set(false);
            page.set_locked(false);
            match outcome {
                Outcome::Changed | Outcome::Dismissed => {}
                Outcome::NotAuthorized => page
                    .context
                    .notify(&page.context.tr.get("agent-not-authorized")),
                Outcome::Failed(message) => {
                    page.context.notify_error("agent-change-failed", message)
                }
            }
            page.reload();
        });
    }

    fn set_locked(&self, locked: bool) {
        self.clients.frame.set_sensitive(!locked);
        self.apps.frame.set_sensitive(!locked);
        self.update_buttons();
    }

    /// [+] AI: 접근을 요청했던 AI 가운데 하나를 고르거나 다른 프로그램을 고른다
    fn show_add_client(self: &Rc<Self>) {
        let tr = &self.context.tr;
        let sheet = Sheet::new(
            self.context.window().as_ref(),
            Some(&badge(ICON, Tint::Pink, BadgeSize::Medium)),
            &tr.get("agent-client-sheet-title"),
            Some(&tr.get("agent-client-sheet-description")),
            Some(&tr.get("agent-cancel")),
            &tr.get("agent-add"),
        );
        let requests: Vec<Request> = {
            let policy = self.policy.borrow();
            // 이미 목록에 있거나 지워진 프로그램은 빼고 보인다
            bunt_access::requests()
                .into_iter()
                .filter(|request| {
                    policy
                        .find(&request.exe, request.script.as_deref())
                        .is_none()
                })
                .filter(|request| {
                    request.exe.is_file() && request.script.as_deref().is_none_or(Path::is_file)
                })
                .collect()
        };
        let apps = gio::AppInfo::all();
        let list = picker_list(&tr.get("agent-client-sheet-empty"));
        list.set_selection_mode(gtk::SelectionMode::Single);
        for request in &requests {
            let icon = program_icon(&request.exe, request.script.as_deref(), &apps);
            let path = request.script.as_deref().unwrap_or(&request.exe);
            let (row, _) = widgets::rich_row(
                Some(&icon),
                &request.name,
                Some(&path.display().to_string()),
            );
            row.set_tooltip_text(Some(&command_line(&request.exe, request.script.as_deref())));
            list.append(&row);
        }
        sheet.content.append(&picker_frame(&list));
        let other = gtk::Button::builder()
            .label(tr.get("agent-other-program"))
            .halign(gtk::Align::Start)
            .build();
        sheet.content.append(&other);

        sheet.confirm.set_sensitive(false);
        let confirm = sheet.confirm.clone();
        list.connect_selected_rows_changed(move |list| {
            confirm.set_sensitive(list.selected_row().is_some());
        });
        let requests = Rc::new(requests);
        let add = {
            let page = Rc::downgrade(self);
            let window = sheet.window.clone();
            let list = list.clone();
            let requests = requests.clone();
            move || {
                let request = list
                    .selected_row()
                    .and_then(|row| usize::try_from(row.index()).ok())
                    .and_then(|index| requests.get(index));
                let (Some(page), Some(request)) = (page.upgrade(), request) else {
                    return;
                };
                window.close();
                page.change(client_arguments(
                    "add-client",
                    Some(&request.name),
                    &request.exe,
                    request.script.as_deref(),
                ));
            }
        };
        let add = Rc::new(add);
        let on_confirm = add.clone();
        sheet.confirm.connect_clicked(move |_| on_confirm());
        list.connect_row_activated(move |_, _| add());
        let page = Rc::downgrade(self);
        let window = sheet.window.clone();
        other.connect_clicked(move |_| {
            if let Some(page) = page.upgrade() {
                page.choose_program(&window);
            }
        });
        sheet.present();
    }

    /// "다른 프로그램…": 실행 파일이나 스크립트를 골라 더한다
    fn choose_program(self: &Rc<Self>, sheet: &gtk::Window) {
        let dialog = gtk::FileDialog::builder()
            .title(self.context.tr.get("agent-program-dialog-title"))
            .modal(true)
            .build();
        if let Some(home) = glib::home_dir().to_str().map(PathBuf::from) {
            // AI 명령줄 도구는 흔히 ~/.local/bin에 있다
            let bin = home.join(".local/bin");
            let start = if bin.is_dir() { bin } else { home };
            dialog.set_initial_folder(Some(&gio::File::for_path(start)));
        }
        let page = Rc::downgrade(self);
        let parent = sheet.clone();
        let sheet = sheet.clone();
        dialog.open(Some(&parent), gio::Cancellable::NONE, move |result| {
            let (Some(page), Ok(file)) = (page.upgrade(), result) else {
                return;
            };
            let Some(path) = file.path() else {
                return;
            };
            let tr = &page.context.tr;
            match bunt_access::program_client(&path) {
                Ok(client) => {
                    sheet.close();
                    page.change(client_arguments(
                        "add-client",
                        Some(&client.name),
                        &client.exe,
                        client.script.as_deref(),
                    ));
                }
                Err(ProgramError::Shell) => page.context.notify(&tr.get("agent-program-shell")),
                Err(ProgramError::Interpreter(_)) => {
                    page.context.notify(&tr.get("agent-program-interpreter"))
                }
                Err(ProgramError::Io(err)) => {
                    page.context.notify_error("agent-program-failed", err)
                }
            }
        });
    }

    /// [+] 앱: 설치된 앱에서 여럿을 골라 막는다
    fn show_add_apps(self: &Rc<Self>) {
        let tr = &self.context.tr;
        let sheet = Sheet::new(
            self.context.window().as_ref(),
            Some(&badge(ICON, Tint::Pink, BadgeSize::Medium)),
            &tr.get("agent-app-sheet-title"),
            Some(&tr.get("agent-app-sheet-description")),
            Some(&tr.get("agent-cancel")),
            &tr.get("agent-add"),
        );
        let search = gtk::SearchEntry::builder()
            .placeholder_text(tr.get("agent-app-search"))
            .css_classes(["search"])
            .build();
        search.update_property(&[gtk::accessible::Property::Label(
            &tr.get("agent-app-search"),
        )]);
        sheet.content.append(&search);

        let mut apps: Vec<(String, gio::AppInfo)> = {
            let policy = self.policy.borrow();
            gio::AppInfo::all()
                .into_iter()
                .filter(|app| app.should_show())
                .filter_map(|app| {
                    let id = app.id()?.strip_suffix(".desktop")?.to_owned();
                    (!policy.blocks(&id)).then_some((id, app))
                })
                .collect()
        };
        apps.sort_by_cached_key(|(_, app)| app.display_name().to_lowercase());
        let list = picker_list(&tr.get("agent-app-sheet-empty"));
        list.set_activate_on_single_click(true);
        let checks = Rc::new(RefCell::new(BTreeSet::<usize>::new()));
        let names: Vec<String> = apps
            .iter()
            .map(|(_, app)| app.display_name().to_lowercase())
            .collect();
        for (_, app) in &apps {
            let icon = app_icon(Some(app));
            let (row, accessories) = widgets::rich_row(Some(&icon), &app.display_name(), None);
            row.set_activatable(true);
            let check = gtk::Image::builder()
                .icon_name("object-select-symbolic")
                .css_classes(["row-check"])
                .visible(false)
                .build();
            accessories.append(&check);
            list.append(&row);
        }
        sheet.content.append(&picker_frame(&list));

        sheet.confirm.set_sensitive(false);
        let confirm = sheet.confirm.clone();
        let checked = checks.clone();
        list.connect_row_activated(move |_, row| {
            let Ok(index) = usize::try_from(row.index()) else {
                return;
            };
            let on = {
                let mut checked = checked.borrow_mut();
                if !checked.remove(&index) {
                    checked.insert(index);
                }
                checked.contains(&index)
            };
            if let Some(check) = row_check(row) {
                check.set_visible(on);
            }
            let state = if on {
                gtk::AccessibleTristate::True
            } else {
                gtk::AccessibleTristate::False
            };
            row.update_state(&[gtk::accessible::State::Checked(state)]);
            confirm.set_sensitive(!checked.borrow().is_empty());
        });
        list.set_filter_func(glib::clone!(
            #[weak]
            search,
            #[upgrade_or]
            true,
            move |row| {
                let query = search.text().to_lowercase();
                let query = query.trim();
                query.is_empty()
                    || usize::try_from(row.index())
                        .ok()
                        .and_then(|index| names.get(index))
                        .is_some_and(|name| name.contains(query))
            }
        ));
        search.connect_search_changed(glib::clone!(
            #[weak]
            list,
            move |_| list.invalidate_filter()
        ));

        let page = Rc::downgrade(self);
        let window = sheet.window.clone();
        sheet.confirm.connect_clicked(move |_| {
            let Some(page) = page.upgrade() else {
                return;
            };
            let mut arguments = vec!["block-app".to_owned()];
            arguments.extend(
                checks
                    .borrow()
                    .iter()
                    .filter_map(|index| apps.get(*index))
                    .map(|(id, _)| id.clone()),
            );
            if arguments.len() > 1 {
                window.close();
                page.change(arguments);
            }
        });
        sheet.present();
        search.grab_focus();
    }
}

/// 막은 앱 줄: 아이콘과 이름. 지운 앱은 데스크톱 ID로 보인다
fn app_row(id: &str, apps: &[gio::AppInfo]) -> gtk::ListBoxRow {
    let app = find_app(apps, id);
    let icon = app_icon(app);
    let name = app.map_or_else(|| id.to_owned(), |app| app.display_name().to_string());
    let (row, _) = widgets::rich_row(Some(&icon), &name, None);
    row.set_tooltip_text(Some(id));
    row
}

fn find_app<'a>(apps: &'a [gio::AppInfo], id: &str) -> Option<&'a gio::AppInfo> {
    let file = format!("{id}.desktop");
    apps.iter()
        .find(|app| app.id().as_deref() == Some(file.as_str()))
}

fn app_icon(app: Option<&gio::AppInfo>) -> gtk::Widget {
    match app.and_then(|app| app.icon()) {
        Some(icon) => {
            let image = gtk::Image::from_gicon(&icon);
            image.set_pixel_size(ROW_ICON);
            image.upcast()
        }
        None => badge(APP_ICON, Tint::Gray, BadgeSize::Small).upcast(),
    }
}

/// AI의 아이콘: Exec가 맞는 .desktop 앱의 아이콘, 없으면 명령줄 프로그램 아이콘
fn program_icon(exe: &Path, script: Option<&Path>, apps: &[gio::AppInfo]) -> gtk::Widget {
    let app = bunt_access::desktop_app(exe, script).and_then(|app| find_app(apps, &app.id));
    match app.and_then(|app| app.icon()) {
        Some(icon) => {
            let image = gtk::Image::from_gicon(&icon);
            image.set_pixel_size(ROW_ICON);
            image.upcast()
        }
        None => badge(PROGRAM_ICON, Tint::Graphite, BadgeSize::Small).upcast(),
    }
}

/// 고르기 목록 (시트 안)
fn picker_list(empty: &str) -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["agent-list", "agent-picker"])
        .build();
    list.set_placeholder(Some(
        &gtk::Label::builder()
            .label(empty)
            .wrap(true)
            .css_classes(["agent-empty"])
            .build(),
    ));
    list
}

/// 묶음 상자처럼 보이는 스크롤 틀
fn picker_frame(list: &gtk::ListBox) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(160)
        .max_content_height(300)
        .propagate_natural_height(true)
        .child(list)
        .css_classes(["file-types-frame"])
        .build()
}

/// 앱 고르기 줄의 체크 표시 (rich_row의 부속 상자 안)
fn row_check(row: &gtk::ListBoxRow) -> Option<gtk::Image> {
    let content = row.child()?;
    let accessories = content.last_child()?;
    accessories.first_child()?.downcast().ok()
}

/// 도우미 인자: 명령 [이름] 실행 파일 [스크립트]
fn client_arguments(
    command: &str,
    name: Option<&str>,
    exe: &Path,
    script: Option<&Path>,
) -> Vec<String> {
    let mut arguments = vec![command.to_owned()];
    arguments.extend(name.map(str::to_owned));
    arguments.push(exe.display().to_string());
    arguments.extend(script.map(|script| script.display().to_string()));
    arguments
}

fn command_line(exe: &Path, script: Option<&Path>) -> String {
    match script {
        Some(script) => format!("{} {}", exe.display(), script.display()),
        None => exe.display().to_string(),
    }
}

enum Outcome {
    Changed,
    /// 인증 창에서 취소했다 (pkexec 126)
    Dismissed,
    /// 암호를 맞히지 못했거나 권한이 없다 (pkexec 127)
    NotAuthorized,
    Failed(String),
}

/// `pkexec bunt-access-helper 인자…`
async fn run_helper(arguments: &[String]) -> Outcome {
    let mut argv: Vec<&OsStr> = vec![OsStr::new("pkexec"), OsStr::new(HELPER_PATH)];
    argv.extend(arguments.iter().map(OsStr::new));
    let process = match gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_PIPE,
    ) {
        Ok(process) => process,
        Err(err) => return Outcome::Failed(err.to_string()),
    };
    let stderr = match process.communicate_utf8_future(None).await {
        Ok((_, stderr)) => stderr.map(|text| text.to_string()).unwrap_or_default(),
        Err(err) => return Outcome::Failed(err.to_string()),
    };
    // 알림에는 마지막 줄만 보인다 ("bunt-access-helper: …")
    let message = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| {
            line.trim_start_matches("bunt-access-helper: ")
                .trim()
                .to_owned()
        })
        .unwrap_or_default();
    if !process.has_exited() {
        return Outcome::Failed(message);
    }
    match process.exit_status() {
        0 => Outcome::Changed,
        126 => Outcome::Dismissed,
        127 if message.is_empty() || stderr.contains("Not authorized") => Outcome::NotAuthorized,
        _ => Outcome::Failed(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_helper_arguments() {
        assert_eq!(
            client_arguments(
                "add-client",
                Some("Claude"),
                Path::new("/usr/bin/node"),
                Some(Path::new("/opt/cli.js"))
            ),
            ["add-client", "Claude", "/usr/bin/node", "/opt/cli.js"]
        );
        assert_eq!(
            client_arguments("disable-client", None, Path::new("/opt/agent"), None),
            ["disable-client", "/opt/agent"]
        );
        assert_eq!(
            command_line(Path::new("/usr/bin/node"), Some(Path::new("/opt/cli.js"))),
            "/usr/bin/node /opt/cli.js"
        );
    }
}
