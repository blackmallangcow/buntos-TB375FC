// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 메뉴바: 왼쪽에 로고 메뉴(macOS의 Apple 메뉴 순서: 이 시스템에 관하여, 시스템 설정, 강제 종료, 잠자기,
//! 다시 시작, 시스템 종료, 화면 잠금, 로그아웃)와 앞에 있는 앱의 메뉴,
//! 오른쪽에 메뉴 막대 항목(AI 제어 표시, 입력 소스, Bluetooth, 사운드, 배터리, Wi-Fi), 제어 센터,
//! 날짜·시간(누르면 알림 센터). macOS처럼 평소에는 배경이 투명하고 글씨가 흰색이며,
//! 앞에 있는 창이 화면을 채우면(최대화, 전체 화면) 배경이 검은색이 된다. "메뉴 막대 배경 보기"를
//! 켜면 늘 흐린 유리 바탕이다. 자동으로 숨고 위쪽 가장자리에서 나오는 것은 컴포지터가 한다
//! (comp/src/panels.rs, 설정은 compositor.rs가 알린다).

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, LayerShell};

use crate::{
    agent_control, appmenu, bluetooth, control_center,
    control_center::Services,
    input_source::{self, InputSources},
    interface_settings, layer_window, menu_names, network, notification_center,
    notifications::Notifications,
    popups, power, recording, session, shell_settings, sound, status_items,
    toplevels::Toplevels,
};

/// 시스템 설정 앱의 실행 파일. 인자는 처음 보여 줄 페이지 이름이다
const SETTINGS_COMMAND: &str = "bunt-settings";
/// 로고 메뉴의 아이콘 (shell/data/icons). 없으면 글자로 보인다
const LOGO_ICON: &str = "org.buntos.Logo-symbolic";
/// 셸 설정 키: 메뉴 막대 배경 보기
const BACKGROUND_KEY: &str = "menu-bar-background";
/// 움직임으로 넘겨 연 메뉴가 이 안에 닫히고 포인터가 그 항목 위에 있으면 다시 연다 (HoverSwitch)
const REOPEN_WITHIN: Duration = Duration::from_millis(500);

pub fn build(
    app: &gtk::Application,
    tr: &Rc<Translations>,
    toplevels: Option<&Toplevels>,
    notifications: &Notifications,
    services: &Services,
    input_sources: &InputSources,
) -> gtk::ApplicationWindow {
    let window = layer_window(app, "bunt-menubar");
    for edge in [Edge::Top, Edge::Left, Edge::Right] {
        window.set_anchor(edge, true);
    }
    window.auto_exclusive_zone_enable();

    add_actions(app, tr);
    let start = gtk::Box::builder().spacing(2).build();
    start.append(&logo_menu(tr));
    if let Some(toplevels) = toplevels {
        start.append(&appmenu::build(tr, toplevels));
    }
    // 오른쪽 항목은 macOS와 같은 순서다: ... 배터리, Wi-Fi, 제어 센터, 시계
    let end = gtk::Box::builder().spacing(2).build();
    // 화면 기록 중에만 보이는 기록 중단 단추는 맨 왼쪽에 둔다 (macOS)
    end.append(&recording::stop_button(tr));
    // AI가 제어하는 동안과 사람이 멈춘 동안만 보이는 AI 표시. 누르면 사람만 누르는 AI 제어 창을 연다
    end.append(&agent_control::indicator(app, tr));
    // 앱이 넣는 항목은 시스템 항목 왼쪽에 둔다
    end.append(&status_items::build());
    end.append(&control_center::focus_extra(tr));
    end.append(&input_source::extra(tr, input_sources));
    end.append(&bluetooth::extra(tr, &services.bluetooth));
    end.append(&sound::extra(tr, &services.sound));
    end.append(&power::extra(tr, &services.power));
    end.append(&network::extra(tr, &services.network));
    end.append(&control_center::extra(tr, services));
    end.append(&clock(tr, notifications));
    HoverSwitch::attach(&end);

    let bar = gtk::CenterBox::builder().css_classes(["bar"]).build();
    bar.set_start_widget(Some(&start));
    bar.set_end_widget(Some(&end));
    if let Some(toplevels) = toplevels {
        let bar = bar.clone();
        toplevels.subscribe(move |toplevels| {
            let fills_screen = toplevels
                .active()
                .is_some_and(|active| active.maximized || active.fullscreen);
            if fills_screen {
                bar.add_css_class("solid");
            } else {
                bar.remove_css_class("solid");
            }
        });
    }

    window.set_child(Some(&bar));
    if let Some(settings) = shell_settings() {
        follow_background(&settings, &bar);
    }
    window.present();
    window
}

/// 메뉴 막대 배경 보기를 켜면 흐린 유리 바탕에 화면 모드의 글씨색을 쓴다 (macOS 26과 같다)
fn follow_background(settings: &gio::Settings, bar: &gtk::CenterBox) {
    let apply = |settings: &gio::Settings, bar: &gtk::CenterBox| {
        if settings.boolean(BACKGROUND_KEY) {
            bar.add_css_class("background");
        } else {
            bar.remove_css_class("background");
        }
    };
    // 처리기가 설정 객체를 붙들어 셸이 끝날 때까지 둔다
    let (keep_alive, target) = (settings.clone(), bar.clone());
    settings.connect_changed(Some(BACKGROUND_KEY), move |settings, _| {
        let _ = &keep_alive;
        apply(settings, &target);
    });
    apply(settings, bar);
}

/// 로고 메뉴의 묶음과 항목(메시지 ID, 동작). macOS 26의 Apple 메뉴 순서와 묶음이다. 마지막 묶음 끝에
/// "<별칭> 로그아웃…"(LOG_OUT_ACTION)이 붙는다. 강제 종료, 잠자기, 화면 잠금, 로그아웃 동작은 session.rs가 단다
const LOGO_MENU: [&[(&str, &str)]; 5] = [
    &[("menu-about", "app.about")],
    &[("menu-settings", "app.settings")],
    &[("menu-force-quit", "app.force-quit")],
    &[
        ("menu-sleep", "app.sleep"),
        ("menu-restart", "app.restart"),
        ("menu-power-off", "app.power-off"),
    ],
    &[("menu-lock-screen", "app.lock-screen")],
];
const LOG_OUT_ACTION: &str = "app.log-out";

/// 로고 메뉴의 모델과, 로그아웃 항목이 든 마지막 묶음
fn logo_menu_model(tr: &Translations, log_out: &str) -> (gio::Menu, gio::Menu) {
    let menu = gio::Menu::new();
    let mut account = gio::Menu::new();
    for items in LOGO_MENU {
        account = gio::Menu::new();
        for (id, action) in items {
            account.append(Some(&tr.get(id)), Some(action));
        }
        menu.append_section(None, &account);
    }
    account.append(Some(log_out), Some(LOG_OUT_ACTION));
    (menu, account)
}

/// 로고 메뉴 단추
/// 메뉴 막대 항목의 메뉴가 열린 동안 다른 항목 위로 포인터가 가면 그 항목의 메뉴를 바로 연다(macOS). GTK는 열린
/// 팝오버 밖을 누르면 팝오버만 닫고 그 누르기를 버려, 다른 항목을 누르면 열린 메뉴만 닫혔다. 메뉴가 열린 동안에는
/// 다른 단추의 이벤트 컨트롤러도 움직임을 받지 못하지만, GTK는 포인터 아래 단추의 hover 상태(PRELIGHT)는 바꾸므로
/// 그 상태로 넘긴다. 포인터를 옮기자마자 누르면(에이전트의 누르기) 그 누르기가 방금 연 메뉴를 닫으므로,
/// REOPEN_WITHIN 안에 닫히고 포인터가 아직 그 항목 위에 있으면 다시 연다
struct HoverSwitch {
    area: gtk::Box,
    /// 움직임으로 넘겨 연 항목과 그때
    switched: RefCell<Option<(gtk::MenuButton, Instant)>>,
    /// hover 상태를 지켜보는 단추와 닫힘을 지켜보는 팝오버
    buttons: RefCell<Vec<glib::WeakRef<gtk::MenuButton>>>,
    popovers: RefCell<Vec<glib::WeakRef<gtk::Popover>>>,
    /// 넘기기를 기다리는 항목. 포인터가 여러 항목을 한꺼번에 지나가도 마지막 하나만 연다
    pending: RefCell<Option<gtk::MenuButton>>,
}

impl HoverSwitch {
    fn attach(area: &gtk::Box) {
        let switch = Rc::new(HoverSwitch {
            area: area.clone(),
            switched: RefCell::default(),
            buttons: RefCell::default(),
            popovers: RefCell::default(),
            pending: RefCell::default(),
        });
        // 앱이 넣는 항목은 나중에 붙으므로 포인터가 막대에 들어올 때마다 새 단추를 찾는다. 메뉴를 열려면 먼저
        // 막대에 들어와 누르므로 열린 메뉴의 단추와 그 이웃은 이미 지켜보고 있다
        switch.follow_buttons();
        let motion = gtk::EventControllerMotion::new();
        let entered = Rc::downgrade(&switch);
        motion.connect_enter(move |_, _, _| {
            if let Some(switch) = entered.upgrade() {
                switch.follow_buttons();
            }
        });
        area.add_controller(motion);
        // 단추의 처리기가 약한 참조만 드므로 상자가 사는 동안 함께 산다
        area.connect_destroy(move |_| {
            let _ = &switch;
        });
    }

    /// 막대 안의 메뉴 단추마다 hover 상태가 켜지는 것을 듣는다 (이미 듣는 단추는 건너뛴다)
    fn follow_buttons(self: &Rc<Self>) {
        let mut found = Vec::new();
        menu_buttons(self.area.upcast_ref(), &mut found);
        let mut buttons = self.buttons.borrow_mut();
        buttons.retain(|known| known.upgrade().is_some());
        for button in found {
            if buttons
                .iter()
                .any(|known| known.upgrade().as_ref() == Some(&button))
            {
                continue;
            }
            buttons.push(button.downgrade());
            let weak = Rc::downgrade(self);
            button.connect_state_flags_changed(move |button, old| {
                let hover = gtk::StateFlags::PRELIGHT;
                if button.state_flags().contains(hover)
                    && !old.contains(hover)
                    && let Some(switch) = weak.upgrade()
                {
                    switch.hovered(button);
                }
            });
        }
    }

    /// 포인터가 그 단추 위로 왔다. 다른 단추의 메뉴가 열려 있으면 이 단추의 메뉴로 바꾼다
    fn hovered(self: &Rc<Self>, hovered: &gtk::MenuButton) {
        let Some(open) = open_menu_button(self.area.upcast_ref()) else {
            return;
        };
        if open == *hovered {
            self.pending.take();
            return;
        }
        // 포인터 이벤트를 다루는 중이므로 메뉴를 바꾸는 것은 그 뒤에 한다. 포인터가 여러 항목을 한꺼번에 지나가면
        // 그 뒤가 오기 전에 여기로 여러 번 온다. 전에는 그때마다 넘기기를 잡아, 둘째부터는 방금 연 메뉴가 떠 있는
        // 채로 다음 메뉴를 띄웠다. GTK는 그때 누르기를 가로챈 채 남아 셸 전체가 눌리지 않았다(popups.rs)
        if self.pending.replace(Some(hovered.clone())).is_some() {
            return;
        }
        let switch = self.clone();
        glib::idle_add_local_once(move || switch.switch_pending());
    }

    /// 기다리던 항목의 메뉴로 넘긴다. 그사이 메뉴가 닫혔거나(사람이 눌렀다) 포인터가 그 항목을 떠났으면 그만둔다
    fn switch_pending(self: &Rc<Self>) {
        let Some(target) = self.pending.take() else {
            return;
        };
        let Some(open) = open_menu_button(self.area.upcast_ref()) else {
            return;
        };
        if open == target || !target.state_flags().contains(gtk::StateFlags::PRELIGHT) {
            return;
        }
        open.popdown();
        target.popup();
        self.switched
            .replace(Some((target.clone(), Instant::now())));
        if let Some(popover) = target.popover() {
            self.watch(&popover);
        }
    }

    /// 움직임으로 연 메뉴가 그 뒤의 누르기로 바로 닫히면 다시 연다
    fn watch(self: &Rc<Self>, popover: &gtk::Popover) {
        let mut popovers = self.popovers.borrow_mut();
        popovers.retain(|known| known.upgrade().is_some());
        if popovers
            .iter()
            .any(|known| known.upgrade().as_ref() == Some(popover))
        {
            return;
        }
        popovers.push(popover.downgrade());
        let weak = Rc::downgrade(self);
        popover.connect_closed(move |popover| {
            let Some(switch) = weak.upgrade() else {
                return;
            };
            let Some((button, at)) = switch.switched.take() else {
                return;
            };
            let same = button.popover().as_ref() == Some(popover);
            let still_over = button.state_flags().contains(gtk::StateFlags::PRELIGHT);
            if same && still_over && at.elapsed() < REOPEN_WITHIN {
                glib::idle_add_local_once(move || {
                    // 그사이 사람이 다른 메뉴를 열었으면 그대로 둔다
                    let over = button.state_flags().contains(gtk::StateFlags::PRELIGHT);
                    if over && open_menu_button(switch.area.upcast_ref()).is_none() {
                        button.popup();
                    }
                });
            }
        });
    }
}

/// 그 아래의 메뉴 단추를 모두 모은다 (앱이 넣는 항목은 상자 안에 있다)
fn menu_buttons(widget: &gtk::Widget, found: &mut Vec<gtk::MenuButton>) {
    if let Some(button) = widget.downcast_ref::<gtk::MenuButton>() {
        found.push(button.clone());
        return;
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        menu_buttons(&widget, found);
        child = widget.next_sibling();
    }
}

/// 메뉴가 열린(active) 메뉴 단추. 그 아래를 모두 훑는다(앱이 넣는 항목은 상자 안에 있다)
fn open_menu_button(widget: &gtk::Widget) -> Option<gtk::MenuButton> {
    if let Some(button) = widget.downcast_ref::<gtk::MenuButton>() {
        return button.is_active().then(|| button.clone());
    }
    std::iter::successors(widget.first_child(), gtk::Widget::next_sibling)
        .find_map(|child| open_menu_button(&child))
}

fn logo_menu(tr: &Rc<Translations>) -> gtk::MenuButton {
    let log_out = session::log_out_label(tr);
    let (menu, account) = logo_menu_model(tr, &log_out);

    // 아이콘이나 레이블을 자식으로 두면 펼침 화살표가 붙지 않는다
    let has_logo = gtk::gdk::Display::default()
        .is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(LOGO_ICON));
    let child: gtk::Widget = if has_logo {
        gtk::Image::from_icon_name(LOGO_ICON).upcast()
    } else {
        gtk::Label::new(Some(&tr.get("menubar-logo"))).upcast()
    };
    let button = gtk::MenuButton::builder()
        .child(&child)
        .menu_model(&menu)
        .tooltip_text(tr.get("menubar-logo"))
        .css_classes(["logo"])
        .build();
    popups::guard(&button);
    // 항목이 접근성 트리에 이름 없이 menuitem으로만 나와 에이전트가 순서로만 골랐다(GTK 4.24의 GtkModelButton)
    if let Some(popover) = button.popover().and_downcast::<gtk::PopoverMenu>() {
        menu_names::follow(&popover);
    }
    // 메뉴를 열 때마다 "<별칭> 로그아웃…"의 별칭을 다시 읽는다 (시스템 설정의 사용자 및 그룹에서 바뀐다)
    let tr = tr.clone();
    let shown = std::cell::RefCell::new(log_out);
    button.connect_active_notify(move |button| {
        if !button.is_active() {
            return;
        }
        let label = session::log_out_label(&tr);
        if *shown.borrow() != label {
            account.remove(account.n_items() - 1);
            account.append(Some(&label), Some(LOG_OUT_ACTION));
            shown.replace(label);
        }
    });
    button
}

fn add_actions(app: &gtk::Application, tr: &Rc<Translations>) {
    let about = gio::ActionEntry::builder("about")
        .activate(|_, _, _| open_settings(Some("about")))
        .build();
    let settings = gio::ActionEntry::builder("settings")
        .activate(|_, _, _| open_settings(None))
        .build();
    let restart_tr = tr.clone();
    let restart = gio::ActionEntry::builder("restart")
        .activate(move |_, _, _| confirm_power(&restart_tr, Power::Restart))
        .build();
    let power_off_tr = tr.clone();
    let power_off = gio::ActionEntry::builder("power-off")
        .activate(move |_, _, _| confirm_power(&power_off_tr, Power::PowerOff))
        .build();
    app.add_action_entries([about, settings, restart, power_off]);
}

/// 시스템 설정 앱을 연다. page는 페이지 이름이다 (예: "about", "wifi", "battery").
/// 이미 떠 있으면 그 창이 그 페이지로 바뀐다
pub fn open_settings(page: Option<&str>) {
    let command = match page {
        Some(page) => format!("{SETTINGS_COMMAND} {page}"),
        None => SETTINGS_COMMAND.to_owned(),
    };
    let info = match gio::AppInfo::create_from_commandline(
        &command,
        Some(SETTINGS_COMMAND),
        gio::AppInfoCreateFlags::NONE,
    ) {
        Ok(info) => info,
        Err(err) => {
            eprintln!("bunt-shell: {command}: {err}");
            return;
        }
    };
    let context = gtk::gdk::Display::default().map(|display| display.app_launch_context());
    if let Err(err) = info.launch(&[], context.as_ref()) {
        eprintln!("bunt-shell: failed to start {command}: {err}");
    }
}

#[derive(Clone, Copy)]
enum Power {
    Restart,
    PowerOff,
}

impl Power {
    /// org.freedesktop.login1.Manager의 메서드
    fn method(self) -> &'static str {
        match self {
            Self::Restart => "Reboot",
            Self::PowerOff => "PowerOff",
        }
    }

    /// 확인 질문과 확인 버튼의 Fluent 메시지 ID
    fn messages(self) -> (&'static str, &'static str) {
        match self {
            Self::Restart => ("restart-confirm", "dialog-restart"),
            Self::PowerOff => ("power-off-confirm", "dialog-power-off"),
        }
    }
}

fn confirm_power(tr: &Translations, power: Power) {
    let (question, button) = power.messages();
    let dialog = gtk::AlertDialog::builder()
        .modal(true)
        .message(tr.get(question))
        .buttons(vec![tr.get("dialog-cancel"), tr.get(button)])
        .cancel_button(0)
        .default_button(1)
        .build();
    dialog.choose(
        None::<&gtk::Window>,
        gio::Cancellable::NONE,
        move |choice| {
            if choice == Ok(1) {
                request_power(power);
            }
        },
    );
}

/// logind에 다시 시작이나 종료를 요청한다. 허용 여부는 polkit이 정한다.
fn request_power(power: Power) {
    let bus = match gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
        Ok(bus) => bus,
        Err(err) => {
            eprintln!("bunt-shell: no system bus: {err}");
            return;
        }
    };
    bus.call(
        Some("org.freedesktop.login1"),
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        power.method(),
        // interactive: polkit이 인증을 물을 수 있게 한다
        Some(&(true,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| {
            if let Err(err) = result {
                eprintln!("bunt-shell: {} failed: {err}", power.method());
            }
        },
    );
}

/// 날짜와 시간. 누르면 알림 센터가 열린다
fn clock(tr: &Rc<Translations>, notifications: &Notifications) -> gtk::MenuButton {
    let label = gtk::Label::new(None);
    let formats = [
        tr.get("menubar-clock-format-12h"),
        tr.get("menubar-clock-format-24h"),
    ];
    let interface = interface_settings();
    // 매초 다시 읽으므로 시스템 설정에서 바꾼 시계 형식과 시간대가 바로 반영된다
    let update = move |label: &gtk::Label| {
        let use_24h = interface
            .as_ref()
            .is_some_and(|settings| settings.string("clock-format") == "24h");
        let format = &formats[usize::from(use_24h)];
        if let Ok(text) = glib::DateTime::now_local().and_then(|now| now.format(format)) {
            label.set_text(&text);
        }
    };
    update(&label);
    let ticking = label.clone();
    glib::timeout_add_seconds_local(1, move || {
        update(&ticking);
        glib::ControlFlow::Continue
    });
    let button = gtk::MenuButton::builder()
        .child(&label)
        .popover(&notification_center::popover(notifications, tr))
        .css_classes(["clock"])
        .build();
    popups::guard(&button);
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 메뉴 모델의 묶음마다 (레이블, 동작)
    fn sections(menu: &gio::Menu) -> Vec<Vec<(String, String)>> {
        let text = |model: &gio::MenuModel, item, attribute| {
            model
                .item_attribute_value(item, attribute, Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>())
                .unwrap_or_default()
        };
        (0..menu.n_items())
            .filter_map(|index| menu.item_link(index, gio::MENU_LINK_SECTION))
            .map(|section| {
                (0..section.n_items())
                    .map(|item| {
                        (
                            text(&section, item, gio::MENU_ATTRIBUTE_LABEL),
                            text(&section, item, gio::MENU_ATTRIBUTE_ACTION),
                        )
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_logo_menu_follows_the_apple_menu() {
        let (language, source) = crate::TRANSLATIONS[0];
        assert_eq!(language, "ko");
        let tr = Translations::from_source(language, source);
        let (menu, account) = logo_menu_model(&tr, "버니 로그아웃…");
        let expected: Vec<Vec<(&str, &str)>> = vec![
            vec![("이 시스템에 관하여", "app.about")],
            vec![("시스템 설정…", "app.settings")],
            vec![("강제 종료…", "app.force-quit")],
            vec![
                ("잠자기", "app.sleep"),
                ("다시 시작…", "app.restart"),
                ("시스템 종료…", "app.power-off"),
            ],
            vec![
                ("화면 잠금", "app.lock-screen"),
                ("버니 로그아웃…", "app.log-out"),
            ],
        ];
        let expected: Vec<Vec<(String, String)>> = expected
            .into_iter()
            .map(|items| {
                items
                    .into_iter()
                    .map(|(label, action)| (label.to_owned(), action.to_owned()))
                    .collect()
            })
            .collect();
        assert_eq!(sections(&menu), expected);
        // 별칭이 바뀌면 고치는 묶음은 마지막 묶음이다
        assert_eq!(account.n_items(), 2);
    }

    /// 조건이 될 때까지 메인 루프를 돌린다
    fn wait(done: &dyn Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() && Instant::now() < deadline {
            context.iteration(false);
        }
        done()
    }

    /// 밀린 일이 없을 때까지 메인 루프를 돌린다
    fn settle() {
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while context.pending() && Instant::now() < deadline {
            context.iteration(false);
        }
    }

    /// 메뉴가 열린 채 포인터가 항목 여럿을 한꺼번에 지나가도 메뉴는 하나만 뜬다. 전에는 지나간 항목의 메뉴가 떠 있는
    /// 채로 다음 메뉴를 띄워 GTK가 누르기를 가로챈 채 남았다(popups.rs). GTK는 처음 초기화한 스레드에서만 쓸 수
    /// 있어 따로 띄운 프로세스에서 돈다. 화면이 없으면 건너뛴다
    #[test]
    fn sweeping_over_several_items_opens_one_menu() {
        const INSIDE: &str = "BUNT_SHELL_TEST_HOVER_PROCESS";
        const TEST: &str = "menubar::tests::sweeping_over_several_items_opens_one_menu";
        if std::env::var_os(INSIDE).is_none() {
            // 출력은 파이프가 아니라 파일로 받는다 (input_mode의 시험과 같은 까닭)
            let log = std::env::temp_dir()
                .join(format!("bunt-shell-hover-test-{}.log", std::process::id()));
            let file = std::fs::File::create(&log).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(INSIDE, "1")
                .env("GSK_RENDERER", "cairo")
                .stdout(file.try_clone().unwrap())
                .stderr(file)
                .status()
                .unwrap();
            let output = std::fs::read_to_string(&log).unwrap_or_default();
            let _ = std::fs::remove_file(&log);
            assert!(status.success(), "{output}");
            return;
        }
        if gtk::init().is_err() {
            eprintln!("no display: skipping the menu switching test");
            return;
        }
        let window = gtk::Window::new();
        let area = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        window.set_child(Some(&area));
        // 누르기 없이 연 팝업의 잡기는 컴포지터가 바로 닫으므로, 시험에서는 잡지 않는 팝오버로 본다
        let menus: Vec<(gtk::MenuButton, gtk::Popover)> = ["first", "second", "third"]
            .into_iter()
            .map(|name| {
                let popover = gtk::Popover::builder()
                    .child(&gtk::Label::new(Some(name)))
                    .build();
                popover.set_autohide(false);
                let button = gtk::MenuButton::builder()
                    .label(name)
                    .popover(&popover)
                    .build();
                popups::guard(&button);
                area.append(&button);
                (button, popover)
            })
            .collect();
        HoverSwitch::attach(&area);
        // 뜬 차례와, 한꺼번에 떠 있던 메뉴의 가장 큰 수
        let shown = Rc::new(RefCell::new(Vec::new()));
        let most = Rc::new(std::cell::Cell::new(0));
        let popovers: Vec<gtk::Popover> =
            menus.iter().map(|(_, popover)| popover.clone()).collect();
        for (index, popover) in popovers.iter().enumerate() {
            let (shown, most, popovers) = (shown.clone(), most.clone(), popovers.clone());
            popover.connect_map(move |popover| {
                shown.borrow_mut().push(index);
                let others = popovers
                    .iter()
                    .filter(|other| *other != popover && other.is_mapped())
                    .count();
                most.set(most.get().max(others + 1));
            });
        }
        window.present();
        let (first, second, third) = (&menus[0], &menus[1], &menus[2]);
        assert!(wait(&|| first.0.is_mapped()), "the window did not open");
        let hover = gtk::StateFlags::PRELIGHT;

        first.0.popup();
        assert!(wait(&|| first.1.is_mapped()), "the first menu did not open");
        // 포인터가 둘째 항목을 지나 셋째 항목에 멈춘다. 메뉴가 열린 동안 GTK는 hover 상태만 바꾼다
        second.0.set_state_flags(hover, false);
        second.0.unset_state_flags(hover);
        third.0.set_state_flags(hover, false);
        assert!(wait(&|| third.1.is_mapped()), "the third menu did not open");
        settle();
        assert!(!first.1.is_mapped() && !second.1.is_mapped());
        assert!(third.0.is_active() && !first.0.is_active() && !second.0.is_active());
        // 지나간 둘째 항목의 메뉴는 뜬 적이 없고, 메뉴 둘이 함께 뜬 적도 없다
        assert_eq!(*shown.borrow(), [0, 2]);
        assert_eq!(most.get(), 1);

        // 넘기기를 기다리는 사이 사람이 메뉴를 닫으면(바깥 누르기, Esc) 넘기지 않는다
        third.0.unset_state_flags(hover);
        first.0.set_state_flags(hover, false);
        third.0.popdown();
        settle();
        assert!(popovers.iter().all(|popover| !popover.is_mapped()));
        assert_eq!(*shown.borrow(), [0, 2]);

        // 넘기기를 기다리는 사이 포인터가 그 항목을 떠나면 열려 있던 메뉴가 남는다
        first.0.unset_state_flags(hover);
        third.0.popup();
        assert!(wait(&|| third.1.is_mapped()));
        second.0.set_state_flags(hover, false);
        second.0.unset_state_flags(hover);
        settle();
        assert!(third.1.is_mapped() && !second.1.is_mapped());
        assert_eq!(*shown.borrow(), [0, 2, 2]);
        assert_eq!(most.get(), 1);
        window.destroy();
    }
}
