// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 디스플레이: 컴포지터 API(org.buntos.Compositor1)로 출력마다 배율, 해상도, 주사율을 바꾸고,
//! 내장 화면이면 밝기(/sys/class/backlight를 읽고 logind로 쓴다)를 바꾼다

use std::{cell::RefCell, collections::BTreeSet, rc::Rc, time::Duration};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio, glib, prelude::*};

use super::Context;
use crate::{
    dbus::{self, Debounce, Props, Remote},
    widgets::{self, BadgeSize, Tint, badge},
};

const COMPOSITOR: &str = "org.buntos.Compositor";
const COMPOSITOR_PATH: &str = "/org/buntos/Compositor";
const COMPOSITOR_INTERFACE: &str = "org.buntos.Compositor1";
const LOGIN: &str = "org.freedesktop.login1";
const SESSION_PATH: &str = "/org/freedesktop/login1/session/auto";
const SESSION: &str = "org.freedesktop.login1.Session";
const BACKLIGHT_DIR: &str = "/sys/class/backlight";
const ICON: &str = "video-display-symbolic";
const REFRESH_DELAY: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, PartialEq)]
struct Output {
    name: String,
    description: String,
    builtin: bool,
    width: i32,
    height: i32,
    /// mHz
    refresh: i32,
    scale: f64,
    /// (너비, 높이, mHz), 선호 모드가 먼저
    modes: Vec<(i32, i32, i32)>,
    scales: Vec<f64>,
}

impl Output {
    fn from_props(props: &Props) -> Self {
        let modes = props
            .value("modes")
            .map(|modes| {
                modes
                    .iter()
                    .filter_map(|mode| {
                        Some((
                            mode.child_value(0).get::<i32>()?,
                            mode.child_value(1).get::<i32>()?,
                            mode.child_value(2).get::<i32>()?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            name: props.string("name"),
            description: props.string("description"),
            builtin: props.bool("builtin"),
            width: props.get("width").unwrap_or(0),
            height: props.get("height").unwrap_or(0),
            refresh: props.get("refresh").unwrap_or(0),
            scale: props.get("scale").unwrap_or(1.0),
            modes,
            scales: props.get::<Vec<f64>>("scales").unwrap_or_default(),
        }
    }

    /// 해상도 목록 (넓은 것부터, 같은 크기는 하나만)
    fn resolutions(&self) -> Vec<(i32, i32)> {
        let mut sizes: Vec<(i32, i32)> = self
            .modes
            .iter()
            .map(|(width, height, _)| (*width, *height))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        sizes.sort_by_key(|(width, height)| std::cmp::Reverse(width * height));
        sizes
    }

    /// 지금 해상도의 주사율들 (높은 것부터)
    fn refresh_rates(&self, width: i32, height: i32) -> Vec<i32> {
        let mut rates: Vec<i32> = self
            .modes
            .iter()
            .filter(|(w, h, _)| *w == width && *h == height)
            .map(|(_, _, refresh)| *refresh)
            .collect();
        rates.sort_unstable_by(|a, b| b.cmp(a));
        rates.dedup();
        rates
    }
}

fn hz(refresh: i32) -> i32 {
    (refresh + 500) / 1000
}

/// 내장 화면 밝기 장치 (이름, 지금 값, 최댓값)
fn backlight() -> Option<(String, u32, u32)> {
    let entry = std::fs::read_dir(BACKLIGHT_DIR).ok()?.flatten().next()?;
    let read = |file: &str| -> Option<u32> {
        std::fs::read_to_string(entry.path().join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    };
    Some((
        entry.file_name().to_string_lossy().into_owned(),
        read("brightness")?,
        read("max_brightness")?,
    ))
}

struct DisplayPage {
    context: Rc<Context>,
    content: gtk::Box,
    compositor: RefCell<Option<Remote>>,
    outputs: RefCell<Vec<Output>>,
    debounce: Debounce,
    subscription: RefCell<Option<gio::SignalSubscription>>,
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let (scroller, column) = widgets::page();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(20)
        .build();
    column.append(&content);
    let page = Rc::new(DisplayPage {
        context: context.clone(),
        content,
        compositor: RefCell::new(None),
        outputs: RefCell::default(),
        debounce: Debounce::default(),
        subscription: RefCell::new(None),
    });
    // 컴포지터가 나중에 API를 열 수도 있으니 보일 때마다 다시 읽는다
    let weak = Rc::downgrade(&page);
    widgets::while_mapped(
        &scroller,
        move || {
            if let Some(page) = weak.upgrade() {
                page.schedule_refresh();
            }
        },
        || {},
    );
    glib::spawn_future_local(page.clone().start());
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

impl DisplayPage {
    async fn start(self: Rc<Self>) {
        let Ok(bus) = dbus::session().await else {
            self.render_unavailable();
            return;
        };
        let weak = Rc::downgrade(&self);
        let subscription = dbus::subscribe(
            &bus,
            Some(COMPOSITOR),
            Some(COMPOSITOR_INTERFACE),
            Some("OutputsChanged"),
            None,
            move |_| {
                if let Some(page) = weak.upgrade() {
                    page.schedule_refresh();
                }
            },
        );
        self.subscription.replace(Some(subscription));
        self.compositor
            .replace(Some(Remote::new(&bus, COMPOSITOR, COMPOSITOR_PATH)));
        self.refresh().await;
    }

    fn schedule_refresh(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.debounce.run(REFRESH_DELAY, move || {
            if let Some(page) = weak.upgrade() {
                glib::spawn_future_local(async move { page.refresh().await });
            }
        });
    }

    async fn refresh(self: &Rc<Self>) {
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        match compositor
            .call(COMPOSITOR_INTERFACE, "GetOutputs", None)
            .await
        {
            Ok(reply) => {
                let outputs: Vec<Output> = reply
                    .child_value(0)
                    .iter()
                    .map(|dict| Output::from_props(&Props::new(dict)))
                    .collect();
                if *self.outputs.borrow() == outputs && self.content.first_child().is_some() {
                    return;
                }
                self.outputs.replace(outputs);
                self.render();
            }
            Err(_) => self.render_unavailable(),
        }
    }

    fn clear(&self) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
    }

    fn render_unavailable(&self) {
        let tr = &self.context.tr;
        self.clear();
        self.outputs.borrow_mut().clear();
        self.content.append(&widgets::placeholder(
            ICON,
            Tint::Gray,
            &tr.get("display-unavailable-title"),
            &tr.get("display-unavailable-description"),
        ));
        self.brightness_group();
    }

    fn render(self: &Rc<Self>) {
        let tr = &self.context.tr;
        self.clear();
        let outputs = self.outputs.borrow().clone();
        if outputs.is_empty() {
            self.render_unavailable();
            return;
        }
        for output in &outputs {
            self.output_groups(tr, output);
        }
        self.brightness_group();
    }

    fn output_groups(self: &Rc<Self>, tr: &Translations, output: &Output) {
        let title = if output.description.is_empty() {
            output.name.clone()
        } else {
            output.description.clone()
        };
        let mut args = FluentArgs::new();
        args.set("width", output.width);
        args.set("height", output.height);
        args.set("refresh", hz(output.refresh));
        let subtitle = tr.get_with("display-current-mode", &args);
        let kind = if output.builtin {
            tr.get("display-builtin")
        } else {
            tr.get("display-external")
        };
        let list = widgets::group(&self.content, None, None);
        list.append(&widgets::hero_row(
            &badge(ICON, Tint::Blue, BadgeSize::Medium),
            &title,
            &format!("{kind} · {subtitle}"),
            None,
        ));

        if output.scales.len() > 1 {
            list.append(&self.scale_row(tr, output));
        }

        // 해상도
        let resolutions = output.resolutions();
        if !resolutions.is_empty() {
            let preferred = output.modes.first().map(|(w, h, _)| (*w, *h));
            let labels: Vec<String> = resolutions
                .iter()
                .map(|(width, height)| {
                    let mut args = FluentArgs::new();
                    args.set("width", *width);
                    args.set("height", *height);
                    let id = if Some((*width, *height)) == preferred {
                        "display-resolution-preferred"
                    } else {
                        "display-resolution"
                    };
                    tr.get_with(id, &args)
                })
                .collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let dropdown = gtk::DropDown::from_strings(&refs);
            if let Some(index) = resolutions
                .iter()
                .position(|size| *size == (output.width, output.height))
            {
                dropdown.set_selected(index as u32);
            }
            let page = self.clone();
            let name = output.name.clone();
            let output_copy = output.clone();
            dropdown.connect_selected_notify(move |dropdown| {
                let Some((width, height)) = resolutions.get(dropdown.selected() as usize) else {
                    return;
                };
                // 같은 해상도의 가장 높은 주사율로 바꾼다
                let refresh = output_copy
                    .refresh_rates(*width, *height)
                    .first()
                    .copied()
                    .unwrap_or(0);
                page.set_mode(&name, *width, *height, refresh);
            });
            list.append(&widgets::row(
                &tr.get("display-resolution-title"),
                &dropdown,
            ));
        }

        // 주사율
        let rates = output.refresh_rates(output.width, output.height);
        if rates.len() > 1 {
            let labels: Vec<String> = rates
                .iter()
                .map(|rate| {
                    let mut args = FluentArgs::new();
                    args.set("refresh", hz(*rate));
                    tr.get_with("display-refresh-value", &args)
                })
                .collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let dropdown = gtk::DropDown::from_strings(&refs);
            if let Some(index) = rates.iter().position(|rate| *rate == output.refresh) {
                dropdown.set_selected(index as u32);
            }
            let page = self.clone();
            let (name, width, height) = (output.name.clone(), output.width, output.height);
            dropdown.connect_selected_notify(move |dropdown| {
                if let Some(rate) = rates.get(dropdown.selected() as usize) {
                    page.set_mode(&name, width, height, *rate);
                }
            });
            list.append(&widgets::row(&tr.get("display-refresh"), &dropdown));
        } else if output.refresh > 0 {
            let mut args = FluentArgs::new();
            args.set("refresh", hz(output.refresh));
            list.append(&widgets::info_row(
                &tr.get("display-refresh"),
                &tr.get_with("display-refresh-value", &args),
            ));
        }
    }

    /// macOS의 "텍스트 더 크게 ↔ 공간 더 많이"처럼 배율을 고르는 행. 큰 배율이 왼쪽이다
    fn scale_row(self: &Rc<Self>, tr: &Translations, output: &Output) -> gtk::ListBoxRow {
        let mut scales = output.scales.clone();
        scales.sort_by(|a, b| b.total_cmp(a));
        let choices = gtk::Box::builder()
            .spacing(10)
            .halign(gtk::Align::Center)
            .build();
        let mut first: Option<gtk::ToggleButton> = None;
        let count = scales.len();
        for (index, scale) in scales.iter().enumerate() {
            // 배율이 클수록 미리보기 글자가 크다
            let sample = gtk::Label::builder()
                .label("Aa")
                .css_classes([format!("scale-sample-{}", (count - 1 - index).min(4)).as_str()])
                .build();
            let screen = gtk::Box::builder()
                .width_request(64)
                .height_request(42)
                .hexpand(false)
                .css_classes(["scale-screen"])
                .build();
            sample.set_hexpand(true);
            screen.append(&sample);
            let button = gtk::ToggleButton::builder()
                .child(&screen)
                .active((scale - output.scale).abs() < 0.01)
                .tooltip_text(format!("{:.0}%", scale * 100.0))
                .css_classes(["scale-choice"])
                .build();
            match &first {
                Some(first) => button.set_group(Some(first)),
                None => first = Some(button.clone()),
            }
            let page = self.clone();
            let (name, scale) = (output.name.clone(), *scale);
            button.connect_toggled(move |button| {
                if button.is_active() {
                    page.set_scale(&name, scale);
                }
            });
            let choice = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(4)
                .build();
            choice.append(&button);
            choice.append(
                &gtk::Label::builder()
                    .label(format!("{:.0}%", scale * 100.0))
                    .css_classes(["choice-label"])
                    .build(),
            );
            choices.append(&choice);
        }
        let ends = gtk::Box::builder().build();
        ends.append(
            &gtk::Label::builder()
                .label(tr.get("display-larger-text"))
                .xalign(0.0)
                .hexpand(true)
                .css_classes(["row-subtitle"])
                .build(),
        );
        ends.append(
            &gtk::Label::builder()
                .label(tr.get("display-more-space"))
                .xalign(1.0)
                .css_classes(["row-subtitle"])
                .build(),
        );
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        column.append(
            &gtk::Label::builder()
                .label(tr.get("display-scale"))
                .xalign(0.0)
                .css_classes(["row-title"])
                .build(),
        );
        column.append(&choices);
        column.append(&ends);
        gtk::ListBoxRow::builder()
            .child(&column)
            .activatable(false)
            .build()
    }

    /// 내장 화면 밝기. 밝기 장치가 없으면(가상 머신, 데스크톱) 보이지 않는다
    fn brightness_group(&self) {
        let Some((device, current, max)) = backlight() else {
            return;
        };
        let tr = &self.context.tr;
        let list = widgets::group(&self.content, Some(&tr.get("display-brightness")), None);
        let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, f64::from(max), 1.0);
        scale.set_value(f64::from(current));
        scale.set_draw_value(false);
        scale.set_hexpand(true);
        scale.set_width_request(240);
        let context = self.context.clone();
        let debounce = Debounce::default();
        scale.connect_value_changed(move |scale| {
            let weak = scale.downgrade();
            let (device, context) = (device.clone(), context.clone());
            // 끄는 동안 여러 번 불리므로 잠시 뒤 그때의 값을 쓴다
            debounce.run(Duration::from_millis(60), move || {
                let Some(scale) = weak.upgrade() else {
                    return;
                };
                let value = scale.value().round() as u32;
                glib::spawn_future_local(async move {
                    let Ok(bus) = dbus::system().await else {
                        return;
                    };
                    let session = Remote::new(&bus, LOGIN, SESSION_PATH);
                    let args = ("backlight", device.as_str(), value).to_variant();
                    if let Err(err) = session.call(SESSION, "SetBrightness", Some(args)).await {
                        context.notify_error("display-error", dbus::message(&err));
                    }
                });
            });
        });
        let low = widgets::symbol("display-brightness-symbolic");
        let (row, accessories) = widgets::rich_row(Some(low.upcast_ref()), "", None);
        accessories.append(&scale);
        list.append(&row);
    }

    fn set_mode(self: &Rc<Self>, name: &str, width: i32, height: i32, refresh: i32) {
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        let page = self.clone();
        let args = (name, width, height, refresh).to_variant();
        glib::spawn_future_local(async move {
            if let Err(err) = compositor
                .call(COMPOSITOR_INTERFACE, "SetOutputMode", Some(args))
                .await
            {
                page.context
                    .notify_error("display-error", dbus::message(&err));
                page.schedule_refresh();
            }
        });
    }

    fn set_scale(self: &Rc<Self>, name: &str, scale: f64) {
        let Some(compositor) = self.compositor.borrow().clone() else {
            return;
        };
        let current = self
            .outputs
            .borrow()
            .iter()
            .find(|output| output.name == name)
            .map(|output| output.scale);
        if current.is_some_and(|current| (current - scale).abs() < 0.01) {
            return;
        }
        let page = self.clone();
        let args = (name, scale).to_variant();
        glib::spawn_future_local(async move {
            if let Err(err) = compositor
                .call(COMPOSITOR_INTERFACE, "SetOutputScale", Some(args))
                .await
            {
                page.context
                    .notify_error("display-error", dbus::message(&err));
                page.schedule_refresh();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output() -> Output {
        Output {
            name: "Virtual-1".into(),
            description: String::new(),
            builtin: false,
            width: 1280,
            height: 800,
            refresh: 60000,
            scale: 1.0,
            modes: vec![
                (1280, 800, 60000),
                (1920, 1080, 60000),
                (1920, 1080, 74973),
                (1024, 768, 60000),
            ],
            scales: vec![1.0, 2.0],
        }
    }

    #[test]
    fn resolutions_are_unique_and_largest_first() {
        assert_eq!(
            output().resolutions(),
            vec![(1920, 1080), (1280, 800), (1024, 768)]
        );
    }

    #[test]
    fn refresh_rates_for_one_resolution() {
        assert_eq!(output().refresh_rates(1920, 1080), vec![74973, 60000]);
        assert_eq!(hz(74973), 75);
        assert_eq!(hz(59940), 60);
    }
}
