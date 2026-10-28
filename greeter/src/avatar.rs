// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 계정 사진. AccountsService의 사진(IconFile)이 있으면 원 안에 채우고, 없으면 macOS처럼 이름
//! 글자(모노그램)를 회색 원에 쓴다. 시스템 설정(settings/src/window/account.rs)과 같은 모양이다

use std::path::Path;

use gtk4::{self as gtk, gdk, pango, prelude::*};

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

/// 지름 `size`의 사진
pub fn avatar(name: &str, icon: Option<&Path>, size: i32) -> gtk::Box {
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
    set_avatar(&frame, name, icon, size);
    frame
}

/// 이미 만든 사진의 내용을 바꾼다
pub fn set_avatar(frame: &gtk::Box, name: &str, icon: Option<&Path>, size: i32) {
    while let Some(child) = frame.first_child() {
        frame.remove(&child);
    }
    let texture = icon
        .filter(|path| path.is_file())
        .and_then(|path| gdk::Texture::from_filename(path).ok());
    if let Some(texture) = texture {
        frame.remove_css_class("monogram");
        // GtkPicture는 그림의 원래 크기를 요구해 틀이 커지므로 크기를 정해 그리는 GtkImage를 쓴다
        let image = gtk::Image::from_paintable(Some(&texture));
        image.set_pixel_size(size);
        image.set_hexpand(true);
        image.set_vexpand(true);
        frame.append(&image);
    } else {
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
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn monogram_letters() {
        assert_eq!(initials("홍길동"), "길동");
        assert_eq!(initials("김수"), "김수");
        assert_eq!(initials("남궁 민수"), "민수");
        assert_eq!(initials("John Appleseed"), "JA");
        assert_eq!(initials("john ronald reuel tolkien"), "JT");
        assert_eq!(initials("buntOS"), "B");
        assert_eq!(initials("  "), "");
    }
}
