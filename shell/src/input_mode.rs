// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 입력 모드(설계안 5.1절)를 따른다. 컴포지터(org.buntos.Compositor1)가 붙은 입력 장치로 정한 포인터 모드와
//! 터치 모드를 읽고(GetInputMode) 바뀌면 받아(InputModeChanged), 터치 모드이면 셸의 모든 창에 CSS 클래스
//! `touch`를 단다. style.css는 그때 누를 영역을 넓힌다(디자인 스펙 11장). 무엇이 어디 있는지와
//! 글자·아이콘 크기는 그대로다.
//!
//! - 메뉴, 제어 센터, 알림 센터 같은 팝오버는 창의 CSS 노드 밑에 있어 창의 클래스를 따른다
//! - 나중에 뜨는 창(인증 창, 강제 종료 창, 화면 키보드)도 뜰 때(realize) 지금 모드를 받는다
//! - 컴포지터가 (다시) 뜨면 다시 읽는다. 이 API가 없는 컴포지터에서는 포인터 모드로 남는다

use std::{cell::Cell, rc::Rc};

use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::dbus;

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
/// 터치 모드에서 셸 창에 다는 CSS 클래스 (style.css의 window.touch)
pub const TOUCH_CLASS: &str = "touch";

/// 입력 모드
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    #[default]
    Pointer,
    Touch,
}

impl InputMode {
    /// 컴포지터가 알린 이름("pointer", "touch"). 모르는 이름은 누를 영역을 키우지 않는 포인터 모드로 읽는다
    pub fn parse(name: &str) -> Self {
        match name {
            "touch" => Self::Touch,
            _ => Self::Pointer,
        }
    }
}

/// 셸의 창들이 입력 모드를 따르게 한다. 셸이 뜰 때 창을 만들기 전에 한 번 부른다
pub fn follow() {
    let mode = Rc::new(Cell::new(InputMode::Pointer));
    style_new_windows(&mode);
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(connection) => watch(&connection, move |changed| {
                mode.set(changed);
                style_windows(changed);
            }),
            Err(err) => eprintln!("bunt-shell: session bus: {err}"),
        },
    );
}

/// 떠 있는 모든 창에 모드를 쓴다
fn style_windows(mode: InputMode) {
    for widget in gtk::Window::list_toplevels() {
        style(&widget, mode);
    }
}

fn style(window: &gtk::Widget, mode: InputMode) {
    match mode {
        InputMode::Touch => window.add_css_class(TOUCH_CLASS),
        InputMode::Pointer => window.remove_css_class(TOUCH_CLASS),
    }
}

/// 새 창은 뜰 때 지금 모드를 받는다. 창을 만드는 중에 클래스를 달면 빌더가 css-classes를 쓰며 지우므로
/// realize까지 미룬다
fn style_new_windows(mode: &Rc<Cell<InputMode>>) {
    let mode = mode.clone();
    gtk::Window::toplevels().connect_items_changed(move |list, position, _, added| {
        for index in position..position.saturating_add(added) {
            let Some(window) = list.item(index).and_downcast::<gtk::Widget>() else {
                continue;
            };
            let mode = mode.clone();
            window.connect_realize(move |window| style(window, mode.get()));
        }
    });
}

/// 컴포지터의 입력 모드를 지켜본다. 컴포지터가 (다시) 뜨면 읽고, 바뀌면 신호로 받는다. 모드가 바뀔 때만
/// changed를 부른다. 시험은 개인 버스의 연결을 준다
fn watch(connection: &gio::DBusConnection, changed: impl Fn(InputMode) + 'static) {
    let latest = Rc::new(Latest::default());
    let changed: Rc<dyn Fn(InputMode)> = Rc::new(changed);
    // 신호를 먼저 받는다. 읽은 뒤에 달면 그 사이에 바뀐 것을 놓친다
    let (on_signal, notify) = (latest.clone(), changed.clone());
    let subscription = connection.subscribe_to_signal(
        Some(NAME),
        Some(INTERFACE),
        Some("InputModeChanged"),
        Some(PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if let Some((name,)) = signal.parameters.get::<(String,)>()
                && let Some(mode) = on_signal.signalled(InputMode::parse(&name))
            {
                notify(mode);
            }
        },
    );
    // 셸이 끝날 때까지 받는다
    std::mem::forget(subscription);
    // 감시는 놓아도 멈추지 않는다 (dbus::watch_name)
    dbus::watch_name(
        connection,
        NAME,
        move |connection| read(&connection, &latest, &changed),
        || {},
    );
}

/// 지금 모드를 묻는다
fn read(connection: &gio::DBusConnection, latest: &Rc<Latest>, changed: &Rc<dyn Fn(InputMode)>) {
    let started = latest.read_started();
    let (latest, changed) = (latest.clone(), changed.clone());
    connection.call(
        Some(NAME),
        PATH,
        INTERFACE,
        "GetInputMode",
        None,
        Some(glib::VariantTy::new("(s)").unwrap()),
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(reply) => {
                if let Some((name,)) = reply.get::<(String,)>()
                    && let Some(mode) = latest.answered(started, InputMode::parse(&name))
                {
                    changed(mode);
                }
            }
            Err(err) => eprintln!("bunt-shell: {INTERFACE}.GetInputMode: {err}"),
        },
    );
}

/// 읽은 답(GetInputMode)과 신호(InputModeChanged)의 차례를 맞춘다. 읽는 동안 신호가 왔으면 신호가 더 새 값이라
/// 늦게 온 답은 버린다
#[derive(Default)]
struct Latest {
    mode: Cell<InputMode>,
    /// 받은 신호 수. 읽기를 시작할 때 적어 두었다가 답이 오면 그사이 신호가 왔는지 본다
    signals: Cell<u64>,
}

impl Latest {
    fn read_started(&self) -> u64 {
        self.signals.get()
    }

    /// 읽은 답. 모드가 바뀌었으면 새 모드
    fn answered(&self, started: u64, mode: InputMode) -> Option<InputMode> {
        if self.signals.get() != started {
            return None;
        }
        self.set(mode)
    }

    /// 받은 신호. 모드가 바뀌었으면 새 모드
    fn signalled(&self, mode: InputMode) -> Option<InputMode> {
        self.signals.set(self.signals.get().wrapping_add(1));
        self.set(mode)
    }

    fn set(&self, mode: InputMode) -> Option<InputMode> {
        (self.mode.replace(mode) != mode).then_some(mode)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn mode_names_from_the_compositor() {
        assert_eq!(InputMode::parse("touch"), InputMode::Touch);
        assert_eq!(InputMode::parse("pointer"), InputMode::Pointer);
        // 앞으로 더해질 모드나 틀린 이름은 누를 영역을 키우지 않는다
        assert_eq!(InputMode::parse("pen"), InputMode::Pointer);
        assert_eq!(InputMode::parse(""), InputMode::Pointer);
    }

    #[test]
    fn a_reply_older_than_a_signal_is_dropped() {
        let latest = Latest::default();
        // 읽는 동안 터치 모드로 바뀌었다는 신호가 먼저 왔다. 그 전에 읽은 답(포인터)은 버린다
        let started = latest.read_started();
        assert_eq!(latest.signalled(InputMode::Touch), Some(InputMode::Touch));
        assert_eq!(latest.answered(started, InputMode::Pointer), None);
        assert_eq!(latest.mode.get(), InputMode::Touch);
        // 그 뒤에 시작한 읽기의 답은 쓴다. 같은 모드면 바뀐 것이 없다
        let started = latest.read_started();
        assert_eq!(latest.answered(started, InputMode::Touch), None);
        assert_eq!(
            latest.answered(started, InputMode::Pointer),
            Some(InputMode::Pointer)
        );
        // 같은 신호가 거듭 와도 한 번만 바뀐다
        assert_eq!(latest.signalled(InputMode::Pointer), None);
    }

    /// 컴포지터 노릇을 하는 객체를 개인 버스에 두고, 셸이 처음 읽은 모드와 신호로 바뀐 모드를 받는지 본다.
    /// 컴포지터가 나중에 떠도(이름이 생겨도) 읽는다. dbus-daemon이 없으면 건너뛴다
    #[test]
    fn follows_the_compositor_over_dbus() {
        const INTROSPECTION: &str = r#"<node>
  <interface name="org.buntos.Compositor1">
    <method name="GetInputMode"><arg type="s" direction="out"/></method>
    <signal name="InputModeChanged"><arg type="s"/></signal>
  </interface>
</node>"#;

        let Some((_serial, bus)) = crate::service_dialog::test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let connect = || {
                    gio::DBusConnection::for_address_sync(
                        &address,
                        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                        None,
                        gio::Cancellable::NONE,
                    )
                    .unwrap()
                };
                let (shell, compositor) = (connect(), connect());
                let seen = Rc::new(RefCell::new(Vec::new()));
                let record = seen.clone();
                watch(&shell, move |mode| record.borrow_mut().push(mode));
                let wait_for = |expected: &[InputMode]| {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while seen.borrow().as_slice() != expected {
                        assert!(
                            Instant::now() < deadline,
                            "{:?} != {expected:?}",
                            seen.borrow()
                        );
                        context.iteration(false);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                };

                // 셸이 먼저 뜨고 컴포지터가 나중에 이름을 얻는다. 터치 모드로 떠 있었다
                let mode = Rc::new(RefCell::new("touch"));
                let answer = mode.clone();
                let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).unwrap();
                let interface = node.lookup_interface(INTERFACE).unwrap();
                let _registration = compositor
                    .register_object(PATH, &interface)
                    .method_call(move |_, _, _, _, method, _, invocation| {
                        assert_eq!(method, "GetInputMode");
                        invocation.return_value(Some(&(*answer.borrow(),).to_variant()));
                    })
                    .build()
                    .unwrap();
                compositor
                    .call_sync(
                        Some("org.freedesktop.DBus"),
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                        "RequestName",
                        Some(&(NAME, 0u32).to_variant()),
                        None,
                        gio::DBusCallFlags::NONE,
                        -1,
                        gio::Cancellable::NONE,
                    )
                    .unwrap();
                wait_for(&[InputMode::Touch]);

                // 키보드를 붙였다: 포인터 모드 신호. 같은 모드를 다시 알려도 한 번만 받는다
                let emit = |name: &str| {
                    compositor
                        .emit_signal(
                            None,
                            PATH,
                            INTERFACE,
                            "InputModeChanged",
                            Some(&(name,).to_variant()),
                        )
                        .unwrap();
                };
                mode.replace("pointer");
                emit("pointer");
                emit("pointer");
                wait_for(&[InputMode::Touch, InputMode::Pointer]);
                // 다른 이름(다른 연결)이 보낸 같은 신호는 듣지 않는다. 들었다면 터치 모드가 되었다가 뒤의
                // pointer로 한 번 더 바뀌어, 끝에 받은 차례가 길어진다
                let stranger = connect();
                stranger
                    .emit_signal(
                        None,
                        PATH,
                        INTERFACE,
                        "InputModeChanged",
                        Some(&("touch",).to_variant()),
                    )
                    .unwrap();
                stranger.flush_sync(gio::Cancellable::NONE).unwrap();
                emit("pointer");
                mode.replace("touch");
                emit("touch");
                let expected = [InputMode::Touch, InputMode::Pointer, InputMode::Touch];
                wait_for(&expected);
                // 늦게 오는 신호가 없는지 잠시 더 돌린다
                let settle = Instant::now() + Duration::from_millis(200);
                while Instant::now() < settle {
                    context.iteration(false);
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(seen.borrow().as_slice(), expected);
            })
            .unwrap();
    }

    /// 셸 스타일(theme/tokens.css와 style.css)로 GTK가 실제로 배치한 크기를 잰다: 터치 모드에서 누를 영역이
    /// 44px 이상(메뉴 막대와 메뉴의 줄은 36px)이고, 포인터 모드에서는 그대로 작고, 그림만 두고 넓힌 단추는 그림
    /// 크기와 카드 높이가 그대로인지.
    /// GTK는 처음 초기화한 스레드에서만 쓸 수 있어(menu_names의 시험) 이 시험은 따로 띄운 프로세스에서 돈다.
    /// 화면(Wayland·X11)이 없으면 건너뛴다. 화면 없는 곳에서는 xvfb-run cargo test로 돌린다
    #[test]
    fn touch_mode_targets_have_their_sizes() {
        const INSIDE: &str = "BUNT_SHELL_TEST_GTK_PROCESS";
        const TEST: &str = "input_mode::tests::touch_mode_targets_have_their_sizes";
        if std::env::var_os(INSIDE).is_none() {
            // 띄운 프로세스의 출력은 파이프가 아니라 파일로 받는다. 같은 프로세스의 D-Bus 시험이 GLib TestDBus를
            // 처음 올릴 때 exec 없이 fork하는 감시 프로세스가 파이프의 쓰기 끝을 물려받으면 끝을 받지 못해 멈췄다
            let log = std::env::temp_dir()
                .join(format!("bunt-shell-gtk-test-{}.log", std::process::id()));
            let file = std::fs::File::create(&log).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(INSIDE, "1")
                // 화면 없는 시험 환경(Xvfb)에는 GL이 없을 수 있다
                .env("GSK_RENDERER", "cairo")
                .stdout(file.try_clone().unwrap())
                .stderr(file)
                .status()
                .unwrap();
            let output = std::fs::read_to_string(&log).unwrap_or_default();
            let _ = std::fs::remove_file(&log);
            // 띄운 프로세스의 기록(GTK 경고 포함)은 실패할 때만 보인다
            assert!(status.success(), "{output}");
            return;
        }
        if gtk::init().is_err() {
            eprintln!("no display; skipping the touch target size test");
            return;
        }
        let parse = |css: &str| {
            let errors = Rc::new(RefCell::new(Vec::new()));
            let provider = gtk::CssProvider::new();
            let record = errors.clone();
            provider.connect_parsing_error(move |_, section, error| {
                record
                    .borrow_mut()
                    .push(format!("{}: {error}", section.to_str()));
            });
            provider.load_from_string(css);
            let errors = errors.borrow().clone();
            (provider, errors)
        };
        // 터치 모드 규칙은 CSS 변수 없이 써서 어느 GTK에서나 읽힌다. 셸 스타일 전체는 CSS 변수를 읽는 GTK 4.16부터
        // 오류가 없다(이미지는 4.24, theme/tokens.css)
        let touch_rules = &crate::STYLE[crate::STYLE.find("/* 터치 모드(설계안 5.1절)").unwrap()..];
        let (_, errors) = parse(touch_rules);
        assert!(errors.is_empty(), "{errors:?}");
        let (provider, errors) = parse(crate::STYLE);
        if gtk::check_version(4, 20, 0).is_none() {
            assert!(errors.is_empty(), "{errors:?}");
        }
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let pointer = Sample::new(InputMode::Pointer);
        let touch = Sample::new(InputMode::Touch);
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !(pointer.ready() && touch.ready()) {
            assert!(Instant::now() < deadline, "the windows did not open");
            context.iteration(false);
        }
        // 배치가 끝나도록 조금 더 돌린다
        let settle = Instant::now() + Duration::from_millis(100);
        while Instant::now() < settle {
            context.iteration(false);
        }

        for (name, widget) in touch.targets() {
            let (width, height) = touch.size(&widget);
            assert!(
                width >= 44.0 && height >= 44.0,
                "touch {name}: {width}×{height}"
            );
        }
        for (name, widget) in touch.rows() {
            let (_, height) = touch.size(&widget);
            assert!(height >= 44.0, "touch {name}: height {height}");
        }
        // 메뉴 막대와 메뉴의 줄은 36px다. 글자와 아이콘이 그대로라 더 높으면 여백이 너무 넓다
        for (name, widget) in touch.menu_targets() {
            let (width, height) = touch.size(&widget);
            assert!(
                width >= 36.0 && height >= 36.0,
                "touch {name}: {width}×{height}"
            );
        }
        assert_eq!(touch.size(touch.bar_button.upcast_ref()), (36.0, 36.0));
        // 항목이 내용보다 넓어도 내용은 가운데에 있다. 상자를 넣은 배터리 항목이 치우쳤다
        let item = touch.boxed_item.compute_bounds(&touch.window).unwrap();
        let icon = touch.boxed_icon.compute_bounds(&touch.window).unwrap();
        let offset = icon.x() + icon.width() / 2.0 - item.x() - item.width() / 2.0;
        assert!(offset.abs() <= 0.5, "the icon is {offset}px off the center");
        assert_eq!(touch.size(&touch.bar_item).1, 36.0);
        assert_eq!(touch.size(touch.title.upcast_ref()).1, 36.0);
        assert_eq!(touch.size(touch.extra_row.upcast_ref()).1, 36.0);
        assert!(touch.size(touch.scale.upcast_ref()).1 >= 36.0);
        // 포인터 모드는 그대로다: 작은 단추와 줄은 터치 모드의 크기보다 작다
        for (name, widget) in pointer.targets().into_iter().chain(pointer.rows()) {
            let (width, height) = pointer.size(&widget);
            assert!(
                width < 44.0 || height < 44.0,
                "pointer {name}: {width}×{height}"
            );
        }
        for (name, widget) in pointer.menu_targets() {
            let (width, height) = pointer.size(&widget);
            assert!(
                width < 36.0 || height < 36.0,
                "pointer {name}: {width}×{height}"
            );
        }
        // 그림만 두고 넓힌 것: 그림(내용)은 그대로이고 알림 카드의 높이도 그대로다. 막대 홈은 음소거 단추가 44px로
        // 넓어진 만큼 가로가 짧아지고 높이는 그대로다
        let content = |widget: &gtk::Widget| (widget.width(), widget.height());
        let pictures: [(&str, &gtk::Widget, &gtk::Widget); 6] = [
            (
                "panel mode button",
                pointer.mode_button.upcast_ref(),
                touch.mode_button.upcast_ref(),
            ),
            (
                "clipboard copy",
                pointer.clipboard_copy.upcast_ref(),
                touch.clipboard_copy.upcast_ref(),
            ),
            (
                "card close",
                pointer.close.upcast_ref(),
                touch.close.upcast_ref(),
            ),
            ("chip", pointer.chip.upcast_ref(), touch.chip.upcast_ref()),
            (
                "clear",
                pointer.clear.upcast_ref(),
                touch.clear.upcast_ref(),
            ),
            (
                "switch",
                pointer.switch.upcast_ref(),
                touch.switch.upcast_ref(),
            ),
        ];
        for (name, before, after) in pictures {
            let (before, after) = (content(before), content(after));
            // 분류 단추의 글자 높이는 글꼴에 따라 다르고, 터치 모드는 44px를 맞추려 16px 아래로 두지 않는다
            let height = if name == "chip" {
                before.1.max(16)
            } else {
                before.1
            };
            assert_eq!(after, (before.0, height), "{name}");
        }
        assert_eq!(content(touch.close.upcast_ref()), (18, 18));
        let trough = |sample: &Sample| content(&sample.scale.first_child().unwrap()).1;
        assert_eq!((trough(&pointer), trough(&touch)), (22, 22));
        assert_eq!(
            pointer.size(pointer.card.upcast_ref()).1,
            touch.size(touch.card.upcast_ref()).1
        );
        // 더 보기와 닫기의 누를 영역은 겹치지 않는다
        let expand = touch.expand.compute_bounds(&touch.window).unwrap();
        let close = touch.close.compute_bounds(&touch.window).unwrap();
        assert!(
            expand.x() + expand.width() <= close.x() + 0.5,
            "{expand:?} {close:?}"
        );
        // 클립보드 기록의 줄은 포인터 모드에서도 44px이다
        for sample in [&pointer, &touch] {
            assert!(sample.size(sample.clipboard_item.upcast_ref()).1 >= 44.0);
        }
        // 메뉴의 줄은 팝오버를 띄우지 않고 잰다
        assert_eq!(
            touch.menu_item.measure(gtk::Orientation::Vertical, -1).0,
            36
        );
        assert!(pointer.menu_item.measure(gtk::Orientation::Vertical, -1).0 < 36);
        pointer.window.destroy();
        touch.window.destroy();
    }

    /// 셸의 누를 것들을 한 창에 모은 견본. 클래스와 짜임은 셸과 같다(menubar.rs, extras.rs, control_center.rs,
    /// notification_center.rs, launcher.rs, dock.rs, service_dialog.rs)
    struct Sample {
        window: gtk::Window,
        bar_button: gtk::Button,
        bar_item: gtk::Widget,
        boxed_item: gtk::MenuButton,
        boxed_icon: gtk::Image,
        menu_item: gtk::Widget,
        title: gtk::Box,
        switch: gtk::Switch,
        extra_row: gtk::Button,
        slider_icon: gtk::Button,
        scale: gtk::Scale,
        cc_button: gtk::Button,
        card: gtk::Box,
        expand: gtk::Button,
        close: gtk::Button,
        clear: gtk::Button,
        search: gtk::SearchEntry,
        chips: gtk::Box,
        chip: gtk::ToggleButton,
        mode_button: gtk::Button,
        more_button: gtk::Button,
        clipboard_title: gtk::Label,
        clipboard_item: gtk::ListBoxRow,
        clipboard_copy: gtk::Button,
        clipboard_allow: gtk::Button,
        dock_item: gtk::Button,
        auth_button: gtk::Button,
        auth_field: gtk::Entry,
    }

    impl Sample {
        fn new(mode: InputMode) -> Self {
            let window = gtk::Window::builder()
                .css_classes(["bunt-menubar"])
                .default_width(800)
                .default_height(900)
                .build();
            style(window.upcast_ref(), mode);
            let column = gtk::Box::new(gtk::Orientation::Vertical, 8);

            // 메뉴 막대: 아이콘 항목과 앱 메뉴의 항목
            let bar = gtk::CenterBox::builder().css_classes(["bar"]).build();
            let start = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            let menu = gio::Menu::new();
            let file = gio::Menu::new();
            file.append(Some("이 시스템에 관하여"), Some("app.about"));
            menu.append_submenu(Some("파일"), &file);
            // 아이콘 항목의 메뉴(로고 메뉴처럼): 띄우지 않고 줄의 크기만 잰다
            let popover = gtk::PopoverMenu::from_model(Some(&file));
            let icon = gtk::MenuButton::builder()
                .icon_name("open-menu-symbolic")
                .popover(&popover)
                .css_classes(["status-item"])
                .build();
            let menubar = gtk::PopoverMenuBar::from_model(Some(&menu));
            start.append(&icon);
            start.append(&menubar);
            // 상자를 넣은 항목 (배터리, power.rs)
            let boxed_icon = gtk::Image::from_icon_name("battery-level-30-symbolic");
            let boxed = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            boxed.append(&boxed_icon);
            let boxed_item = crate::extras::button(&boxed, &crate::extras::Menu::new(), "배터리");
            start.append(&boxed_item);
            bar.set_start_widget(Some(&start));
            column.append(&bar);

            // 메뉴 막대 항목의 메뉴: 스위치가 있는 제목 줄, 줄, 막대
            let extra = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .css_classes(["extra-menu"])
                .build();
            let title = gtk::Box::builder().css_classes(["extra-title"]).build();
            title.append(&gtk::Label::builder().label("Wi-Fi").hexpand(true).build());
            let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
            title.append(&switch);
            let extra_row = gtk::Button::builder()
                .label("집")
                .css_classes(["extra-row"])
                .build();
            let slider = gtk::Box::builder().css_classes(["slider-row"]).build();
            let slider_icon = gtk::Button::builder()
                .icon_name("audio-volume-high-symbolic")
                .css_classes(["slider-icon"])
                .build();
            let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.1);
            scale.set_hexpand(true);
            slider.append(&slider_icon);
            slider.append(&scale);
            let cc_button = gtk::Button::builder()
                .label("Bluetooth")
                .css_classes(["cc-button"])
                .build();
            for widget in [
                title.upcast_ref::<gtk::Widget>(),
                extra_row.upcast_ref(),
                slider.upcast_ref(),
                cc_button.upcast_ref(),
            ] {
                extra.append(widget);
            }
            column.append(&extra);

            // 알림 카드와 알림 센터의 모두 지우기
            let card = gtk::Box::builder()
                .spacing(10)
                .css_classes(["notification-card"])
                .build();
            let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            header.append(
                &gtk::Label::builder()
                    .label("텍스트 편집기")
                    .hexpand(true)
                    .build(),
            );
            let expand = gtk::Button::builder()
                .icon_name("pan-down-symbolic")
                .valign(gtk::Align::Center)
                .css_classes(["card-expand"])
                .build();
            let close = gtk::Button::builder()
                .icon_name("window-close-symbolic")
                .valign(gtk::Align::Center)
                .css_classes(["card-close"])
                .build();
            header.append(&expand);
            header.append(&close);
            text.append(&header);
            text.append(
                &gtk::Label::builder()
                    .label("문서를 저장했습니다")
                    .can_target(false)
                    .build(),
            );
            card.append(&text);
            column.append(&card);
            let clear = gtk::Button::builder()
                .label("모두 지우기")
                .css_classes(["flat", "center-clear"])
                .halign(gtk::Align::End)
                .build();
            column.append(&clear);

            // 모든 앱 패널: 검색 칸과 분류 단추
            let search = gtk::SearchEntry::builder()
                .css_classes(["launcher-search"])
                .build();
            column.append(&search);
            let chips = gtk::Box::builder()
                .spacing(6)
                .css_classes(["launcher-chips"])
                .build();
            let chip = gtk::ToggleButton::builder()
                .label("유틸리티")
                .css_classes(["launcher-chip"])
                .build();
            chips.append(&chip);
            column.append(&chips);

            // 패널 머리의 단추와 클립보드 기록 (launcher.rs, clipboard_panel.rs)
            let mode_button = gtk::Button::builder()
                .icon_name("view-app-grid-symbolic")
                .valign(gtk::Align::Center)
                .css_classes(["launcher-mode"])
                .build();
            let more = gtk::MenuButton::builder()
                .icon_name("view-more-symbolic")
                .valign(gtk::Align::Center)
                .css_classes(["launcher-mode"])
                .build();
            let clipboard_title = gtk::Label::builder()
                .label("클립보드 기록")
                .css_classes(["clipboard-title"])
                .build();
            let panel_header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            panel_header.append(&clipboard_title);
            panel_header.append(&mode_button);
            panel_header.append(&more);
            column.append(&panel_header);
            let clipboard_copy = gtk::Button::builder()
                .icon_name("edit-copy-symbolic")
                .valign(gtk::Align::Center)
                .css_classes(["clipboard-copy"])
                .build();
            let clipboard_line = gtk::Box::builder().css_classes(["clipboard-row"]).build();
            clipboard_line.append(&clipboard_copy);
            let clipboard_item = gtk::ListBoxRow::builder()
                .child(&clipboard_line)
                .css_classes(["clipboard-item"])
                .build();
            let clipboard_list = gtk::ListBox::builder()
                .css_classes(["clipboard-list"])
                .build();
            clipboard_list.append(&clipboard_item);
            column.append(&clipboard_list);
            let clipboard_allow = gtk::Button::builder()
                .label("허용")
                .halign(gtk::Align::Center)
                .css_classes(["clipboard-allow"])
                .build();
            column.append(&clipboard_allow);

            // Dock: 가장 작은 아이콘(32px)
            let dock_item = gtk::Button::builder().css_classes(["dock-item"]).build();
            let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
            let image = gtk::Image::from_icon_name("application-x-executable");
            image.set_pixel_size(32);
            content.append(&image);
            content.append(&gtk::Box::builder().css_classes(["running-dot"]).build());
            dock_item.set_child(Some(&content));
            let dock = gtk::Box::builder().css_classes(["dock"]).build();
            dock.append(&dock_item);
            column.append(&dock);

            // 인증 창의 단추와 글 칸
            let auth_buttons = gtk::Box::builder().css_classes(["auth-buttons"]).build();
            let auth_button = gtk::Button::with_label("취소");
            auth_buttons.append(&auth_button);
            let auth_field = gtk::Entry::builder().css_classes(["auth-field"]).build();
            column.append(&auth_field);
            column.append(&auth_buttons);

            window.set_child(Some(&column));
            window.present();
            let bar_button = icon.first_child().and_downcast::<gtk::Button>().unwrap();
            let bar_item = find(menubar.upcast_ref(), "item").unwrap();
            let menu_item = find(popover.upcast_ref(), "modelbutton").unwrap();
            let more_button = more.first_child().and_downcast::<gtk::Button>().unwrap();
            Self {
                window,
                bar_button,
                bar_item,
                boxed_item,
                boxed_icon,
                menu_item,
                title,
                switch,
                extra_row,
                slider_icon,
                scale,
                cc_button,
                card,
                expand,
                close,
                clear,
                search,
                chips,
                chip,
                mode_button,
                more_button,
                clipboard_title,
                clipboard_item,
                clipboard_copy,
                clipboard_allow,
                dock_item,
                auth_button,
                auth_field,
            }
        }

        fn ready(&self) -> bool {
            self.window.is_mapped() && self.auth_button.width() > 0
        }

        /// 누르는 자리(테두리 상자)의 크기
        fn size(&self, widget: &gtk::Widget) -> (f32, f32) {
            let bounds = widget.compute_bounds(&self.window).unwrap();
            (bounds.width(), bounds.height())
        }

        /// 메뉴 막대와 메뉴 안의 것: 가로·세로 모두 36px이어야 한다
        fn menu_targets(&self) -> Vec<(&'static str, gtk::Widget)> {
            vec![
                ("menu bar icon", self.bar_button.clone().upcast()),
                ("menu bar item", self.bar_item.clone()),
                ("switch", self.switch.clone().upcast()),
                ("slider icon", self.slider_icon.clone().upcast()),
            ]
        }

        /// 가로·세로 모두 44px이어야 하는 것
        fn targets(&self) -> Vec<(&'static str, gtk::Widget)> {
            vec![
                ("card expand", self.expand.clone().upcast()),
                ("card close", self.close.clone().upcast()),
                ("dock item", self.dock_item.clone().upcast()),
                ("panel mode button", self.mode_button.clone().upcast()),
                ("panel more button", self.more_button.clone().upcast()),
                ("clipboard copy", self.clipboard_copy.clone().upcast()),
            ]
        }

        /// 높이만 44px이어야 하는 것(줄, 막대, 칸)
        fn rows(&self) -> Vec<(&'static str, gtk::Widget)> {
            vec![
                ("control center row", self.cc_button.clone().upcast()),
                ("clear notifications", self.clear.clone().upcast()),
                ("search field", self.search.clone().upcast()),
                ("category chip", self.chip.clone().upcast()),
                ("category row", self.chips.clone().upcast()),
                ("dialog button", self.auth_button.clone().upcast()),
                ("dialog field", self.auth_field.clone().upcast()),
                ("clipboard title", self.clipboard_title.clone().upcast()),
                ("clipboard allow", self.clipboard_allow.clone().upcast()),
            ]
        }
    }

    /// CSS 이름이 `name`인 첫 자손
    fn find(widget: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
        if widget.css_name() == name {
            return Some(widget.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(found) = find(&current, name) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
}
