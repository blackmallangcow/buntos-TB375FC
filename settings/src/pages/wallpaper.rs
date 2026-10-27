// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 배경화면: 시스템 그림(/usr/share/backgrounds/buntos)과 사용자가 넣은 그림(~/.local/share/backgrounds)을
//! 축소판으로 보이고, 고른 그림을 공용 설정(org.gnome.desktop.background의 picture-uri,
//! picture-uri-dark)에 쓴다. 화면에 그리는 것은 bunt-shell이다

use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gdk_pixbuf, gio, glib, prelude::*};

use super::{Context, settings_for};
use crate::widgets::{self, Tint};

const SYSTEM_DIR: &str = "/usr/share/backgrounds/buntos";
const BACKGROUND_SCHEMA: &str = "org.gnome.desktop.background";
const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "svg", "jxl"];
/// 축소판 크기 (픽셀). GtkPicture는 그림의 픽셀 크기를 제 크기로 쓰므로 이 크기로 줄여 읽는다
const TILE_WIDTH: i32 = 136;
const TILE_HEIGHT: i32 = 85;
const PREVIEW_WIDTH: i32 = 176;
const PREVIEW_HEIGHT: i32 = 110;

struct WallpaperPage {
    context: Rc<Context>,
    settings: gio::Settings,
    preview: gtk::Picture,
    current_name: gtk::Label,
    system: gtk::FlowBox,
    mine: gtk::FlowBox,
    mine_section: gtk::Box,
    /// (그림 URI, 축소판 단추)
    tiles: RefCell<Vec<(String, gtk::Button)>>,
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let Some(settings) = settings_for(BACKGROUND_SCHEMA) else {
        column.append(&widgets::placeholder(
            "preferences-desktop-wallpaper-symbolic",
            Tint::Gray,
            &tr.get("wallpaper-unavailable-title"),
            &tr.get("wallpaper-unavailable-description"),
        ));
        return scroller.upcast();
    };

    // 지금 배경화면: 미리보기, 이름, 사진 추가
    let current = widgets::group(&column, None, None);
    let preview = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .width_request(PREVIEW_WIDTH)
        .height_request(PREVIEW_HEIGHT)
        .css_classes(["wallpaper-preview"])
        .build();
    preview.set_overflow(gtk::Overflow::Hidden);
    let current_name = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["hero-title"])
        .build();
    let add = gtk::Button::with_label(&tr.get("wallpaper-add"));
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    text.append(&current_name);
    text.append(
        &gtk::Label::builder()
            .label(tr.get("wallpaper-description"))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["hero-description"])
            .build(),
    );
    add.set_halign(gtk::Align::Start);
    text.append(&add);
    let content = gtk::Box::builder().spacing(16).build();
    content.append(&preview);
    content.append(&text);
    current.append(
        &gtk::ListBoxRow::builder()
            .child(&content)
            .activatable(false)
            .css_classes(["hero-row"])
            .build(),
    );

    let system = tile_grid();
    tile_section(&column, &tr.get("wallpaper-system"), &system);
    let mine = tile_grid();
    let mine_section = tile_section(&column, &tr.get("wallpaper-mine"), &mine);

    let page = Rc::new(WallpaperPage {
        context: context.clone(),
        settings: settings.clone(),
        preview,
        current_name,
        system,
        mine,
        mine_section,
        tiles: RefCell::default(),
    });
    page.fill();

    let weak = Rc::downgrade(&page);
    add.connect_clicked(move |_| {
        if let Some(page) = weak.upgrade() {
            glib::spawn_future_local(page.add_picture());
        }
    });
    let weak = Rc::downgrade(&page);
    settings.connect_changed(Some("picture-uri"), move |_, _| {
        if let Some(page) = weak.upgrade() {
            page.show_current();
        }
    });
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

fn tile_grid() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(8)
        .row_spacing(10)
        .column_spacing(10)
        .valign(gtk::Align::Start)
        .build()
}

/// 제목과 축소판 격자를 묶은 칸
fn tile_section(column: &gtk::Box, title: &str, grid: &gtk::FlowBox) -> gtk::Box {
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
        .css_classes(["settings-group", "tile-group"])
        .build();
    grid.set_hexpand(true);
    frame.append(grid);
    section.append(&frame);
    column.append(&section);
    section
}

/// 사용자가 넣은 그림을 두는 곳
fn user_dir() -> PathBuf {
    glib::user_data_dir().join("backgrounds")
}

/// 디렉터리의 그림 파일, 이름순
fn pictures_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut pictures: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| EXTENSIONS.contains(&extension.to_lowercase().as_str()))
        })
        .collect();
    pictures.sort();
    pictures
}

/// 보이는 이름. 기본 그림(buntos-*)은 번역된 이름(wallpaper-name-*)이 있으면 그것을 쓴다
fn localized_name(tr: &Translations, path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(name) = stem.strip_prefix("buntos-") {
        let id = format!("wallpaper-name-{name}");
        let translated = tr.get(&id);
        if translated != id {
            return translated;
        }
    }
    display_name(path)
}

/// 파일 이름에서 보이는 이름 ("buntos-blue-hour.png" → "Blue Hour"). 기본 그림의 "buntos-" 앞머리는 뗀다
fn display_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = stem.strip_prefix("buntos-").unwrap_or(&stem);
    stem.split(['-', '_', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 그림을 다른 스레드에서 읽어 줄인다. 읽기는 GTK(glycin)가 한다. Debian forky의 gdk-pixbuf는
/// 그림 형식 로더 없이 glycin에 맡기므로 gdk-pixbuf로는 파일을 읽지 못하고 크기만 줄인다
async fn thumbnail(path: PathBuf, width: i32) -> Option<gdk::Texture> {
    gio::spawn_blocking(move || {
        let texture = gdk::Texture::from_filename(&path).ok()?;
        let mut downloader = gdk::TextureDownloader::new(&texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = downloader.download_bytes();
        let (full_width, full_height) = (texture.width(), texture.height());
        let pixbuf = gdk_pixbuf::Pixbuf::from_bytes(
            &bytes,
            gdk_pixbuf::Colorspace::Rgb,
            true,
            8,
            full_width,
            full_height,
            i32::try_from(stride).ok()?,
        );
        let width = width.min(full_width).max(1);
        let height = (full_height * width / full_width.max(1)).max(1);
        let small = pixbuf.scale_simple(width, height, gdk_pixbuf::InterpType::Bilinear)?;
        let texture: gdk::Texture = gdk::MemoryTexture::new(
            small.width(),
            small.height(),
            gdk::MemoryFormat::R8g8b8a8,
            &small.read_pixel_bytes(),
            small.rowstride() as usize,
        )
        .upcast();
        Some(texture)
    })
    .await
    .ok()?
}

impl WallpaperPage {
    fn fill(self: &Rc<Self>) {
        for grid in [&self.system, &self.mine] {
            while let Some(child) = grid.first_child() {
                grid.remove(&child);
            }
        }
        self.tiles.borrow_mut().clear();
        let system = pictures_in(Path::new(SYSTEM_DIR));
        let mine = pictures_in(&user_dir());
        if system.is_empty() {
            self.system.append(
                &gtk::Label::builder()
                    .label(self.context.tr.get("wallpaper-none"))
                    .css_classes(["settings-description"])
                    .build(),
            );
        }
        for path in system {
            self.system.append(&self.tile(&path));
        }
        self.mine_section.set_visible(!mine.is_empty());
        for path in mine {
            self.mine.append(&self.tile(&path));
        }
        self.show_current();
    }

    fn tile(self: &Rc<Self>, path: &Path) -> gtk::Button {
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(TILE_WIDTH)
            .height_request(TILE_HEIGHT)
            .build();
        picture.set_overflow(gtk::Overflow::Hidden);
        let button = gtk::Button::builder()
            .child(&picture)
            .tooltip_text(localized_name(&self.context.tr, path))
            .css_classes(["wallpaper-tile"])
            .build();
        let uri = gio::File::for_path(path).uri().to_string();
        let page = self.clone();
        let chosen = uri.clone();
        button.connect_clicked(move |_| page.choose(&chosen));
        self.tiles.borrow_mut().push((uri, button.clone()));
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            if let Some(texture) = thumbnail(path, TILE_WIDTH).await {
                picture.set_paintable(Some(&texture));
            }
        });
        button
    }

    fn choose(&self, uri: &str) {
        for key in ["picture-uri", "picture-uri-dark"] {
            if let Err(err) = self.settings.set_string(key, uri) {
                self.context.notify_error("wallpaper-error", err);
                return;
            }
        }
        // 화면을 채우도록 늘린다 (macOS의 "화면 채우기")
        let _ = self.settings.set_string("picture-options", "zoom");
    }

    /// 고른 그림에 테두리를 두르고 위쪽 미리보기를 바꾼다
    fn show_current(&self) {
        let uri = self.settings.string("picture-uri").to_string();
        for (candidate, button) in self.tiles.borrow().iter() {
            if *candidate == uri {
                button.add_css_class("selected");
            } else {
                button.remove_css_class("selected");
            }
        }
        let file = gio::File::for_uri(&uri);
        let path = file.path().filter(|path| path.exists());
        match path {
            Some(path) => {
                self.current_name
                    .set_text(&localized_name(&self.context.tr, &path));
                let preview = self.preview.clone();
                glib::spawn_future_local(async move {
                    if let Some(texture) = thumbnail(path, PREVIEW_WIDTH).await {
                        preview.set_paintable(Some(&texture));
                    }
                });
            }
            None => {
                self.current_name
                    .set_text(&self.context.tr.get("wallpaper-default"));
                self.preview.set_paintable(None::<&gdk::Paintable>);
            }
        }
    }

    /// "사진 추가...": 고른 그림을 사용자 디렉터리에 복사하고 배경화면으로 한다
    async fn add_picture(self: Rc<Self>) {
        let tr = &self.context.tr;
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&tr.get("wallpaper-filter")));
        filter.add_pixbuf_formats();
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(tr.get("wallpaper-add-title"))
            .modal(true)
            .filters(&filters)
            .default_filter(&filter)
            .build();
        let Ok(file) = dialog.open_future(self.context.window().as_ref()).await else {
            return;
        };
        let Some(source) = file.path() else {
            return;
        };
        let target_dir = user_dir();
        let copied = gio::spawn_blocking(move || -> std::io::Result<PathBuf> {
            std::fs::create_dir_all(&target_dir)?;
            let name = source
                .file_name()
                .map(|name| name.to_owned())
                .unwrap_or_else(|| "wallpaper.png".into());
            let mut target = target_dir.join(&name);
            // 같은 이름이 있으면 번호를 붙인다
            let mut number = 2;
            while target.exists() {
                let stem = Path::new(&name)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let extension = Path::new(&name)
                    .extension()
                    .map(|extension| format!(".{}", extension.to_string_lossy()))
                    .unwrap_or_default();
                target = target_dir.join(format!("{stem} {number}{extension}"));
                number += 1;
            }
            std::fs::copy(&source, &target)?;
            Ok(target)
        })
        .await;
        match copied {
            Ok(Ok(target)) => {
                self.fill();
                self.choose(&gio::File::for_path(&target).uri());
                let mut args = FluentArgs::new();
                args.set("name", localized_name(&self.context.tr, &target));
                self.context
                    .notify(&self.context.tr.get_with("wallpaper-added", &args));
            }
            Ok(Err(err)) => self.context.notify_error("wallpaper-error", err),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_drop_separators() {
        assert_eq!(display_name(Path::new("/x/blue-hour_2.png")), "Blue Hour 2");
        assert_eq!(display_name(Path::new("/x/buntos-dawn.png")), "Dawn");
        assert_eq!(display_name(Path::new("/x/내 사진.jpg")), "내 사진");
    }

    #[test]
    fn built_in_wallpapers_have_translated_names() {
        let tr =
            Translations::from_source("ko", include_str!("../../../i18n/ko/bunt-settings.ftl"));
        assert_eq!(localized_name(&tr, Path::new("/x/buntos-dawn.png")), "새벽");
        assert_eq!(localized_name(&tr, Path::new("/x/buntos-new.png")), "New");
        assert_eq!(localized_name(&tr, Path::new("/x/내 사진.jpg")), "내 사진");
    }
}
