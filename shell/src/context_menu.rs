// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 단축 메뉴(오른쪽 단추, 손가락으로 길게 누르기)를 macOS처럼 띄운다.
//!
//! 단축 메뉴는 누르는 순간 포인터 바로 옆에 뜬다. GTK의 메뉴 항목(GtkModelButton)은 제게 오지 않은 누르기의
//! 떼기(`unpaired-release`)도 항목 안에서 일어나면 누른 것으로 친다. 누른 채 끌어다 떼어 고르는 길이다. 그런데 첫
//! 항목이 포인터에서 오른쪽 6px, 아래 8px에 있어, 오른쪽 단추를 한 번 눌렀다 떼는 사이 포인터가 그만큼만 흘러도 첫
//! 항목이 눌렸다. 데스크탑의 빈 곳에서는 그 항목이 새로운 폴더라, 누르지 않은 빈 "무제 폴더"가 생겼다
//! (TB375FC, 2026-11-17).
//!
//! macOS는 짧게 눌렀다 떼면 메뉴만 남기고, 누른 채 끌어다 떼면 그 항목을 고른다. 그대로 한다:
//! - 메뉴를 연 누르기의 떼기는 메뉴가 뜬 지 [`hold_time`](두 번 누르기 간격) 안이면 버린다. 그보다 오래 누르고 있다
//!   뗀 것은 GTK에 맡긴다
//! - 메뉴가 뜰 때 항목을 골라 두지 않는다. GTK는 팝오버를 띄우며 첫 항목에 키보드 포커스를 주고 GtkPopoverMenu는 그
//!   항목을 고른 모양으로 칠해, Return과 스페이스가 그 항목을 눌렀다. 이제 ↓가 첫 항목, ↑가 마지막 항목을 고른다

use std::{cell::Cell, rc::Rc, time::Duration};

use gtk4::{self as gtk, gdk, glib, prelude::*};

/// GTK의 두 번 누르기 간격을 알 수 없을 때 (GTK의 기본값)
const DEFAULT_HOLD: Duration = Duration::from_millis(400);

/// 메뉴에 온 단추·손가락 입력
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Input {
    Press,
    Release,
}

/// 메뉴를 연 누르기. 그 누르기의 떼기가 메뉴 밖(메뉴를 띄운 창)에서 일어나면 [`OpeningPress::ended`]로 알린다
#[derive(Clone, Debug, Default)]
pub struct OpeningPress {
    /// 아직 그 누르기의 떼기를 보지 못했고 [`hold_time`]도 지나지 않았다
    waiting: Rc<Cell<bool>>,
}

impl OpeningPress {
    fn begin() -> Self {
        Self {
            waiting: Rc::new(Cell::new(true)),
        }
    }

    /// 메뉴를 연 누르기가 끝났다. 이제부터 메뉴는 보통처럼 눌린다
    pub fn ended(&self) {
        self.waiting.set(false);
    }

    /// 메뉴에 온 입력을 버려야 하는지. 메뉴를 연 누르기의 떼기 하나만 버린다
    fn swallows(&self, input: Input) -> bool {
        match input {
            // 새 누르기가 왔으면 메뉴를 연 누르기는 이미 끝났다 (그 떼기를 이 창이 받지 못했다)
            Input::Press => {
                self.ended();
                false
            }
            Input::Release => self.waiting.replace(false),
        }
    }
}

/// 이보다 오래 누르고 있다 떼면 끌어다 고른 것으로 본다: 두 번 누르기 간격(시스템 설정의 마우스 페이지)
fn hold_time() -> Duration {
    gtk::Settings::default()
        .and_then(|settings| u64::try_from(settings.gtk_double_click_time()).ok())
        .filter(|millis| *millis > 0)
        .map_or(DEFAULT_HOLD, Duration::from_millis)
}

/// 단축 메뉴를 띄운다. 떠 있는 다른 팝오버는 닫는다(popups.rs). 누르고 있는 동안 부른다
pub fn popup(menu: &gtk::PopoverMenu) -> OpeningPress {
    let press = OpeningPress::begin();
    let events = gtk::EventControllerLegacy::new();
    // 항목(GtkModelButton)의 누르기 제스처도 capture 단계다. 메뉴는 항목의 조상이라 먼저 본다
    events.set_propagation_phase(gtk::PropagationPhase::Capture);
    let seen = press.clone();
    events.connect_event(move |_, event| {
        let input = match event.event_type() {
            gdk::EventType::ButtonPress | gdk::EventType::TouchBegin => Input::Press,
            gdk::EventType::ButtonRelease | gdk::EventType::TouchEnd => Input::Release,
            _ => return glib::Propagation::Proceed,
        };
        if seen.swallows(input) {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    menu.add_controller(events);
    let held = press.clone();
    glib::timeout_add_local_once(hold_time(), move || held.ended());

    crate::popups::close_others(menu.upcast_ref());
    popup_unselected(menu);
    press
}

/// 항목을 골라 두지 않고 띄운다. GTK는 띄우면서 포커스를 받을 수 있는 첫 항목에 포커스를 주므로, 띄우는 동안만 항목이
/// 포커스를 받지 않게 하고 포커스는 메뉴 자신에 둔다. 키는 그대로 메뉴가 받는다(Esc, 화살표)
fn popup_unselected(menu: &gtk::PopoverMenu) {
    let items: Vec<gtk::Widget> = items(menu.upcast_ref())
        .into_iter()
        .filter(gtk::Widget::is_focusable)
        .collect();
    for item in &items {
        item.set_focusable(false);
    }
    menu.popup();
    for item in &items {
        item.set_focusable(true);
    }
    if let Some(root) = menu.root() {
        root.set_focus(Some(menu));
    }
}

/// 메뉴의 항목(GtkModelButton, 하위 메뉴의 것도). GTK가 내보이지 않는 형식이라 이름으로 찾는다
fn items(widget: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut found = Vec::new();
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if widget.type_().name() == "GtkModelButton" {
            found.push(widget.clone());
        } else {
            found.extend(items(&widget));
        }
        child = widget.next_sibling();
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::gio;
    use std::time::Instant;

    #[test]
    fn the_release_of_the_opening_press_is_dropped_once() {
        // 오른쪽 단추를 눌러 메뉴가 떴고, 그 떼기가 항목 위에서 왔다: 버린다. 그다음 누르기와 떼기는 항목을 누른다
        let press = OpeningPress::begin();
        assert!(press.swallows(Input::Release));
        assert!(!press.swallows(Input::Press));
        assert!(!press.swallows(Input::Release));
    }

    #[test]
    fn a_release_after_the_hold_time_or_outside_the_menu_is_kept() {
        // 오래 누르고 있다 뗐다(시간이 지나 ended가 불렸다): 끌어다 고른 것이므로 항목을 누른다
        let held = OpeningPress::begin();
        held.ended();
        assert!(!held.swallows(Input::Release));
        // 떼기를 받지 못한 채 사람이 항목을 눌렀다: 그 누르기의 떼기는 버리지 않는다
        let missed = OpeningPress::begin();
        assert!(!missed.swallows(Input::Press));
        assert!(!missed.swallows(Input::Release));
    }

    fn wait(done: &dyn Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() && Instant::now() < deadline {
            context.iteration(false);
        }
        done()
    }

    fn selected(menu: &gtk::PopoverMenu) -> Vec<String> {
        items(menu.upcast_ref())
            .into_iter()
            .filter(|item| item.state_flags().contains(gtk::StateFlags::SELECTED))
            .filter_map(|item| item.property::<Option<String>>("text"))
            .collect()
    }

    /// 메뉴가 뜰 때 고른 항목이 없고, ↓가 첫 항목을, ↑가 마지막 항목을 고른다. GTK는 처음 초기화한 스레드에서만 쓸 수
    /// 있어 따로 띄운 프로세스에서 돈다(popups.rs의 시험과 같다). X11 화면이 없으면 건너뛴다
    #[test]
    fn the_menu_opens_with_no_item_selected() {
        const INSIDE: &str = "BUNT_SHELL_TEST_CONTEXT_MENU_PROCESS";
        const TEST: &str = "context_menu::tests::the_menu_opens_with_no_item_selected";
        if std::env::var_os(INSIDE).is_none() {
            // 출력은 파이프가 아니라 파일로 받는다 (input_mode의 시험과 같은 까닭)
            let log = std::env::temp_dir().join(format!(
                "bunt-shell-context-menu-test-{}.log",
                std::process::id()
            ));
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
            eprintln!("no display: skipping the context menu test");
            return;
        }
        // GTK는 밖을 누르면 닫히는(입력을 잡는) 팝오버를 띄울 때만 첫 항목에 포커스를 준다. 그런 팝업은 Wayland에서
        // 누르기 없이 띄우면 컴포지터가 바로 닫으므로 X11 화면(xvfb-run)에서만 본다
        let x11 = gdk::Display::default()
            .is_some_and(|display| display.type_().name() == "GdkX11Display");
        if !x11 {
            eprintln!("not an X11 display: skipping the context menu test");
            return;
        }
        let window = gtk::Window::new();
        let area = gtk::Fixed::new();
        area.set_focusable(true);
        window.set_child(Some(&area));
        window.set_default_size(400, 300);
        window.present();
        assert!(wait(&|| area.is_mapped()), "the window did not open");
        area.grab_focus();

        let build = || {
            let model = gio::Menu::new();
            model.append(Some("first"), Some("test.first"));
            model.append(Some("second"), Some("test.second"));
            model.append(Some("last"), Some("test.last"));
            let menu = gtk::PopoverMenu::from_model(Some(&model));
            let actions = gio::SimpleActionGroup::new();
            for name in ["first", "second", "last"] {
                actions.add_action(&gio::SimpleAction::new(name, None));
            }
            menu.insert_action_group("test", Some(&actions));
            menu.set_parent(&area);
            menu.set_has_arrow(false);
            menu.set_pointing_to(Some(&gdk::Rectangle::new(40, 40, 1, 1)));
            menu
        };

        // GTK가 하던 대로 띄우면 첫 항목이 골라져 있다 (이 시험이 보는 것이 맞는지 확인한다)
        let plain = build();
        plain.popup();
        assert!(wait(&|| plain.is_mapped()), "the plain menu did not open");
        assert_eq!(selected(&plain), ["first"]);
        plain.popdown();
        plain.unparent();

        let menu = build();
        let press = popup(&menu);
        assert!(wait(&|| menu.is_mapped()), "the menu did not open");
        assert_eq!(selected(&menu), [""; 0]);
        // 키는 메뉴가 받는다
        let focus = gtk::prelude::GtkWindowExt::focus(&window);
        assert_eq!(focus.as_ref(), Some(menu.upcast_ref::<gtk::Widget>()));
        // 메뉴를 연 누르기는 아직 끝나지 않았다
        assert!(press.waiting.get());
        // ↓는 첫 항목을 고른다
        assert!(menu.child_focus(gtk::DirectionType::Down));
        assert_eq!(selected(&menu), ["first"]);
        menu.popdown();
        menu.unparent();

        // ↑는 마지막 항목을 고른다
        let menu = build();
        popup(&menu);
        assert!(wait(&|| menu.is_mapped()), "the menu did not open again");
        assert!(menu.child_focus(gtk::DirectionType::Up));
        assert_eq!(selected(&menu), ["last"]);
        // 누르고 있는 시간이 지나면 메뉴를 연 누르기는 끝난 것으로 본다
        let press = popup(&build());
        assert!(wait(&|| !press.waiting.get()));
        window.destroy();
    }
}
