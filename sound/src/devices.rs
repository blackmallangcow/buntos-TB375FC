// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 장치의 연결 방식. macOS 시스템 설정 > 사운드의 장치 목록에 보이는 "유형"(내장, HDMI, USB, Bluetooth)이다.
//! pw-dump의 오디오 노드 속성으로 정한다: Bluetooth는 device.api(bluez5), 나머지는 device.bus와 노드 이름
//! (alsa_output.usb-…, ….hdmi-stereo)이다. 둘 다 아니면 내장이다(가상 장치 virtio-snd도 내장으로 보인다)

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    BuiltIn,
    Hdmi,
    Usb,
    Bluetooth,
}

/// pw-dump의 오디오 노드(media.class가 Audio/로 시작)마다 (id, 연결 방식)
pub fn kinds(dump: &str) -> Vec<(u32, Kind)> {
    let Ok(Value::Array(objects)) = serde_json::from_str::<Value>(dump) else {
        return Vec::new();
    };
    objects
        .iter()
        .filter_map(|object| {
            if !object.get("type")?.as_str()?.ends_with(":Node") {
                return None;
            }
            let id = u32::try_from(object.get("id")?.as_u64()?).ok()?;
            let props = object.get("info")?.get("props")?;
            let text = |key: &str| props.get(key).and_then(Value::as_str).unwrap_or_default();
            if !text("media.class").starts_with("Audio/") {
                return None;
            }
            Some((
                id,
                kind_of(text("device.api"), text("device.bus"), text("node.name")),
            ))
        })
        .collect()
}

fn kind_of(api: &str, bus: &str, node_name: &str) -> Kind {
    let name = node_name.to_ascii_lowercase();
    if api == "bluez5" || name.starts_with("bluez_") {
        Kind::Bluetooth
    } else if bus == "usb" || name.contains(".usb-") {
        Kind::Usb
    } else if name.contains("hdmi") || name.contains("displayport") {
        Kind::Hdmi
    } else {
        Kind::BuiltIn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_nodes_get_their_connection() {
        let dump = r#"[
            {"id": 30, "type": "PipeWire:Interface:Device", "info": {"props": {"device.api": "alsa"}}},
            {"id": 52, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Audio/Sink", "device.api": "alsa",
                "node.name": "alsa_output.pci-0000_00_1f.3.analog-stereo"}}},
            {"id": 53, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Audio/Sink", "device.api": "alsa",
                "node.name": "alsa_output.pci-0000_00_1f.3.hdmi-stereo"}}},
            {"id": 54, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Audio/Source", "device.api": "alsa",
                "node.name": "alsa_input.usb-Generic_USB_Audio-00.mono-fallback"}}},
            {"id": 55, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Audio/Sink", "device.api": "bluez5",
                "node.name": "bluez_output.AA_BB_CC_DD_EE_FF.1"}}},
            {"id": 56, "type": "PipeWire:Interface:Node", "info": {"props": {
                "media.class": "Video/Source", "node.name": "v4l2_input.usb-camera"}}},
            {"id": 57, "type": "PipeWire:Interface:Node", "info": null}
        ]"#;
        assert_eq!(
            kinds(dump),
            [
                (52, Kind::BuiltIn),
                (53, Kind::Hdmi),
                (54, Kind::Usb),
                (55, Kind::Bluetooth),
            ]
        );
    }

    #[test]
    fn broken_dumps_have_no_kinds() {
        assert!(kinds("").is_empty());
        assert!(kinds("{}").is_empty());
    }
}
