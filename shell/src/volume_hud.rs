// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 음량 키의 표시 (macOS 26). 키를 누르면 메뉴 막대 아래 오른쪽에 작은 음량 막대를
//! 잠깐 띄운다. 키는 컴포지터가 받아 ShortcutActivated로 알린다. 막대를 끌어도
//! 음량이 바뀌고, 포인터가 위에 있는 동안은 닫지 않는다. 출력 장치가 없으면 뜨지 않는다.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, glib, prelude::*};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

use crate::{
    clip_input_to_child, layer_window,
    sound::{self, Sound},
};

/// 마지막 키 뒤로 보이는 시간
const SHOWN: Duration = Duration::from_millis(1500);
/// 사라지는 시간 (style.css의 --duration)
const FADE: Duration = Duration::from_millis(200);
const WIDTH: i32 = 240;

/// 복제해도 같은 표시를 가리킨다
#[derive(Clone)]
pub struct VolumeHud(Rc<Inner>);

struct Inner {
    window: gtk::ApplicationWindow,
    panel: gtk::Box,
    sound: Sound,
    timer: RefCell<Option<glib::SourceId>>,
    hovered: Cell<bool>,
}

impl VolumeHud {
    pub fn start(app: &gtk::Application, tr: &Translations, sound: &Sound) -> Self {
        let window = layer_window(app, "bunt-volume-hud");
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Top, true);
        window.set_anchor(Edge::Right, true);
        window.set_resizable(false);
        clip_input_to_child(&window);
        let panel = gtk::Box::builder()
            .width_request(WIDTH)
            .css_classes(["volume-hud"])
            .build();
        panel.append(&sound::slider(tr, sound));
        window.set_child(Some(&panel));
        let hud = Self(Rc::new(Inner {
            window,
            panel,
            sound: sound.clone(),
            timer: RefCell::default(),
            hovered: Cell::new(false),
        }));
        let motion = gtk::EventControllerMotion::new();
        let (entered, left) = (hud.clone(), hud.clone());
        motion.connect_enter(move |_, _, _| {
            entered.0.hovered.set(true);
            entered.stop_timer();
        });
        motion.connect_leave(move |_| {
            left.0.hovered.set(false);
            left.start_timer();
        });
        hud.0.panel.add_controller(motion);
        hud
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    pub fn step(&self, up: bool) {
        self.0.sound.step(up);
        self.show();
    }

    pub fn toggle_mute(&self) {
        self.0.sound.toggle_mute();
        self.show();
    }

    fn show(&self) {
        if self.0.sound.volume().is_none() {
            return;
        }
        // 떠 있는 창을 다시 띄우면 그때마다 배치를 새로 해서 막대가 끊긴다
        if !self.0.window.is_visible() {
            self.0.window.set_visible(true);
            self.0.window.present();
        }
        // 첫 그림 뒤에 켜야 나타나는 전환이 돈다
        let panel = self.0.panel.clone();
        glib::idle_add_local_once(move || panel.add_css_class("shown"));
        self.start_timer();
    }

    fn stop_timer(&self) {
        if let Some(timer) = self.0.timer.take() {
            timer.remove();
        }
    }

    fn start_timer(&self) {
        self.stop_timer();
        if self.0.hovered.get() {
            return;
        }
        let hud = self.clone();
        let timer = glib::timeout_add_local_once(SHOWN, move || {
            hud.0.timer.take();
            hud.0.panel.remove_css_class("shown");
            let faded = hud.clone();
            let timer = glib::timeout_add_local_once(FADE, move || {
                faded.0.timer.take();
                faded.0.window.set_visible(false);
            });
            hud.0.timer.replace(Some(timer));
        });
        self.0.timer.replace(Some(timer));
    }
}
