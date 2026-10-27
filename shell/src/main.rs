// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-shell: 메뉴바와 Dock (wlr-layer-shell 클라이언트, ARCHITECTURE 5.2)

mod config;
mod i18n;

use std::ffi::OsStr;

use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{config::Config, i18n::Translations};

const APP_ID: &str = "org.buntos.Shell";
const STYLE: &str = include_str!("../data/style.css");

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(|app| {
        let config = Config::load();
        let tr = Translations::load();
        load_style();
        build_menubar(app, &tr);
        build_dock(app, &config, &tr);
    });
    app.run()
}

fn load_style() {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(STYLE);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn layer_window(app: &gtk::Application, name: &str) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .css_classes([name])
        .build();
    window.init_layer_shell();
    window.set_namespace(Some(name));
    window.set_layer(Layer::Top);
    window.set_keyboard_mode(KeyboardMode::None);
    window
}

fn build_menubar(app: &gtk::Application, tr: &Translations) {
    let window = layer_window(app, "bunt-menubar");
    for edge in [Edge::Top, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.auto_exclusive_zone_enable();

    let bar = gtk::CenterBox::builder().css_classes(["bar"]).build();
    let logo = gtk::Label::builder()
        .label(tr.get("menubar-logo"))
        .css_classes(["logo"])
        .build();
    bar.set_start_widget(Some(&logo));

    let clock = gtk::Label::builder().css_classes(["clock"]).build();
    let format = tr.get("menubar-clock-format");
    update_clock(&clock, &format);
    let ticking = clock.clone();
    glib::timeout_add_seconds_local(1, move || {
        update_clock(&ticking, &format);
        glib::ControlFlow::Continue
    });
    bar.set_end_widget(Some(&clock));

    window.set_child(Some(&bar));
    window.present();
}

fn update_clock(label: &gtk::Label, format: &str) {
    if let Ok(text) = glib::DateTime::now_local().and_then(|now| now.format(format)) {
        label.set_text(&text);
    }
}

fn build_dock(app: &gtk::Application, config: &Config, tr: &Translations) {
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
        let image = gtk::Image::from_icon_name(&item.icon);
        image.set_pixel_size(config.dock.icon_size);
        let button = gtk::Button::builder()
            .child(&image)
            .tooltip_text(tr.get(&item.label))
            .css_classes(["dock-item"])
            .build();
        let command = item.command.clone();
        button.connect_clicked(move |_| launch(&command));
        dock.append(&button);
    }

    window.set_child(Some(&dock));
    window.present();
}

fn launch(command: &[String]) {
    let argv: Vec<&OsStr> = command.iter().map(OsStr::new).collect();
    if argv.is_empty() {
        return;
    }
    // GSubprocess가 끝난 프로세스를 거둔다
    if let Err(err) = gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
        eprintln!("bunt-shell: failed to start {}: {err}", command[0]);
    }
}
