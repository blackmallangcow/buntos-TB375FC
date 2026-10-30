// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! bunt-comp: buntOS Wayland 컴포지터 (Smithay 기반, ARCHITECTURE D4)

mod agent;
mod background_effect;
mod blur;
mod config;
mod cursor;
mod dbus;
mod dmabuf;
mod gpu;
mod grabs;
mod ime;
mod input;
mod panels;
mod prefs;
mod render;
mod screenshot;
mod shmdiff;
mod shortcuts;
mod state;
#[cfg(test)]
mod testing;
mod toplevels;
mod udev;
mod windows;
// 세션: 화면 잠금, 로그아웃, 강제 종료, logind
mod logind;
mod session;
mod session_lock;
// ⇧⌘5의 고칠 수 있는 선택 영역과 화면 기록, 창 최소화 효과
mod minimize;
mod recording;
mod selection;

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
