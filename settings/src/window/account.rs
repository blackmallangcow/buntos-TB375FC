// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 이 기기의 계정 (AccountsService). 사이드바 맨 위 계정 카드와 사용자 및 그룹 페이지가 함께 쓴다:
//! 전체 이름, 계정 이름, 사진, 계정 종류를 읽고 전체 이름과 사진을 바꾼다. 자기 계정의 이름과 사진은
//! polkit이 인증 없이 허락한다 (org.freedesktop.accounts.change-own-user-data). 사진 모양은 로그인
//! 화면(greeter/src/avatar.rs)과 같다

use std::path::PathBuf;

use gtk4::{self as gtk, gdk, gio, glib, graphene, gsk, pango, prelude::*};

use crate::dbus::{self, Remote};

const ACCOUNTS: &str = "org.freedesktop.Accounts";
const ACCOUNTS_PATH: &str = "/org/freedesktop/Accounts";
const USER_INTERFACE: &str = "org.freedesktop.Accounts.User";
/// AccountsService의 AccountType에서 관리자
const ADMINISTRATOR: i32 = 1;
/// 저장하는 사진의 크기. AccountsService는 1MB가 넘는 사진을 받지 않는다
pub const PICTURE_SIZE: i32 = 256;

#[derive(Clone, Debug)]
pub struct Account {
    remote: Remote,
    pub user_name: String,
    pub real_name: String,
    pub icon_file: Option<PathBuf>,
    pub administrator: bool,
    pub home: String,
}

impl Account {
    /// 앱을 실행한 사용자의 계정
    pub async fn current() -> Result<Self, glib::Error> {
        Self::find(&glib::user_name().to_string_lossy()).await
    }

    pub async fn find(user_name: &str) -> Result<Self, glib::Error> {
        let bus = dbus::system().await?;
        let manager = Remote::new(&bus, ACCOUNTS, ACCOUNTS_PATH);
        let reply = manager
            .call(ACCOUNTS, "FindUserByName", Some((user_name,).to_variant()))
            .await?;
        let path = reply
            .child_value(0)
            .str()
            .map(str::to_owned)
            .ok_or_else(|| {
                glib::Error::new(gio::IOErrorEnum::InvalidData, "unexpected D-Bus reply")
            })?;
        Self::load(manager.at(path)).await
    }

    async fn load(remote: Remote) -> Result<Self, glib::Error> {
        let props = remote.get_all(USER_INTERFACE).await?;
        let icon_file = Some(PathBuf::from(props.string("IconFile")))
            .filter(|path| !path.as_os_str().is_empty() && path.is_file());
        Ok(Self {
            user_name: props.string("UserName"),
            real_name: props.string("RealName"),
            icon_file,
            administrator: props.get::<i32>("AccountType") == Some(ADMINISTRATOR),
            home: props.string("HomeDirectory"),
            remote,
        })
    }

    /// 바뀐 뒤 다시 읽는다
    pub async fn reload(&self) -> Result<Self, glib::Error> {
        Self::load(self.remote.clone()).await
    }

    /// 보일 이름: 전체 이름, 없으면 계정 이름
    pub fn display_name(&self) -> &str {
        if self.real_name.trim().is_empty() {
            &self.user_name
        } else {
            &self.real_name
        }
    }

    pub async fn set_real_name(&self, name: &str) -> Result<(), glib::Error> {
        self.remote
            .call(USER_INTERFACE, "SetRealName", Some((name,).to_variant()))
            .await
            .map(|_| ())
    }

    /// 사진 파일을 AccountsService에 넘긴다 (서비스가 /var/lib/AccountsService/icons에 복사한다).
    /// None이면 사진을 지우고 모노그램을 쓴다
    pub async fn set_icon_file(&self, path: Option<&std::path::Path>) -> Result<(), glib::Error> {
        let path = path
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.remote
            .call(USER_INTERFACE, "SetIconFile", Some((path,).to_variant()))
            .await
            .map(|_| ())
    }

    /// 계정이 바뀌면(이름, 사진) 부른다. 돌려준 값을 놓으면 받지 않는다
    pub fn watch(&self, callback: impl Fn() + 'static) -> gio::SignalSubscription {
        dbus::subscribe(
            &self.remote.bus,
            Some(ACCOUNTS),
            Some(USER_INTERFACE),
            Some("Changed"),
            Some(&self.remote.path),
            move |_| callback(),
        )
    }
}

/// 모노그램 글자. 한글 이름은 성을 뺀 이름(두 글자까지), 띄어 쓴 이름은 첫 낱말과 끝 낱말의
/// 머리글자, 한 낱말은 첫 글자다. 예: 홍길동 → 길동, John Appleseed → JA
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let hangul = |word: &str| word.chars().all(|c| ('가'..='힣').contains(&c));
    match words.as_slice() {
        [] => String::new(),
        [word] if hangul(word) => {
            let chars: Vec<char> = word.chars().collect();
            let start = if chars.len() >= 3 { chars.len() - 2 } else { 0 };
            chars[start..].iter().collect()
        }
        [word] => word.chars().take(1).flat_map(char::to_uppercase).collect(),
        [first, .., last] if hangul(first) && hangul(last) => last.chars().take(2).collect(),
        [first, .., last] => first
            .chars()
            .take(1)
            .chain(last.chars().take(1))
            .flat_map(char::to_uppercase)
            .collect(),
    }
}

/// 지름 `size`의 사진. 사진 파일이 없으면 모노그램
pub fn avatar(name: &str, icon: Option<&std::path::Path>, size: i32) -> gtk::Box {
    let frame = gtk::Box::builder()
        .width_request(size)
        .height_request(size)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(false)
        .vexpand(false)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["avatar"])
        .build();
    let texture = icon
        .filter(|path| path.is_file())
        .and_then(|path| gdk::Texture::from_filename(path).ok());
    set_avatar(&frame, name, texture.as_ref(), size);
    frame
}

/// 이미 만든 사진의 내용을 바꾼다. 그림이 없으면 모노그램
pub fn set_avatar(frame: &gtk::Box, name: &str, texture: Option<&gdk::Texture>, size: i32) {
    while let Some(child) = frame.first_child() {
        frame.remove(&child);
    }
    for class in DEFAULT_PICTURES.iter().map(|picture| picture.css) {
        frame.remove_css_class(class);
    }
    if let Some(texture) = texture {
        frame.remove_css_class("monogram");
        // GtkPicture는 그림의 원래 크기를 요구해 틀이 커지므로 크기를 정해 그리는 GtkImage를 쓴다.
        // 저장하는 사진은 정사각형이다 (render_photo)
        let image = gtk::Image::from_paintable(Some(texture));
        image.set_pixel_size(size);
        image.set_hexpand(true);
        image.set_vexpand(true);
        frame.append(&image);
        return;
    }
    frame.add_css_class("monogram");
    let label = gtk::Label::builder()
        .label(initials(name))
        .hexpand(true)
        .vexpand(true)
        .css_classes(["monogram-text"])
        .build();
    // 글자 크기는 원의 크기를 따른다
    let attributes = pango::AttrList::new();
    attributes.insert(pango::AttrSize::new_size_absolute(
        (f64::from(size) * 0.4 * f64::from(pango::SCALE)) as i32,
    ));
    label.set_attributes(Some(&attributes));
    frame.append(&label);
}

/// 기본 사진: 시스템 색 그라데이션 위의 흰 기호. 색은 theme/tokens.css의 밝게 값과 같다
pub struct DefaultPicture {
    pub icon: &'static str,
    /// data/users.css의 바탕 클래스
    pub css: &'static str,
    /// 접근성 이름 (Fluent 메시지 ID)
    pub name_id: &'static str,
    color: [f32; 3],
}

pub const DEFAULT_PICTURES: [DefaultPicture; 8] = [
    DefaultPicture {
        icon: "weather-clear-symbolic",
        css: "picture-orange",
        name_id: "users-picture-sun",
        color: [255.0, 141.0, 40.0],
    },
    DefaultPicture {
        icon: "weather-clear-night-symbolic",
        css: "picture-indigo",
        name_id: "users-picture-moon",
        color: [97.0, 85.0, 245.0],
    },
    DefaultPicture {
        icon: "starred-symbolic",
        css: "picture-purple",
        name_id: "users-picture-star",
        color: [203.0, 48.0, 224.0],
    },
    DefaultPicture {
        icon: "emoji-nature-symbolic",
        css: "picture-green",
        name_id: "users-picture-leaf",
        color: [52.0, 199.0, 89.0],
    },
    DefaultPicture {
        icon: "emoji-food-symbolic",
        css: "picture-red",
        name_id: "users-picture-food",
        color: [255.0, 56.0, 60.0],
    },
    DefaultPicture {
        icon: "emoji-travel-symbolic",
        css: "picture-blue",
        name_id: "users-picture-travel",
        color: [0.0, 136.0, 255.0],
    },
    DefaultPicture {
        icon: "emoji-activities-symbolic",
        css: "picture-teal",
        name_id: "users-picture-ball",
        color: [0.0, 195.0, 208.0],
    },
    DefaultPicture {
        icon: "face-smile-symbolic",
        css: "picture-pink",
        name_id: "users-picture-smile",
        color: [255.0, 45.0, 85.0],
    },
];

/// 기본 사진을 화면에 보이는 위젯 (CSS 그라데이션과 기호)
pub fn default_picture(picture: &DefaultPicture, size: i32) -> gtk::Box {
    let frame = gtk::Box::builder()
        .width_request(size)
        .height_request(size)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["avatar", "default-picture", picture.css])
        .build();
    let image = gtk::Image::from_icon_name(picture.icon);
    image.set_pixel_size(size * 11 / 20);
    image.set_hexpand(true);
    frame.append(&image);
    frame
}

/// 기본 사진을 PICTURE_SIZE 크기 그림으로 그린다 (AccountsService에 넘길 파일)
pub fn render_default_picture(
    widget: &impl IsA<gtk::Widget>,
    picture: &DefaultPicture,
) -> Option<gdk::Texture> {
    let size = PICTURE_SIZE as f32;
    let rgba = |[r, g, b]: [f32; 3]| gdk::RGBA::new(r / 255.0, g / 255.0, b / 255.0, 1.0);
    // CSS의 color-mix(in srgb, 색 70%, white)와 같은 위쪽 색
    let light = picture.color.map(|channel| channel * 0.7 + 255.0 * 0.3);
    let snapshot = gtk::Snapshot::new();
    let bounds = graphene::Rect::new(0.0, 0.0, size, size);
    snapshot.append_linear_gradient(
        &bounds,
        &graphene::Point::new(0.0, 0.0),
        &graphene::Point::new(0.0, size),
        &[
            gsk::ColorStop::new(0.0, rgba(light)),
            gsk::ColorStop::new(1.0, rgba(picture.color)),
        ],
    );
    let symbol = (size * 0.55).round();
    let icon = gtk::IconTheme::for_display(&widget.display()).lookup_icon(
        picture.icon,
        &[],
        symbol as i32,
        1,
        gtk::TextDirection::None,
        gtk::IconLookupFlags::FORCE_SYMBOLIC,
    );
    snapshot.save();
    snapshot.translate(&graphene::Point::new(
        (size - symbol) / 2.0,
        (size - symbol) / 2.0,
    ));
    icon.snapshot_symbolic(
        &snapshot,
        f64::from(symbol),
        f64::from(symbol),
        &[gdk::RGBA::WHITE],
    );
    snapshot.restore();
    render(widget, snapshot, size)
}

/// 고른 그림을 가운데에서 정사각형으로 잘라 PICTURE_SIZE 크기로 줄인다
pub fn render_photo(
    widget: &impl IsA<gtk::Widget>,
    texture: &gdk::Texture,
) -> Option<gdk::Texture> {
    let size = PICTURE_SIZE as f32;
    let (width, height) = (texture.width() as f32, texture.height() as f32);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let scale = size / width.min(height);
    let (drawn_width, drawn_height) = (width * scale, height * scale);
    let snapshot = gtk::Snapshot::new();
    snapshot.push_clip(&graphene::Rect::new(0.0, 0.0, size, size));
    snapshot.append_scaled_texture(
        texture,
        gsk::ScalingFilter::Trilinear,
        &graphene::Rect::new(
            (size - drawn_width) / 2.0,
            (size - drawn_height) / 2.0,
            drawn_width,
            drawn_height,
        ),
    );
    snapshot.pop();
    render(widget, snapshot, size)
}

fn render(
    widget: &impl IsA<gtk::Widget>,
    snapshot: gtk::Snapshot,
    size: f32,
) -> Option<gdk::Texture> {
    let renderer = widget.native()?.renderer()?;
    let node = snapshot.to_node()?;
    Some(renderer.render_texture(node, Some(&graphene::Rect::new(0.0, 0.0, size, size))))
}

/// 그림을 캐시 폴더에 PNG로 저장한다. AccountsService가 복사해 가므로 잠깐만 쓴다
pub fn save_picture(texture: &gdk::Texture) -> Result<PathBuf, glib::BoolError> {
    let directory = glib::user_cache_dir().join("bunt-settings");
    std::fs::create_dir_all(&directory)
        .map_err(|err| glib::bool_error!("{}: {err}", directory.display()))?;
    let path = directory.join("account-picture.png");
    texture.save_to_png(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn monogram_letters() {
        assert_eq!(initials("홍길동"), "길동");
        assert_eq!(initials("김수"), "김수");
        assert_eq!(initials("John Appleseed"), "JA");
        assert_eq!(initials("buntOS"), "B");
        assert_eq!(initials(""), "");
    }
}
