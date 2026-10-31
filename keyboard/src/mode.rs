// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 입력 모드 (설계안 5.1절): 포인터 모드와 터치 모드. 컴포지터가 입력 장치로 정하고 알린다
//! (`org.buntos.Compositor1.GetInputMode() -> s`, 신호 `InputModeChanged(s)`, "pointer" 또는 "touch").
//! 화면 키보드는 터치 모드에서만 뜬다. 컴포지터가 이 API를 모르거나 버스에 없으면 포인터 모드로 본다.

use std::rc::Rc;

use gtk4::{gio, glib};

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
const CALL_TIMEOUT: i32 = 2000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Pointer,
    Touch,
}

impl InputMode {
    /// 컴포지터가 알린 이름. 모르는 이름은 포인터 모드다
    pub fn parse(name: &str) -> Self {
        match name {
            "touch" => InputMode::Touch,
            _ => InputMode::Pointer,
        }
    }
}

/// 입력 모드를 지켜본다. 처음 한 번 읽어 알리고, 바뀔 때마다 알린다. 처리기는 프로그램이 끝날 때까지 산다
pub fn watch(changed: impl Fn(InputMode) + 'static) {
    let changed = Rc::new(changed);
    glib::spawn_future_local(async move {
        let bus = match gio::bus_get_future(gio::BusType::Session).await {
            Ok(bus) => bus,
            Err(err) => {
                eprintln!("bunt-keyboard: no session bus, staying in pointer mode: {err}");
                changed(InputMode::Pointer);
                return;
            }
        };
        // 읽기 전에 신호부터 받는다. 같은 연결의 답과 신호는 보낸 차례로 오므로 늦게 온 답이 새 값을 덮지 않는다
        let on_signal = changed.clone();
        let subscription = bus.subscribe_to_signal(
            Some(NAME),
            Some(INTERFACE),
            Some("InputModeChanged"),
            Some(PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let Some((mode,)) = signal.parameters.get::<(String,)>() {
                    on_signal(InputMode::parse(&mode));
                }
            },
        );
        // 끝날 때까지 받는다
        std::mem::forget(subscription);
        let mode = match bus
            .call_future(
                Some(NAME),
                PATH,
                INTERFACE,
                "GetInputMode",
                None,
                Some(glib::VariantTy::new("(s)").expect("a valid type")),
                gio::DBusCallFlags::NONE,
                CALL_TIMEOUT,
            )
            .await
        {
            Ok(reply) => reply
                .child_value(0)
                .get::<String>()
                .map_or(InputMode::Pointer, |mode| InputMode::parse(&mode)),
            // 입력 모드를 모르는 컴포지터 (터치·입력 모드 작업 이전)
            Err(_) => InputMode::Pointer,
        };
        changed(mode);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_touch_is_touch_mode() {
        assert_eq!(InputMode::parse("touch"), InputMode::Touch);
        assert_eq!(InputMode::parse("pointer"), InputMode::Pointer);
        assert_eq!(InputMode::parse(""), InputMode::Pointer);
        assert_eq!(InputMode::parse("Touch"), InputMode::Pointer);
        assert_eq!(InputMode::default(), InputMode::Pointer);
    }
}
