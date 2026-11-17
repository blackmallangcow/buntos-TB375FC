// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 팝오버를 겹쳐 띄우지 않게 한다.
//!
//! GTK(4.24.1, `gdk_wayland_surface_present_popup`)는 밖을 누르면 닫히는 팝오버를 띄울 때 먼저 입력을 잡고
//! (`gdk_seat_grab`) 그다음 화면에 올린다. 다른 팝오버가 떠 있으면 올리기를 그만두는데("Tried to map a grabbing
//! popup with a non-top most parent") 잡은 것은 놓지 않는다: 놓는 곳(`gdk_wayland_popup_hide_surface`)은 화면에
//! 올린 팝오버만 놓는다. 남은 잡기는 그 뒤의 모든 누르기를 "팝오버 밖을 눌렀다"로 보고 버린다
//! (`check_autohide`). 그러면 Dock도 메뉴 막대도 눌리지 않고, 포인터 움직임은 그대로라 도움말만 뜬다. 셸을 다시
//! 띄울 때까지 풀리지 않는다 (TB375FC, 2026-11-17).
//!
//! 사람의 누르기로는 겹치지 않는다. 열린 팝오버 밖을 누르면 GTK가 그것을 닫고 그 누르기를 버리기 때문이다.
//! 겹치는 것은 누르기 없이 여는 길이고, 셸에는 하나 있다: 메뉴 막대에서 포인터를 옮겨 메뉴를 넘길 때(menubar.rs의
//! HoverSwitch). 포인터가 항목 여럿을 한꺼번에 지나가면 넘기기가 겹쳐, 방금 연 메뉴가 떠 있는 채로 다음 메뉴를
//! 띄웠다. 에이전트로는 겹치지 않는다: 이 단추들에는 접근성 동작이 없어 포인터로 누르고, 사람이 연 메뉴가 떠 있는
//! 동안에는 컴포지터가 그 누르기를 거절한다. 그래도 누르기 없이 여는 길이 하나라도 더 생기면 같은 일이 나므로,
//! 팝오버를 띄우는 곳마다 그 직전에 떠 있는 다른 팝오버를 닫는다.

use gtk4::{self as gtk, prelude::*};

/// `button`의 메뉴가 뜨기 직전에 떠 있는 다른 팝오버를 닫고 `before`를 부른다. 누르기, `popup()`, 접근성 동작이
/// 모두 이 길을 지난다(GtkMenuButton의 create-popup)
pub fn guard_with(button: &gtk::MenuButton, before: impl Fn(&gtk::MenuButton) + 'static) {
    button.set_create_popup_func(move |button| {
        match button.popover() {
            Some(popover) => close_others(&popover),
            None => close_all(),
        }
        before(button);
    });
}

/// [`guard_with`]와 같되 더 할 일이 없다
pub fn guard(button: &gtk::MenuButton) {
    guard_with(button, |_| {});
}

/// `popover`를 띄우기 직전에 부른다. 떠 있는 다른 팝오버를 닫는다. `popover`가 들어 있는 팝오버(하위 메뉴의 부모)는
/// 둔다
pub fn close_others(popover: &gtk::Popover) {
    for open in open_popovers() {
        if open != *popover && !popover.is_ancestor(&open) {
            open.popdown();
        }
    }
}

fn close_all() {
    for open in open_popovers() {
        open.popdown();
    }
}

/// 셸의 모든 창에서 떠 있는 팝오버. 바깥 것이 먼저 나온다
fn open_popovers() -> Vec<gtk::Popover> {
    let mut found = Vec::new();
    for toplevel in gtk::Window::list_toplevels() {
        collect(&toplevel, &mut found);
    }
    found
}

fn collect(widget: &gtk::Widget, found: &mut Vec<gtk::Popover>) {
    if let Some(popover) = widget.downcast_ref::<gtk::Popover>() {
        if !popover.is_mapped() {
            return;
        }
        found.push(popover.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        collect(&widget, found);
        child = widget.next_sibling();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::glib;

    fn wait(done: &dyn Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !done() && std::time::Instant::now() < deadline {
            context.iteration(false);
        }
        done()
    }

    /// 누르기 없이 연 팝업의 잡기는 컴포지터가 바로 닫으므로, 시험에서는 잡지 않는 팝오버로 본다
    fn popover(child: &impl IsA<gtk::Widget>) -> gtk::Popover {
        let popover = gtk::Popover::builder().child(child).build();
        popover.set_autohide(false);
        popover
    }

    /// GTK는 처음 초기화한 스레드에서만 쓸 수 있어(menu_names의 시험) 이 시험은 따로 띄운 프로세스에서 돈다. 화면이
    /// 없으면 건너뛴다
    #[test]
    fn opening_a_menu_closes_the_other_ones_but_not_its_parent() {
        const INSIDE: &str = "BUNT_SHELL_TEST_POPUPS_PROCESS";
        const TEST: &str = "popups::tests::opening_a_menu_closes_the_other_ones_but_not_its_parent";
        if std::env::var_os(INSIDE).is_none() {
            // 출력은 파이프가 아니라 파일로 받는다 (input_mode의 시험과 같은 까닭)
            let log = std::env::temp_dir()
                .join(format!("bunt-shell-popups-test-{}.log", std::process::id()));
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
            eprintln!("no display: skipping the popover test");
            return;
        }
        let window = gtk::Window::new();
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        window.set_child(Some(&row));
        // 메뉴 단추 둘과, 첫 메뉴 안의 단추(하위 메뉴)
        let inner = gtk::MenuButton::builder().label("inner").build();
        let inner_menu = popover(&gtk::Label::new(Some("inner menu")));
        inner.set_popover(Some(&inner_menu));
        let first_menu = popover(&inner);
        let second_menu = popover(&gtk::Label::new(Some("second menu")));
        let first = gtk::MenuButton::builder()
            .label("first")
            .popover(&first_menu)
            .build();
        let second = gtk::MenuButton::builder()
            .label("second")
            .popover(&second_menu)
            .build();
        for button in [&first, &second, &inner] {
            guard(button);
        }
        row.append(&first);
        row.append(&second);
        window.present();
        assert!(wait(&|| first.is_mapped()), "the window did not open");

        first.popup();
        assert!(
            wait(&|| first_menu.is_mapped()),
            "the first menu did not open"
        );
        // 하위 메뉴를 열어도 부모 메뉴는 남는다
        inner.popup();
        assert!(
            wait(&|| inner_menu.is_mapped()),
            "the inner menu did not open"
        );
        assert!(first_menu.is_mapped());
        assert_eq!(open_popovers(), [first_menu.clone(), inner_menu.clone()]);
        // 누르기 없이 다른 메뉴를 열면(포인터로 넘기기, 접근성 동작) 떠 있던 것들이 먼저 닫힌다
        second.popup();
        assert!(
            wait(&|| second_menu.is_mapped()),
            "the second menu did not open"
        );
        assert!(wait(&|| !first_menu.is_mapped() && !inner_menu.is_mapped()));
        assert!(!first.is_active());
        assert_eq!(open_popovers(), std::slice::from_ref(&second_menu));
        // 직접 띄우는 팝오버도 같다
        let loose = popover(&gtk::Label::new(Some("loose")));
        loose.set_parent(&row);
        close_others(&loose);
        loose.popup();
        assert!(wait(&|| loose.is_mapped() && !second_menu.is_mapped()));
        assert_eq!(open_popovers(), std::slice::from_ref(&loose));
        loose.popdown();
        loose.unparent();
        window.destroy();
    }
}
