// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! wpctl(WirePlumber 0.5)의 출력 읽기. 셸의 사운드 메뉴(shell/src/sound.rs)에 있던 것을 시스템 설정과 함께 쓰려고
//! 옮기고, 입력 장치(Sources)도 읽게 넓혔다.

/// 기본 출력 장치를 가리키는 wpctl의 이름
pub const DEFAULT_SINK: &str = "@DEFAULT_AUDIO_SINK@";
/// 기본 입력 장치를 가리키는 wpctl의 이름
pub const DEFAULT_SOURCE: &str = "@DEFAULT_AUDIO_SOURCE@";

/// `wpctl status`의 Audio 아래 장치 묶음
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// 출력 장치
    Sinks,
    /// 입력 장치
    Sources,
}

impl Section {
    fn heading(self) -> &'static str {
        match self {
            Self::Sinks => "Sinks:",
            Self::Sources => "Sources:",
        }
    }
}

/// 장치 하나 (wpctl status의 줄)
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// PipeWire 객체 id (wpctl에 넘기는 번호)
    pub id: u32,
    pub name: String,
    /// 기본 장치인지 (줄 앞의 *)
    pub default: bool,
}

/// `wpctl get-volume` 출력: "Volume: 0.40" 또는 "Volume: 0.40 [MUTED]"
pub fn parse_volume(text: &str) -> Option<(f64, bool)> {
    let rest = text.trim().strip_prefix("Volume:")?.trim();
    let (number, flags) = rest.split_once(' ').unwrap_or((rest, ""));
    Some((number.parse().ok()?, flags.contains("MUTED")))
}

/// `wpctl status`의 Audio 아래 Sinks(출력)나 Sources(입력) 목록
pub fn parse_nodes(status: &str, section: Section) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut in_audio = false;
    let mut in_section = false;
    for line in status.lines() {
        if !line.starts_with([' ', '│', '├', '└']) {
            in_audio = line.trim() == "Audio";
            in_section = false;
            continue;
        }
        let item = line.trim_start_matches([' ', '│', '├', '└', '─']);
        if item.ends_with(':') {
            in_section = in_audio && item == section.heading();
            continue;
        }
        if !in_section || item.is_empty() {
            continue;
        }
        let default = item.starts_with('*');
        let item = item.trim_start_matches('*').trim_start();
        let Some((id, rest)) = item.split_once('.') else {
            continue;
        };
        let Ok(id) = id.trim().parse() else {
            continue;
        };
        let name = rest
            .split(" [vol:")
            .next()
            .unwrap_or(rest)
            .trim()
            .to_owned();
        nodes.push(Node { id, name, default });
    }
    nodes
}

/// `wpctl inspect`의 첫 줄 "id 52, type PipeWire:Interface:Node"의 id
pub fn parse_inspect_id(text: &str) -> Option<u32> {
    let first = text.lines().next()?.trim();
    let (id, _) = first.strip_prefix("id ")?.split_once(',')?;
    id.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "\
PipeWire 'pipewire-0' [1.4.9, buntos@buntos, cookie:1]
 └─ Clients:
        33. WirePlumber                         [1.4.9, buntos@buntos, pid:812]

Audio
 ├─ Devices:
 │      45. Built-in Audio                      [alsa]
 │
 ├─ Sinks:
 │  *   52. Built-in Audio Analog Stereo        [vol: 0.40]
 │      60. HDMI Output                         [vol: 1.00 MUTED]
 │
 ├─ Sources:
 │  *   53. Built-in Audio Analog Stereo        [vol: 1.00]
 │      61. USB Microphone Mono                 [vol: 0.75]
 │
 ├─ Filters:
 │
 └─ Streams:
        70. Firefox
             71. output_FL       > Built-in Audio:playback_FL	[active]

Video
 ├─ Devices:
 ├─ Sinks:
 │      80. Not audio                           [vol: 1.00]
";

    #[test]
    fn volumes_are_parsed() {
        assert_eq!(parse_volume("Volume: 0.40\n"), Some((0.40, false)));
        assert_eq!(parse_volume("Volume: 0.75 [MUTED]\n"), Some((0.75, true)));
        assert_eq!(parse_volume("nothing"), None);
        assert_eq!(parse_volume("Volume: loud"), None);
    }

    #[test]
    fn sinks_and_sources_are_read_from_status() {
        assert_eq!(
            parse_nodes(STATUS, Section::Sinks),
            vec![
                Node {
                    id: 52,
                    name: "Built-in Audio Analog Stereo".into(),
                    default: true
                },
                Node {
                    id: 60,
                    name: "HDMI Output".into(),
                    default: false
                },
            ]
        );
        assert_eq!(
            parse_nodes(STATUS, Section::Sources),
            vec![
                Node {
                    id: 53,
                    name: "Built-in Audio Analog Stereo".into(),
                    default: true
                },
                Node {
                    id: 61,
                    name: "USB Microphone Mono".into(),
                    default: false
                },
            ]
        );
        // 소리 장치가 없는 VM
        assert!(parse_nodes("Audio\n ├─ Devices:\n ├─ Sinks:\n", Section::Sinks).is_empty());
        assert!(parse_nodes("", Section::Sources).is_empty());
    }

    #[test]
    fn inspect_gives_the_object_id() {
        let inspect = "id 52, type PipeWire:Interface:Node\n    alsa.card = \"0\"\n";
        assert_eq!(parse_inspect_id(inspect), Some(52));
        assert_eq!(
            parse_inspect_id("Translate ID error: no default sink"),
            None
        );
        assert_eq!(parse_inspect_id(""), None);
    }
}
