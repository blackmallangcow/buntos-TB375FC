// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 시동음: 부팅을 마치고 로그인 화면이 처음 뜰 때 한 번 낸다. 사운드 테마의 system-ready(buntos 테마의
//! 시동음, tools/sounds/generate.py)를 로그인 화면 사용자의 PipeWire로 재생한다. 소리 크기는 그 사용자의 출력
//! 음량을 따른다. 로그아웃하거나 로그인 화면이 다시 떠도 같은 부팅에서는 다시 내지 않는다

use std::{fs::OpenOptions, io::ErrorKind};

use bunt_sound::{run, theme};
use gtk4::{gio, prelude::*};

/// 이번 부팅에 시동음을 냈다는 표시. /run은 부팅마다 비고, 디렉터리는 tmpfiles.d(bunt-greeter.conf)가 로그인
/// 화면 사용자의 것으로 만든다
const PLAYED: &str = "/run/bunt-greeter/startup-sound-played";
/// 부팅을 마쳐 로그인할 수 있게 되었을 때의 소리 (freedesktop 사운드 이름)
const SOUND: &str = "system-ready";
/// 사운드 테마 이름 (GNOME 공용 설정)
const SOUND_SCHEMA: &str = "org.gnome.desktop.sound";

/// 이번 부팅에 아직 내지 않았으면 시동음을 낸다. 표시를 먼저 만들므로(create_new) 두 번 불러도 한 번만 낸다
pub fn play_once() {
    match OpenOptions::new().write(true).create_new(true).open(PLAYED) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::AlreadyExists => return,
        Err(err) => {
            eprintln!("bunt-greeter: {PLAYED}: {err}, no startup sound");
            return;
        }
    }
    let Some(file) = theme::find(SOUND, &sound_theme(), &theme::data_dirs()) else {
        return;
    };
    if let Err(err) = run::play(&file, 1.0) {
        eprintln!("bunt-greeter: could not play {}: {err}", file.display());
    }
}

/// 고른 사운드 테마. 로그인 화면 사용자에게는 사용자 값이 없으므로 시스템 기본값(buntos)이다
fn sound_theme() -> String {
    gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(SOUND_SCHEMA, true))
        .map_or_else(
            || theme::FALLBACK_THEME.to_owned(),
            |_| {
                gio::Settings::new(SOUND_SCHEMA)
                    .string("theme-name")
                    .to_string()
            },
        )
}
