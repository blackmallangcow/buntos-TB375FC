// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dock: 화면 아래에서 앱을 실행한다

use bunt_i18n::Translations;
use gtk4::{self as gtk, prelude::*};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::{config::Config, layer_window};

pub fn build(app: &gtk::Application, config: &Config, tr: &Translations) -> gtk::ApplicationWindow {
    let window = layer_window(app, "bunt-dock");
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Bottom, config.dock.margin_bottom);
    window.auto_exclusive_zone_enable();

    let dock = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .css_classes(["dock"])
        .build();
    for item in &config.dock.items {
        let Some(info) = gio_unix::DesktopAppInfo::new(&item.app) else {
            eprintln!("bunt-shell: {} is not installed; skipping it", item.app);
            continue;
        };
        let name = match &item.label {
            Some(id) => tr.get(id),
            None => info.name().into(),
        };
        let image = match info.icon() {
            Some(icon) => gtk::Image::from_gicon(&icon),
            None => gtk::Image::from_icon_name("application-x-executable"),
        };
        image.set_pixel_size(config.dock.icon_size);
        let button = gtk::Button::builder()
            .child(&image)
            .tooltip_text(name)
            .css_classes(["dock-item"])
            .build();
        let id = item.app.clone();
        button.connect_clicked(move |button| {
            // D-Bus로 활성화하는 앱이면 GIO가 D-Bus로 띄운다
            let context = button.display().app_launch_context();
            if let Err(err) = info.launch(&[], Some(&context)) {
                eprintln!("bunt-shell: failed to start {id}: {err}");
            }
        });
        dock.append(&button);
    }

    window.set_child(Some(&dock));
    window.present();
    window
}
