// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 알림: macOS 26 시스템 설정 > 알림을 따른다.
//! - 알림 센터: 미리 보기 보기(항상, 잠금 해제 시, 안 함). 셸 설정 org.buntos.shell notification-previews
//! - 응용 프로그램 알림: 알림을 보낸 적 있는 앱(GNOME의 application-children)과 알림을 쓴다고 밝힌 앱
//!   (데스크톱 항목의 X-GNOME-UsesNotifications). 줄에는 지금 설정을 줄여 보이고, 누르면 시트에서 알림 허용, 알림
//!   스타일(없음, 배너, 알림), 알림 센터에 표시, 알림 사운드 재생을 바꾼다. macOS는 같은 창 안의 하위 화면으로
//!   들어가지만 buntOS 시스템 설정의 세부 설정처럼 시트로 연다(DESIGN-SPEC 15장)
//! - 앱마다의 설정은 GNOME과 같은 GSettings(org.gnome.desktop.notifications.application, 경로는 GNOME 셸처럼
//!   정규화한 앱 ID)와, GNOME 키에 없는 macOS 설정을 두는 buntOS 경로(org.buntos.shell.app-notifications)에 둔다.
//!   셸의 알림 서버(shell/src/notification_policy.rs)가 바로 따른다

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, prelude::*};

use super::{Context, bind_choice, bind_switch, has_key, settings_for, shell_settings};
use crate::widgets::{self, BadgeSize, Sheet, Tint, badge};

const ICON: &str = "preferences-system-notifications-symbolic";
const NOTIFICATIONS_SCHEMA: &str = "org.gnome.desktop.notifications";
const APP_SCHEMA: &str = "org.gnome.desktop.notifications.application";
const APP_PATH: &str = "/org/gnome/desktop/notifications/application/";
/// GNOME 키에 없는 앱별 설정 (shell/data/org.buntos.shell.gschema.xml)
const EXTRA_SCHEMA: &str = "org.buntos.shell.app-notifications";
const EXTRA_PATH: &str = "/org/buntos/shell/app-notifications/";

/// 드롭다운 항목 순서대로의 notification-previews 값 (macOS와 같은 순서)
const PREVIEWS: [&str; 3] = ["always", "when-unlocked", "never"];
const PREVIEW_LABELS: [&str; 3] = [
    "notifications-previews-always",
    "notifications-previews-when-unlocked",
    "notifications-previews-never",
];

/// 알림 스타일 (macOS의 없음, 배너, 알림)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    None,
    Banners,
    Alerts,
}

impl Style {
    /// GNOME의 show-banners와 buntOS의 alerts로 정한다
    fn of(show_banners: bool, alerts: bool) -> Self {
        match (show_banners, alerts) {
            (false, _) => Self::None,
            (true, false) => Self::Banners,
            (true, true) => Self::Alerts,
        }
    }

    /// (show-banners, alerts)
    fn keys(self) -> (bool, bool) {
        match self {
            Self::None => (false, false),
            Self::Banners => (true, false),
            Self::Alerts => (true, true),
        }
    }
}

/// 앱 하나의 알림 설정 (줄의 요약에 쓴다)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct State {
    enabled: bool,
    style: Style,
    sound: bool,
    in_center: bool,
}

impl State {
    /// 줄에 보일 요약의 메시지 ID들 (macOS의 "배너, 사운드"). 보이지도 들리지도 않으면 "끔"
    fn summary_ids(self) -> Vec<&'static str> {
        if !self.enabled {
            return vec!["notifications-summary-off"];
        }
        let mut ids = Vec::new();
        match self.style {
            Style::Banners => ids.push("notifications-summary-banners"),
            Style::Alerts => ids.push("notifications-summary-alerts"),
            Style::None if self.in_center => ids.push("notifications-summary-center"),
            Style::None => {}
        }
        if self.sound {
            ids.push("notifications-summary-sounds");
        }
        if ids.is_empty() {
            ids.push("notifications-summary-off");
        }
        ids
    }
}

/// GNOME 셸의 앱 ID 정규화(messageTray.js의 _canonicalizeId). 셸의 notification_policy::canonical_id와 같은
/// 규칙이다: 소문자로 바꾸고, 영문 소문자·숫자·'-'가 아닌 글자는 '-'로, 이어진 '-'는 하나로 줄인다
fn canonical_id(app_id: &str) -> String {
    let mut out = String::with_capacity(app_id.len());
    for char in app_id.trim_end_matches(".desktop").chars() {
        let char = char.to_ascii_lowercase();
        let char = if char.is_ascii_lowercase() || char.is_ascii_digit() || char == '-' {
            char
        } else {
            '-'
        };
        if !(char == '-' && out.ends_with('-')) {
            out.push(char);
        }
    }
    out
}

/// 경로를 받는 스키마를 연다. 스키마가 없으면 GSettings가 프로세스를 끝내므로 먼저 확인한다
fn settings_at(schema: &str, path: &str) -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(schema, true)?;
    Some(gio::Settings::with_path(schema, path))
}

/// 목록의 앱 하나
struct App {
    info: gio::AppInfo,
    /// .desktop을 뗀 데스크톱 ID
    id: String,
    canonical: String,
    gnome: gio::Settings,
    extra: Option<gio::Settings>,
}

impl App {
    fn new(info: gio::AppInfo) -> Option<Self> {
        let id = info.id()?.trim_end_matches(".desktop").to_owned();
        let canonical = canonical_id(&id);
        if canonical.is_empty() {
            return None;
        }
        let gnome = settings_at(APP_SCHEMA, &format!("{APP_PATH}{canonical}/"))?;
        let extra = settings_at(EXTRA_SCHEMA, &format!("{EXTRA_PATH}{canonical}/"));
        Some(Self {
            info,
            id,
            canonical,
            gnome,
            extra,
        })
    }

    fn state(&self) -> State {
        let extra = |key: &str, default: bool| {
            self.extra
                .as_ref()
                .map_or(default, |extra| extra.boolean(key))
        };
        State {
            enabled: self.gnome.boolean("enable"),
            style: Style::of(self.gnome.boolean("show-banners"), extra("alerts", false)),
            sound: self.gnome.boolean("enable-sound-alerts"),
            in_center: extra("show-in-center", true),
        }
    }

    fn set_style(&self, style: Style) {
        let (banners, alerts) = style.keys();
        let _ = self.gnome.set_boolean("show-banners", banners);
        if let Some(extra) = &self.extra {
            let _ = extra.set_boolean("alerts", alerts);
        }
    }

    /// 셸이 처음 알림을 받았을 때처럼 알림 설정 목록에 올린다 (application-id, application-children)
    fn remember(&self) {
        let desktop = format!("{}.desktop", self.id);
        if self.gnome.string("application-id") != desktop.as_str() {
            let _ = self.gnome.set_string("application-id", &desktop);
        }
        let Some(all) = settings_for(NOTIFICATIONS_SCHEMA) else {
            return;
        };
        let mut children: Vec<String> = all
            .strv("application-children")
            .iter()
            .map(|child| child.to_string())
            .collect();
        if !children.contains(&self.canonical) {
            children.push(self.canonical.clone());
            let children: Vec<&str> = children.iter().map(String::as_str).collect();
            let _ = all.set_strv("application-children", children);
        }
    }
}

/// 알림 설정에 올릴 앱: 알림을 보낸 적 있는 앱과 알림을 쓴다고 밝힌 앱, 이름순
fn apps() -> Vec<App> {
    let mut apps: Vec<App> = Vec::new();
    let mut add = |info: gio::AppInfo| {
        if let Some(app) = App::new(info)
            && !apps.iter().any(|known| known.canonical == app.canonical)
        {
            apps.push(app);
        }
    };
    // 셸이 적어 둔 앱 (지운 앱은 데스크톱 항목이 없어 빠진다)
    if let Some(all) = settings_for(NOTIFICATIONS_SCHEMA) {
        for child in all.strv("application-children") {
            let Some(settings) = settings_at(APP_SCHEMA, &format!("{APP_PATH}{child}/")) else {
                continue;
            };
            let desktop = settings.string("application-id");
            if let Some(info) = gio_desktop(&desktop) {
                add(info);
            }
        }
    }
    for info in gio::AppInfo::all() {
        let uses = info
            .downcast_ref::<gio_unix::DesktopAppInfo>()
            .is_some_and(|desktop| desktop.boolean("X-GNOME-UsesNotifications"));
        if uses && info.should_show() {
            add(info);
        }
    }
    apps.sort_by_key(|app| app.info.display_name().to_lowercase());
    apps
}

fn gio_desktop(id: &str) -> Option<gio::AppInfo> {
    if id.is_empty() {
        return None;
    }
    gio_unix::DesktopAppInfo::new(id).map(|info| info.upcast())
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Red, BadgeSize::Medium),
        &tr.get("page-notifications"),
        &tr.get("notifications-description"),
        None,
    ));

    let center = widgets::group(
        &column,
        Some(&tr.get("notifications-center")),
        Some(&tr.get("notifications-previews-description")),
    );
    let labels = PREVIEW_LABELS.map(|id| tr.get(id));
    let previews = gtk::DropDown::from_strings(&labels.each_ref().map(String::as_str));
    previews.set_valign(gtk::Align::Center);
    match shell_settings().filter(|settings| has_key(settings, "notification-previews")) {
        Some(settings) => bind_choice(&settings, "notification-previews", &previews, &PREVIEWS),
        None => previews.set_sensitive(false),
    }
    center.append(&widgets::row(&tr.get("notifications-previews"), &previews));

    let list = widgets::group(&column, Some(&tr.get("notifications-apps")), None);
    let page = Rc::new(AppList {
        context: context.clone(),
        list: list.clone(),
        apps: RefCell::default(),
    });
    let weak = Rc::downgrade(&page);
    list.connect_row_activated(move |_, row| {
        if let Some(page) = weak.upgrade() {
            page.open(row.widget_name().as_str());
        }
    });
    // 앱 목록은 페이지가 보일 때마다 다시 읽는다 (그사이 알림을 처음 보낸 앱이 생길 수 있다)
    let weak = Rc::downgrade(&page);
    widgets::while_mapped(
        &scroller,
        move || {
            if let Some(page) = weak.upgrade() {
                page.fill();
            }
        },
        || {},
    );
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

fn summary_text(tr: &Translations, state: State) -> String {
    state
        .summary_ids()
        .into_iter()
        .map(|id| tr.get(id))
        .collect::<Vec<_>>()
        .join(", ")
}

fn app_icon(info: &gio::AppInfo, size: i32) -> gtk::Image {
    let icon = gtk::Image::builder().pixel_size(size).build();
    match info.icon() {
        Some(gicon) => icon.set_from_gicon(&gicon),
        None => icon.set_icon_name(Some("application-x-executable")),
    }
    icon
}

/// 응용 프로그램 알림 목록. 줄의 이름(widget_name)이 앱의 정규화한 ID다
struct AppList {
    context: Rc<Context>,
    list: gtk::ListBox,
    apps: RefCell<HashMap<String, Rc<App>>>,
}

impl AppList {
    fn fill(&self) {
        let tr = &self.context.tr;
        widgets::clear(&self.list);
        let apps = apps();
        if apps.is_empty() {
            let (row, _) = widgets::rich_row(None, &tr.get("notifications-no-apps"), None);
            self.list.append(&row);
        }
        let mut known = HashMap::new();
        for app in apps {
            let icon = app_icon(&app.info, 24);
            let summary = summary_text(tr, app.state());
            let (row, accessories) = widgets::rich_row(
                Some(icon.upcast_ref()),
                &app.info.display_name(),
                Some(&summary),
            );
            accessories.append(&widgets::symbol("go-next-symbolic"));
            row.set_activatable(true);
            row.set_widget_name(&app.canonical);
            self.list.append(&row);
            known.insert(app.canonical.clone(), Rc::new(app));
        }
        self.apps.replace(known);
    }

    /// 앱 하나의 알림 시트. 바꾸는 즉시 적용하고(셸이 바로 따른다) [완료]로 닫는다. 닫으면 목록의 요약을 고친다
    fn open(self: &Rc<Self>, canonical: &str) {
        let Some(app) = self.apps.borrow().get(canonical).cloned() else {
            return;
        };
        let tr = &self.context.tr;
        app.remember();
        let icon = app_icon(&app.info, 40);
        icon.set_hexpand(true);
        let tile = gtk::Box::builder()
            .width_request(48)
            .height_request(48)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        tile.append(&icon);
        let sheet = Sheet::new(
            self.context.window().as_ref(),
            Some(&tile),
            &app.info.display_name(),
            Some(&tr.get("notifications-app-description")),
            None,
            &tr.get("done"),
        );
        let group = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["settings-group"])
            .build();

        let (allow_row, allow) = widgets::switch_row(&tr.get("notifications-allow"), None);
        bind_switch(&app.gnome, "enable", &allow);
        group.append(&allow_row);

        let style_row = gtk::ListBoxRow::builder()
            .child(&style_choices(tr, &app))
            .activatable(false)
            .build();
        group.append(&style_row);

        let (center_row, center) =
            widgets::switch_row(&tr.get("notifications-show-in-center"), None);
        match &app.extra {
            Some(extra) => bind_switch(extra, "show-in-center", &center),
            None => center.set_sensitive(false),
        }
        group.append(&center_row);

        let (sound_row, sound) = widgets::switch_row(&tr.get("notifications-play-sound"), None);
        bind_switch(&app.gnome, "enable-sound-alerts", &sound);
        group.append(&sound_row);

        // macOS처럼 알림을 허용하지 않으면 나머지는 흐리게 둔다
        for dependent in [&style_row, &center_row, &sound_row] {
            allow
                .bind_property("active", dependent, "sensitive")
                .sync_create()
                .build();
        }
        sheet.content.append(&group);

        let window = sheet.window.clone();
        sheet.confirm.connect_clicked(move |_| window.close());
        let weak = Rc::downgrade(self);
        sheet.window.connect_destroy(move |_| {
            if let Some(page) = weak.upgrade() {
                page.fill();
            }
        });
        sheet.present();
    }
}

/// 알림 스타일: 그림이 있는 세 단추 (macOS의 없음, 배너, 알림)
fn style_choices(tr: &Translations, app: &Rc<App>) -> gtk::Box {
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .build();
    column.append(
        &gtk::Label::builder()
            .label(tr.get("notifications-style"))
            .xalign(0.0)
            .css_classes(["row-title"])
            .build(),
    );
    let choices = gtk::Box::builder()
        .spacing(16)
        .halign(gtk::Align::Center)
        .build();
    let current = app.state().style;
    let mut first: Option<gtk::ToggleButton> = None;
    for (style, label_id, class) in [
        (Style::None, "notifications-style-none", "style-none"),
        (
            Style::Banners,
            "notifications-style-banners",
            "style-banners",
        ),
        (Style::Alerts, "notifications-style-alerts", "style-alerts"),
    ] {
        let preview = gtk::Box::builder()
            .width_request(84)
            .height_request(56)
            .hexpand(false)
            .css_classes(["notification-style-preview", class])
            .build();
        // 그림 안의 배너 (없음에는 없다)
        if style != Style::None {
            preview.append(
                &gtk::Box::builder()
                    .halign(gtk::Align::End)
                    .valign(gtk::Align::Start)
                    .css_classes(["preview-banner"])
                    .build(),
            );
        }
        let button = gtk::ToggleButton::builder()
            .child(&preview)
            .active(style == current)
            .css_classes(["appearance-choice"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(&tr.get(label_id))]);
        match &first {
            Some(first) => button.set_group(Some(first)),
            None => first = Some(button.clone()),
        }
        let app = app.clone();
        button.connect_toggled(move |button| {
            if button.is_active() {
                app.set_style(style);
            }
        });
        let choice = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        choice.append(&button);
        choice.append(
            &gtk::Label::builder()
                .label(tr.get(label_id))
                .css_classes(["choice-label"])
                .build(),
        );
        choices.append(&choice);
    }
    column.append(&choices);
    column.append(
        &gtk::Label::builder()
            .label(tr.get("notifications-style-description"))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["row-subtitle"])
            .build(),
    );
    column
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_are_canonicalized_like_gnome_shell() {
        // 셸(shell/src/notification_policy.rs)의 시험과 같은 값이다. 둘이 달라지면 설정이 셸에 닿지 않는다
        assert_eq!(canonical_id("org.gnome.Nautilus"), "org-gnome-nautilus");
        assert_eq!(
            canonical_id("org.gnome.Nautilus.desktop"),
            "org-gnome-nautilus"
        );
        assert_eq!(canonical_id("firefox-esr"), "firefox-esr");
        assert_eq!(canonical_id("my_._app"), "my-app");
        assert_eq!(canonical_id("한글앱"), "-");
        assert_eq!(canonical_id(""), "");
    }

    #[test]
    fn styles_map_to_gnome_and_buntos_keys() {
        for style in [Style::None, Style::Banners, Style::Alerts] {
            let (banners, alerts) = style.keys();
            assert_eq!(Style::of(banners, alerts), style);
        }
        // 배너를 끄면 alerts는 보지 않는다
        assert_eq!(Style::of(false, true), Style::None);
    }

    #[test]
    fn summaries_read_like_macos() {
        let state = |enabled, style, sound, in_center| State {
            enabled,
            style,
            sound,
            in_center,
        };
        assert_eq!(
            state(true, Style::Banners, true, true).summary_ids(),
            [
                "notifications-summary-banners",
                "notifications-summary-sounds"
            ]
        );
        assert_eq!(
            state(true, Style::Alerts, false, true).summary_ids(),
            ["notifications-summary-alerts"]
        );
        assert_eq!(
            state(true, Style::None, false, true).summary_ids(),
            ["notifications-summary-center"]
        );
        assert_eq!(
            state(true, Style::None, false, false).summary_ids(),
            ["notifications-summary-off"]
        );
        assert_eq!(
            state(false, Style::Banners, true, true).summary_ids(),
            ["notifications-summary-off"]
        );
    }
}
