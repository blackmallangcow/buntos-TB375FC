// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 키보드: 키 반복 속도와 지연 시간(컴포지터 API), 입력 소스 목록·추가·지우기(fcitx5 D-Bus),
//! 입력 소스 전환 단축키 안내, 메뉴 막대의 입력 메뉴

use std::{cell::RefCell, collections::HashMap, rc::Rc, time::Duration};

use bunt_i18n::Translations;
use gtk4::{self as gtk, glib, prelude::*};

use super::{Context, bind_switch, shell_settings};
use crate::{
    dbus::{self, Debounce, Remote},
    widgets::{self, BadgeSize, Sheet, Tint, badge},
};

const COMPOSITOR: &str = "org.buntos.Compositor";
const COMPOSITOR_PATH: &str = "/org/buntos/Compositor";
const COMPOSITOR_INTERFACE: &str = "org.buntos.Compositor1";
const FCITX: &str = "org.fcitx.Fcitx5";
const FCITX_PATH: &str = "/controller";
const FCITX_INTERFACE: &str = "org.fcitx.Fcitx.Controller1";
const ICON: &str = "input-keyboard-symbolic";

/// 키 반복 속도 눈금 (초당 횟수). 맨 왼쪽은 반복 끔
const RATES: [u32; 9] = [0, 2, 5, 10, 15, 20, 25, 33, 40];
/// 반복 지연 시간 눈금 (밀리초). 왼쪽이 길다
const DELAYS: [u32; 8] = [1200, 900, 700, 550, 400, 300, 250, 200];
const APPLY_DELAY: Duration = Duration::from_millis(250);

/// fcitx5가 아는 입력기 하나
#[derive(Clone, Debug)]
struct InputMethod {
    unique_name: String,
    name: String,
    native_name: String,
    language: String,
}

impl InputMethod {
    fn label(&self) -> String {
        if self.native_name.is_empty() {
            self.name.clone()
        } else {
            self.native_name.clone()
        }
    }
}

/// 입력 소스 그룹: (그룹 이름, 기본 키보드 배치, [(입력기 이름, 배치)])
#[derive(Clone, Debug, Default)]
struct Group {
    name: String,
    layout: String,
    items: Vec<(String, String)>,
}

struct KeyboardPage {
    context: Rc<Context>,
    fcitx: RefCell<Option<Remote>>,
    compositor: RefCell<Option<Remote>>,
    repeat_list: gtk::ListBox,
    rate: gtk::Scale,
    delay: gtk::Scale,
    sources: gtk::ListBox,
    add_source: gtk::Button,
    available: RefCell<HashMap<String, InputMethod>>,
    group: RefCell<Group>,
    debounce: Debounce,
}

/// 가장 가까운 눈금
fn nearest(steps: &[u32], value: u32) -> usize {
    steps
        .iter()
        .enumerate()
        .min_by_key(|(_, step)| step.abs_diff(value))
        .map_or(0, |(index, _)| index)
}

fn step_scale(steps: usize) -> gtk::Scale {
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, (steps - 1) as f64, 1.0);
    scale.set_round_digits(0);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_width_request(220);
    for index in 0..steps {
        scale.add_mark(index as f64, gtk::PositionType::Bottom, None);
    }
    scale
}

fn language_name(tr: &Translations, code: &str) -> String {
    match code.split(['_', '-']).next().unwrap_or_default() {
        "ko" => tr.get("keyboard-language-korean"),
        "en" => tr.get("keyboard-language-english"),
        "ja" => tr.get("keyboard-language-japanese"),
        "zh" => tr.get("keyboard-language-chinese"),
        "" => String::new(),
        other => other.to_owned(),
    }
}

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = widgets::page();
    let hero = widgets::group(&column, None, None);
    hero.append(&widgets::hero_row(
        &badge(ICON, Tint::Gray, BadgeSize::Medium),
        &tr.get("page-keyboard"),
        &tr.get("keyboard-description"),
        None,
    ));

    let repeat_list = widgets::group(&column, Some(&tr.get("keyboard-repeat")), None);
    let rate = step_scale(RATES.len());
    let delay = step_scale(DELAYS.len());
    repeat_list.append(&widgets::slider_row(
        &tr.get("keyboard-repeat-rate"),
        &rate,
        &tr.get("keyboard-off"),
        &tr.get("keyboard-fast"),
    ));
    repeat_list.append(&widgets::slider_row(
        &tr.get("keyboard-repeat-delay"),
        &delay,
        &tr.get("keyboard-long"),
        &tr.get("keyboard-short"),
    ));
    let test = gtk::Entry::builder()
        .placeholder_text(tr.get("keyboard-test"))
        .hexpand(true)
        .build();
    let test_row = gtk::ListBoxRow::builder()
        .child(&test)
        .activatable(false)
        .build();
    repeat_list.append(&test_row);

    let sources = widgets::group(
        &column,
        Some(&tr.get("keyboard-sources")),
        Some(&tr.get("keyboard-sources-description")),
    );
    let footer = widgets::footer(&column);
    let add_source = gtk::Button::with_label(&tr.get("keyboard-add-source"));
    footer.append(&add_source);

    let shortcuts = widgets::group(&column, Some(&tr.get("keyboard-shortcuts")), None);
    shortcuts.append(&widgets::info_row(
        &tr.get("keyboard-switch-source"),
        &tr.get("keyboard-switch-keys"),
    ));
    if let Some(settings) = shell_settings() {
        let (row, switch) = widgets::switch_row(&tr.get("keyboard-show-input-menu"), None);
        bind_switch(&settings, "show-input-source", &switch);
        shortcuts.append(&row);
    }

    let page = Rc::new(KeyboardPage {
        context: context.clone(),
        fcitx: RefCell::new(None),
        compositor: RefCell::new(None),
        repeat_list,
        rate: rate.clone(),
        delay: delay.clone(),
        sources,
        add_source: add_source.clone(),
        available: RefCell::default(),
        group: RefCell::default(),
        debounce: Debounce::default(),
    });

    for scale in [&rate, &delay] {
        let weak = Rc::downgrade(&page);
        scale.connect_change_value(move |scale, _, value| {
            // 눈금에 맞춘다
            scale.set_value(value.round());
            if let Some(page) = weak.upgrade() {
                page.schedule_repeat();
            }
            glib::Propagation::Stop
        });
    }
    let weak = Rc::downgrade(&page);
    add_source.connect_clicked(move |_| {
        if let Some(page) = weak.upgrade() {
            page.choose_source();
        }
    });
    // fcitx5나 컴포지터가 나중에 뜰 수 있으니 보일 때마다 다시 읽는다
    let weak = Rc::downgrade(&page);
    widgets::while_mapped(
        &scroller,
        move || {
            if let Some(page) = weak.upgrade() {
                glib::spawn_future_local(async move { page.refresh().await });
            }
        },
        || {},
    );
    widgets::keep_alive(&scroller, page);
    scroller.upcast()
}

impl KeyboardPage {
    async fn refresh(self: &Rc<Self>) {
        let Ok(bus) = dbus::session().await else {
            return;
        };
        let compositor = Remote::new(&bus, COMPOSITOR, COMPOSITOR_PATH);
        let fcitx = Remote::new(&bus, FCITX, FCITX_PATH);
        self.compositor.replace(Some(compositor.clone()));
        self.fcitx.replace(Some(fcitx.clone()));
        self.refresh_repeat(&compositor).await;
        self.refresh_sources(&fcitx).await;
    }

    async fn refresh_repeat(&self, compositor: &Remote) {
        match compositor
            .call(COMPOSITOR_INTERFACE, "GetKeyboardRepeat", None)
            .await
        {
            Ok(reply) => {
                let delay = reply.child_value(0).get::<u32>().unwrap_or(500);
                let rate = reply.child_value(1).get::<u32>().unwrap_or(25);
                self.rate.set_value(nearest(&RATES, rate) as f64);
                self.delay.set_value(nearest(&DELAYS, delay) as f64);
                self.set_repeat_available(true);
            }
            Err(_) => self.set_repeat_available(false),
        }
    }

    fn set_repeat_available(&self, available: bool) {
        self.rate.set_sensitive(available);
        self.delay.set_sensitive(available);
        // 쓸 수 없을 때만 묶음 아래에 까닭을 적는다
        let section = self.repeat_list.parent().and_downcast::<gtk::Box>();
        let Some(section) = section else {
            return;
        };
        let note = section
            .last_child()
            .filter(|child| child.has_css_class("settings-description"));
        match (available, note) {
            (false, None) => section.append(&widgets::description_label(
                &self.context.tr.get("keyboard-repeat-unavailable"),
            )),
            (true, Some(note)) => section.remove(&note),
            _ => {}
        }
    }

    fn schedule_repeat(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.debounce.run(APPLY_DELAY, move || {
            let Some(page) = weak.upgrade() else {
                return;
            };
            let Some(compositor) = page.compositor.borrow().clone() else {
                return;
            };
            let rate = RATES[(page.rate.value().round() as usize).min(RATES.len() - 1)];
            let delay = DELAYS[(page.delay.value().round() as usize).min(DELAYS.len() - 1)];
            glib::spawn_future_local(async move {
                let args = (delay, rate).to_variant();
                if let Err(err) = compositor
                    .call(COMPOSITOR_INTERFACE, "SetKeyboardRepeat", Some(args))
                    .await
                {
                    page.context
                        .notify_error("keyboard-repeat-error", dbus::message(&err));
                }
            });
        });
    }

    async fn refresh_sources(self: &Rc<Self>, fcitx: &Remote) {
        let available = match fcitx
            .call(FCITX_INTERFACE, "AvailableInputMethods", None)
            .await
        {
            Ok(reply) => reply
                .child_value(0)
                .iter()
                .map(|entry| {
                    let text = |index| {
                        entry
                            .child_value(index)
                            .str()
                            .unwrap_or_default()
                            .to_owned()
                    };
                    let method = InputMethod {
                        unique_name: text(0),
                        name: text(1),
                        native_name: text(2),
                        language: text(5),
                    };
                    (method.unique_name.clone(), method)
                })
                .collect::<HashMap<_, _>>(),
            Err(_) => {
                self.render_sources_unavailable();
                return;
            }
        };
        let name = match fcitx
            .call(FCITX_INTERFACE, "CurrentInputMethodGroup", None)
            .await
        {
            Ok(reply) => reply.child_value(0).str().unwrap_or("Default").to_owned(),
            Err(_) => "Default".to_owned(),
        };
        let args = (name.as_str(),).to_variant();
        let group = match fcitx
            .call(FCITX_INTERFACE, "InputMethodGroupInfo", Some(args))
            .await
        {
            Ok(reply) => Group {
                name,
                layout: reply.child_value(0).str().unwrap_or_default().to_owned(),
                items: reply
                    .child_value(1)
                    .iter()
                    .filter_map(|item| {
                        Some((
                            item.child_value(0).str()?.to_owned(),
                            item.child_value(1).str().unwrap_or_default().to_owned(),
                        ))
                    })
                    .collect(),
            },
            Err(_) => {
                self.render_sources_unavailable();
                return;
            }
        };
        self.available.replace(available);
        self.group.replace(group);
        self.render_sources();
    }

    fn render_sources_unavailable(&self) {
        widgets::clear(&self.sources);
        let (row, _) = widgets::rich_row(
            None,
            &self.context.tr.get("keyboard-sources-unavailable"),
            None,
        );
        self.sources.append(&row);
        self.add_source.set_sensitive(false);
    }

    fn render_sources(self: &Rc<Self>) {
        let tr = &self.context.tr;
        widgets::clear(&self.sources);
        self.add_source.set_sensitive(true);
        let group = self.group.borrow().clone();
        let available = self.available.borrow().clone();
        let removable = group.items.len() > 1;
        for (index, (unique_name, _)) in group.items.iter().enumerate() {
            let method = available.get(unique_name);
            let title = method.map_or_else(|| unique_name.clone(), InputMethod::label);
            let subtitle = method
                .map(|method| language_name(tr, &method.language))
                .filter(|language| !language.is_empty());
            let leading = badge(
                if unique_name.starts_with("keyboard-") {
                    ICON
                } else {
                    "preferences-desktop-locale-symbolic"
                },
                Tint::Gray,
                BadgeSize::Small,
            );
            let (row, accessories) =
                widgets::rich_row(Some(leading.upcast_ref()), &title, subtitle.as_deref());
            let remove = gtk::Button::builder()
                .icon_name("list-remove-symbolic")
                .tooltip_text(tr.get("keyboard-remove-source"))
                .sensitive(removable)
                .css_classes(["circular-button"])
                .build();
            let page = self.clone();
            remove.connect_clicked(move |_| page.remove_source(index));
            accessories.append(&remove);
            self.sources.append(&row);
        }
    }

    fn save_group(self: &Rc<Self>, group: Group) {
        let Some(fcitx) = self.fcitx.borrow().clone() else {
            return;
        };
        let page = self.clone();
        glib::spawn_future_local(async move {
            let args = (
                group.name.as_str(),
                group.layout.as_str(),
                group.items.clone(),
            )
                .to_variant();
            if let Err(err) = fcitx
                .call(FCITX_INTERFACE, "SetInputMethodGroupInfo", Some(args))
                .await
            {
                page.context
                    .notify_error("keyboard-sources-error", dbus::message(&err));
            }
            page.refresh_sources(&fcitx).await;
        });
    }

    fn remove_source(self: &Rc<Self>, index: usize) {
        let mut group = self.group.borrow().clone();
        if group.items.len() <= 1 || index >= group.items.len() {
            return;
        }
        group.items.remove(index);
        self.save_group(group);
    }

    /// 입력 소스 추가: 찾아서 고르는 시트
    fn choose_source(self: &Rc<Self>) {
        let tr = &self.context.tr;
        let group = self.group.borrow().clone();
        let mut candidates: Vec<InputMethod> = self
            .available
            .borrow()
            .values()
            .filter(|method| {
                !group
                    .items
                    .iter()
                    .any(|(name, _)| *name == method.unique_name)
            })
            .cloned()
            .collect();
        // 한국어와 영어를 먼저, 그 안에서는 이름순
        candidates.sort_by_key(|method| {
            let rank = match method.language.split(['_', '-']).next().unwrap_or_default() {
                "ko" => 0,
                "en" => 1,
                _ => 2,
            };
            (rank, method.label().to_lowercase())
        });

        let sheet = Sheet::new(
            self.context.window().as_ref(),
            Some(&badge(ICON, Tint::Gray, BadgeSize::Medium)),
            &tr.get("keyboard-add-source-title"),
            Some(&tr.get("keyboard-add-source-description")),
            Some(&tr.get("cancel")),
            &tr.get("keyboard-add"),
        );
        let search = gtk::SearchEntry::builder()
            .placeholder_text(tr.get("sidebar-search"))
            .build();
        let list = gtk::ListBox::builder()
            .css_classes(["settings-group", "source-picker"])
            .build();
        for method in &candidates {
            let language = language_name(tr, &method.language);
            let (row, _) = widgets::rich_row(
                None,
                &method.label(),
                (!language.is_empty()).then_some(language.as_str()),
            );
            row.set_widget_name(&method.unique_name);
            row.set_activatable(true);
            list.append(&row);
        }
        let keys: Vec<String> = candidates
            .iter()
            .map(|method| {
                format!(
                    "{} {} {} {}",
                    method.name, method.native_name, method.unique_name, method.language
                )
                .to_lowercase()
            })
            .collect();
        let query = Rc::new(RefCell::new(String::new()));
        let filter_query = query.clone();
        list.set_filter_func(move |row| {
            let query = filter_query.borrow();
            keys.get(row.index() as usize)
                .is_none_or(|key| key.contains(query.as_str()))
        });
        search.connect_search_changed(glib::clone!(
            #[weak]
            list,
            move |search| {
                *query.borrow_mut() = search.text().trim().to_lowercase();
                list.invalidate_filter();
            }
        ));
        let scroller = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(260)
            .max_content_height(260)
            .build();
        sheet.content.append(&search);
        sheet.content.append(&scroller);
        sheet.confirm.set_sensitive(false);
        list.connect_row_selected(glib::clone!(
            #[weak(rename_to = confirm)]
            sheet.confirm,
            move |_, row| confirm.set_sensitive(row.is_some())
        ));

        let add = {
            let page = self.clone();
            let window = sheet.window.clone();
            move |name: String| {
                window.close();
                let mut group = page.group.borrow().clone();
                let layout = String::new();
                group.items.push((name, layout));
                page.save_group(group);
            }
        };
        let add = Rc::new(add);
        list.connect_row_activated(glib::clone!(
            #[strong]
            add,
            move |_, row| add(row.widget_name().to_string())
        ));
        sheet.confirm.connect_clicked(glib::clone!(
            #[weak]
            list,
            move |_| {
                if let Some(row) = list.selected_row() {
                    add(row.widget_name().to_string());
                }
            }
        ));
        sheet.present();
        search.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_snap_to_nearest_step() {
        assert_eq!(nearest(&RATES, 25), 6);
        assert_eq!(nearest(&RATES, 0), 0);
        assert_eq!(nearest(&DELAYS, 600), 3);
        assert_eq!(nearest(&DELAYS, 200), DELAYS.len() - 1);
    }
}
