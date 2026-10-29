// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 시스템 서비스가 사람에게 묻는 창의 틀. Wi-Fi 암호 창(secret_agent.rs)과 Bluetooth 페어링 창
//! (pairing_agent.rs)이 쓴다. 관리자 인증 창(polkit_agent.rs)과 같은 모양이다: 화면 가운데의 유리 판,
//! 64px 그림, 굵은 제목, 설명, 칸, 같은 너비의 캡슐 단추(style.css의 auth-* 클래스).
//!
//! 창의 레이어 이름은 사람만 답하는 창의 목록(comp/src/agent.rs, agent/src/atspi.rs)에 있어야 한다.
//! 컴포지터는 이 창이 떠 있는 동안 에이전트의 입력을 거절하고, 에이전트 데몬은 이 창을 읽지 않는다.

use std::cell::Cell;

use gtk4::{self as gtk, gdk, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};

/// 사람이 친 비밀(암호, PIN)의 사본. 버릴 때 0으로 덮는다. D-Bus로 답할 때 GLib이 만드는 사본은 덮지
/// 못하므로 사본을 오래 들고 있지 않는다
pub(crate) struct Secret(Vec<u8>);

impl Secret {
    pub fn new(text: &str) -> Self {
        Self(text.as_bytes().to_vec())
    }

    pub fn as_str(&self) -> &str {
        // new()가 &str에서 만들었으므로 늘 UTF-8이다
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&self.0);
    }
}

/// 기록에 비밀이 새지 않게 내용 대신 길이만 보인다
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret({} bytes)", self.0.len())
    }
}

/// 묻는 창 하나: 레이어 창과 그 안의 판
pub(crate) struct Frame {
    pub window: gtk::ApplicationWindow,
    pub panel: gtk::Box,
    /// 흔들기 애니메이션을 다시 시작하려고 두 클래스를 번갈아 쓴다
    shakes: Cell<u32>,
    /// 사람이 답해 닫은 창을 에이전트가 다시 닫아도 된다
    closed: Cell<bool>,
}

impl Frame {
    /// `keyboard`: 사람이 방금 고른 일에 대한 물음은 키보드를 독점(Exclusive)하고, 저절로 뜨는 물음은
    /// 창을 눌렀을 때만 키보드를 받는다(OnDemand). 다른 앱에 치던 글이 암호 칸으로 들어가지 않게 한다
    pub fn new(app: &gtk::Application, namespace: &str, keyboard: KeyboardMode) -> Self {
        let window = crate::layer_window(app, namespace);
        window.set_layer(Layer::Overlay);
        window.set_keyboard_mode(keyboard);
        if crate::interface_settings()
            .is_some_and(|settings| settings.string("color-scheme") == "prefer-dark")
        {
            window.add_css_class("dark");
        }
        let panel = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["auth-panel"])
            .build();
        window.set_child(Some(&panel));
        Self {
            window,
            panel,
            shakes: Cell::new(0),
            closed: Cell::new(false),
        }
    }

    /// 64px 색 타일 위의 흰 기호 (관리자 인증 창의 회색 자물쇠 타일과 같은 크기)
    pub fn tile(&self, icon: &str, class: &str) {
        let image = gtk::Image::from_icon_name(icon);
        image.set_pixel_size(34);
        image.set_hexpand(true);
        image.set_vexpand(true);
        let tile = gtk::Box::builder()
            .width_request(64)
            .height_request(64)
            .css_classes(["auth-lock-tile", class])
            .build();
        tile.append(&image);
        let frame = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .css_classes(["auth-icon"])
            .build();
        frame.append(&tile);
        self.panel.append(&frame);
    }

    pub fn title(&self, text: &str) -> gtk::Label {
        let label = text_label(text, "auth-title");
        self.panel.append(&label);
        label
    }

    /// 칸을 쌓는 상자
    pub fn fields(&self) -> gtk::Box {
        let fields = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .css_classes(["auth-fields"])
            .build();
        self.panel.append(&fields);
        fields
    }

    /// 칸 아래의 안내 줄. 오류(error)는 빨간 글이다. 처음에는 숨어 있다
    pub fn info(&self) -> gtk::Label {
        let label = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .justify(gtk::Justification::Center)
            .max_width_chars(34)
            .visible(false)
            .css_classes(["auth-info"])
            .build();
        self.panel.append(&label);
        label
    }

    /// 같은 너비의 캡슐 단추. 확인 단추가 있으면 기본 단추(Enter)다
    pub fn buttons(
        &self,
        cancel: &str,
        confirm: Option<&str>,
    ) -> (gtk::Button, Option<gtk::Button>) {
        let buttons = gtk::Box::builder()
            .spacing(8)
            .homogeneous(true)
            .css_classes(["auth-buttons"])
            .build();
        let cancel = gtk::Button::with_label(cancel);
        buttons.append(&cancel);
        let confirm = confirm.map(|label| {
            let button = gtk::Button::builder()
                .label(label)
                .css_classes(["suggested-action"])
                .build();
            buttons.append(&button);
            self.window.set_default_widget(Some(&button));
            button
        });
        self.panel.append(&buttons);
        (cancel, confirm)
    }

    /// Esc를 누르면 `cancel`을 부른다
    pub fn on_escape(&self, cancel: impl Fn() + 'static) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            cancel();
            glib::Propagation::Stop
        });
        self.window.add_controller(keys);
    }

    /// 틀렸을 때처럼 판을 옆으로 흔든다 (style.css의 shake-a, shake-b)
    pub fn shake(&self) {
        let shake = self.shakes.get();
        self.shakes.set(shake.wrapping_add(1));
        let (old, new) = if shake.is_multiple_of(2) {
            ("shake-b", "shake-a")
        } else {
            ("shake-a", "shake-b")
        };
        self.panel.remove_css_class(old);
        self.panel.add_css_class(new);
    }

    /// 창을 띄운다. 포커스는 띄우기 전에 정해야 칸의 글이 선택된 채로 남지 않는다
    pub fn present(&self, focus: Option<&gtk::Widget>) {
        if let Some(focus) = focus {
            GtkWindowExt::set_focus(&self.window, Some(focus));
        }
        self.window.present();
    }

    pub fn close(&self) {
        if !self.closed.replace(true) {
            self.window.destroy();
        }
    }
}

fn text_label(text: &str, class: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .justify(gtk::Justification::Center)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(34)
        .css_classes([class])
        .build()
}

/// 암호 칸과 그 아래 "암호 보기" 체크 상자 (macOS의 Wi-Fi 암호 창). 칸은 GTK의 암호 칸이라 글을 바꿀 수
/// 없는 메모리에 두고, 체크 상자는 칸 안의 글 위젯의 보이기만 바꾼다
pub(crate) fn password_field(
    fields: &gtk::Box,
    placeholder: &str,
    show_label: Option<&str>,
) -> gtk::PasswordEntry {
    let entry = gtk::PasswordEntry::builder()
        .placeholder_text(placeholder)
        .activates_default(true)
        .css_classes(["auth-field"])
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(placeholder)]);
    fields.append(&entry);
    if let Some(show_label) = show_label {
        let show = gtk::CheckButton::builder()
            .label(show_label)
            .halign(gtk::Align::Start)
            .css_classes(["auth-check"])
            .build();
        let text = entry.delegate().and_downcast::<gtk::Text>();
        show.connect_toggled(move |show| {
            if let Some(text) = &text {
                text.set_visibility(show.is_active());
            }
        });
        fields.append(&show);
    }
    entry
}

/// 보이는 글 칸 (사용자 이름, PIN)
pub(crate) fn text_field(fields: &gtk::Box, placeholder: &str, text: &str) -> gtk::Entry {
    let entry = gtk::Entry::builder()
        .text(text)
        .placeholder_text(placeholder)
        .activates_default(true)
        .css_classes(["auth-field"])
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(placeholder)]);
    fields.append(&entry);
    entry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_do_not_show_in_debug_output() {
        let secret = Secret::new("correct horse");
        assert_eq!(secret.as_str(), "correct horse");
        assert_eq!(format!("{secret:?}"), "Secret(13 bytes)");
    }
}
