// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 사운드: macOS 26 시스템 설정 > 사운드를 따른다.
//! - 사운드 효과: 경고음과 경고음 음량. 셸 설정(org.buntos.shell의 alert-sound, alert-volume)에 두고, 셸이 앱이
//!   소리를 정하지 않은 알림에 낸다. 고르거나 음량 막대를 놓으면 그 소리를 한 번 들려준다
//! - 출력 및 입력: [출력 | 입력] 단추로 장치 목록을 바꿔 본다. 줄을 누르면 기본 장치가 된다. 출력 쪽은 출력 음량,
//!   음소거, 균형(채널이 하나인 장치는 숨긴다), 입력 쪽은 입력 음량과 입력 레벨이다
//! - 장치와 음량은 셸의 사운드 메뉴와 같은 bunt-sound(wpctl, pw-dump, pw-cli)로 읽고 바꾼다. 페이지가 보이는 동안
//!   몇 초마다 다시 읽어 메뉴 막대나 키로 바꾼 음량을 따라간다
//! - 입력 레벨은 입력 쪽이 보이는 동안만 기본 입력 장치를 녹음해(pw-record) 잰다. 녹음한 소리는 남기지 않는다

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use bunt_sound::{
    devices::Kind,
    mixer::Volume,
    run, theme,
    wpctl::{DEFAULT_SINK, DEFAULT_SOURCE, Node},
};
use gtk4::{self as gtk, gio, glib, prelude::*};

use super::{Context, bind_scale, has_key, settings_for, shell_settings};
use crate::widgets::{self, BadgeSize, Tint, badge};

const ICON: &str = "audio-volume-high-symbolic";
/// 사운드 테마 이름 (GNOME 공용 설정)
const SOUND_SCHEMA: &str = "org.gnome.desktop.sound";
/// 페이지가 보이는 동안 장치와 음량을 다시 읽는 간격 (셸의 사운드 메뉴와 같다)
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// 막대를 끄는 동안 바꾸는 명령을 모으는 시간
const SET_DELAY: Duration = Duration::from_millis(60);
/// 사람이 막대를 움직인 뒤 이만큼은 다시 읽은 값으로 막대를 옮기지 않는다
const HOLD_AFTER_CHANGE: Duration = Duration::from_millis(1500);
/// 경고음 음량 막대를 놓은 뒤 소리를 들려주기까지
const PREVIEW_DELAY: Duration = Duration::from_millis(300);
/// 입력 레벨 막대의 칸 수
const LEVEL_BLOCKS: f64 = 15.0;
/// 균형 막대가 가운데에 붙는 거리
const BALANCE_SNAP: f64 = 0.05;
/// 음량 막대의 너비 (widgets::slider_row와 같다)
const SLIDER_WIDTH: i32 = 260;

/// [출력 | 입력] 가운데 보이는 쪽
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Output,
    Input,
}

/// 사람이 움직이는 막대
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Output,
    Balance,
    Input,
}

/// 막대를 끄는 동안의 값. 바꾸는 명령이 끝나기 전에 온 값은 마지막 것만 적용한다
#[derive(Default)]
struct Pending {
    value: Cell<Option<f64>>,
    running: Cell<bool>,
}

struct SoundPage {
    context: Rc<Context>,
    direction: Cell<Direction>,
    /// 맨 위 줄은 [출력 | 입력] 단추, 그 아래가 장치 줄이다
    devices: gtk::ListBox,
    device_rows: RefCell<Vec<gtk::ListBoxRow>>,
    /// 목록을 다시 그릴지 정하는 요약. 같으면 그대로 둔다
    rendered: RefCell<String>,
    output_group: gtk::ListBox,
    output_scale: gtk::Scale,
    mute: gtk::CheckButton,
    balance_row: gtk::ListBoxRow,
    balance_scale: gtk::Scale,
    input_group: gtk::ListBox,
    input_scale: gtk::Scale,
    level: gtk::LevelBar,
    sinks: RefCell<Vec<Node>>,
    sources: RefCell<Vec<Node>>,
    /// 장치 id → 연결 방식. 새 장치가 보일 때만 pw-dump로 읽는다
    kinds: RefCell<HashMap<u32, Kind>>,
    output: RefCell<Option<Volume>>,
    input: RefCell<Option<Volume>>,
    /// 코드에서 막대와 체크 상자를 옮기는 중이면 바꾸는 명령을 보내지 않는다
    updating: Cell<bool>,
    touched: Cell<Option<Instant>>,
    output_pending: Pending,
    balance_pending: Pending,
    input_pending: Pending,
    visible: Cell<bool>,
    poll: RefCell<Option<glib::SourceId>>,
    /// 입력 레벨 녹음의 세대. 바뀌면 돌던 녹음이 멈춘다
    meter: Cell<u32>,
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();

    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Pink, BadgeSize::Medium),
        &tr.get("page-sound"),
        &tr.get("sound-description"),
        None,
    ));

    sound_effects(context, &column);

    let devices = widgets::group(&column, Some(&tr.get("sound-output-input")), None);
    let segments = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .css_classes(["segmented"])
        .build();
    let output_button = gtk::ToggleButton::builder()
        .label(tr.get("sound-output"))
        .active(true)
        .build();
    let input_button = gtk::ToggleButton::builder()
        .label(tr.get("sound-input"))
        .group(&output_button)
        .build();
    segments.append(&output_button);
    segments.append(&input_button);
    devices.append(
        &gtk::ListBoxRow::builder()
            .child(&segments)
            .activatable(false)
            .css_classes(["segmented-row"])
            .build(),
    );

    let output_group = widgets::group(&column, None, None);
    let output_scale = volume_scale();
    let mute = gtk::CheckButton::with_label(&tr.get("sound-mute"));
    output_group.append(&widgets::row(
        &tr.get("sound-output-volume"),
        &icon_slider(
            &output_scale,
            "audio-volume-low-symbolic",
            "audio-volume-high-symbolic",
            &tr.get("sound-output-volume"),
            Some(&mute),
        ),
    ));
    let balance_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, -1.0, 1.0, 0.01);
    balance_scale.set_draw_value(false);
    balance_scale.add_mark(0.0, gtk::PositionType::Bottom, None);
    let balance_row = widgets::slider_row(
        &tr.get("sound-balance"),
        &balance_scale,
        &tr.get("sound-balance-left"),
        &tr.get("sound-balance-right"),
    );
    output_group.append(&balance_row);

    let input_group = widgets::group(&column, None, Some(&tr.get("sound-input-description")));
    let input_scale = volume_scale();
    input_group.append(&widgets::row(
        &tr.get("sound-input-volume"),
        &icon_slider(
            &input_scale,
            "microphone-sensitivity-low-symbolic",
            "microphone-sensitivity-high-symbolic",
            &tr.get("sound-input-volume"),
            None,
        ),
    ));
    let level = gtk::LevelBar::builder()
        .mode(gtk::LevelBarMode::Discrete)
        .min_value(0.0)
        .max_value(LEVEL_BLOCKS)
        .width_request(SLIDER_WIDTH)
        .valign(gtk::Align::Center)
        .build();
    // 레벨 막대는 색이 바뀌는 구간 없이 초록이다 (macOS와 같다)
    for name in ["low", "high", "full"] {
        level.remove_offset_value(Some(name));
    }
    level.update_property(&[gtk::accessible::Property::Label(
        &tr.get("sound-input-level"),
    )]);
    input_group.append(&widgets::row(&tr.get("sound-input-level"), &level));
    widgets::set_group_visible(&input_group, false);

    let page = Rc::new(SoundPage {
        context: context.clone(),
        direction: Cell::new(Direction::Output),
        devices: devices.clone(),
        device_rows: RefCell::default(),
        rendered: RefCell::default(),
        output_group,
        output_scale: output_scale.clone(),
        mute: mute.clone(),
        balance_row,
        balance_scale: balance_scale.clone(),
        input_group,
        input_scale: input_scale.clone(),
        level,
        sinks: RefCell::default(),
        sources: RefCell::default(),
        kinds: RefCell::default(),
        output: RefCell::default(),
        input: RefCell::default(),
        updating: Cell::new(false),
        touched: Cell::new(None),
        output_pending: Pending::default(),
        balance_pending: Pending::default(),
        input_pending: Pending::default(),
        visible: Cell::new(false),
        poll: RefCell::new(None),
        meter: Cell::new(0),
    });

    let weak = Rc::downgrade(&page);
    input_button.connect_toggled(move |button| {
        if let Some(page) = weak.upgrade() {
            page.direction.set(if button.is_active() {
                Direction::Input
            } else {
                Direction::Output
            });
            page.render();
            page.update_meter();
        }
    });
    let weak = Rc::downgrade(&page);
    devices.connect_row_activated(move |_, row| {
        let Some(page) = weak.upgrade() else {
            return;
        };
        let Ok(id) = row.widget_name().parse::<u32>() else {
            return;
        };
        glib::spawn_future_local(async move {
            run::set_default(id).await;
            page.refresh().await;
        });
    });
    for (scale, control) in [
        (&output_scale, Control::Output),
        (&balance_scale, Control::Balance),
        (&input_scale, Control::Input),
    ] {
        let weak = Rc::downgrade(&page);
        scale.connect_value_changed(move |scale| {
            if let Some(page) = weak.upgrade()
                && !page.updating.get()
            {
                let mut value = scale.value();
                // 균형은 가운데 가까이 오면 가운데에 붙는다
                if control == Control::Balance && value.abs() < BALANCE_SNAP {
                    value = 0.0;
                }
                page.change(control, value);
            }
        });
    }
    let weak = Rc::downgrade(&page);
    mute.connect_toggled(move |mute| {
        let Some(page) = weak.upgrade() else {
            return;
        };
        if page.updating.get() {
            return;
        }
        let muted = mute.is_active();
        page.touched.set(Some(Instant::now()));
        glib::spawn_future_local(async move {
            run::set_mute(DEFAULT_SINK, muted).await;
            page.refresh().await;
        });
    });

    let (start, stop) = (Rc::downgrade(&page), Rc::downgrade(&page));
    widgets::while_mapped(
        &scroller,
        move || {
            if let Some(page) = start.upgrade() {
                page.start();
            }
        },
        move || {
            if let Some(page) = stop.upgrade() {
                page.stop();
            }
        },
    );
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

/// 0~1 음량 막대 (사람이 보는 세제곱 눈금, wpctl과 같다)
fn volume_scale() -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale
}

/// 양 끝에 작은·큰 기호가 붙은 막대 (macOS의 음량 막대). 오른쪽에 체크 상자(음소거)를 둘 수 있다
fn icon_slider(
    scale: &gtk::Scale,
    low: &str,
    high: &str,
    title: &str,
    after: Option<&gtk::CheckButton>,
) -> gtk::Box {
    scale.update_property(&[gtk::accessible::Property::Label(title)]);
    let control = gtk::Box::builder()
        .spacing(6)
        .width_request(SLIDER_WIDTH)
        .hexpand(false)
        .css_classes(["icon-slider"])
        .build();
    control.append(&widgets::symbol(low));
    control.append(scale);
    control.append(&widgets::symbol(high));
    if let Some(after) = after {
        after.set_margin_start(6);
        control.append(after);
    }
    control
}

/// 경고음과 경고음 음량. 셸 설정이 없으면(셸이 없는 이미지) 묶음을 끈다
fn sound_effects(context: &Rc<Context>, column: &gtk::Box) {
    let tr = &context.tr;
    let group = widgets::group(column, Some(&tr.get("sound-effects")), None);
    let settings = shell_settings()
        .filter(|settings| has_key(settings, "alert-sound") && has_key(settings, "alert-volume"));

    let theme_name = sound_theme();
    let mut values: Vec<String> = theme::ALERT_SOUNDS
        .iter()
        .filter(|name| has_own_file(name, &theme_name))
        .map(|name| (*name).to_owned())
        .collect();
    let current = settings
        .as_ref()
        .map(|settings| settings.string("alert-sound").to_string());
    // 목록에 없는 값(gsettings로 정한 것)도 보이게 더한다
    if let Some(current) = current.as_ref().filter(|value| !values.contains(value)) {
        values.push(current.clone());
    }
    let labels: Vec<String> = values.iter().map(|value| alert_label(tr, value)).collect();
    let dropdown =
        gtk::DropDown::from_strings(&labels.iter().map(String::as_str).collect::<Vec<_>>());
    dropdown.set_valign(gtk::Align::Center);
    group.append(&widgets::row(&tr.get("sound-alert"), &dropdown));

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.01);
    volume.set_draw_value(false);
    volume.set_hexpand(true);
    group.append(&widgets::row(
        &tr.get("sound-alert-volume"),
        &icon_slider(
            &volume,
            "audio-volume-low-symbolic",
            "audio-volume-high-symbolic",
            &tr.get("sound-alert-volume"),
            None,
        ),
    ));

    let Some(settings) = settings else {
        dropdown.set_sensitive(false);
        volume.set_sensitive(false);
        return;
    };
    let values = Rc::new(values);
    let select = {
        let (dropdown, values) = (dropdown.clone(), values.clone());
        move |value: &str| {
            if let Some(index) = values.iter().position(|choice| choice == value) {
                dropdown.set_selected(index as u32);
            }
        }
    };
    select(&settings.string("alert-sound"));
    // 다른 곳(gsettings)에서 바꿔도 따라간다. 같은 값이면 notify가 오지 않는다
    settings.connect_changed(Some("alert-sound"), move |settings, key| {
        select(&settings.string(key));
    });
    let chooser = settings.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        let Some(value) = values.get(dropdown.selected() as usize) else {
            return;
        };
        if chooser.string("alert-sound") != value.as_str() {
            let _ = chooser.set_string("alert-sound", value);
            preview(&chooser);
        }
    });

    bind_scale(&settings, "alert-volume", &volume);
    // 사람이 막대를 움직이면(change-value) 놓은 뒤 한 번 들려준다
    let last_change: Rc<Cell<Option<Instant>>> = Rc::default();
    volume.connect_change_value(move |_, _, _| {
        let waiting = last_change.replace(Some(Instant::now())).is_some();
        if !waiting {
            let (last_change, settings) = (last_change.clone(), settings.clone());
            glib::spawn_future_local(async move {
                while let Some(changed) = last_change.get() {
                    let quiet = changed.elapsed();
                    if quiet >= PREVIEW_DELAY {
                        break;
                    }
                    glib::timeout_future(PREVIEW_DELAY - quiet).await;
                }
                last_change.set(None);
                preview(&settings);
            });
        }
        glib::Propagation::Proceed
    });
}

/// 고른 사운드 테마 (없으면 freedesktop)
fn sound_theme() -> String {
    settings_for(SOUND_SCHEMA).map_or_else(
        || theme::FALLBACK_THEME.to_owned(),
        |settings| settings.string("theme-name").to_string(),
    )
}

/// 이름 그대로의 파일이 있는 소리인지. 없는 이름은 테마가 비슷한 소리(message-new-instant → message)로 대신하므로
/// 목록에 두면 같은 소리가 두 번 보인다
fn has_own_file(name: &str, theme_name: &str) -> bool {
    theme::find(name, theme_name, &theme::data_dirs())
        .is_some_and(|file| file.file_stem().and_then(|stem| stem.to_str()) == Some(name))
}

/// 경고음 이름의 보이는 글. 번역이 없는 이름은 그대로 보인다
fn alert_label(tr: &bunt_i18n::Translations, name: &str) -> String {
    if name.is_empty() {
        return tr.get("sound-alert-none");
    }
    if theme::ALERT_SOUNDS.contains(&name) {
        tr.get(&format!("sound-alert-{name}"))
    } else {
        name.to_owned()
    }
}

/// 고른 경고음을 경고음 음량으로 한 번 들려준다 (셸의 알림 소리와 같은 계산)
fn preview(settings: &gio::Settings) {
    let name = settings.string("alert-sound");
    let level = settings.double("alert-volume").clamp(0.0, 1.0);
    let Some(file) = theme::find(&name, &sound_theme(), &theme::data_dirs()) else {
        return;
    };
    if let Err(err) = run::play(&file, level.powi(3)) {
        eprintln!("bunt-settings: could not play {}: {err}", file.display());
    }
}

/// 장치 줄의 연결 방식 글 (macOS 장치 목록의 "유형")
fn kind_label_id(kind: Kind) -> &'static str {
    match kind {
        Kind::BuiltIn => "sound-kind-built-in",
        Kind::Hdmi => "sound-kind-hdmi",
        Kind::Usb => "sound-kind-usb",
        Kind::Bluetooth => "sound-kind-bluetooth",
    }
}

impl SoundPage {
    fn start(self: &Rc<Self>) {
        self.visible.set(true);
        let page = self.clone();
        glib::spawn_future_local(async move { page.refresh().await });
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local(POLL_INTERVAL, move || {
            let Some(page) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            glib::spawn_future_local(async move { page.refresh().await });
            glib::ControlFlow::Continue
        });
        if let Some(old) = self.poll.replace(Some(source)) {
            old.remove();
        }
        self.update_meter();
    }

    fn stop(&self) {
        self.visible.set(false);
        if let Some(source) = self.poll.take() {
            source.remove();
        }
        // 녹음을 멈춘다
        self.meter.set(self.meter.get().wrapping_add(1));
        self.level.set_value(0.0);
    }

    /// 장치 목록과 기본 장치의 음량을 다시 읽는다
    async fn refresh(self: &Rc<Self>) {
        let (sinks, sources) = run::devices().await;
        let unknown = sinks
            .iter()
            .chain(&sources)
            .any(|node| !self.kinds.borrow().contains_key(&node.id));
        if unknown {
            let found = run::kinds().await;
            let mut kinds = self.kinds.borrow_mut();
            kinds.extend(found);
            // pw-dump가 모르는 장치는 내장으로 두고 다시 묻지 않는다
            for node in sinks.iter().chain(&sources) {
                kinds.entry(node.id).or_insert(Kind::BuiltIn);
            }
        }
        let volume_of = |nodes: &[Node]| {
            nodes
                .iter()
                .find(|node| node.default)
                .map(|node| node.id.to_string())
        };
        let output = match volume_of(&sinks) {
            Some(id) => run::volume(&id).await,
            None => None,
        };
        let input = match volume_of(&sources) {
            Some(id) => run::volume(&id).await,
            None => None,
        };
        self.sinks.replace(sinks);
        self.sources.replace(sources);
        self.output.replace(output);
        self.input.replace(input);
        self.render();
    }

    /// 읽은 상태를 화면에 옮긴다
    fn render(&self) {
        let direction = self.direction.get();
        widgets::set_group_visible(&self.output_group, direction == Direction::Output);
        widgets::set_group_visible(&self.input_group, direction == Direction::Input);
        self.render_devices();

        // 사람이 막 움직인 막대는 그대로 둔다
        let held = self
            .touched
            .get()
            .is_some_and(|touched| touched.elapsed() < HOLD_AFTER_CHANGE)
            || [Control::Output, Control::Balance, Control::Input]
                .into_iter()
                .any(|control| self.pending(control).running.get());
        let output = self.output.borrow().clone();
        let input = self.input.borrow().clone();
        self.output_scale.set_sensitive(output.is_some());
        self.mute.set_sensitive(output.is_some());
        self.input_scale.set_sensitive(input.is_some());
        let balance = output.as_ref().and_then(Volume::balance);
        self.balance_row.set_visible(balance.is_some());
        if held {
            return;
        }
        self.updating.set(true);
        if let Some(output) = &output {
            self.output_scale.set_value(output.level());
            self.mute.set_active(output.muted);
        }
        if let Some(balance) = balance {
            self.balance_scale.set_value(balance);
        }
        if let Some(input) = &input {
            self.input_scale.set_value(input.level());
        }
        self.updating.set(false);
    }

    /// 보이는 쪽의 장치 줄. 기본 장치에 체크 표시를 둔다. 바뀐 것이 없으면 그대로 둔다
    fn render_devices(&self) {
        let tr = &self.context.tr;
        let direction = self.direction.get();
        let nodes = match direction {
            Direction::Output => self.sinks.borrow().clone(),
            Direction::Input => self.sources.borrow().clone(),
        };
        let kinds = self.kinds.borrow();
        let shown: Vec<_> = nodes
            .iter()
            .map(|node| (node.id, &node.name, node.default, kinds.get(&node.id)))
            .collect();
        let summary = format!("{direction:?} {shown:?}");
        if *self.rendered.borrow() == summary {
            return;
        }
        self.rendered.replace(summary);
        for row in self.device_rows.take() {
            self.devices.remove(&row);
        }
        let mut rows = Vec::new();
        if nodes.is_empty() {
            let (row, _) = widgets::rich_row(
                None,
                &tr.get(match direction {
                    Direction::Output => "sound-no-output",
                    Direction::Input => "sound-no-input",
                }),
                None,
            );
            rows.push(row);
        }
        for node in &nodes {
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.add_css_class("row-check");
            check.set_opacity(if node.default { 1.0 } else { 0.0 });
            let kind = kinds.get(&node.id).map(|kind| tr.get(kind_label_id(*kind)));
            let (row, _) = widgets::rich_row(Some(check.upcast_ref()), &node.name, kind.as_deref());
            row.set_activatable(true);
            row.set_widget_name(&node.id.to_string());
            rows.push(row);
        }
        for row in &rows {
            self.devices.append(row);
        }
        self.device_rows.replace(rows);
    }

    fn pending(&self, control: Control) -> &Pending {
        match control {
            Control::Output => &self.output_pending,
            Control::Balance => &self.balance_pending,
            Control::Input => &self.input_pending,
        }
    }

    /// 막대를 움직였다. 값을 모아 하나씩 적용하고, 다 적용하면 다시 읽는다
    fn change(self: &Rc<Self>, control: Control, value: f64) {
        self.touched.set(Some(Instant::now()));
        let pending = self.pending(control);
        pending.value.set(Some(value));
        if pending.running.replace(true) {
            return;
        }
        let page = self.clone();
        glib::spawn_future_local(async move {
            loop {
                glib::timeout_future(SET_DELAY).await;
                let Some(value) = page.pending(control).value.take() else {
                    break;
                };
                page.apply(control, value).await;
            }
            page.pending(control).running.set(false);
            page.refresh().await;
        });
    }

    async fn apply(&self, control: Control, value: f64) {
        match control {
            Control::Output => {
                let Some(volume) = self.output.borrow().clone() else {
                    return;
                };
                // macOS처럼 음량을 올리면 음소거가 풀린다
                if volume.muted && value > 0.0 {
                    run::set_mute(DEFAULT_SINK, false).await;
                }
                run::set_level(DEFAULT_SINK, &volume, value).await;
            }
            Control::Balance => {
                let Some(volume) = self.output.borrow().clone() else {
                    return;
                };
                run::set_balance(&volume, value).await;
            }
            Control::Input => {
                let Some(volume) = self.input.borrow().clone() else {
                    return;
                };
                if volume.muted && value > 0.0 {
                    run::set_mute(DEFAULT_SOURCE, false).await;
                }
                run::set_level(DEFAULT_SOURCE, &volume, value).await;
            }
        }
    }

    /// 입력 쪽이 보이는 동안만 기본 입력 장치를 녹음해 레벨을 잰다
    fn update_meter(self: &Rc<Self>) {
        let generation = self.meter.get().wrapping_add(1);
        self.meter.set(generation);
        self.level.set_value(0.0);
        if !self.visible.get() || self.direction.get() != Direction::Input {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Some(meter) = run::Meter::start() else {
                return;
            };
            while let Some(level) = meter.next().await {
                let Some(page) = weak.upgrade() else {
                    break;
                };
                if page.meter.get() != generation {
                    break;
                }
                page.level.set_value((level * LEVEL_BLOCKS).round());
            }
            // meter를 버리면 녹음이 끝난다
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_sounds_and_kinds_have_labels() {
        for (language, source) in crate::TRANSLATIONS {
            let tr = bunt_i18n::Translations::from_source(language, source);
            for name in theme::ALERT_SOUNDS {
                let id = format!("sound-alert-{name}");
                assert_ne!(alert_label(&tr, name), id, "{language}: {id}");
            }
            assert_ne!(alert_label(&tr, ""), "sound-alert-none");
            // 목록에 없는 이름은 그대로 보인다
            assert_eq!(alert_label(&tr, "custom-sound"), "custom-sound");
            for kind in [Kind::BuiltIn, Kind::Hdmi, Kind::Usb, Kind::Bluetooth] {
                assert_ne!(tr.get(kind_label_id(kind)), kind_label_id(kind));
            }
        }
    }
}
