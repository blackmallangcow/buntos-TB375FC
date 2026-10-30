// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 기록 중에 메뉴 막대에 뜨는 기록 중단 단추 (macOS). 기록은 ⇧⌘5 도구 막대가 시작하고(screenshot.rs)
//! 컴포지터가 한다. 컴포지터가 기록을 시작하고 멈출 때 알리면(RecordingChanged) 단추를 보이고 숨기고, 셸이
//! 다시 떠도 기록 중이면 보이게 처음에 묻는다(IsRecording). 누르면 컴포지터가 기록을 멈추고 파일을
//! 마무리한다(StopRecording). 저장한 기록의 미리 보기는 스크린샷 미리 보기가 띄운다

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
/// 기록 중단 아이콘 (Adwaita 기호 아이콘)
const ICON: &str = "media-playback-stop-symbolic";

/// 메뉴 막대의 기록 중단 단추. 기록하는 동안만 보인다
pub fn stop_button(tr: &Translations) -> gtk::Button {
    let label = tr.get("recording-stop");
    let button = gtk::Button::builder()
        .icon_name(ICON)
        .tooltip_text(&label)
        .css_classes(["recording-stop"])
        .visible(false)
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&label)]);
    // 컴포지터가 멈췄다고 알리면(RecordingChanged) 숨긴다. 미리 숨기면 부르기가 실패했을 때 멈출 길이 없다.
    // 두 번 눌러도 두 번째는 아무것도 하지 않는다
    button.connect_clicked(|_| call("StopRecording", None, |_| {}));
    follow(&button);
    button
}

/// 컴포지터가 기록을 시작하고 멈출 때마다 단추를 보이고 숨긴다. 처음에는 지금 기록 중인지 묻는다
fn follow(button: &gtk::Button) {
    let weak = button.downgrade();
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let connection = match result {
                Ok(connection) => connection,
                Err(err) => {
                    eprintln!("bunt-shell: session bus: {err}");
                    return;
                }
            };
            let changed = weak.clone();
            let subscription = connection.subscribe_to_signal(
                Some(NAME),
                Some(INTERFACE),
                Some("RecordingChanged"),
                Some(PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if let (Some((recording,)), Some(button)) =
                        (signal.parameters.get::<(bool,)>(), changed.upgrade())
                    {
                        button.set_visible(recording);
                    }
                },
            );
            // 셸이 끝날 때까지 받는다
            std::mem::forget(subscription);
            call(
                "IsRecording",
                glib::VariantTy::new("(b)").ok(),
                move |reply| {
                    if let (Some((recording,)), Some(button)) =
                        (reply.get::<(bool,)>(), weak.upgrade())
                    {
                        button.set_visible(recording);
                    }
                },
            );
        },
    );
}

/// 컴포지터의 메서드를 부르고 답을 `done`에 준다. 실패하면 기록만 남긴다
fn call(
    method: &'static str,
    reply: Option<&'static glib::VariantTy>,
    done: impl FnOnce(glib::Variant) + 'static,
) {
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let Ok(connection) = result else {
                return;
            };
            connection.call(
                Some(NAME),
                PATH,
                INTERFACE,
                method,
                None,
                reply,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                move |result| match result {
                    Ok(reply) => done(reply),
                    Err(err) => eprintln!("bunt-shell: {INTERFACE}.{method}: {err}"),
                },
            );
        },
    );
}
