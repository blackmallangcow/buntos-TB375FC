// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 채널마다의 음량과 균형. pw-dump로 읽고 pw-cli로 바꾼다.
//!
//! wpctl set-volume은 모든 채널을 같은 값으로 둬서 균형이 가운데로 돌아간다. 그래서 균형을 둔 장치는 채널 음량을
//! 직접 바꾼다. 바꾸는 곳은 WirePlumber의 mixer-api(module-mixer-api.c)와 같다.
//! - ALSA 카드처럼 카드의 경로(Route)가 음량을 가진 노드: 카드(Device) 객체의 Route 매개변수(index, device,
//!   props, save). save를 켜 WirePlumber가 다음 부팅에도 되살린다
//! - 그 밖의 노드(Bluetooth, 가상 장치): 노드의 Props
//!
//! 사람이 보는 음량은 wpctl처럼 세제곱 눈금이다(선형 음량의 세제곱근). PulseAudio(pa_cvolume)처럼 음량은 가장 큰
//! 채널이고, 균형은 왼쪽·오른쪽 채널 평균의 비다(-1 왼쪽 끝, 0 가운데, 1 오른쪽 끝).

use serde_json::Value;

/// 이 차이보다 작은 음량은 같다고 본다 (pw-dump는 소수 여섯 자리까지 낸다)
const EPSILON: f64 = 1e-4;

/// 채널이 놓인 쪽
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    /// 가운데, 저음, 모노 등
    Center,
}

/// SPA 채널 이름(FL, RL, FLC, TFL, FLW …)의 쪽. 끝의 C(가운데 쪽), W(넓게), H(높게)를 떼고 마지막 글자가 L이면
/// 왼쪽, R이면 오른쪽이다. PulseAudio의 on_left·on_right와 같은 채널을 고른다
pub fn side(position: &str) -> Side {
    let mut core = position;
    if core.len() > 2 {
        core = core.trim_end_matches(['C', 'W', 'H']);
    }
    match core.as_bytes() {
        [.., b'L'] if core.len() >= 2 => Side::Left,
        [.., b'R'] if core.len() >= 2 => Side::Right,
        _ => Side::Center,
    }
}

/// 채널 하나
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// SPA 채널 이름 (FL, FR, MONO 등). 모르면 비어 있다
    pub position: String,
    /// 선형 음량
    pub linear: f64,
}

impl Channel {
    /// 사람이 보는 세제곱 눈금의 음량
    pub fn level(&self) -> f64 {
        self.linear.max(0.0).cbrt()
    }
}

/// 음량을 바꾸는 곳
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// 카드의 경로: 카드 객체 id(device.id)와 카드 안 장치 번호(card.profile.device)
    Route { device: u32, card_device: i64 },
    /// 노드의 Props
    Node,
}

/// 장치(노드) 하나의 음량
#[derive(Debug, Clone, PartialEq)]
pub struct Volume {
    pub node: u32,
    pub channels: Vec<Channel>,
    pub muted: bool,
    pub target: Target,
}

impl Volume {
    /// 사람이 보는 음량(0~1): 가장 큰 채널
    pub fn level(&self) -> f64 {
        self.channels.iter().map(Channel::level).fold(0.0, f64::max)
    }

    /// 왼쪽과 오른쪽 채널의 평균 (세제곱 눈금). 한쪽이라도 없으면 None
    fn left_right(&self) -> Option<(f64, f64)> {
        let average = |wanted: Side| {
            let levels: Vec<f64> = self
                .channels
                .iter()
                .filter(|channel| side(&channel.position) == wanted)
                .map(Channel::level)
                .collect();
            (!levels.is_empty()).then(|| levels.iter().sum::<f64>() / levels.len() as f64)
        };
        Some((average(Side::Left)?, average(Side::Right)?))
    }

    /// 균형 -1(왼쪽)~1(오른쪽). 왼쪽·오른쪽 채널이 없으면(모노) None (pa_cvolume_get_balance)
    pub fn balance(&self) -> Option<f64> {
        let (left, right) = self.left_right()?;
        Some(if (left - right).abs() < EPSILON {
            0.0
        } else if left > right {
            -1.0 + right / left
        } else {
            1.0 - left / right
        })
    }

    /// 모든 채널이 같은 음량인지. 같으면 wpctl set-volume으로 바꿔도 잃는 것이 없다
    pub fn is_uniform(&self) -> bool {
        self.channels
            .windows(2)
            .all(|pair| (pair[0].level() - pair[1].level()).abs() < EPSILON)
    }

    /// 음량을 `level`(0~1)로 바꾼 채널 음량(선형). 채널 사이의 비는 그대로 둔다(pa_cvolume_scale)
    pub fn scaled(&self, level: f64) -> Vec<f64> {
        let level = level.clamp(0.0, 1.0);
        let current = self.level();
        self.channels
            .iter()
            .map(|channel| {
                let new = if current < EPSILON {
                    level
                } else {
                    channel.level() * level / current
                };
                new.powi(3)
            })
            .collect()
    }

    /// 균형을 `balance`(-1~1)로 바꾼 채널 음량(선형). 큰 쪽은 그대로 두고 작은 쪽을 줄인다(pa_cvolume_set_balance)
    pub fn balanced(&self, balance: f64) -> Vec<f64> {
        let balance = balance.clamp(-1.0, 1.0);
        let Some((left, right)) = self.left_right() else {
            return self.channels.iter().map(|channel| channel.linear).collect();
        };
        let top = left.max(right);
        let (new_left, new_right) = if balance <= 0.0 {
            (top, (balance + 1.0) * top)
        } else {
            ((1.0 - balance) * top, top)
        };
        self.channels
            .iter()
            .map(|channel| {
                let (old, new) = match side(&channel.position) {
                    Side::Left => (left, new_left),
                    Side::Right => (right, new_right),
                    Side::Center => return channel.linear,
                };
                let level = if old < EPSILON {
                    new
                } else {
                    channel.level() * new / old
                };
                level.powi(3)
            })
            .collect()
    }

    /// 채널 음량을 바꾸는 pw-cli 인자. 카드의 경로로 바꾸는 노드는 경로 번호(route_index)가 있어야 한다
    pub fn set_args(&self, linear: &[f64], route_index: Option<i64>) -> Option<Vec<String>> {
        let volumes = linear
            .iter()
            .map(|volume| format!("{:.6}", volume.max(0.0)))
            .collect::<Vec<_>>()
            .join(", ");
        let (object, param, json) = match self.target {
            Target::Route {
                device,
                card_device,
            } => (
                device,
                "Route",
                format!(
                    "{{ \"index\": {}, \"device\": {card_device}, \"props\": {{ \"channelVolumes\": [ {volumes} ] }}, \"save\": true }}",
                    route_index?
                ),
            ),
            Target::Node => (
                self.node,
                "Props",
                format!("{{ \"channelVolumes\": [ {volumes} ] }}"),
            ),
        };
        Some(vec![
            "set-param".to_owned(),
            object.to_string(),
            param.to_owned(),
            json,
        ])
    }
}

/// JSON 값의 정수. pw-dump는 숫자 꼴 속성을 숫자로 내지만 문자열일 때도 읽는다
fn integer(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

/// pw-dump의 객체 배열에서 id와 종류(Node, Device)가 맞는 객체
fn object<'a>(dump: &'a Value, id: u32, kind: &str) -> Option<&'a Value> {
    dump.as_array()?.iter().find(|object| {
        object.get("id").and_then(Value::as_u64) == Some(u64::from(id))
            && object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|name| name.ends_with(kind))
    })
}

/// 매개변수 묶음(params.Props, params.Route의 props)에서 채널 음량과 음소거를 읽는다
fn channels_of(props: &Value) -> Option<(Vec<Channel>, bool)> {
    let volumes = props.get("channelVolumes")?.as_array()?;
    let positions: Vec<String> = props
        .get("channelMap")
        .and_then(Value::as_array)
        .map(|map| {
            map.iter()
                .map(|name| name.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default();
    let channels = volumes
        .iter()
        .enumerate()
        .map(|(index, volume)| Channel {
            position: positions.get(index).cloned().unwrap_or_default(),
            linear: volume.as_f64().unwrap_or(0.0),
        })
        .collect();
    let muted = props.get("mute").and_then(Value::as_bool).unwrap_or(false);
    Some((channels, muted))
}

/// pw-dump(노드 id로 거른 것도 된다)에서 노드의 음량을 읽는다. 카드의 경로가 음량을 가진 노드(card.profile.device)는
/// 카드 객체가 함께 있으면 그 경로의 값을 쓴다(mixer-api의 collect_node_info). 없으면 노드의 Props를 쓴다. 카드의
/// 경로는 그 값을 노드의 Props에도 알리므로 같은 값이다
pub fn node_volume(dump: &str, node: u32) -> Option<Volume> {
    let dump: Value = serde_json::from_str(dump).ok()?;
    let found = object(&dump, node, ":Node")?;
    let info = found.get("info")?;
    let props = info.get("props");
    let property = |key: &str| props.and_then(|props| props.get(key)).and_then(integer);
    let target = match (property("device.id"), property("card.profile.device")) {
        (Some(device), Some(card_device)) => Target::Route {
            device: u32::try_from(device).ok()?,
            card_device,
        },
        _ => Target::Node,
    };
    let from_route = match target {
        Target::Route {
            device,
            card_device,
        } => object(&dump, device, ":Device")
            .and_then(|device| {
                routes(device).find(|route| route_device(route) == Some(card_device))
            })
            .and_then(|route| route.get("props"))
            .and_then(channels_of),
        Target::Node => None,
    };
    let (channels, muted) = match from_route {
        Some(found) => found,
        None => info
            .get("params")?
            .get("Props")?
            .as_array()?
            .iter()
            .find_map(channels_of)?,
    };
    Some(Volume {
        node,
        channels,
        muted,
        target,
    })
}

fn routes(device: &Value) -> impl Iterator<Item = &Value> {
    device
        .get("info")
        .and_then(|info| info.get("params"))
        .and_then(|params| params.get("Route"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn route_device(route: &Value) -> Option<i64> {
    route.get("device").and_then(integer)
}

/// 카드(pw-dump로 거른 카드 객체도 된다)에서 노드가 쓰는 경로의 번호(index)
pub fn route_index(dump: &str, device: u32, card_device: i64) -> Option<i64> {
    let dump: Value = serde_json::from_str(dump).ok()?;
    routes(object(&dump, device, ":Device")?)
        .find(|route| route_device(route) == Some(card_device))
        .and_then(|route| route.get("index"))
        .and_then(integer)
}

/// 부호 있는 16비트 PCM(리틀 엔디언)의 가장 큰 진폭 (0~1). 입력 레벨에 쓴다
pub fn peak_s16le(bytes: &[u8]) -> f64 {
    let (samples, _) = bytes.as_chunks::<2>();
    samples
        .iter()
        .map(|sample| f64::from(i16::from_le_bytes(*sample)).abs() / 32768.0)
        .fold(0.0, f64::max)
        .min(1.0)
}

/// 진폭을 레벨 막대의 칸(0~1)으로: -60dBFS 아래는 0, 0dBFS는 1 (사람 귀에 맞게 데시벨로)
pub fn meter_level(peak: f64) -> f64 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(channels: &[(&str, f64)]) -> Volume {
        Volume {
            node: 52,
            channels: channels
                .iter()
                .map(|(position, level)| Channel {
                    position: (*position).to_owned(),
                    linear: level.powi(3),
                })
                .collect(),
            muted: false,
            target: Target::Node,
        }
    }

    fn levels(linear: &[f64]) -> Vec<f64> {
        linear
            .iter()
            .map(|value| (value.cbrt() * 1000.0).round() / 1000.0)
            .collect()
    }

    #[test]
    fn channel_sides_follow_pulseaudio() {
        for left in [
            "FL", "RL", "SL", "FLC", "RLC", "TFL", "TRL", "TSL", "FLW", "FLH",
        ] {
            assert_eq!(side(left), Side::Left, "{left}");
        }
        for right in ["FR", "RR", "SR", "FRC", "TFR", "TRR", "FRW"] {
            assert_eq!(side(right), Side::Right, "{right}");
        }
        for center in ["FC", "LFE", "RC", "MONO", "TC", "TFC", "", "AUX0", "L", "R"] {
            assert_eq!(side(center), Side::Center, "{center}");
        }
    }

    #[test]
    fn level_is_the_loudest_channel_and_balance_is_the_ratio() {
        let centered = volume(&[("FL", 0.5), ("FR", 0.5)]);
        assert!((centered.level() - 0.5).abs() < 1e-9);
        assert_eq!(centered.balance(), Some(0.0));
        assert!(centered.is_uniform());
        // 오른쪽 반: 왼쪽이 오른쪽의 반이다
        let right = volume(&[("FL", 0.25), ("FR", 0.5)]);
        assert!((right.level() - 0.5).abs() < 1e-9);
        assert!((right.balance().unwrap() - 0.5).abs() < 1e-9);
        assert!(!right.is_uniform());
        let left_end = volume(&[("FL", 0.8), ("FR", 0.0)]);
        assert!((left_end.balance().unwrap() + 1.0).abs() < 1e-9);
        // 모노는 균형이 없다
        assert_eq!(volume(&[("MONO", 0.7)]).balance(), None);
        assert_eq!(volume(&[]).level(), 0.0);
    }

    #[test]
    fn scaling_keeps_the_balance() {
        let right = volume(&[("FL", 0.25), ("FR", 0.5)]);
        assert_eq!(levels(&right.scaled(1.0)), [0.5, 1.0]);
        assert_eq!(levels(&right.scaled(0.0)), [0.0, 0.0]);
        // 범위 밖은 자른다
        assert_eq!(levels(&right.scaled(3.0)), [0.5, 1.0]);
        // 소리가 없던 장치는 모든 채널을 같게 올린다
        let silent = volume(&[("FL", 0.0), ("FR", 0.0)]);
        assert_eq!(levels(&silent.scaled(0.4)), [0.4, 0.4]);
    }

    #[test]
    fn balance_lowers_one_side_and_keeps_the_center() {
        let surround = volume(&[("FL", 0.6), ("FR", 0.6), ("FC", 0.3), ("LFE", 0.6)]);
        // 오른쪽으로 0.5: 왼쪽 채널이 반이 되고 가운데·저음은 그대로다
        assert_eq!(levels(&surround.balanced(0.5)), [0.3, 0.6, 0.3, 0.6]);
        assert_eq!(levels(&surround.balanced(-1.0)), [0.6, 0.0, 0.3, 0.6]);
        // 다시 가운데로
        let shifted = volume(&[("FL", 0.3), ("FR", 0.6)]);
        assert_eq!(levels(&shifted.balanced(0.0)), [0.6, 0.6]);
        // 범위 밖은 자른다
        assert_eq!(levels(&shifted.balanced(-5.0)), [0.6, 0.0]);
        // 모노는 바꾸지 않는다
        let mono = volume(&[("MONO", 0.7)]);
        assert_eq!(levels(&mono.balanced(0.5)), [0.7]);
    }

    /// pw-dump의 실제 꼴(PipeWire 1.4, 줄임): ALSA 카드의 노드와 카드
    const DUMP: &str = r#"[
      {
        "id": 45,
        "type": "PipeWire:Interface:Device",
        "info": {
          "props": { "device.name": "alsa_card.pci-0000_00_1f.3" },
          "params": {
            "Route": [
              { "index": 1, "direction": "Input", "device": 3,
                "props": { "mute": false, "channelVolumes": [ 1.0, 1.0 ], "channelMap": [ "FL", "FR" ] } },
              { "index": 4, "direction": "Output", "device": 4,
                "props": { "mute": true, "channelVolumes": [ 0.015625, 0.125 ], "channelMap": [ "FL", "FR" ] },
                "save": false }
            ]
          }
        }
      },
      {
        "id": 52,
        "type": "PipeWire:Interface:Node",
        "info": {
          "props": { "device.id": 45, "card.profile.device": 4, "media.class": "Audio/Sink" },
          "params": {
            "Props": [
              { "volume": 1.0, "mute": true, "channelVolumes": [ 0.015625, 0.125 ], "channelMap": [ "FL", "FR" ] },
              { "params": [ ] }
            ]
          }
        }
      },
      {
        "id": 70,
        "type": "PipeWire:Interface:Node",
        "info": {
          "props": { "media.class": "Audio/Sink", "node.name": "bluez_output.AA" },
          "params": {
            "Props": [ { "params": [ ] }, { "mute": false, "channelVolumes": [ 0.512 ], "channelMap": [ "MONO" ] } ]
          }
        }
      }
    ]"#;

    #[test]
    fn card_nodes_read_and_change_their_route() {
        let volume = node_volume(DUMP, 52).unwrap();
        assert_eq!(
            volume.target,
            Target::Route {
                device: 45,
                card_device: 4
            }
        );
        assert!(volume.muted);
        assert_eq!(
            levels(&volume.channels.iter().map(|c| c.linear).collect::<Vec<_>>()),
            [0.25, 0.5]
        );
        assert!((volume.balance().unwrap() - 0.5).abs() < 1e-9);
        assert_eq!(route_index(DUMP, 45, 4), Some(4));
        assert_eq!(route_index(DUMP, 45, 9), None);
        let args = volume.set_args(&volume.balanced(0.0), Some(4)).unwrap();
        assert_eq!(args[..3], ["set-param", "45", "Route"]);
        assert_eq!(
            args[3],
            "{ \"index\": 4, \"device\": 4, \"props\": { \"channelVolumes\": [ 0.125000, 0.125000 ] }, \"save\": true }"
        );
        // 경로 번호를 모르면 바꾸지 않는다
        assert_eq!(volume.set_args(&[0.1, 0.1], None), None);
    }

    #[test]
    fn other_nodes_change_their_props() {
        let volume = node_volume(DUMP, 70).unwrap();
        assert_eq!(volume.target, Target::Node);
        assert_eq!(volume.channels[0].position, "MONO");
        assert!((volume.level() - 0.8).abs() < 1e-9);
        let args = volume.set_args(&[0.25], None).unwrap();
        assert_eq!(
            args,
            [
                "set-param",
                "70",
                "Props",
                "{ \"channelVolumes\": [ 0.250000 ] }"
            ]
        );
        assert_eq!(node_volume(DUMP, 99), None);
        assert_eq!(node_volume("not json", 52), None);
        // 노드만 거른 pw-dump(카드가 없다)에서도 노드의 Props로 읽는다
        let only_node = format!(
            "[{}]",
            serde_json::to_string(&serde_json::from_str::<Value>(DUMP).unwrap()[1]).unwrap()
        );
        let volume = node_volume(&only_node, 52).unwrap();
        assert_eq!(volume.channels.len(), 2);
    }

    #[test]
    fn input_levels_use_decibels() {
        assert_eq!(peak_s16le(&[]), 0.0);
        assert_eq!(peak_s16le(&[0, 0, 0, 0]), 0.0);
        // -32768은 가장 큰 진폭이다
        assert_eq!(peak_s16le(&[0x00, 0x80]), 1.0);
        assert!((peak_s16le(&[0x00, 0x40, 0x00, 0xc0]) - 0.5).abs() < 1e-3);
        // 홀수 바이트는 버린다
        assert_eq!(peak_s16le(&[0x00, 0x80, 0x7f]), 1.0);
        assert_eq!(meter_level(0.0), 0.0);
        assert_eq!(meter_level(1.0), 1.0);
        assert!((meter_level(0.001) - 0.0).abs() < 1e-9);
        assert!((meter_level(0.1) - 2.0 / 3.0).abs() < 1e-9);
    }
}
