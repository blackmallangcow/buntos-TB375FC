// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 기본 앱: 링크와 파일을 열 앱. 웹 브라우저, 메일, 텍스트처럼 종류마다 한 앱을 고르고, Windows의
//! "파일 형식별 기본 앱"처럼 확장자마다 따로 고를 수도 있다. macOS에는 이 페이지가 없다(기본 웹
//! 브라우저는 데스크탑 및 Dock에, 나머지는 Finder의 "정보 가져오기 > 다음으로 열기"에 있다).
//!
//! XDG 방식대로 MIME 형식마다 앱을 정해 `~/.config/mimeapps.list`에 적는다(GIO). 확장자는
//! shared-mime-info가 형식에 묶어 두므로 같은 형식의 확장자(.jpg와 .jpeg)는 함께 바뀐다.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use gtk4::{self as gtk, gio, glib, prelude::*};

use super::Context;
use crate::widgets::{self, BadgeSize, Sheet, Tint, badge};

const ICON: &str = "view-app-grid-symbolic";

/// 앱 종류 하나. 첫 형식이 대표다: 그 형식을 여는 앱을 늘어놓고, 고르면 그 앱이 여는 나머지
/// 형식에도 정한다
pub struct Category {
    /// 이름의 Fluent 메시지 ID
    title: &'static str,
    icon: &'static str,
    tint: Tint,
    types: &'static [&'static str],
}

const WEB_BROWSER: Category = Category {
    title: "default-apps-web",
    icon: "web-browser-symbolic",
    tint: Tint::Blue,
    types: &[
        "x-scheme-handler/https",
        "x-scheme-handler/http",
        "text/html",
        "application/xhtml+xml",
    ],
};

/// 데스크탑 및 Dock 페이지의 "기본 웹 브라우저" 행 (macOS와 같은 자리). 웹 브라우저와 같은 형식을 바꾼다
pub const DEFAULT_WEB_BROWSER: Category = Category {
    title: "desktop-dock-browser",
    ..WEB_BROWSER
};

const CATEGORIES: [&Category; 8] = [
    &WEB_BROWSER,
    &Category {
        title: "default-apps-mail",
        icon: "mail-unread-symbolic",
        tint: Tint::Blue,
        types: &["x-scheme-handler/mailto"],
    },
    &Category {
        title: "default-apps-text",
        icon: "text-x-generic-symbolic",
        tint: Tint::Gray,
        types: &["text/plain"],
    },
    &Category {
        title: "default-apps-images",
        icon: "image-x-generic-symbolic",
        tint: Tint::Cyan,
        types: &[
            "image/png",
            "image/jpeg",
            "image/gif",
            "image/webp",
            "image/bmp",
            "image/tiff",
            "image/svg+xml",
            "image/avif",
            "image/heif",
        ],
    },
    &Category {
        title: "default-apps-music",
        icon: "audio-x-generic-symbolic",
        tint: Tint::Pink,
        types: &[
            "audio/mpeg",
            "audio/flac",
            "audio/ogg",
            "audio/x-vorbis+ogg",
            "audio/x-wav",
            "audio/mp4",
            "audio/aac",
        ],
    },
    &Category {
        title: "default-apps-video",
        icon: "video-x-generic-symbolic",
        tint: Tint::Indigo,
        types: &[
            "video/mp4",
            "video/webm",
            "video/x-matroska",
            "video/quicktime",
            "video/x-msvideo",
            "video/mpeg",
        ],
    },
    &Category {
        title: "default-apps-pdf",
        icon: "x-office-document-symbolic",
        tint: Tint::Gray,
        types: &["application/pdf"],
    },
    &Category {
        title: "default-apps-folders",
        icon: "folder-symbolic",
        tint: Tint::Blue,
        types: &["inode/directory"],
    },
];

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Blue, BadgeSize::Medium),
        &tr.get("page-default-apps"),
        &tr.get("default-apps-description"),
        None,
    ));

    let list = widgets::group(
        &column,
        None,
        Some(&tr.get("default-apps-categories-description")),
    );
    let mut rows = 0;
    for category in CATEGORIES {
        if let Some(row) = category_row(context, category, true) {
            list.append(&row);
            rows += 1;
        }
    }
    if rows == 0 {
        // 기본 앱을 고를 앱이 하나도 없으면(최소 이미지) 묶음을 숨긴다
        widgets::set_group_visible(&list, false);
    }

    let footer = widgets::footer(&column);
    let by_type = gtk::Button::with_label(&tr.get("default-apps-types-open"));
    let page = context.clone();
    by_type.connect_clicked(move |_| show_file_types(&page));
    footer.append(&by_type);
    scroller.upcast()
}

/// 종류 하나의 행: (배지,) 이름과 앱 고르기. 그 종류를 열 앱이 없으면 None
pub fn category_row(
    context: &Rc<Context>,
    category: &'static Category,
    with_badge: bool,
) -> Option<gtk::ListBoxRow> {
    let (apps, selected) = candidates(category.types[0]);
    if apps.n_items() == 0 {
        return None;
    }
    let dropdown = app_dropdown();
    dropdown.set_model(Some(&apps));
    dropdown.set_selected(selected.unwrap_or(gtk::INVALID_LIST_POSITION));
    let page = context.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        if let Some(app) = dropdown.selected_item().and_downcast::<gio::AppInfo>() {
            set_default(&page, &app, category.types);
        }
    });
    let leading = with_badge.then(|| badge(category.icon, category.tint, BadgeSize::Small));
    let (row, accessories) = widgets::rich_row(
        leading.as_ref().map(|badge| badge.upcast_ref()),
        &context.tr.get(category.title),
        None,
    );
    accessories.append(&dropdown);
    Some(row)
}

/// 이 형식을 열 수 있는 앱과 그 가운데 지금 기본 앱의 자리. 숨긴 앱(NoDisplay)은 빼지만, 지금
/// 기본 앱이면 목록에 없더라도 맨 앞에 넣는다
fn candidates(mime: &str) -> (gio::ListStore, Option<u32>) {
    let mut apps: Vec<gio::AppInfo> = gio::AppInfo::all_for_type(mime)
        .into_iter()
        .filter(|app| app.should_show())
        .collect();
    let default = gio::AppInfo::default_for_type(mime, false);
    if let Some(default) = &default
        && !apps.iter().any(|app| app.equal(default))
    {
        apps.insert(0, default.clone());
    }
    let selected = default
        .and_then(|default| apps.iter().position(|app| app.equal(&default)))
        .map(|index| index as u32);
    let store = gio::ListStore::new::<gio::AppInfo>();
    for app in &apps {
        store.append(app);
    }
    (store, selected)
}

/// 앱 아이콘과 이름을 보이는 드롭다운. 목록은 나중에 넣는다
fn app_dropdown() -> gtk::DropDown {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let line = gtk::Box::builder().spacing(6).build();
        line.append(&gtk::Image::builder().pixel_size(16).build());
        line.append(&gtk::Label::builder().xalign(0.0).build());
        item.set_child(Some(&line));
    });
    factory.connect_bind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(app), Some(line)) = (item.item().and_downcast::<gio::AppInfo>(), item.child())
        else {
            return;
        };
        if let Some(image) = line.first_child().and_downcast::<gtk::Image>() {
            match app.icon() {
                Some(icon) => image.set_from_gicon(&icon),
                None => image.set_icon_name(Some("application-x-executable")),
            }
        }
        if let Some(label) = line.last_child().and_downcast::<gtk::Label>() {
            // display_name은 긴 이름(X-GNOME-FullName, "Firefox ESR 웹 브라우저")일 수 있다
            label.set_label(&app.name());
        }
    });
    gtk::DropDown::builder()
        .factory(&factory)
        .valign(gtk::Align::Center)
        .build()
}

/// 앱을 이 형식들의 기본 앱으로 정한다. 대표 형식(첫째)은 늘, 나머지는 그 앱이 열 수 있을 때만
fn set_default(context: &Context, app: &gio::AppInfo, types: &[&str]) {
    for (index, mime) in types.iter().enumerate() {
        if gio::AppInfo::default_for_type(mime, false).is_some_and(|current| current.equal(app)) {
            continue;
        }
        let opens = index == 0
            || gio::AppInfo::all_for_type(mime)
                .iter()
                .any(|other| other.equal(app));
        if !opens {
            continue;
        }
        if let Err(err) = app.set_as_default_for_type(mime) {
            context.notify_error("default-apps-error", err.message());
            return;
        }
    }
}

/// 파일 형식 하나와 그 확장자 (점 없이, 소문자)
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileType {
    mime: String,
    extensions: Vec<String>,
}

impl FileType {
    /// 찾는 말(소문자)이 확장자의 앞부분이거나 형식 이름·설명에 들어 있는지.
    /// 점으로 시작하면(".jp") 확장자만 본다
    fn matches(&self, query: &str, description: &str) -> bool {
        let query = query.trim();
        let starts = |prefix: &str| {
            self.extensions
                .iter()
                .any(|extension| extension.starts_with(prefix))
        };
        if let Some(extension) = query.strip_prefix('.') {
            return starts(extension);
        }
        query.is_empty()
            || starts(query)
            || self.mime.contains(query)
            || description.to_lowercase().contains(query)
    }
}

/// shared-mime-info의 globs2(가중치:형식:패턴[:플래그])에서 확장자 패턴(*.ext)만 읽어, 확장자마다
/// 가중치가 가장 큰 형식을 고른다 (.png는 image/apng에도 있지만 image/png가 이긴다). 가중치가 같으면
/// 먼저 읽은 것이다. 값은 (가중치, 형식)
fn parse_globs(text: &str, extensions: &mut BTreeMap<String, (u32, String)>) {
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(':');
        let (Some(weight), Some(mime), Some(pattern)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let (Ok(weight), Some(extension)) = (weight.parse::<u32>(), pattern.strip_prefix("*."))
        else {
            continue;
        };
        if extension.is_empty() || extension.contains(['*', '?', '[', ']']) {
            continue;
        }
        let extension = extension.to_lowercase();
        if extensions
            .get(&extension)
            .is_none_or(|(best, _)| weight > *best)
        {
            extensions.insert(extension, (weight, mime.to_owned()));
        }
    }
}

/// 확장자를 형식별로 묶는다. 한 형식 안에서는 짧은 것부터, 그다음 이름순이다 (.jpg, .jpeg)
fn group_by_type(extensions: BTreeMap<String, (u32, String)>) -> BTreeMap<String, Vec<String>> {
    let mut types: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (extension, (_, mime)) in extensions {
        types.entry(mime).or_default().push(extension);
    }
    for extensions in types.values_mut() {
        extensions.sort_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)));
    }
    types
}

/// 열 앱이 있는 파일 형식. 첫 확장자의 이름순이다
fn file_types() -> Vec<FileType> {
    let mut extensions = BTreeMap::new();
    let dirs = std::iter::once(glib::user_data_dir()).chain(glib::system_data_dirs());
    for dir in dirs {
        if let Ok(text) = std::fs::read_to_string(dir.join("mime/globs2")) {
            parse_globs(&text, &mut extensions);
        }
    }
    let mut types: Vec<FileType> = group_by_type(extensions)
        .into_iter()
        .filter(|(mime, _)| {
            gio::AppInfo::all_for_type(mime)
                .iter()
                .any(|app| app.should_show())
        })
        .map(|(mime, extensions)| FileType { mime, extensions })
        .collect();
    types.sort_by(|a, b| a.extensions.cmp(&b.extensions));
    types
}

/// 확장자마다 열 앱을 고르는 시트. 찾기 칸으로 거른다
fn show_file_types(context: &Rc<Context>) {
    let tr = &context.tr;
    let sheet = Sheet::new(
        context.window().as_ref(),
        Some(&badge(ICON, Tint::Blue, BadgeSize::Medium)),
        &tr.get("default-apps-types-title"),
        Some(&tr.get("default-apps-types-description")),
        None,
        &tr.get("done"),
    );
    sheet.window.set_default_width(600);
    let search = gtk::SearchEntry::builder()
        .placeholder_text(tr.get("default-apps-types-search"))
        .build();
    sheet.content.append(&search);

    let store = gio::ListStore::new::<glib::BoxedAnyObject>();
    for file_type in file_types() {
        let description = gio::content_type_get_description(&file_type.mime).to_string();
        store.append(&glib::BoxedAnyObject::new((file_type, description)));
    }
    let query = Rc::new(RefCell::new(String::new()));
    let filter = {
        let query = query.clone();
        gtk::CustomFilter::new(move |object| {
            object
                .downcast_ref::<glib::BoxedAnyObject>()
                .is_some_and(|object| {
                    let (file_type, description) = &*object.borrow::<(FileType, String)>();
                    file_type.matches(&query.borrow(), description)
                })
        })
    };
    let filtered = gtk::FilterListModel::new(Some(store), Some(filter.clone()));
    search.connect_search_changed(move |search| {
        query.replace(search.text().to_lowercase());
        filter.changed(gtk::FilterChange::Different);
    });

    let view = gtk::ListView::builder()
        .model(&gtk::NoSelection::new(Some(filtered.clone())))
        .factory(&file_type_factory(context))
        .css_classes(["file-types"])
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .child(&view)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(360)
        .max_content_height(360)
        .css_classes(["file-types-frame"])
        .build();
    let empty = widgets::description_label(&tr.get("default-apps-types-empty"));
    empty.set_halign(gtk::Align::Center);
    empty.set_margin_top(24);
    let overlay = gtk::Overlay::builder().child(&scroller).build();
    overlay.add_overlay(&empty);
    let update_empty = move |model: &gtk::FilterListModel| empty.set_visible(model.n_items() == 0);
    update_empty(&filtered);
    filtered.connect_items_changed(move |model, _, _, _| update_empty(model));
    sheet.content.append(&overlay);

    let window = sheet.window.clone();
    sheet.confirm.connect_clicked(move |_| window.close());
    sheet.present();
    search.grab_focus();
}

/// 시트의 행: 확장자와 형식 설명, 앱 고르기. 행은 다시 쓰이므로 앱을 고르면 그때 붙어 있는 형식에 정한다
fn file_type_factory(context: &Rc<Context>) -> gtk::SignalListItemFactory {
    // 행에 값을 채우는 동안 드롭다운이 바뀐 것은 사용자가 고른 것이 아니다
    let binding = Rc::new(Cell::new(false));
    let factory = gtk::SignalListItemFactory::new();
    let (page, guard) = (context.clone(), binding.clone());
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        item.set_activatable(false);
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        text.append(
            &gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["row-title"])
                .build(),
        );
        text.append(
            &gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["row-subtitle"])
                .build(),
        );
        let dropdown = app_dropdown();
        let line = gtk::Box::builder().spacing(12).build();
        line.append(&text);
        line.append(&dropdown);
        item.set_child(Some(&line));

        let (item, page, guard) = (item.downgrade(), page.clone(), guard.clone());
        dropdown.connect_selected_notify(move |dropdown| {
            if guard.get() {
                return;
            }
            let Some(object) = item
                .upgrade()
                .and_then(|item| item.item())
                .and_downcast::<glib::BoxedAnyObject>()
            else {
                return;
            };
            let mime = object.borrow::<(FileType, String)>().0.mime.clone();
            if let Some(app) = dropdown.selected_item().and_downcast::<gio::AppInfo>() {
                set_default(&page, &app, &[mime.as_str()]);
            }
        });
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(object), Some(line)) = (
            item.item().and_downcast::<glib::BoxedAnyObject>(),
            item.child(),
        ) else {
            return;
        };
        let (file_type, description) = &*object.borrow::<(FileType, String)>();
        let text = line.first_child();
        if let Some(label) = text
            .as_ref()
            .and_then(|text| text.first_child())
            .and_downcast::<gtk::Label>()
        {
            let extensions: Vec<String> = file_type
                .extensions
                .iter()
                .map(|extension| format!(".{extension}"))
                .collect();
            label.set_label(&extensions.join("  "));
        }
        if let Some(label) = text
            .as_ref()
            .and_then(|text| text.last_child())
            .and_downcast::<gtk::Label>()
        {
            label.set_label(&format!("{description} · {}", file_type.mime));
        }
        if let Some(dropdown) = line.last_child().and_downcast::<gtk::DropDown>() {
            let (apps, selected) = candidates(&file_type.mime);
            binding.set(true);
            dropdown.set_model(Some(&apps));
            dropdown.set_selected(selected.unwrap_or(gtk::INVALID_LIST_POSITION));
            binding.set(false);
        }
    });
    factory
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLOBS: &str = "# This file was automatically generated by the update-mime-database\n\
        50:image/jpeg:*.jpeg\n\
        50:image/jpeg:*.jpg\n\
        50:image/jpeg:*.JPE:cs\n\
        50:image/png:*.png\n\
        50:text/x-csrc:*.c\n\
        50:application/x-compressed-tar:*.tar.gz\n\
        50:text/x-readme:README*\n\
        40:image/apng:*.png\n\
        50:image/apng:*.apng\n\
        10:text/x-matlab:*.[mM]\n\
        50:image/jpeg:*.jpg\n";

    #[test]
    fn extensions_come_from_globs() {
        let mut extensions = BTreeMap::new();
        parse_globs(GLOBS, &mut extensions);
        let types = group_by_type(extensions);
        assert_eq!(types["image/jpeg"], ["jpe", "jpg", "jpeg"]);
        assert_eq!(types["text/x-csrc"], ["c"]);
        assert_eq!(types["application/x-compressed-tar"], ["tar.gz"]);
        // 확장자 하나는 가중치가 큰 형식에만 둔다
        assert_eq!(types["image/png"], ["png"]);
        assert_eq!(types["image/apng"], ["apng"]);
        // 확장자가 아닌 패턴은 뺀다
        assert!(!types.contains_key("text/x-readme"));
        assert!(!types.contains_key("text/x-matlab"));
    }

    #[test]
    fn search_matches_extensions_and_descriptions() {
        let jpeg = FileType {
            mime: "image/jpeg".into(),
            extensions: vec!["jpg".into(), "jpeg".into()],
        };
        assert!(jpeg.matches("", "JPEG 이미지"));
        assert!(jpeg.matches(".jp", "JPEG 이미지"));
        assert!(jpeg.matches("jpeg", "JPEG 이미지"));
        assert!(jpeg.matches("이미지", "JPEG 이미지"));
        assert!(jpeg.matches("image/", "JPEG 이미지"));
        assert!(!jpeg.matches("png", "JPEG 이미지"));
        // 확장자는 앞부분만 본다 (.g가 .jpg에 걸리지 않게)
        assert!(!jpeg.matches(".g", "JPEG 이미지"));
    }
}
