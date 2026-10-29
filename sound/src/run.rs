// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 명령을 GLib 하위 프로세스로 실행한다. 셸과 시스템 설정의 GTK 주 루프에서 기다린다.

use std::{ffi::OsStr, path::Path};

use gio::{glib, prelude::*};

use crate::{
    devices::{self, Kind},
    mixer::{self, Channel, Target, Volume},
    theme,
    wpctl::{self, Node, Section},
};

/// 명령을 실행해 표준 출력을 받는다. 없는 명령이거나 실패(0이 아닌 끝 코드)면 None
pub async fn output(argv: &[&str]) -> Option<String> {
    let argv: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
    let process = gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
    )
    .ok()?;
    let (stdout, _) = process.communicate_utf8_future(None).await.ok()?;
    process
        .is_successful()
        .then(|| stdout.map(|text| text.to_string()))?
}

/// 명령이 성공했는지
pub async fn run(argv: &[&str]) -> bool {
    output(argv).await.is_some()
}

/// 출력 장치(Sinks)나 입력 장치(Sources) 목록
pub async fn nodes(section: Section) -> Vec<Node> {
    output(&["wpctl", "status"])
        .await
        .map(|status| wpctl::parse_nodes(&status, section))
        .unwrap_or_default()
}

/// 출력 장치와 입력 장치 목록. wpctl status를 한 번만 부른다
pub async fn devices() -> (Vec<Node>, Vec<Node>) {
    let status = output(&["wpctl", "status"]).await.unwrap_or_default();
    (
        wpctl::parse_nodes(&status, Section::Sinks),
        wpctl::parse_nodes(&status, Section::Sources),
    )
}

/// 오디오 장치마다의 연결 방식. pw-dump로 그래프 전체를 읽으므로 새 장치가 보일 때만 부른다
pub async fn kinds() -> Vec<(u32, Kind)> {
    output(&["pw-dump"])
        .await
        .map(|dump| devices::kinds(&dump))
        .unwrap_or_default()
}

/// 장치(번호나 @DEFAULT_AUDIO_SINK@ 같은 wpctl 이름)의 음량. pw-dump가 채널 음량을 주지 않으면 wpctl get-volume의
/// 음량(첫 채널)을 가운데 채널 하나로 본다. 그런 장치는 균형이 없다
pub async fn volume(target: &str) -> Option<Volume> {
    let id = match target.parse::<u32>() {
        Ok(id) => Some(id),
        Err(_) => output(&["wpctl", "inspect", target])
            .await
            .and_then(|text| wpctl::parse_inspect_id(&text)),
    };
    if let Some(id) = id
        && let Some(dump) = output(&["pw-dump", &id.to_string()]).await
        && let Some(volume) = mixer::node_volume(&dump, id)
    {
        return Some(volume);
    }
    let (level, muted) = output(&["wpctl", "get-volume", target])
        .await
        .and_then(|text| wpctl::parse_volume(&text))?;
    Some(Volume {
        node: id.unwrap_or(0),
        channels: vec![Channel {
            position: String::new(),
            linear: level.powi(3),
        }],
        muted,
        target: Target::Node,
    })
}

/// 음량을 `level`(0~1)로 바꾼다. 채널이 모두 같으면 wpctl set-volume으로 바꾸고(셸이 하던 대로), 균형을 둔
/// 장치는 채널 사이의 비를 두고 바꾼다
pub async fn set_level(target: &str, volume: &Volume, level: f64) -> bool {
    let level = level.clamp(0.0, 1.0);
    if volume.is_uniform() || volume.node == 0 {
        let level = format!("{level:.2}");
        return run(&["wpctl", "set-volume", "-l", "1.0", target, &level]).await;
    }
    set_channels(volume, &volume.scaled(level)).await
}

/// 균형을 `balance`(-1 왼쪽 ~ 1 오른쪽)로 바꾼다
pub async fn set_balance(volume: &Volume, balance: f64) -> bool {
    set_channels(volume, &volume.balanced(balance)).await
}

/// 채널 음량(선형)을 pw-cli로 바꾼다. 카드의 경로가 음량을 가진 노드는 그 경로의 번호를 먼저 읽는다
async fn set_channels(volume: &Volume, linear: &[f64]) -> bool {
    let route = match volume.target {
        Target::Route {
            device,
            card_device,
        } => {
            let Some(dump) = output(&["pw-dump", &device.to_string()]).await else {
                return false;
            };
            let Some(index) = mixer::route_index(&dump, device, card_device) else {
                return false;
            };
            Some(index)
        }
        Target::Node => None,
    };
    let Some(args) = volume.set_args(linear, route) else {
        return false;
    };
    let mut argv = vec!["pw-cli"];
    argv.extend(args.iter().map(String::as_str));
    run(&argv).await
}

pub async fn set_mute(target: &str, muted: bool) -> bool {
    run(&["wpctl", "set-mute", target, if muted { "1" } else { "0" }]).await
}

pub async fn toggle_mute(target: &str) -> bool {
    run(&["wpctl", "set-mute", target, "toggle"]).await
}

pub async fn set_default(id: u32) -> bool {
    run(&["wpctl", "set-default", &id.to_string()]).await
}

/// 경고음을 재생한다. 끝나기를 기다리지 않는다 (GLib이 끝난 프로세스를 거둔다)
pub fn play(file: &Path, volume: f64) -> Result<(), glib::Error> {
    let args = theme::play_args(file, volume);
    let argv: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_SILENCE | gio::SubprocessFlags::STDERR_SILENCE,
    )
    .map(|_| ())
}

/// 기본 입력 장치의 레벨을 재는 녹음(pw-record, 16kHz 모노). 버리면 녹음을 멈춘다
pub struct Meter {
    process: gio::Subprocess,
    input: gio::InputStream,
}

/// 한 번에 읽는 양: 16kHz 16비트 모노의 50ms
const METER_CHUNK: usize = 1600;

impl Meter {
    pub fn start() -> Option<Meter> {
        let argv = [
            "pw-record",
            "--raw",
            "--format",
            "s16",
            "--rate",
            "16000",
            "--channels",
            "1",
            "-",
        ];
        let argv: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
        let process = gio::Subprocess::newv(
            &argv,
            gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_SILENCE,
        )
        .ok()?;
        let input = process.stdout_pipe()?;
        Some(Meter { process, input })
    }

    /// 다음 50ms의 레벨(0~1). 녹음이 끝났으면 None
    pub async fn next(&self) -> Option<f64> {
        let bytes = self
            .input
            .read_bytes_future(METER_CHUNK, glib::Priority::DEFAULT)
            .await
            .ok()?;
        (!bytes.is_empty()).then(|| mixer::meter_level(mixer::peak_s16le(&bytes)))
    }
}

impl Meter {
    /// 녹음을 끝낸다. 기다리던 next()는 None을 돌려준다
    pub fn stop(&self) {
        self.process.force_exit();
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        self.stop();
    }
}
