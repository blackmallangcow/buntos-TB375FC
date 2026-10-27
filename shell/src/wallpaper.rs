// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 배경화면. 공용 설정(org.gnome.desktop.background)의 그림을 화면 맨 아래 층에 화면을 채우게
//! 그린다(남는 부분은 잘라 낸다). 다크 모드에서는 picture-uri-dark를 쓴다. 시스템 설정의 배경화면
//! 페이지가 이 값을 바꾸면 바로 따른다. 그림이 없거나 읽을 수 없으면 창을 숨겨 컴포지터의
//! 그라데이션이 보이게 한다.

use std::rc::Rc;

use gtk4::{self as gtk, gdk, gio, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::interface_settings;

const BACKGROUND_SCHEMA: &str = "org.gnome.desktop.background";

pub fn build(app: &gtk::Application) {
    let Some(settings) = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(BACKGROUND_SCHEMA, true))
        .map(|_| gio::Settings::new(BACKGROUND_SCHEMA))
    else {
        return;
    };
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .css_classes(["bunt-wallpaper"])
        .build();
    window.init_layer_shell();
    window.set_namespace(Some("bunt-wallpaper"));
    window.set_layer(Layer::Background);
    for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    // 메뉴바 아래까지 화면 전체를 채운다
    window.set_exclusive_zone(-1);
    window.set_keyboard_mode(KeyboardMode::None);
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .build();
    window.set_child(Some(&picture));

    let interface = interface_settings();
    let apply = {
        let (settings, interface) = (settings.clone(), interface.clone());
        Rc::new(move || {
            let dark = interface
                .as_ref()
                .is_some_and(|interface| interface.string("color-scheme") == "prefer-dark");
            let uri = settings.string(if dark {
                "picture-uri-dark"
            } else {
                "picture-uri"
            });
            let texture = (!uri.is_empty())
                .then(|| gio::File::for_uri(&uri))
                .filter(|file| file.query_exists(gio::Cancellable::NONE))
                .map(|file| gdk::Texture::from_file(&file));
            match texture {
                Some(Ok(texture)) => {
                    picture.set_paintable(Some(&texture));
                    window.present();
                }
                Some(Err(err)) => {
                    eprintln!("bunt-shell: wallpaper {uri}: {err}");
                    window.set_visible(false);
                }
                None => window.set_visible(false),
            }
        })
    };
    apply();
    // 변경 알림은 설정 객체가 살아 있을 때만 온다. 처리기가 객체를 붙든다
    let changed = apply.clone();
    let keep = settings.clone();
    settings.connect_changed(None, move |_, key| {
        let _ = &keep;
        if key.starts_with("picture-uri") {
            changed();
        }
    });
    if let Some(interface) = interface {
        let keep = interface.clone();
        interface.connect_changed(Some("color-scheme"), move |_, _| {
            let _ = &keep;
            apply();
        });
    }
}
