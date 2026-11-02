// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설치 창. macOS의 설치 프로그램을 따른다: 왼쪽에 단계(소개, 설치, 요약), 오른쪽에 내용, 아래에 단추다. 설치 파일을
//! 두 번 누르면 뜨고, 파일마다 창이 하나다. 받은 파일을 읽어 보여 주고, 사람이 [설치]를 누른 뒤에야 설치한다.
//! 설치하는 동안에는 창을 닫지 못한다(macOS와 같다)

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::{Duration, SystemTime},
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::{
    appimage, flatpak,
    installed::{self, Existing, RemovalPlan, RemoveError, Source},
    package::{self, Detected, Info, Kind},
    packagekit::{self, FLAG_ALLOW_DOWNGRADE, FLAG_ALLOW_REINSTALL, FLAG_SIMULATE, Failure, Plan},
    ui,
};

/// apt 목록을 다시 받는 간격. 이보다 오래되었으면 의존성을 보기 전에 받는다
const LISTS_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const LISTS_DIR: &str = "/var/lib/apt/lists";

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Introduction,
    Install,
    Summary,
}

struct Wizard {
    tr: Rc<Translations>,
    window: gtk::ApplicationWindow,
    steps: Vec<(Step, gtk::Box)>,
    heading: gtk::Label,
    stack: gtk::Stack,
    // 소개
    icon: gtk::Image,
    name: gtk::Label,
    subtitle: gtk::Label,
    summary: gtk::Label,
    description: gtk::Label,
    facts: gtk::Grid,
    fact_rows: Cell<i32>,
    notice: gtk::Label,
    status_row: gtk::Box,
    spinner: gtk::Spinner,
    status: gtk::Label,
    // 설치
    progress: gtk::ProgressBar,
    progress_text: gtk::Label,
    // 요약
    result_icon: gtk::Image,
    result_title: gtk::Label,
    result_text: gtk::Label,
    trash: gtk::Button,
    // 단추
    remove: gtk::Button,
    close: gtk::Button,
    primary: gtk::Button,

    path: PathBuf,
    kind: Cell<Option<Kind>>,
    info: RefCell<Info>,
    existing: RefCell<Existing>,
    /// 설치나 제거가 도는 중이다. 창을 닫지 못한다
    busy: Cell<bool>,
    /// 다 설치한 앱의 .desktop
    desktop: RefCell<Option<PathBuf>>,
    /// 이 저장소/설치본에 넣었다
    scope: Cell<flatpak::Scope>,
    /// 주 단추가 하는 일
    action: Cell<Action>,
}

#[derive(Clone, Copy, PartialEq)]
enum Action {
    None,
    Install,
    Open,
    Close,
}

/// 파일을 연다: 창을 띄우고 뒤에서 읽는다
pub fn open(app: &gtk::Application, tr: &Rc<Translations>, file: &gio::File) {
    let wizard = Wizard::new(app, tr, file.path().unwrap_or_default());
    wizard.window.present();
    if file.path().is_none() {
        wizard.block(
            &tr.get("installer-title-cannot"),
            &tr.get("installer-not-local"),
        );
        return;
    }
    glib::spawn_future_local(async move { wizard.load().await });
}

fn step_box(title: &str) -> gtk::Box {
    let dot = gtk::Box::builder()
        .valign(gtk::Align::Center)
        .css_classes(["pm-step-dot"])
        .build();
    let row = gtk::Box::builder()
        .spacing(8)
        .css_classes(["pm-step"])
        .build();
    row.append(&dot);
    row.append(&gtk::Label::builder().label(title).xalign(0.0).build());
    row
}

fn secondary_label() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["pm-secondary"])
        .build()
}

impl Wizard {
    fn new(app: &gtk::Application, tr: &Rc<Translations>, path: PathBuf) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title(tr.get("app-name"))
            .default_width(640)
            .default_height(460)
            .resizable(false)
            .css_classes(["pm-window"])
            .build();
        ui::title_bar(&window, &tr.get("app-name"), None);
        ui::follow_color_scheme(&window);

        // 왼쪽 단계
        let steps: Vec<(Step, gtk::Box)> = [
            (Step::Introduction, "installer-step-introduction"),
            (Step::Install, "installer-step-install"),
            (Step::Summary, "installer-step-summary"),
        ]
        .into_iter()
        .map(|(step, id)| (step, step_box(&tr.get(id))))
        .collect();
        let step_list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .width_request(132)
            .css_classes(["pm-steps"])
            .build();
        for (_, row) in &steps {
            step_list.append(row);
        }

        // 소개
        // 아이콘이 없는 패키지는 이 앱의 상자 그림으로 보인다
        let icon = gtk::Image::from_icon_name("org.buntos.PackageManager");
        icon.set_pixel_size(64);
        icon.set_valign(gtk::Align::Start);
        let name = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .css_classes(["pm-package-name"])
            .build();
        let subtitle = secondary_label();
        let summary = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .build();
        let head_text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();
        head_text.append(&name);
        head_text.append(&subtitle);
        head_text.append(&summary);
        let head = gtk::Box::builder().spacing(14).build();
        head.append(&icon);
        head.append(&head_text);
        let description = secondary_label();
        description.set_visible(false);
        let facts = gtk::Grid::builder()
            .row_spacing(4)
            .column_spacing(12)
            .build();
        let notice = secondary_label();
        notice.set_visible(false);
        notice.add_css_class("pm-notice");
        let card_content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .css_classes(["pm-card-content"])
            .build();
        card_content.append(&head);
        card_content.append(&description);
        card_content.append(&facts);
        card_content.append(&notice);
        let card = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&card_content)
            .css_classes(["pm-card"])
            .build();
        let spinner = gtk::Spinner::new();
        let status = secondary_label();
        let status_row = gtk::Box::builder().spacing(8).build();
        status_row.append(&spinner);
        status_row.append(&status);
        status_row.set_visible(false);
        let intro = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        intro.append(&card);
        intro.append(&status_row);

        // 설치
        let progress = gtk::ProgressBar::builder()
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        let progress_text = secondary_label();
        progress_text.set_xalign(0.5);
        progress_text.set_justify(gtk::Justification::Center);
        let installing = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .valign(gtk::Align::Center)
            .css_classes(["pm-progress"])
            .build();
        installing.append(&progress);
        installing.append(&progress_text);
        let installing_card = gtk::Box::builder()
            .vexpand(true)
            .css_classes(["pm-card"])
            .build();
        installing.set_hexpand(true);
        installing_card.append(&installing);

        // 요약
        let result_icon = gtk::Image::builder()
            .pixel_size(40)
            .halign(gtk::Align::Center)
            .css_classes(["pm-result-icon"])
            .build();
        let result_title = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .justify(gtk::Justification::Center)
            .css_classes(["pm-result-title"])
            .build();
        let result_text = secondary_label();
        result_text.set_xalign(0.5);
        result_text.set_justify(gtk::Justification::Center);
        let trash = gtk::Button::builder()
            .label(tr.get("installer-move-to-trash"))
            .halign(gtk::Align::Center)
            .css_classes(["pm-link"])
            .visible(false)
            .build();
        let result = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .css_classes(["pm-result"])
            .build();
        result.append(&result_icon);
        result.append(&result_title);
        result.append(&result_text);
        result.append(&trash);
        let result_card = gtk::Box::builder()
            .vexpand(true)
            .css_classes(["pm-card"])
            .build();
        result_card.append(&result);

        let stack = gtk::Stack::builder()
            .vexpand(true)
            .hexpand(true)
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&intro, Some("intro"));
        stack.add_named(&installing_card, Some("progress"));
        stack.add_named(&result_card, Some("summary"));

        // 아래 단추
        let remove = gtk::Button::builder()
            .label(tr.get("installer-remove"))
            .visible(false)
            .css_classes(["destructive-action"])
            .build();
        let close = gtk::Button::with_label(&tr.get("installer-cancel"));
        let primary = gtk::Button::builder()
            .label(tr.get("installer-install"))
            .sensitive(false)
            .css_classes(["suggested-action"])
            .build();
        let spacer = gtk::Box::builder().hexpand(true).build();
        let buttons = gtk::Box::builder().spacing(8).build();
        buttons.append(&remove);
        buttons.append(&spacer);
        buttons.append(&close);
        buttons.append(&primary);

        let heading = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .css_classes(["pm-heading"])
            .build();
        let main = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .hexpand(true)
            .css_classes(["pm-main"])
            .build();
        main.append(&heading);
        main.append(&stack);
        main.append(&buttons);
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&step_list);
        body.append(&main);
        window.set_child(Some(&body));
        window.set_default_widget(Some(&primary));

        let wizard = Rc::new(Self {
            tr: tr.clone(),
            window,
            steps,
            heading,
            stack,
            icon,
            name,
            subtitle,
            summary,
            description,
            facts,
            fact_rows: Cell::new(0),
            notice,
            status_row,
            spinner,
            status,
            progress,
            progress_text,
            result_icon,
            result_title,
            result_text,
            trash,
            remove,
            close,
            primary,
            path,
            kind: Cell::new(None),
            info: RefCell::new(Info::default()),
            existing: RefCell::new(Existing::None),
            busy: Cell::new(false),
            desktop: RefCell::new(None),
            scope: Cell::new(flatpak::Scope::System),
            action: Cell::new(Action::None),
        });
        wizard.connect();
        wizard.keep_alive_with_window();
        wizard.show_step(Step::Introduction);
        wizard.heading.set_label(&tr.get("installer-title-reading"));
        wizard.name.set_label(&package::display_name(&wizard.path));
        wizard.set_status(Some(&tr.get("installer-status-reading")));
        wizard
    }

    /// 처리기는 약한 참조를 쥔다. 창이 있는 동안 이 객체를 붙들고, 창이 없어지면 놓는다
    fn keep_alive_with_window(self: &Rc<Self>) {
        let holder = RefCell::new(Some(self.clone()));
        self.window.connect_destroy(move |_| {
            holder.borrow_mut().take();
        });
    }

    fn connect(self: &Rc<Self>) {
        // 설치하는 동안에는 닫지 못한다. 관리자 인증 뒤의 일을 도중에 두지 않는다
        let this = Rc::downgrade(self);
        self.window.connect_close_request(move |_| {
            if let Some(this) = this.upgrade()
                && this.busy.get()
            {
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        let this = Rc::downgrade(self);
        self.close.connect_clicked(move |_| {
            if let Some(this) = this.upgrade() {
                this.window.close();
            }
        });
        let this = Rc::downgrade(self);
        self.primary.connect_clicked(move |_| {
            let Some(this) = this.upgrade() else {
                return;
            };
            match this.action.get() {
                Action::Install => {
                    let wizard = this.clone();
                    glib::spawn_future_local(async move { wizard.install().await });
                }
                Action::Open => this.open_app(),
                Action::Close => this.window.close(),
                Action::None => {}
            }
        });
        let this = Rc::downgrade(self);
        self.remove.connect_clicked(move |_| {
            if let Some(this) = this.upgrade() {
                let wizard = this.clone();
                glib::spawn_future_local(async move { wizard.confirm_remove().await });
            }
        });
        let this = Rc::downgrade(self);
        self.trash.connect_clicked(move |button| {
            let Some(this) = this.upgrade() else {
                return;
            };
            match gio::File::for_path(&this.path).trash(gio::Cancellable::NONE) {
                Ok(()) => {
                    button.set_sensitive(false);
                    button.set_label(&this.tr.get("installer-moved-to-trash"));
                }
                Err(err) => eprintln!("bunt-packages: cannot move to the trash: {err}"),
            }
        });
        ui::close_on_escape(&self.window);
    }

    fn show_step(&self, current: Step) {
        let mut passed = true;
        for (step, row) in &self.steps {
            if *step == current {
                row.add_css_class("current");
                row.remove_css_class("done");
                passed = false;
            } else {
                row.remove_css_class("current");
                if passed {
                    row.add_css_class("done");
                } else {
                    row.remove_css_class("done");
                }
            }
        }
    }

    fn set_status(&self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.status.set_label(text);
                self.status_row.set_visible(true);
                self.spinner.start();
            }
            None => {
                self.status_row.set_visible(false);
                self.spinner.stop();
            }
        }
    }

    fn add_fact(&self, label_id: &str, value: &str) {
        let row = self.fact_rows.get();
        self.fact_rows.set(row + 1);
        self.facts.attach(
            &gtk::Label::builder()
                .label(self.tr.get(label_id))
                .xalign(1.0)
                .valign(gtk::Align::Start)
                .css_classes(["pm-fact-label"])
                .build(),
            0,
            row,
            1,
            1,
        );
        self.facts.attach(
            &gtk::Label::builder()
                .label(value)
                .xalign(0.0)
                .hexpand(true)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .selectable(true)
                .css_classes(["pm-fact-value"])
                .build(),
            1,
            row,
            1,
            1,
        );
    }

    /// 설치할 수 없다: 까닭을 보이고 닫기만 남긴다
    fn block(&self, title: &str, reason: &str) {
        self.heading.set_label(title);
        self.set_status(None);
        self.notice.set_label(reason);
        self.notice.set_visible(true);
        self.action.set(Action::Close);
        self.primary.set_label(&self.tr.get("installer-close"));
        self.primary.set_sensitive(true);
        self.close.set_visible(false);
    }

    async fn load(self: Rc<Self>) {
        let path = self.path.clone();
        let detected = gio::spawn_blocking(move || package::detect_file(&path)).await;
        let detected = match detected {
            Ok(Ok(detected)) => detected,
            Ok(Err(err)) => {
                self.block(&self.tr.get("installer-title-cannot"), &err.to_string());
                return;
            }
            Err(_) => return,
        };
        let kind = match detected {
            Detected::Package(kind) => kind,
            Detected::Rpm => {
                self.block(
                    &self.tr.get("installer-title-cannot"),
                    &self.tr.get("installer-rpm"),
                );
                return;
            }
            Detected::Snap => {
                self.block(
                    &self.tr.get("installer-title-cannot"),
                    &self.tr.get("installer-snap"),
                );
                return;
            }
            Detected::Unknown => {
                self.block(
                    &self.tr.get("installer-title-cannot"),
                    &self.tr.get("installer-unknown"),
                );
                return;
            }
        };
        self.kind.set(Some(kind));
        let path = self.path.clone();
        let inspected = gio::spawn_blocking(move || {
            let info = package::inspect(&path, kind)?;
            let foreign = package::foreign_architectures();
            let existing = existing_state(kind, &info);
            let scope = flatpak::install_scope();
            std::io::Result::Ok((info, foreign, existing, scope))
        })
        .await;
        let (info, foreign, existing, scope) = match inspected {
            Ok(Ok(result)) => result,
            Ok(Err(err)) => {
                let mut args = FluentArgs::new();
                args.set("error", err.to_string());
                self.block(
                    &self.tr.get("installer-title-cannot"),
                    &self.tr.get_with("installer-read-failed", &args),
                );
                return;
            }
            Err(_) => return,
        };
        self.scope.set(scope);
        self.show_info(kind, &info);
        *self.existing.borrow_mut() = existing.clone();
        *self.info.borrow_mut() = info.clone();

        if !package::architecture_matches(info.architecture.as_deref(), &foreign) {
            let mut args = FluentArgs::new();
            args.set("arch", info.architecture.clone().unwrap_or_default());
            args.set("host", package::HOST_ARCH);
            self.block(
                &self.tr.get("installer-title-cannot"),
                &self.tr.get_with("installer-wrong-arch", &args),
            );
            return;
        }

        if let Some(reason) = info.blocked {
            self.block(&self.tr.get("installer-title-cannot"), &self.tr.get(reason));
            return;
        }
        self.heading
            .set_label(&with_name(&self.tr, "installer-title-install", &info.title));
        if kind == Kind::FlatpakRepo && existing == Existing::Same {
            self.block(
                &with_name(&self.tr, "installer-title-install", &info.title),
                &self.tr.get("installer-repo-exists"),
            );
            return;
        }
        self.set_trust_notice(kind);
        self.show_existing(kind, &existing);
        if kind == Kind::Deb {
            self.check_dependencies().await;
        } else {
            self.set_status(None);
            self.enable_install();
        }
    }

    fn show_info(&self, kind: Kind, info: &Info) {
        self.name.set_label(&info.title);
        let mut line = Vec::new();
        if let Some(version) = &info.version {
            let mut args = FluentArgs::new();
            args.set("version", version.clone());
            line.push(self.tr.get_with("installer-version", &args));
        }
        line.push(self.tr.get(kind_label(kind)));
        self.subtitle.set_label(&line.join(" · "));
        if let Some(summary) = &info.summary {
            self.summary.set_label(summary);
        }
        if let Some(description) = &info.description {
            self.description.set_label(description);
            self.description.set_visible(true);
        }
        if let Some(icon) = info.icon.as_deref().and_then(ui::texture_from_bytes) {
            self.icon.set_paintable(Some(&icon));
            self.icon.set_pixel_size(64);
        }
        if let Some(size) = info.installed_size {
            self.add_fact("installer-fact-size", &installed::format_size(size));
        }
        if let Some(maintainer) = &info.maintainer {
            self.add_fact("installer-fact-maintainer", maintainer);
        }
        if let Some(homepage) = &info.homepage {
            self.add_fact("installer-fact-homepage", homepage);
        }
        if let Some(source) = &info.source_url {
            self.add_fact("installer-fact-source", source);
        }
        self.add_fact("installer-fact-file", &package::display_name(&self.path));
        self.add_fact(
            "installer-fact-location",
            &self.tr.get(location_label(kind)),
        );
    }

    /// 받은 파일을 믿어도 되는지 판단할 것을 알린다
    fn set_trust_notice(&self, kind: Kind) {
        let id = match kind {
            Kind::Deb => "installer-trust-deb",
            Kind::AppImage => "installer-trust-appimage",
            Kind::FlatpakBundle | Kind::FlatpakRef => "installer-trust-flatpak",
            Kind::FlatpakRepo => "installer-trust-repo",
        };
        self.notice.set_label(&self.tr.get(id));
        self.notice.set_visible(true);
    }

    /// 이미 설치한 것이면 단추와 문구를 바꾼다
    fn show_existing(&self, kind: Kind, existing: &Existing) {
        let installed = !matches!(existing, Existing::None);
        // 저장소 파일은 지우는 길을 두지 않는다 (앱 스토어의 저장소 설정)
        self.remove
            .set_visible(installed && kind != Kind::FlatpakRepo);
        let title = self.info.borrow().title.clone();
        match existing {
            Existing::None => {
                let id = if kind == Kind::FlatpakRepo {
                    "installer-add-repo"
                } else {
                    "installer-install"
                };
                self.primary.set_label(&self.tr.get(id));
            }
            Existing::Same => {
                self.heading
                    .set_label(&with_name(&self.tr, "installer-title-installed", &title));
                self.primary.set_label(&self.tr.get("installer-reinstall"));
            }
            Existing::Older(version) => {
                let mut args = FluentArgs::new();
                args.set("version", version.clone());
                self.add_fact(
                    "installer-fact-installed",
                    &self.tr.get_with("installer-version", &args),
                );
                self.primary.set_label(&self.tr.get("installer-update"));
            }
            Existing::Newer(version) => {
                let mut args = FluentArgs::new();
                args.set("version", version.clone());
                self.add_fact(
                    "installer-fact-installed",
                    &self.tr.get_with("installer-version-newer", &args),
                );
                self.primary.set_label(&self.tr.get("installer-downgrade"));
            }
        }
    }

    fn enable_install(&self) {
        self.action.set(Action::Install);
        self.primary.set_sensitive(true);
    }

    /// .deb: apt 목록이 없거나 오래되었으면 받고, 미리 해 보기로 함께 설치할 패키지를 본다. 의존성을 풀 수 없으면
    /// 암호를 묻기 전에 까닭을 알린다
    async fn check_dependencies(self: &Rc<Self>) {
        let stale = gio::spawn_blocking(lists_are_stale).await.unwrap_or(true);
        if stale {
            self.set_status(Some(&self.tr.get("installer-status-refresh")));
            let this = Rc::downgrade(self);
            let on_progress: Rc<dyn Fn(packagekit::Progress)> = Rc::new(move |progress| {
                if let Some(this) = this.upgrade()
                    && let Some(percent) = progress.percentage
                {
                    let mut args = FluentArgs::new();
                    args.set("percent", percent);
                    this.status
                        .set_label(&this.tr.get_with("installer-status-refresh-percent", &args));
                }
            });
            // 받지 못해도(인터넷 없음) 있는 목록으로 해 본다. 의존성이 없는 패키지는 그대로 설치된다
            if let Err(err) = packagekit::refresh_cache(on_progress).await {
                eprintln!("bunt-packages: refresh: {}", err.details);
            }
        }
        self.set_status(Some(&self.tr.get("installer-status-dependencies")));
        let result = packagekit::install_files(
            self.install_flags() | FLAG_SIMULATE,
            &self.path.to_string_lossy(),
            Rc::new(|_| {}),
        )
        .await;
        self.set_status(None);
        match result {
            Ok(lines) => {
                let plan = packagekit::plan(&lines);
                self.show_plan(&plan);
                self.enable_install();
            }
            Err(failure) => {
                // 이미 있는 판을 다시 넣는 것은 막지 않는다
                if failure.code == Some(packagekit::error::ALL_PACKAGES_ALREADY_INSTALLED) {
                    self.enable_install();
                    return;
                }
                self.block(
                    &with_name(
                        &self.tr,
                        "installer-title-install",
                        &self.info.borrow().title.clone(),
                    ),
                    &failure_message(&self.tr, &failure),
                );
            }
        }
    }

    fn install_flags(&self) -> u64 {
        match *self.existing.borrow() {
            Existing::None | Existing::Older(_) => 0,
            Existing::Same => FLAG_ALLOW_REINSTALL,
            Existing::Newer(_) => FLAG_ALLOW_DOWNGRADE,
        }
    }

    /// 미리 해 보기의 결과: 이 패키지 말고 함께 바뀌는 패키지
    fn show_plan(&self, plan: &Plan) {
        let own = self.info.borrow().id.clone();
        let others = |names: &[String]| -> Vec<String> {
            names.iter().filter(|name| **name != own).cloned().collect()
        };
        let list = |names: &[String]| installed::names_preview(names);
        let install = others(&plan.install);
        if !install.is_empty() {
            let mut args = FluentArgs::new();
            args.set("count", install.len());
            args.set("names", list(&install));
            self.add_fact(
                "installer-fact-also-install",
                &self.tr.get_with("installer-count-names", &args),
            );
        }
        let update = others(&plan.update);
        if !update.is_empty() {
            let mut args = FluentArgs::new();
            args.set("count", update.len());
            args.set("names", list(&update));
            self.add_fact(
                "installer-fact-also-update",
                &self.tr.get_with("installer-count-names", &args),
            );
        }
        let remove = others(&plan.remove);
        if !remove.is_empty() {
            let mut args = FluentArgs::new();
            args.set("count", remove.len());
            args.set("names", list(&remove));
            self.add_fact(
                "installer-fact-also-remove",
                &self.tr.get_with("installer-count-names", &args),
            );
        }
    }

    fn start_work(&self, heading_id: &str, status_id: &str) {
        self.busy.set(true);
        self.show_step(Step::Install);
        self.heading.set_label(&self.tr.get(heading_id));
        self.stack.set_visible_child_name("progress");
        self.progress.set_fraction(0.0);
        self.progress_text.set_label(&self.tr.get(status_id));
        self.primary.set_sensitive(false);
        self.close.set_sensitive(false);
        self.remove.set_visible(false);
    }

    async fn install(self: Rc<Self>) {
        let Some(kind) = self.kind.get() else {
            return;
        };
        self.start_work("installer-title-installing", "installer-status-installing");
        // 진행 막대를 움직이지 못하는 동안(서비스가 백분율을 알리지 않는 동안) 조금씩 흔든다
        let pulse = self.pulse_while_busy();
        let info = self.info.borrow().clone();
        let result: Result<(), InstallError> = match kind {
            Kind::Deb => self.install_deb().await,
            Kind::AppImage => {
                let progress = self.progress.clone();
                let pulse_state = pulse.clone();
                appimage::install(&self.path, &info, move |percent| {
                    pulse_state.set(false);
                    progress.set_fraction(f64::from(percent) / 100.0);
                })
                .await
                .map(|installed| {
                    *self.desktop.borrow_mut() = Some(installed.desktop);
                })
                .map_err(InstallError::Other)
            }
            Kind::FlatpakRepo => flatpak::add_remote(&self.path, &info.id, self.scope.get())
                .await
                .map_err(|details| {
                    if flatpak::is_not_authorized(&details) {
                        InstallError::Cancelled
                    } else {
                        InstallError::Other(details)
                    }
                }),
            Kind::FlatpakBundle | Kind::FlatpakRef => {
                let progress = self.progress.clone();
                let pulse_state = pulse.clone();
                let reinstall = !matches!(*self.existing.borrow(), Existing::None);
                let result = flatpak::install_app(
                    kind,
                    &self.path,
                    self.scope.get(),
                    reinstall,
                    move |line| {
                        if let Some(percent) = flatpak::percent_in(line) {
                            pulse_state.set(false);
                            progress.set_fraction(f64::from(percent) / 100.0);
                        }
                    },
                )
                .await;
                result
                    .map(|scope| {
                        *self.desktop.borrow_mut() = flatpak::desktop_file(&info.id, scope);
                    })
                    .map_err(InstallError::Other)
            }
        };
        pulse.set(false);
        self.busy.set(false);
        self.close.set_sensitive(true);
        match result {
            Ok(()) => self.finish_success(kind),
            Err(InstallError::Cancelled) => self.back_to_introduction(),
            Err(InstallError::PackageKit(failure)) => {
                self.finish_failure(&failure_message(&self.tr, &failure));
            }
            Err(InstallError::Other(details)) => {
                let mut args = FluentArgs::new();
                args.set("error", details);
                self.finish_failure(&self.tr.get_with("installer-failed-details", &args));
            }
        }
    }

    async fn install_deb(self: &Rc<Self>) -> Result<(), InstallError> {
        let this = Rc::downgrade(self);
        let on_progress: Rc<dyn Fn(packagekit::Progress)> = Rc::new(move |progress| {
            let Some(this) = this.upgrade() else {
                return;
            };
            if let Some(id) = status_message(progress.status) {
                this.progress_text.set_label(&this.tr.get(id));
            }
            if let Some(percent) = progress.percentage {
                this.progress.set_fraction(f64::from(percent) / 100.0);
            }
        });
        let result = packagekit::install_files(
            self.install_flags(),
            &self.path.to_string_lossy(),
            on_progress,
        )
        .await;
        match result {
            Ok(_) => {
                let id = self.info.borrow().id.clone();
                let desktop = gio::spawn_blocking(move || installed::desktop_in_package(&id))
                    .await
                    .unwrap_or_default();
                *self.desktop.borrow_mut() = desktop;
                Ok(())
            }
            Err(failure) if failure.is_cancelled() => Err(InstallError::Cancelled),
            Err(failure) => Err(InstallError::PackageKit(failure)),
        }
    }

    /// 백분율이 올 때까지 진행 막대를 흔든다. 돌려준 값을 false로 두면 멈춘다
    fn pulse_while_busy(&self) -> Rc<Cell<bool>> {
        let running = Rc::new(Cell::new(true));
        let state = running.clone();
        let progress = self.progress.clone();
        glib::timeout_add_local(Duration::from_millis(250), move || {
            if !state.get() {
                return glib::ControlFlow::Break;
            }
            // PackageKit은 백분율을 띄엄띄엄 알린다. 0에 머무는 동안만 흔든다
            if progress.fraction() == 0.0 {
                progress.pulse();
            }
            glib::ControlFlow::Continue
        });
        running
    }

    /// 인증 창을 닫았다: 소개로 돌아가 다시 누를 수 있다
    fn back_to_introduction(&self) {
        self.show_step(Step::Introduction);
        let title = self.info.borrow().title.clone();
        self.heading
            .set_label(&with_name(&self.tr, "installer-title-install", &title));
        self.stack.set_visible_child_name("intro");
        self.primary.set_sensitive(true);
        self.remove
            .set_visible(!matches!(*self.existing.borrow(), Existing::None));
        self.status
            .set_label(&self.tr.get("installer-not-authorized"));
        self.status_row.set_visible(true);
        self.spinner.stop();
    }

    fn finish_success(&self, kind: Kind) {
        self.show_step(Step::Summary);
        self.stack.set_visible_child_name("summary");
        self.heading
            .set_label(&self.tr.get("installer-title-summary"));
        self.result_icon
            .set_icon_name(Some("object-select-symbolic"));
        self.result_icon.add_css_class("good");
        self.result_icon.remove_css_class("bad");
        if kind == Kind::FlatpakRepo {
            self.result_title
                .set_label(&self.tr.get("installer-repo-added"));
            self.result_text
                .set_label(&self.tr.get("installer-repo-added-details"));
        } else {
            self.result_title
                .set_label(&self.tr.get("installer-success"));
            self.result_text
                .set_label(&self.tr.get("installer-success-details"));
        }
        self.trash.set_visible(true);
        self.close.set_label(&self.tr.get("installer-close"));
        let openable = self.desktop.borrow().is_some();
        if openable {
            self.action.set(Action::Open);
            self.primary.set_label(&self.tr.get("installer-open"));
            self.primary.set_sensitive(true);
            self.primary.set_visible(true);
        } else {
            self.action.set(Action::Close);
            self.primary.set_label(&self.tr.get("installer-close"));
            self.primary.set_sensitive(true);
            self.close.set_visible(false);
        }
    }

    fn finish_failure(&self, message: &str) {
        self.show_step(Step::Summary);
        self.stack.set_visible_child_name("summary");
        self.heading
            .set_label(&self.tr.get("installer-title-summary"));
        self.result_icon
            .set_icon_name(Some("window-close-symbolic"));
        self.result_icon.add_css_class("bad");
        self.result_icon.remove_css_class("good");
        self.result_title
            .set_label(&self.tr.get("installer-failed"));
        self.result_text.set_label(message);
        self.action.set(Action::Close);
        self.primary.set_label(&self.tr.get("installer-close"));
        self.primary.set_sensitive(true);
        self.close.set_visible(false);
    }

    fn open_app(&self) {
        let Some(desktop) = self.desktop.borrow().clone() else {
            return;
        };
        let shown = desktop.clone();
        ui::launch(&desktop, &self.window, move |err| {
            eprintln!("bunt-packages: cannot open {}: {err}", shown.display());
        });
        self.window.close();
    }

    /// 이미 설치된 것을 지운다 (받은 파일로 연 창에서)
    async fn confirm_remove(self: Rc<Self>) {
        let (Some(kind), info) = (self.kind.get(), self.info.borrow().clone()) else {
            return;
        };
        let source = match kind {
            Kind::Deb => Source::Deb {
                name: info.id.clone(),
            },
            Kind::FlatpakBundle | Kind::FlatpakRef => {
                // 앱이 든 설치본에서 지운다
                let id = info.id.clone();
                let scope = gio::spawn_blocking(move || {
                    flatpak::list_apps()
                        .into_iter()
                        .find(|app| app.id == id)
                        .map(|app| app.scope)
                })
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| self.scope.get());
                Source::Flatpak {
                    id: info.id.clone(),
                    scope,
                }
            }
            Kind::AppImage => match appimage::find_installed(&info.id) {
                Some(installed) => Source::AppImage(installed),
                None => return,
            },
            Kind::FlatpakRepo => return,
        };
        let plan = match &source {
            Source::Deb { name } => match installed::plan_deb_removal(name).await {
                Ok(plan) => plan,
                Err(failure) => {
                    self.finish_failure(&failure_message(&self.tr, &failure));
                    return;
                }
            },
            _ => RemovalPlan::default(),
        };
        let this = self.clone();
        let (target, confirmed_plan) = (source.clone(), plan.clone());
        ask_to_remove(
            &self.window,
            &self.tr,
            &info.title,
            &source,
            &plan,
            move || {
                let wizard = this.clone();
                let source = target.clone();
                let plan = confirmed_plan.clone();
                glib::spawn_future_local(async move { wizard.remove_now(&source, &plan).await });
            },
        );
    }

    async fn remove_now(self: Rc<Self>, source: &Source, plan: &RemovalPlan) {
        self.start_work("installer-title-removing", "installer-status-removing");
        let pulse = self.pulse_while_busy();
        let result = installed::remove(source, plan).await;
        pulse.set(false);
        self.busy.set(false);
        self.close.set_sensitive(true);
        match result {
            Ok(()) => {
                self.show_step(Step::Summary);
                self.stack.set_visible_child_name("summary");
                self.heading
                    .set_label(&self.tr.get("installer-title-summary"));
                self.result_icon
                    .set_icon_name(Some("object-select-symbolic"));
                self.result_icon.add_css_class("good");
                self.result_title
                    .set_label(&self.tr.get("installer-removed"));
                self.result_text.set_label("");
                self.action.set(Action::Close);
                self.primary.set_label(&self.tr.get("installer-close"));
                self.primary.set_sensitive(true);
                self.close.set_visible(false);
            }
            Err(RemoveError::PackageKit(failure)) if failure.is_cancelled() => {
                self.back_to_introduction();
            }
            Err(RemoveError::PackageKit(failure)) => {
                self.finish_failure(&failure_message(&self.tr, &failure));
            }
            Err(RemoveError::Other(details)) => {
                let mut args = FluentArgs::new();
                args.set("error", details);
                self.finish_failure(&self.tr.get_with("installer-failed-details", &args));
            }
        }
    }
}

enum InstallError {
    Cancelled,
    PackageKit(Failure),
    Other(String),
}

/// 지울지 묻는 시트. 함께 지워지는 패키지를 보이고, 시스템의 일부가 끼면 지우지 못하게 한다
pub fn ask_to_remove(
    parent: &impl IsA<gtk::Window>,
    tr: &Rc<Translations>,
    title: &str,
    source: &Source,
    plan: &RemovalPlan,
    on_confirm: impl Fn() + 'static,
) {
    let mut args = FluentArgs::new();
    args.set("name", title.to_owned());
    let question = tr.get_with("remove-question", &args);
    let details = if !plan.blocked.is_empty() {
        let mut args = FluentArgs::new();
        args.set("names", installed::names_preview(&plan.blocked));
        tr.get_with("remove-blocked", &args)
    } else if !plan.also.is_empty() {
        let mut args = FluentArgs::new();
        args.set("names", installed::names_preview(&plan.also));
        tr.get_with("remove-also", &args)
    } else {
        tr.get(match source {
            Source::Deb { .. } => "remove-details-deb",
            Source::Flatpak { .. } => "remove-details-flatpak",
            Source::AppImage(_) => "remove-details-appimage",
        })
    };
    let sheet = ui::Sheet::new(
        parent,
        &question,
        Some(&details),
        &tr.get("remove-cancel"),
        &tr.get("remove-confirm"),
    );
    sheet.confirm.remove_css_class("suggested-action");
    sheet.confirm.add_css_class("destructive-action");
    sheet.confirm.set_sensitive(plan.blocked.is_empty());
    let window = sheet.window.clone();
    sheet.confirm.connect_clicked(move |_| {
        window.close();
        on_confirm();
    });
    sheet.present();
}

/// 이미 설치되었는지 본다. 주 스레드에서 부르지 않는다
fn existing_state(kind: Kind, info: &Info) -> Existing {
    match kind {
        Kind::Deb => installed::deb_state(&info.id, info.version.as_deref()),
        Kind::AppImage => match appimage::find_installed(&info.id) {
            None => Existing::None,
            Some(installed) => match (&installed.version, &info.version) {
                (Some(old), Some(new)) if old != new => Existing::Older(old.clone()),
                _ => Existing::Same,
            },
        },
        Kind::FlatpakBundle | Kind::FlatpakRef => {
            if flatpak::list_apps().iter().any(|app| app.id == info.id) {
                Existing::Same
            } else {
                Existing::None
            }
        }
        Kind::FlatpakRepo => {
            if flatpak::has_remote(&info.id) {
                Existing::Same
            } else {
                Existing::None
            }
        }
    }
}

/// 받아 둔 패키지 목록이 없거나 오래되었는지
fn lists_are_stale() -> bool {
    let Ok(entries) = std::fs::read_dir(LISTS_DIR) else {
        return true;
    };
    let newest = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains("_Packages"))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .max();
    match newest {
        Some(modified) => SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age > LISTS_MAX_AGE),
        None => true,
    }
}

fn with_name(tr: &Translations, id: &str, name: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name.to_owned());
    tr.get_with(id, &args)
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Deb => "kind-deb",
        Kind::AppImage => "kind-appimage",
        Kind::FlatpakBundle | Kind::FlatpakRef => "kind-flatpak",
        Kind::FlatpakRepo => "kind-flatpak-repo",
    }
}

fn location_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Deb => "location-system",
        Kind::AppImage => "location-user",
        Kind::FlatpakBundle | Kind::FlatpakRef => "location-flatpak",
        Kind::FlatpakRepo => "location-repo",
    }
}

/// PackageKit의 단계를 사람의 말로
fn status_message(status: u32) -> Option<&'static str> {
    use packagekit::status::*;
    Some(match status {
        WAIT | SETUP | LOADING_CACHE | QUERY => "installer-status-preparing",
        WAITING_FOR_AUTH => "installer-status-auth",
        WAITING_FOR_LOCK => "installer-status-lock",
        REFRESH_CACHE | DOWNLOAD_REPOSITORY | DOWNLOAD_PACKAGELIST => "installer-status-refresh",
        DEP_RESOLVE => "installer-status-dependencies",
        DOWNLOAD => "installer-status-download",
        SIG_CHECK => "installer-status-check",
        INSTALL | UPDATE | COMMIT => "installer-status-installing",
        REMOVE => "installer-status-removing",
        CLEANUP => "installer-status-cleanup",
        _ => return None,
    })
}

/// PackageKit의 실패를 사람의 말로. 까닭을 모르면 서비스가 준 글을 그대로 보인다
pub fn failure_message(tr: &Translations, failure: &Failure) -> String {
    use packagekit::error::*;
    let id = match failure.code {
        Some(
            NO_NETWORK
            | PACKAGE_DOWNLOAD_FAILED
            | REPO_NOT_AVAILABLE
            | NO_MORE_MIRRORS_TO_TRY
            | CANNOT_FETCH_SOURCES,
        ) => "failure-network",
        Some(DEP_RESOLUTION_FAILED) => "failure-dependencies",
        Some(CANNOT_GET_LOCK) => "failure-lock",
        Some(NO_SPACE_ON_DEVICE) => "failure-space",
        Some(FILE_CONFLICTS | PACKAGE_CONFLICTS) => "failure-conflict",
        Some(INCOMPATIBLE_ARCHITECTURE) => "failure-architecture",
        Some(INVALID_PACKAGE_FILE | PACKAGE_CORRUPT) => "failure-corrupt",
        Some(CANNOT_REMOVE_SYSTEM_PACKAGE) => "failure-system-package",
        None if failure.details.contains("ServiceUnknown") => "failure-service",
        _ => {
            let mut args = FluentArgs::new();
            args.set("error", failure.details.clone());
            return tr.get_with("installer-failed-details", &args);
        }
    };
    let text = tr.get(id);
    if failure.details.is_empty() || id == "failure-service" {
        text
    } else {
        format!("{text}\n\n{}", failure.details)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_read_as_sentences() {
        let tr = Translations::from_source(
            "ko",
            concat!(include_str!("../../i18n/ko/bunt-packages.ftl")),
        );
        let message = failure_message(
            &tr,
            &Failure {
                code: Some(packagekit::error::DEP_RESOLUTION_FAILED),
                details: "libfoo not found".to_owned(),
            },
        );
        assert!(message.starts_with(&tr.get("failure-dependencies")));
        assert!(message.ends_with("libfoo not found"));
        // 모르는 까닭은 서비스의 글을 그대로 보인다
        let message = failure_message(
            &tr,
            &Failure {
                code: Some(999),
                details: "strange".to_owned(),
            },
        );
        assert!(message.contains("strange"));
    }

    #[test]
    fn every_kind_has_labels() {
        let tr = Translations::from_source(
            "ko",
            concat!(include_str!("../../i18n/ko/bunt-packages.ftl")),
        );
        for kind in [
            Kind::Deb,
            Kind::AppImage,
            Kind::FlatpakBundle,
            Kind::FlatpakRef,
            Kind::FlatpakRepo,
        ] {
            assert_ne!(tr.get(kind_label(kind)), kind_label(kind));
            assert_ne!(tr.get(location_label(kind)), location_label(kind));
        }
    }
}
