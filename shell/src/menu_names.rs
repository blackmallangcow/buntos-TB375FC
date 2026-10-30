// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! GTK 메뉴(GtkPopoverMenu) 항목의 접근성 이름. GTK 4.24까지 메뉴 항목(GtkModelButton)은 이름을 안의
//! 레이블에서 가져오는데(labelled-by), 그 레이블의 역할이 presentation이라 GTK의 이름 계산(accname)이
//! 레이블 글을 버린다. 그래서 로고 메뉴, 앱 메뉴, 앱 항목 메뉴의 항목이 접근성 트리에 이름 없이
//! menuitem으로만 나와, 에이전트(`bunt agent snapshot`)와 화면 읽기 프로그램이 이름으로 고르지 못했다.
//! GTK main(4.25 개발판)은 그 레이블을 보통 레이블로 바꿔 고쳤다. 그 판이 이미지에 들어오기 전까지 메뉴가
//! 열릴 때마다 항목에 레이블의 보이는 글을 이름으로 단다. 흐린 항목의 상태(sensitive)는 건드리지 않는다

use gtk4::{self as gtk, gio, glib, prelude::*};

/// 메뉴가 열릴 때마다 항목에 이름을 단다. GTK는 메뉴 모델이 바뀌면 항목을 새로 만들므로, 열린 동안
/// 모델이 바뀌어도 GTK가 항목을 만든 다음에 다시 단다. 앱 항목은 열 때 메뉴를 다시 읽고, 로고 메뉴는
/// 열 때 "<별칭> 로그아웃…" 항목을 바꾼다
pub fn follow(popover: &gtk::PopoverMenu) {
    popover.connect_map(|popover| name_items(popover.upcast_ref()));
    if let Some(model) = popover.menu_model() {
        watch(&model, &popover.downgrade());
    }
}

/// 모델과 그 묶음·하위 메뉴가 바뀌면 열린 메뉴의 항목에 이름을 단다. 묶음 안의 변경은 바깥 모델의
/// items-changed로 오지 않아 모델마다 지켜본다. GTK가 같은 신호에서 항목을 만드므로 그다음 차례에 단다
fn watch(model: &gio::MenuModel, popover: &glib::WeakRef<gtk::PopoverMenu>) {
    let weak = popover.clone();
    model.connect_items_changed(move |model, position, _, added| {
        let later = weak.clone();
        glib::idle_add_local_once(move || {
            if let Some(popover) = later.upgrade().filter(|popover| popover.is_mapped()) {
                name_items(popover.upcast_ref());
            }
        });
        for index in position..position + added {
            watch_links(model, index, &weak);
        }
    });
    for index in 0..model.n_items() {
        watch_links(model, index, popover);
    }
}

/// 항목 하나의 묶음과 하위 메뉴를 지켜본다
fn watch_links(model: &gio::MenuModel, index: i32, popover: &glib::WeakRef<gtk::PopoverMenu>) {
    for link in [gio::MENU_LINK_SECTION, gio::MENU_LINK_SUBMENU] {
        if let Some(child) = model.item_link(index, link) {
            watch(&child, popover);
        }
    }
}

/// 메뉴 막대(GtkPopoverMenuBar)의 메뉴마다 follow를 단다. 메뉴 막대는 모델을 바꿀 때마다 메뉴를 새로
/// 만들므로 모델을 바꾼 뒤에 부른다. 막대의 위 항목(앱 이름, 파일 등)은 보통 레이블이라 이름이 있다
pub fn follow_bar(bar: &gtk::PopoverMenuBar) {
    for_each_descendant(bar.upcast_ref(), &mut |widget| {
        if let Some(popover) = widget.downcast_ref::<gtk::PopoverMenu>() {
            follow(popover);
        }
    });
}

/// 위젯 아래의 메뉴 항목마다 이름을 단다
fn name_items(widget: &gtk::Widget) {
    for_each_descendant(widget, &mut |item| {
        if item.css_name() == "modelbutton" {
            name_item(item);
        }
    });
}

/// labelled-by 관계가 있으면 GTK는 label 속성을 보지 않으므로(accname 2B) 관계를 떼고 글을 직접 단다.
/// 글이 없는 항목(아이콘만 있는 것)은 그대로 둔다
fn name_item(item: &gtk::Widget) {
    let Some(text) = item_text(item) else {
        return;
    };
    item.reset_relation(gtk::AccessibleRelation::LabelledBy);
    item.update_property(&[gtk::accessible::Property::Label(&text)]);
}

/// 항목 안 레이블(css 이름 "label")의 보이는 글(니모닉 밑줄과 마크업을 뺀 것). 단축키 글(css 이름
/// "accelerator")은 이름에 넣지 않는다(GTK는 그것을 key-shortcuts로 알린다)
fn item_text(item: &gtk::Widget) -> Option<String> {
    let mut child = item.first_child();
    while let Some(widget) = child {
        if widget.css_name() == "label"
            && let Some(label) = widget.downcast_ref::<gtk::Label>()
        {
            let text = label.text();
            return (!text.is_empty()).then(|| text.to_string());
        }
        child = widget.next_sibling();
    }
    None
}

/// 위젯 아래의 모든 위젯 (자기 자신은 빼고, 깊이 우선)
fn for_each_descendant(widget: &gtk::Widget, visit: &mut dyn FnMut(&gtk::Widget)) {
    let mut child = widget.first_child();
    while let Some(current) = child {
        visit(&current);
        for_each_descendant(&current, visit);
        child = current.next_sibling();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 메뉴의 항목(GtkModelButton)들
    fn items(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut found = Vec::new();
        for_each_descendant(widget, &mut |item| {
            if item.css_name() == "modelbutton" {
                found.push(item.clone());
            }
        });
        found
    }

    /// 항목의 이름이 레이블 글로 바뀌었는지: label 속성이 있고 labelled-by 관계가 없다
    fn named(item: &gtk::Widget) -> bool {
        gtk::test_accessible_has_property(item, gtk::AccessibleProperty::Label)
            && !gtk::test_accessible_has_relation(item, gtk::AccessibleRelation::LabelledBy)
    }

    /// GTK가 필요한 시험은 이 하나에 모은다. GTK는 처음 초기화한 스레드에서만 쓸 수 있고, 시험은 스레드마다
    /// 돈다. 화면(Wayland·X11)이 없으면 건너뛴다
    #[test]
    fn menu_items_are_named_after_their_labels() {
        if gtk::init().is_err() {
            eprintln!("no display; skipping the GTK menu name test");
            return;
        }
        let menu = gio::Menu::new();
        let section = gio::Menu::new();
        section.append(Some("이 시스템에 관하여"), Some("app.about"));
        section.append(Some("_Settings…"), Some("app.settings"));
        menu.append_section(None, &section);
        let more = gio::Menu::new();
        more.append(Some("잠자기"), Some("app.sleep"));
        menu.append_submenu(Some("More"), &more);
        let popover = gtk::PopoverMenu::from_model(Some(&menu));

        // GTK 4.24까지는 항목이 레이블을 labelled-by로 가리키기만 해서 이름이 비었다. 하위 메뉴 쪽에는
        // 돌아가는 제목 항목("More")도 있다
        let made = items(popover.upcast_ref());
        assert!(made.len() >= 4, "{} items", made.len());
        assert!(made.iter().any(|item| !named(item)));

        name_items(popover.upcast_ref());
        let texts: Vec<Option<String>> = made.iter().map(item_text).collect();
        assert!(texts.contains(&Some("이 시스템에 관하여".into())));
        // 니모닉 밑줄은 이름에 넣지 않는다
        assert!(texts.contains(&Some("Settings…".into())));
        assert!(texts.contains(&Some("More".into())));
        assert!(texts.contains(&Some("잠자기".into())));
        assert!(made.iter().all(named));

        // 열린 동안 묶음이 바뀌어 GTK가 새로 만든 항목도 이름을 받는다
        let window = gtk::Window::new();
        let anchor = gtk::Button::with_label("menu");
        window.set_child(Some(&anchor));
        popover.set_parent(&anchor);
        follow(&popover);
        let context = glib::MainContext::default();
        let wait = |done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !done() && std::time::Instant::now() < deadline {
                context.iteration(false);
            }
            done()
        };
        window.present();
        assert!(wait(&|| anchor.is_mapped()), "the window did not open");
        // 누르기 없이 연 팝업의 잡기(grab)는 컴포지터가 바로 닫으므로 잡지 않고 연다
        popover.set_autohide(false);
        popover.popup();
        assert!(wait(&|| popover.is_mapped()), "the menu did not open");
        section.append(Some("Restart…"), Some("app.restart"));
        assert!(wait(&|| {
            let items = items(popover.upcast_ref());
            items.len() == made.len() + 1 && items.iter().all(named)
        }));
        popover.popdown();
        popover.unparent();
        window.destroy();
    }
}
