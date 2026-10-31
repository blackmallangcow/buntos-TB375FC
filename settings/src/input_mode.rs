// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 입력 모드(설계안 5.1절)를 따른다. 컴포지터(org.buntos.Compositor1)가 붙은 입력 장치로 정한 모드가 터치
//! 모드이면 시스템 설정의 모든 창(설정 창, 시트)에 CSS 클래스 `touch`를 단다. style.css는 그때 목록 줄과 스위치의
//! 누를 영역을 44px 이상으로 넓힌다(디자인 스펙 11장). 배치와 글자 크기는 그대로다.
//!
//! 셸(shell/src/input_mode.rs)과 같은 신호를 받는다. 컴포지터가 다시 뜨면 이 앱도 Wayland 연결을 잃고 끝나므로
//! 컴포지터를 지켜보지 않고, 뜰 때 한 번 읽은 뒤로는 신호만 받는다. 나중에 뜨는 시트도 뜰 때(realize) 지금 모드를
//! 받는다

use std::{cell::Cell, rc::Rc};

use gtk4::{self as gtk, gio, glib, prelude::*};

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
/// 터치 모드에서 창에 다는 CSS 클래스 (style.css의 window.touch)
const TOUCH_CLASS: &str = "touch";

/// 컴포지터가 알린 모드 이름이 터치 모드인지. 모르는 이름은 누를 영역을 키우지 않는다
fn is_touch(name: &str) -> bool {
    name == "touch"
}

/// 설정 앱의 창들이 입력 모드를 따르게 한다. 앱이 뜰 때 창을 만들기 전에 한 번 부른다
pub fn follow() {
    let touch = Rc::new(Cell::new(false));
    style_new_windows(&touch);
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(connection) => watch(&connection, move |changed| {
                touch.set(changed);
                for window in gtk::Window::list_toplevels() {
                    style(&window, changed);
                }
            }),
            Err(err) => eprintln!("bunt-settings: session bus: {err}"),
        },
    );
}

fn style(window: &gtk::Widget, touch: bool) {
    if touch {
        window.add_css_class(TOUCH_CLASS);
    } else {
        window.remove_css_class(TOUCH_CLASS);
    }
}

/// 새 창은 뜰 때 지금 모드를 받는다. 창을 만드는 중에 클래스를 달면 빌더가 css-classes를 쓰며 지우므로
/// realize까지 미룬다
fn style_new_windows(touch: &Rc<Cell<bool>>) {
    let touch = touch.clone();
    gtk::Window::toplevels().connect_items_changed(move |list, position, _, added| {
        for index in position..position.saturating_add(added) {
            let Some(window) = list.item(index).and_downcast::<gtk::Widget>() else {
                continue;
            };
            let touch = touch.clone();
            window.connect_realize(move |window| style(window, touch.get()));
        }
    });
}

/// 신호를 받고 지금 모드를 한 번 읽는다. 모드가 바뀔 때만 changed를 부른다. 읽는 동안 신호가 오면 신호가 더 새
/// 값이라 늦게 온 답은 버린다
fn watch(connection: &gio::DBusConnection, changed: impl Fn(bool) + 'static) {
    let state = Rc::new(State::default());
    let changed: Rc<dyn Fn(bool)> = Rc::new(changed);
    // 신호를 먼저 받는다. 읽은 뒤에 달면 그 사이에 바뀐 것을 놓친다
    let (on_signal, notify) = (state.clone(), changed.clone());
    let subscription = connection.subscribe_to_signal(
        Some(NAME),
        Some(INTERFACE),
        Some("InputModeChanged"),
        Some(PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if let Some((name,)) = signal.parameters.get::<(String,)>()
                && let Some(touch) = on_signal.signalled(is_touch(&name))
            {
                notify(touch);
            }
        },
    );
    // 앱이 끝날 때까지 받는다
    std::mem::forget(subscription);
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
                    && let Some(touch) = state.answered(is_touch(&name))
                {
                    changed(touch);
                }
            }
            Err(err) => eprintln!("bunt-settings: {INTERFACE}.GetInputMode: {err}"),
        },
    );
}

/// 지금 모드와, 처음 읽은 답보다 신호가 먼저 왔는지
#[derive(Default)]
struct State {
    touch: Cell<bool>,
    signalled: Cell<bool>,
}

impl State {
    /// 처음 읽은 답. 신호가 먼저 왔으면 버린다. 모드가 바뀌었으면 새 모드
    fn answered(&self, touch: bool) -> Option<bool> {
        if self.signalled.get() {
            return None;
        }
        self.set(touch)
    }

    /// 받은 신호. 모드가 바뀌었으면 새 모드
    fn signalled(&self, touch: bool) -> Option<bool> {
        self.signalled.set(true);
        self.set(touch)
    }

    fn set(&self, touch: bool) -> Option<bool> {
        (self.touch.replace(touch) != touch).then_some(touch)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn only_touch_enlarges_targets() {
        assert!(is_touch("touch"));
        assert!(!is_touch("pointer"));
        // 앞으로 더해질 모드나 틀린 이름은 누를 영역을 키우지 않는다
        assert!(!is_touch("pen"));
        assert!(!is_touch(""));
    }

    #[test]
    fn a_reply_older_than_a_signal_is_dropped() {
        let state = State::default();
        // 읽는 동안 터치 모드로 바뀌었다는 신호가 먼저 왔다. 그 전에 읽은 답(포인터)은 버린다
        assert_eq!(state.signalled(true), Some(true));
        assert_eq!(state.answered(false), None);
        assert!(state.touch.get());
        // 같은 신호가 거듭 와도 한 번만 바뀐다
        assert_eq!(state.signalled(true), None);
        assert_eq!(state.signalled(false), Some(false));
        // 신호가 오기 전의 답은 쓴다
        let state = State::default();
        assert_eq!(state.answered(true), Some(true));
        assert_eq!(state.answered(true), None);
    }

    /// 시험이 띄우는 버스 데몬. 주소로 붙어 프로세스의 환경 변수를 바꾸지 않는다
    struct Bus {
        daemon: Child,
        address: String,
    }

    impl Bus {
        /// dbus-daemon이 없으면 None
        fn start() -> Option<Self> {
            let mut daemon = match Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address"])
                .stdout(Stdio::piped())
                .spawn()
            {
                Ok(daemon) => daemon,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
                Err(err) => panic!("dbus-daemon: {err}"),
            };
            let mut address = String::new();
            if let Some(stdout) = daemon.stdout.take() {
                BufReader::new(stdout).read_line(&mut address).unwrap();
            }
            let address = address.trim().to_owned();
            assert!(!address.is_empty(), "dbus-daemon did not start");
            Some(Self { daemon, address })
        }

        fn connect(&self) -> gio::DBusConnection {
            gio::DBusConnection::for_address_sync(
                &self.address,
                gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                None,
                gio::Cancellable::NONE,
            )
            .unwrap()
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
        }
    }

    /// 컴포지터 노릇을 하는 객체를 개인 버스에 두고, 설정 앱이 처음 읽은 모드와 신호로 바뀐 모드를 받고 다른
    /// 연결이 보낸 신호는 듣지 않는지 본다. dbus-daemon이 없으면 건너뛴다
    #[test]
    fn follows_the_compositor_over_dbus() {
        const INTROSPECTION: &str = r#"<node>
  <interface name="org.buntos.Compositor1">
    <method name="GetInputMode"><arg type="s" direction="out"/></method>
    <signal name="InputModeChanged"><arg type="s"/></signal>
  </interface>
</node>"#;

        let Some(bus) = Bus::start() else {
            eprintln!("dbus-daemon is not installed; skipping the D-Bus test");
            return;
        };
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                // 컴포지터가 먼저 떠 있다(설정 앱은 컴포지터 위에서만 뜬다). 터치 모드다
                let compositor = bus.connect();
                let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).unwrap();
                let interface = node.lookup_interface(INTERFACE).unwrap();
                let _registration = compositor
                    .register_object(PATH, &interface)
                    .method_call(|_, _, _, _, _, _, invocation| {
                        invocation.return_value(Some(&("touch",).to_variant()));
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

                let seen = Rc::new(RefCell::new(Vec::new()));
                let record = seen.clone();
                watch(&bus.connect(), move |touch| record.borrow_mut().push(touch));
                let pump = |until: Instant, done: &dyn Fn() -> bool| {
                    while !done() && Instant::now() < until {
                        context.iteration(false);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                };
                let wait_for = |expected: &[bool]| {
                    pump(Instant::now() + Duration::from_secs(5), &|| {
                        seen.borrow().as_slice() == expected
                    });
                    assert_eq!(seen.borrow().as_slice(), expected);
                };
                wait_for(&[true]);

                // 다른 연결이 보낸 같은 신호는 듣지 않는다. 들었다면 포인터 모드가 되었다가 뒤의 touch로 한 번 더
                // 바뀐다
                let emit = |connection: &gio::DBusConnection, name: &str| {
                    connection
                        .emit_signal(
                            None,
                            PATH,
                            INTERFACE,
                            "InputModeChanged",
                            Some(&(name,).to_variant()),
                        )
                        .unwrap();
                    connection.flush_sync(gio::Cancellable::NONE).unwrap();
                };
                emit(&bus.connect(), "pointer");
                emit(&compositor, "touch");
                // 키보드를 붙였다
                emit(&compositor, "pointer");
                wait_for(&[true, false]);
                // 늦게 오는 신호가 없는지 잠시 더 돌린다
                pump(Instant::now() + Duration::from_millis(200), &|| false);
                assert_eq!(seen.borrow().as_slice(), [true, false]);
            })
            .unwrap();
    }

    /// 설정 앱 스타일로 GTK가 실제로 배치한 크기를 잰다: 터치 모드에서 사이드바 줄, 묶음 상자 줄, 스위치의 누를
    /// 영역이 44px 이상이고, 포인터 모드는 그대로이며, 스위치는 그림과 줄 높이가 그대로인지. GTK는 처음 초기화한
    /// 스레드에서만 쓸 수 있어 따로 띄운 프로세스에서 돈다. 화면(Wayland·X11)이 없으면 건너뛴다. 화면 없는 곳에서는
    /// xvfb-run cargo test로 돌린다
    #[test]
    fn touch_mode_rows_and_switches_are_at_least_44px() {
        const INSIDE: &str = "BUNT_SETTINGS_TEST_GTK_PROCESS";
        const TEST: &str = "input_mode::tests::touch_mode_rows_and_switches_are_at_least_44px";
        if std::env::var_os(INSIDE).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(INSIDE, "1")
                // 화면 없는 시험 환경(Xvfb)에는 GL이 없을 수 있다
                .env("GSK_RENDERER", "cairo")
                .output()
                .unwrap();
            // 띄운 프로세스의 기록(GTK 경고 포함)은 실패할 때만 보인다
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
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
        // 터치 모드 규칙은 CSS 변수 없이 써서 어느 GTK에서나 읽힌다. 스타일 전체는 CSS 변수를 읽는 GTK 4.16부터
        // 오류가 없다(이미지는 4.24, theme/tokens.css)
        // STYLE에서는 style.css 뒤에 페이지별 CSS가 이어지므로 style.css 끝의 터치 모드 규칙만 자른다
        const SETTINGS_CSS: &str = include_str!("../data/style.css");
        let touch_rules = &SETTINGS_CSS[SETTINGS_CSS.find("/* 터치 모드(설계안 5.1절)").unwrap()..];
        let (_, errors) = parse(touch_rules);
        assert!(errors.is_empty(), "{errors:?}");
        let (provider, errors) = parse(crate::STYLE);
        if gtk::check_version(4, 16, 0).is_none() {
            assert!(errors.is_empty(), "{errors:?}");
        }
        gtk::style_context_add_provider_for_display(
            &gtk::gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let pointer = Sample::new(false);
        let touch = Sample::new(true);
        let context = glib::MainContext::default();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !(pointer.ready() && touch.ready()) {
            assert!(Instant::now() < deadline, "the windows did not open");
            context.iteration(false);
        }
        let settle = Instant::now() + Duration::from_millis(100);
        while Instant::now() < settle {
            context.iteration(false);
        }

        for (name, height) in [
            ("sidebar row", touch.height(&touch.sidebar_row)),
            ("row", touch.height(&touch.row)),
            ("switch row", touch.height(&touch.switch_row)),
            ("switch", touch.height(&touch.switch)),
        ] {
            assert!(height >= 44.0, "touch {name}: height {height}");
        }
        assert!(touch.width(&touch.switch) >= 44.0);
        // 포인터 모드는 그대로다
        for (name, height) in [
            ("sidebar row", pointer.height(&pointer.sidebar_row)),
            ("row", pointer.height(&pointer.row)),
            ("switch", pointer.height(&pointer.switch)),
        ] {
            assert!(height < 44.0, "pointer {name}: height {height}");
        }
        // 스위치는 그림(내용)이 그대로이고, 넓힌 누를 영역이 줄을 높이지 않는다
        assert_eq!(
            (pointer.switch.width(), pointer.switch.height()),
            (touch.switch.width(), touch.switch.height())
        );
        assert_eq!(touch.height(&touch.switch_row), touch.height(&touch.row));
        pointer.window.destroy();
        touch.window.destroy();
    }

    /// 사이드바 줄 하나와 묶음 상자 줄 둘(글만, 스위치). 클래스와 짜임은 설정 앱과 같다(window.rs의 sidebar_row,
    /// widgets.rs의 group, row_parts, switch_row)
    struct Sample {
        window: gtk::Window,
        sidebar_row: gtk::Widget,
        row: gtk::Widget,
        switch_row: gtk::Widget,
        switch: gtk::Widget,
    }

    impl Sample {
        fn new(touch: bool) -> Self {
            let window = gtk::Window::builder()
                .css_classes(["settings-window"])
                .default_width(700)
                .default_height(500)
                .build();
            style(window.upcast_ref(), touch);
            let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
            let sidebar = gtk::ListBox::builder()
                .css_classes(["sidebar-list"])
                .build();
            let sidebar_row = gtk::ListBoxRow::builder()
                .child(&gtk::Label::new(Some("Wi-Fi")))
                .build();
            sidebar.append(&sidebar_row);
            column.append(&sidebar);
            let group = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["settings-group"])
                .build();
            let line = |control: Option<&gtk::Widget>| {
                let content = gtk::Box::builder().spacing(10).build();
                let title = gtk::Label::builder()
                    .label("Wi-Fi")
                    .xalign(0.0)
                    .hexpand(true)
                    .css_classes(["row-title"])
                    .build();
                content.append(&title);
                let accessories = gtk::Box::builder()
                    .spacing(8)
                    .valign(gtk::Align::Center)
                    .build();
                if let Some(control) = control {
                    accessories.append(control);
                }
                content.append(&accessories);
                gtk::ListBoxRow::builder()
                    .child(&content)
                    .activatable(false)
                    .build()
            };
            let switch = gtk::Switch::builder().valign(gtk::Align::Center).build();
            let switch_row = line(Some(switch.upcast_ref()));
            let row = line(None);
            group.append(&switch_row);
            group.append(&row);
            column.append(&group);
            window.set_child(Some(&column));
            window.present();
            Self {
                window,
                sidebar_row: sidebar_row.upcast(),
                row: row.upcast(),
                switch_row: switch_row.upcast(),
                switch: switch.upcast(),
            }
        }

        fn ready(&self) -> bool {
            self.window.is_mapped() && self.switch.width() > 0
        }

        /// 누르는 자리(테두리 상자)의 크기
        fn width(&self, widget: &gtk::Widget) -> f32 {
            widget.compute_bounds(&self.window).unwrap().width()
        }

        fn height(&self, widget: &gtk::Widget) -> f32 {
            widget.compute_bounds(&self.window).unwrap().height()
        }
    }
}
