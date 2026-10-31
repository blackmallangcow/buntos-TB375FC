// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터의 셸 전용 화면 키보드 프로토콜 `bunt_osk_v1`의 클라이언트 쪽 (comp/protocols/bunt-osk-v1.xml, 서버 쪽은
//! comp/src/osk.rs). 셸(bunt-shell)과 로그인 화면(bunt-greeter)은 컴포지터가 셸로 띄운 특권 클라이언트라 이 전역을
//! 본다. 앱은 보지 못한다.
//!
//! - 키: 누른 키를 `press`로 보낸다. 컴포지터가 실제 키보드처럼 넣어 입력기(fcitx5)의 키보드 잡기를 거치므로 한글이
//!   조합된다. 요청은 보낸 차례로 처리되므로 답을 기다리지 않는다.
//! - 글 입력 상태: 셸만 받는다([`CompositorKeyboard::watch_text_input`]). 로그인 화면은 제 창의 글 칸 포커스를
//!   본다(embed.rs).
//! - GTK와 같은 Wayland 연결을 쓴다. 소켓은 GTK가 읽고, 이 객체의 이벤트는 따로 둔 큐에 쌓인다.
//!   [`CompositorKeyboard::take_text_input`]이 그 큐를 처리한다(shell/src/toplevels.rs와 같은 방법).
//! - 바꾼 점: 처음에는 세션 버스의 셸 전용 메서드(`PressOnScreenKeys`)로 보냈다. 같은 사용자의 프로그램이 세션
//!   버스를 엿볼 수 있어(BecomeMonitor, dbus-monitor) 화면 키보드로 친 암호가 보였다.

use std::{cell::RefCell, rc::Rc};

use gtk4::{gdk, prelude::*};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, protocol::wl_registry};

use crate::{KeySink, Output};

use protocol::bunt_osk_v1::{self, BuntOskV1};

/// bunt_osk_v1의 클라이언트 쪽 (wayland-protocols처럼 XML에서 만든다)
mod protocol {
    #![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
    #![allow(non_upper_case_globals, non_snake_case, unused_imports)]
    #![allow(missing_docs, clippy::all)]

    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        // 만든 코드가 wayland_backend::protocol을 부른다. wayland-client가 그 모듈을 다시 내보낸다
        use wayland_client::backend as wayland_backend;
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("../comp/protocols/bunt-osk-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("../comp/protocols/bunt-osk-v1.xml");

    // XML을 고치면 다시 빌드한다. proc macro가 읽은 파일은 cargo가 모른다
    const _: &str = include_str!("../../comp/protocols/bunt-osk-v1.xml");
}

/// 이 크레이트가 아는 판
const VERSION: u32 = 1;

/// 컴포지터의 화면 키보드. 복제해도 같은 연결을 가리킨다. 버릴 때 destroy를 보내지 않는다: 연결은 GTK의 것이라
/// 프로그램이 끝날 때 이미 닫혔을 수 있고, 연결이 끊기면 컴포지터가 객체를 치운다(toplevels.rs와 같다)
#[derive(Clone)]
pub struct CompositorKeyboard(Rc<Inner>);

struct Inner {
    connection: Connection,
    osk: BuntOskV1,
    queue: RefCell<EventQueue<Events>>,
    events: RefCell<Events>,
}

/// 이 연결의 큐가 처리한 것
#[derive(Default)]
struct Events {
    /// 레지스트리에서 찾은 전역
    osk: Option<BuntOskV1>,
    /// 받은 글 입력 상태 (받은 차례)
    text_input: Vec<bool>,
}

impl CompositorKeyboard {
    /// GTK의 Wayland 연결에서 화면 키보드 프로토콜을 찾는다. 컴포지터가 주지 않으면(컴포지터가 셸로 띄우지 않았다,
    /// Wayland가 아니다) None이다
    pub fn connect(display: &gdk::Display) -> Option<Self> {
        let wayland = display.downcast_ref::<gdk4_wayland::WaylandDisplay>()?;
        let wl_display = wayland.wl_display()?;
        Self::bind(Connection::from_backend(wl_display.backend().upgrade()?))
    }

    fn bind(connection: Connection) -> Option<Self> {
        let mut queue = connection.new_event_queue();
        let mut events = Events::default();
        connection.display().get_registry(&queue.handle(), ());
        queue.roundtrip(&mut events).ok()?;
        let osk = events.osk.clone()?;
        Some(Self(Rc::new(Inner {
            connection,
            osk,
            queue: RefCell::new(queue),
            events: RefCell::new(events),
        })))
    }

    /// 글 입력 상태를 받기 시작한다. 컴포지터는 지금 상태를 바로 보내고, 바뀔 때마다 다시 보낸다
    pub fn watch_text_input(&self) {
        self.0.osk.watch_text_input();
        self.flush();
    }

    /// GTK가 소켓에서 읽어 둔 이벤트를 처리하고, 그사이 받은 글 입력 상태를 받은 차례로 돌려준다
    pub fn take_text_input(&self) -> Vec<bool> {
        let mut events = self.0.events.borrow_mut();
        if let Err(err) = self.0.queue.borrow_mut().dispatch_pending(&mut events) {
            eprintln!("bunt-keyboard: the on-screen keyboard protocol: {err}");
        }
        std::mem::take(&mut events.text_input)
    }

    fn flush(&self) {
        if let Err(err) = self.0.connection.flush() {
            eprintln!("bunt-keyboard: the on-screen keyboard protocol: {err}");
        }
    }
}

impl KeySink for CompositorKeyboard {
    fn send(&self, output: Output) {
        self.0.osk.press(output.key_name());
        self.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Events {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _connection: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == BuntOskV1::interface().name
            && state.osk.is_none()
        {
            state.osk = Some(registry.bind(name, version.min(VERSION), handle, ()));
        }
    }
}

impl Dispatch<BuntOskV1, ()> for Events {
    fn event(
        state: &mut Self,
        _osk: &BuntOskV1,
        event: bunt_osk_v1::Event,
        _data: &(),
        _connection: &Connection,
        _handle: &QueueHandle<Self>,
    ) {
        let bunt_osk_v1::Event::TextInput { active } = event;
        state.text_input.push(active != 0);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        os::unix::net::UnixStream,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use wayland_server::{Client, DataInit, Display, DisplayHandle, GlobalDispatch, New};

    use super::*;

    /// 시험용 컴포지터의 bunt_osk_v1 (comp/src/osk.rs와 같은 XML)
    mod server {
        #![allow(dead_code, non_camel_case_types, unused_unsafe, unused_variables)]
        #![allow(non_upper_case_globals, non_snake_case, unused_imports)]
        #![allow(missing_docs, clippy::all)]

        use wayland_server;
        use wayland_server::protocol::*;

        pub mod __interfaces {
            use wayland_server::backend as wayland_backend;
            use wayland_server::protocol::__interfaces::*;
            wayland_scanner::generate_interfaces!("../comp/protocols/bunt-osk-v1.xml");
        }
        use self::__interfaces::*;

        wayland_scanner::generate_server_code!("../comp/protocols/bunt-osk-v1.xml");
    }

    use server::bunt_osk_v1::{self as served, BuntOskV1 as ServedOsk};

    /// 시험용 컴포지터: 받은 키를 적고, 시험이 시키면 글 입력 상태를 보낸다
    #[derive(Default)]
    struct Compositor {
        pressed: Arc<Mutex<Vec<String>>>,
        watchers: Vec<ServedOsk>,
    }

    impl GlobalDispatch<ServedOsk, ()> for Compositor {
        fn bind(
            _state: &mut Self,
            _display: &DisplayHandle,
            _client: &Client,
            resource: New<ServedOsk>,
            _global_data: &(),
            data_init: &mut DataInit<'_, Self>,
        ) {
            data_init.init(resource, ());
        }
    }

    impl wayland_server::Dispatch<ServedOsk, ()> for Compositor {
        fn request(
            state: &mut Self,
            _client: &Client,
            resource: &ServedOsk,
            request: served::Request,
            _data: &(),
            _display: &DisplayHandle,
            _data_init: &mut DataInit<'_, Self>,
        ) {
            match request {
                served::Request::Press { key } => state.pressed.lock().unwrap().push(key),
                served::Request::WatchTextInput => {
                    resource.text_input(0);
                    state.watchers.push(resource.clone());
                }
                served::Request::Destroy => {}
            }
        }
    }

    struct TestClient;

    impl wayland_server::backend::ClientData for TestClient {}

    /// 시험이 컴포지터에 시키는 것: 지켜보는 셸에 글 입력 상태를 보내고, 다 보냈으면 알린다
    type Announce = (bool, mpsc::Sender<()>);

    /// 다른 스레드에서 도는 시험용 컴포지터. 클라이언트의 roundtrip이 답을 받으려면 따로 돌아야 한다
    struct Server {
        pressed: Arc<Mutex<Vec<String>>>,
        announce: mpsc::Sender<Announce>,
        stop: Arc<AtomicBool>,
        thread: Option<thread::JoinHandle<()>>,
    }

    impl Server {
        /// `offer`면 화면 키보드 전역을 준다(셸에게). 아니면 앱에게처럼 주지 않는다
        fn start(stream: UnixStream, offer: bool) -> Self {
            let pressed = Arc::new(Mutex::new(Vec::new()));
            let (announce, announced) = mpsc::channel::<Announce>();
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let pressed = pressed.clone();
                let stop = stop.clone();
                thread::spawn(move || {
                    let mut display = Display::<Compositor>::new().unwrap();
                    let mut handle = display.handle();
                    if offer {
                        handle.create_global::<Compositor, ServedOsk, ()>(1, ());
                    }
                    handle.insert_client(stream, Arc::new(TestClient)).unwrap();
                    let mut state = Compositor {
                        pressed,
                        ..Compositor::default()
                    };
                    while !stop.load(Ordering::Relaxed) {
                        display.dispatch_clients(&mut state).unwrap();
                        while let Ok((active, done)) = announced.try_recv() {
                            for watcher in &state.watchers {
                                watcher.text_input(u32::from(active));
                            }
                            let _ = done.send(());
                        }
                        let _ = display.flush_clients();
                        thread::sleep(Duration::from_millis(1));
                    }
                })
            };
            Self {
                pressed,
                announce,
                stop,
                thread: Some(thread),
            }
        }

        /// 지켜보는 셸에 글 입력 상태를 보낸다. 보낸 뒤에 돌아온다
        fn announce(&self, active: bool) {
            let (done, sent) = mpsc::channel();
            self.announce.send((active, done)).unwrap();
            sent.recv_timeout(Duration::from_secs(10))
                .expect("the test compositor did not announce");
        }

        fn wait_for_keys(&self, count: usize) -> Vec<String> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let pressed = self.pressed.lock().unwrap().clone();
                if pressed.len() >= count {
                    return pressed;
                }
                assert!(Instant::now() < deadline, "only got {pressed:?}");
                thread::sleep(Duration::from_millis(2));
            }
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// 시험용 컴포지터에 붙은 연결. 이 기계에 libwayland-client가 없으면(실행 중에 연다) None이다
    fn connect(offer: bool) -> Option<(Server, Connection)> {
        let (ours, theirs) = UnixStream::pair().unwrap();
        let connection = match Connection::from_socket(ours) {
            Ok(connection) => connection,
            Err(err) => {
                eprintln!("no libwayland-client ({err}); skipping the protocol test");
                return None;
            }
        };
        Some((Server::start(theirs, offer), connection))
    }

    /// GTK가 소켓을 읽는 대신 시험이 읽는다: 컴포지터가 그때까지 보낸 이벤트를 받아 큐에 넣는다
    fn read_socket(keyboard: &CompositorKeyboard) {
        let inner = &keyboard.0;
        inner
            .queue
            .borrow_mut()
            .roundtrip(&mut inner.events.borrow_mut())
            .unwrap();
    }

    /// 셸·로그인 화면의 키가 컴포지터에 누른 차례로 닿고, 셸은 컴포지터가 알린 글 입력 상태를 받은 차례로 받는다
    #[test]
    fn keys_and_text_input_go_over_the_shell_protocol() {
        let Some((server, connection)) = connect(true) else {
            return;
        };
        let keyboard = CompositorKeyboard::bind(connection).expect("the shell sees the global");
        for output in [
            Output::Char('a'),
            Output::Char('B'),
            Output::Char(' '),
            Output::Delete,
            Output::Return,
        ] {
            keyboard.send(output);
        }
        assert_eq!(
            server.wait_for_keys(5),
            ["a", "B", " ", "BackSpace", "Return"]
        );

        // 지켜보기 전에는 글 입력 상태를 받지 않는다 (로그인 화면은 지켜보지 않는다)
        server.announce(true);
        read_socket(&keyboard);
        assert!(keyboard.take_text_input().is_empty());
        // 지켜보면 지금 상태가 바로 오고, 그 뒤로 바뀐 것이 차례로 온다
        keyboard.watch_text_input();
        read_socket(&keyboard);
        server.announce(true);
        server.announce(false);
        server.announce(true);
        read_socket(&keyboard);
        assert_eq!(keyboard.take_text_input(), [false, true, false, true]);
        assert!(keyboard.take_text_input().is_empty());
    }

    /// 컴포지터가 전역을 주지 않으면(앱, 다른 컴포지터) 쓰지 못한다. 로그인 화면은 그때 제 칸에 바로 넣는다
    #[test]
    fn without_the_global_there_is_no_compositor_keyboard() {
        let Some((_server, connection)) = connect(false) else {
            return;
        };
        assert!(CompositorKeyboard::bind(connection).is_none());
    }
}
