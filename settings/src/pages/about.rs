// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 정보: 기기 이름, 운영체제, 하드웨어

use std::rc::Rc;

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, glib, prelude::*};

use super::Context;
use crate::{
    system::{self, HOSTNAME},
    widgets::{group, info_row, page, row},
};

pub fn build(context: &Rc<Context>) -> gtk::ScrolledWindow {
    let tr = &context.tr;
    let (scroller, column) = page(&tr.get("page-about"));

    let device = group(&column, Some(&tr.get("about-device-group")), None);
    device.append(&row(&tr.get("about-device-name"), &device_name(context)));
    if let Some(model) = system::device_model() {
        device.append(&info_row(&tr.get("about-model"), &model));
    }

    let software = group(&column, Some(&tr.get("about-software-group")), None);
    software.append(&info_row(&tr.get("about-os"), &os_description(tr)));
    software.append(&info_row(
        &tr.get("about-kernel"),
        &system::kernel_release().unwrap_or_else(|| tr.get("unknown")),
    ));
    software.append(&info_row(
        &tr.get("about-architecture"),
        std::env::consts::ARCH,
    ));

    let hardware = group(&column, Some(&tr.get("about-hardware-group")), None);
    hardware.append(&info_row(
        &tr.get("about-processor"),
        &processor_description(tr),
    ));
    let memory = system::memory_total().map_or_else(
        || tr.get("unknown"),
        |bytes| glib::format_size_full(bytes, glib::FormatSizeFlags::IEC_UNITS).into(),
    );
    hardware.append(&info_row(&tr.get("about-memory"), &memory));
    hardware.append(&info_row(&tr.get("about-disk"), &disk_description(tr)));
    if let Some(display) = display_description(tr) {
        hardware.append(&info_row(&tr.get("about-display"), &display));
    }
    scroller
}

/// 기기 이름(호스트 이름) 입력란과 변경 버튼. 이름은 hostnamed로 바꾼다
fn device_name(context: &Rc<Context>) -> gtk::Box {
    let entry = gtk::Entry::builder()
        .text(glib::host_name())
        .width_chars(20)
        .build();
    let apply = gtk::Button::builder()
        .label(context.tr.get("about-device-name-apply"))
        .sensitive(false)
        .build();
    // 지금 이름과 다를 때만 바꿀 수 있다
    entry.connect_changed(glib::clone!(
        #[weak]
        apply,
        move |entry| apply
            .set_sensitive(!entry.text().is_empty() && entry.text() != glib::host_name())
    ));

    let context = context.clone();
    let submit = move |entry: &gtk::Entry, apply: &gtk::Button| {
        if !apply.is_sensitive() {
            return;
        }
        apply.set_sensitive(false);
        let name = entry.text().to_string();
        let context = context.clone();
        glib::spawn_future_local(async move {
            let args = (name.as_str(), false).to_variant();
            match HOSTNAME.call("SetStaticHostname", &args).await {
                Ok(_) => {
                    let mut args = FluentArgs::new();
                    args.set("name", name);
                    context.notify(&context.tr.get_with("about-device-name-saved", &args));
                }
                Err(err) => context.notify_error("about-device-name-error", err.message()),
            }
        });
    };
    let submit = Rc::new(submit);
    entry.connect_activate(glib::clone!(
        #[weak]
        apply,
        #[strong]
        submit,
        move |entry| submit(entry, &apply)
    ));
    apply.connect_clicked(glib::clone!(
        #[weak]
        entry,
        move |apply| submit(&entry, apply)
    ));

    let controls = gtk::Box::builder().spacing(6).build();
    controls.append(&entry);
    controls.append(&apply);
    controls
}

fn os_description(tr: &Translations) -> String {
    let release = system::os_release();
    let base = release.pretty_name.unwrap_or_else(|| tr.get("unknown"));
    if release.image_id.as_deref() != Some("buntos") {
        return base;
    }
    let mut args = FluentArgs::new();
    args.set("base", base);
    tr.get_with("about-os-based-on", &args)
}

fn processor_description(tr: &Translations) -> String {
    let mut args = FluentArgs::new();
    args.set("count", system::cpu_count());
    match system::cpu_model() {
        Some(model) => {
            args.set("model", model);
            tr.get_with("about-processor-model", &args)
        }
        None => tr.get_with("about-processor-cores", &args),
    }
}

fn disk_description(tr: &Translations) -> String {
    let Some((total, free)) = system::disk_space() else {
        return tr.get("unknown");
    };
    let mut args = FluentArgs::new();
    args.set("total", glib::format_size(total).to_string());
    args.set("free", glib::format_size(free).to_string());
    tr.get_with("about-disk-free", &args)
}

/// 첫 번째 화면의 실제 픽셀 크기와 주사율
fn display_description(tr: &Translations) -> Option<String> {
    let display = gtk::gdk::Display::default()?;
    let monitor = display
        .monitors()
        .item(0)?
        .downcast::<gtk::gdk::Monitor>()
        .ok()?;
    let geometry = monitor.geometry();
    let scale = monitor.scale_factor();
    let mut args = FluentArgs::new();
    args.set("width", geometry.width() * scale);
    args.set("height", geometry.height() * scale);
    // 주사율은 mHz 단위다
    args.set("refresh", (monitor.refresh_rate() + 500) / 1000);
    Some(tr.get_with("about-display-mode", &args))
}
