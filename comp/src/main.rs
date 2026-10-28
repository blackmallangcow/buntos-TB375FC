// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-comp: buntOS Wayland 컴포지터 (Smithay 기반, ARCHITECTURE D4)

mod blur;
mod config;
mod cursor;
mod dbus;
mod grabs;
mod ime;
mod input;
mod panels;
mod prefs;
mod render;
mod state;
mod toplevels;
mod udev;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let config = config::Config::load();
    if let Err(err) = udev::run(config) {
        tracing::error!("bunt-comp failed: {err}");
        std::process::exit(1);
    }
}
