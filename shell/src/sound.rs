// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴 막대의 사운드 항목과 음량. PipeWire의 세션 관리자(WirePlumber)에 wpctl로 묻고 바꾼다.
//! 출력 장치가 없으면(소리 장치가 없는 VM) 항목을 숨긴다. 누르면 음량 막대와 출력 장치가 나온다.
//! wpctl 출력 읽기와 채널 음량은 시스템 설정의 사운드 페이지와 함께 쓰는 bunt-sound 크레이트에 있다. 시스템
//! 설정에서 균형을 둔 장치는 음량을 바꿔도 균형이 그대로다(wpctl set-volume은 모든 채널을 같게 둔다).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use bunt_i18n::Translations;
use bunt_sound::{
    mixer, run,
    wpctl::{DEFAULT_SINK, Node, Section},
};
use gtk4::{self as gtk, glib, prelude::*};

use crate::{
    extras::{self, Listeners, Menu, Visibility},
    menubar::open_settings,
};

/// 다른 곳(앱, 키)에서 바꾼 음량을 따라가는 간격
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// 막대를 끄는 동안 wpctl을 너무 자주 부르지 않게 모으는 시간
const SET_DELAY: Duration = Duration::from_millis(60);
/// 음량이 바뀔 때 막대가 따라가는 시간 (--duration-fast)
const GLIDE_US: f64 = 120_000.0;

/// 음량 키가 나누는 칸 (macOS와 같다)
const KEY_STEPS: f64 = 16.0;

/// 음량 키를 눌렀을 때의 음량. 칸 사이에 있으면 가까운 칸에서 움직인다
fn stepped(volume: f64, up: bool) -> f64 {
    let step = (volume * KEY_STEPS).round() + if up { 1.0 } else { -1.0 };
    (step / KEY_STEPS).clamp(0.0, 1.0)
}

/// 음량 아이콘
pub fn volume_icon(volume: f64, muted: bool) -> &'static str {
    match volume {
        _ if muted || volume <= 0.0 => "audio-volume-muted-symbolic",
        ..0.34 => "audio-volume-low-symbolic",
        ..0.67 => "audio-volume-medium-symbolic",
        _ => "audio-volume-high-symbolic",
    }
}

/// 음량 상태. 복제해도 같은 상태를 가리킨다. 제어 센터의 사운드 막대도 같은 것을 쓴다
#[derive(Clone)]
pub struct Sound(Rc<Inner>);

struct Inner {
    /// (음량, 음소거). 출력 장치가 없으면 None
    volume: Cell<Option<(f64, bool)>>,
    /// 마지막으로 읽은 채널 음량. 균형을 둔 장치의 음량을 바꿀 때 채널 사이의 비를 둔다
    mixer: RefCell<Option<mixer::Volume>>,
    pending: Cell<Option<f64>>,
    /// 음량을 바꾸는 명령이 도는 중이다. 앞뒤가 바뀌지 않게 한 번에 하나만 돌린다
    applying: Cell<bool>,
    listeners: Listeners<Sound>,
}

impl Sound {
    pub fn start() -> Self {
        let sound = Self(Rc::new(Inner {
            volume: Cell::new(None),
            mixer: RefCell::default(),
            pending: Cell::new(None),
            applying: Cell::new(false),
            listeners: RefCell::default(),
        }));
        sound.refresh();
        let weak = Rc::downgrade(&sound.0);
        glib::timeout_add_local(POLL_INTERVAL, move || match weak.upgrade() {
            Some(inner) => {
                Sound(inner).refresh();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        sound
    }

    pub fn refresh(&self) {
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            // wpctl get-volume은 첫 채널의 음량을 준다. 균형을 둔 장치에서도 가장 큰 채널을 보이려고 채널 음량을 읽는다
            let mixer = run::volume(DEFAULT_SINK).await;
            let volume = mixer.as_ref().map(|mixer| (mixer.level(), mixer.muted));
            let Some(inner) = weak.upgrade() else {
                return;
            };
            // 바꾸는 중이면 그 값이 먼저다. 읽은 값은 이미 지난 값일 수 있다
            if inner.pending.get().is_some() || inner.applying.get() {
                return;
            }
            inner.mixer.replace(mixer);
            if inner.volume.get() == volume {
                return;
            }
            inner.volume.set(volume);
            Sound(inner).notify();
        });
    }

    fn notify(&self) {
        for listener in self.0.listeners.borrow().iter() {
            listener(self);
        }
    }

    pub fn subscribe(&self, listener: impl Fn(&Sound) + 'static) {
        listener(self);
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    /// (음량 0~1, 음소거). 출력 장치가 없으면 None
    pub fn volume(&self) -> Option<(f64, bool)> {
        self.0.volume.get()
    }

    /// 음량을 바꾼다. 막대를 끄는 동안 오는 값은 모아서 마지막 것만 보낸다
    pub fn set_volume(&self, volume: f64) {
        let volume = volume.clamp(0.0, 1.0);
        let first = self.0.pending.replace(Some(volume)).is_none();
        self.0.volume.set(Some((volume, false)));
        // 막대와 아이콘은 바로 따라간다. 명령만 모아서 보낸다
        self.notify();
        if first && !self.0.applying.get() {
            self.apply_later();
        }
    }

    fn apply_later(&self) {
        let weak = Rc::downgrade(&self.0);
        glib::timeout_add_local_once(SET_DELAY, move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let Some(volume) = inner.pending.take() else {
                return;
            };
            inner.applying.set(true);
            let sound = Sound(inner);
            let mixer = sound.0.mixer.borrow().clone();
            glib::spawn_future_local(async move {
                if mixer.as_ref().is_none_or(|mixer| mixer.muted) {
                    run::set_mute(DEFAULT_SINK, false).await;
                }
                // 균형을 둔 장치는 채널 사이의 비를 둔 채 바꾼다. 채널을 모르면 전처럼 wpctl set-volume이다
                match mixer {
                    Some(mixer) => run::set_level(DEFAULT_SINK, &mixer, volume).await,
                    None => {
                        let level = format!("{volume:.2}");
                        run::run(&["wpctl", "set-volume", "-l", "1.0", DEFAULT_SINK, &level]).await
                    }
                };
                sound.0.applying.set(false);
                if sound.0.pending.get().is_some() {
                    sound.apply_later();
                } else {
                    sound.refresh();
                }
            });
        });
    }

    /// 음량 키 한 번. 올리는 키는 소리 끔을 풀고, 내리는 키는 소리를 끈 동안 음량을
    /// 그대로 둔다
    pub fn step(&self, up: bool) {
        let Some((volume, muted)) = self.volume() else {
            return;
        };
        if up || !muted {
            self.set_volume(stepped(volume, up));
        }
    }

    pub fn toggle_mute(&self) {
        let sound = self.clone();
        glib::spawn_future_local(async move {
            run::toggle_mute(DEFAULT_SINK).await;
            sound.0.volume.set(None);
            sound.refresh();
        });
    }

    pub async fn sinks() -> Vec<Node> {
        run::nodes(Section::Sinks).await
    }

    pub fn set_default_sink(&self, id: u32) {
        let sound = self.clone();
        glib::spawn_future_local(async move {
            run::set_default(id).await;
            sound.0.volume.set(None);
            sound.refresh();
        });
    }
}

/// 막대가 새 값으로 움직이는 중의 상태
struct Glide {
    from: f64,
    to: f64,
    /// 첫 그림의 시각(마이크로초)
    started: Option<i64>,
}

/// 막대를 새 값까지 부드럽게 옮긴다. 끄는 중이면 막대가 이미 그 값이라 움직이지 않고,
/// 보이지 않는 막대는 바로 옮긴다. `guard`는 value-changed 처리를 막는다
fn glide_to(
    scale: &gtk::Scale,
    to: f64,
    guard: &Rc<Cell<bool>>,
    glide: &Rc<RefCell<Option<Glide>>>,
) {
    let from = scale.value();
    if (from - to).abs() < 0.001 || !scale.is_mapped() {
        glide.replace(None);
        guard.set(true);
        scale.set_value(to);
        guard.set(false);
        return;
    }
    let running = glide
        .replace(Some(Glide {
            from,
            to,
            started: None,
        }))
        .is_some();
    if running {
        return;
    }
    let (guard, glide) = (guard.clone(), glide.clone());
    scale.add_tick_callback(move |scale, clock| {
        let mut slot = glide.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return glib::ControlFlow::Break;
        };
        let now = clock.frame_time();
        let started = *state.started.get_or_insert(now);
        let progress = ((now - started) as f64 / GLIDE_US).min(1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        guard.set(true);
        scale.set_value(state.from + (state.to - state.from) * eased);
        guard.set(false);
        if progress >= 1.0 {
            *slot = None;
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
}

/// 음량 막대. 제어 센터와 사운드 메뉴가 쓴다
pub fn slider(tr: &Translations, sound: &Sound) -> gtk::Box {
    let icon = gtk::Button::builder()
        .icon_name("audio-volume-medium-symbolic")
        .css_classes(["slider-icon"])
        .build();
    icon.update_property(&[gtk::accessible::Property::Label(&tr.get("sound-mute"))]);
    let scale = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 1.0, 0.01, 0.1, 0.0))
        .hexpand(true)
        .css_classes(["volume-slider"])
        .build();
    scale.update_property(&[gtk::accessible::Property::Label(&tr.get("sound-volume"))]);
    let row = gtk::Box::builder()
        .spacing(6)
        .css_classes(["slider-row"])
        .build();
    row.append(&icon);
    row.append(&scale);

    // 상태에서 막대로 옮길 때는 value-changed 처리를 막는다
    let updating = Rc::new(Cell::new(false));
    let glide = Rc::new(RefCell::new(None));
    let (shown_icon, shown_scale, guard) = (icon.clone(), scale.clone(), updating.clone());
    sound.subscribe(move |sound| {
        let (volume, muted) = sound.volume().unwrap_or((0.0, true));
        let shown = if muted { 0.0 } else { volume };
        glide_to(&shown_scale, shown, &guard, &glide);
        shown_icon.set_icon_name(volume_icon(volume, muted));
    });
    let changer = sound.clone();
    scale.connect_value_changed(move |scale| {
        if !updating.get() {
            changer.set_volume(scale.value());
        }
    });
    let muter = sound.clone();
    icon.connect_clicked(move |_| muter.toggle_mute());
    row
}

pub fn extra(tr: &Rc<Translations>, sound: &Sound) -> gtk::MenuButton {
    let image = gtk::Image::from_icon_name("audio-volume-medium-symbolic");
    let menu = Menu::new();
    let button = extras::button(&image, &menu, &tr.get("sound-title"));
    let visibility = Visibility::new(&button, "show-sound");
    sound.subscribe(move |sound| {
        visibility.set_available(sound.volume().is_some());
        if let Some((volume, muted)) = sound.volume() {
            image.set_icon_name(Some(volume_icon(volume, muted)));
        }
    });

    let tr = tr.clone();
    let sound = sound.clone();
    menu.on_show(move |menu| {
        sound.refresh();
        menu.title(&tr.get("sound-title"));
        menu.append(&slider(&tr, &sound));
        menu.separator();
        menu.section(&tr.get("sound-output"));
        // 장치 목록은 wpctl status를 읽은 뒤 채운다
        let devices = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        menu.append(&devices);
        let (sound, popover) = (sound.clone(), menu.popover().clone());
        glib::spawn_future_local(async move {
            for sink in Sound::sinks().await {
                let mark = gtk::Image::from_icon_name("object-select-symbolic");
                mark.set_opacity(if sink.default { 1.0 } else { 0.0 });
                let content = gtk::Box::builder().spacing(6).build();
                content.append(&mark);
                content.append(
                    &gtk::Label::builder()
                        .label(&sink.name)
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .max_width_chars(32)
                        .build(),
                );
                let row = gtk::Button::builder()
                    .child(&content)
                    .css_classes(["extra-row"])
                    .build();
                let (sound, popover, id) = (sound.clone(), popover.clone(), sink.id);
                row.connect_clicked(move |_| {
                    popover.popdown();
                    sound.set_default_sink(id);
                });
                devices.append(&row);
            }
        });
        // macOS처럼 맨 아래에 시스템 설정의 사운드 페이지로 가는 줄을 둔다
        menu.separator();
        menu.action(&tr.get("sound-settings"), || open_settings(Some("sound")));
    });
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_keys_move_by_a_sixteenth_from_the_nearest_step() {
        assert_eq!(stepped(0.5, true), 0.5625);
        assert_eq!(stepped(0.5, false), 0.4375);
        // PipeWire의 기본값 40%는 칸 사이에 있다
        assert_eq!(stepped(0.4, true), 0.4375);
        assert_eq!(stepped(0.4, false), 0.3125);
        assert_eq!(stepped(1.0, true), 1.0);
        assert_eq!(stepped(0.0, false), 0.0);
    }

    #[test]
    fn volume_icons() {
        assert_eq!(volume_icon(0.5, true), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.2, false), "audio-volume-low-symbolic");
        assert_eq!(volume_icon(0.5, false), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon(0.9, false), "audio-volume-high-symbolic");
    }
}
