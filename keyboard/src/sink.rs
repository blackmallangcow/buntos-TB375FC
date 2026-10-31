// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 키보드가 누른 키를 보내는 곳.
//!
//! - [`CompositorKeys`]: 컴포지터의 셸 전용 메서드 `PressOnScreenKeys`로 실제 키보드처럼 넣는다(셸, 로그인 화면).
//!   입력기(fcitx5)의 키보드 잡기를 거쳐 한글이 조합된다(comp/src/osk.rs). zbus는 요청마다 작업을 띄워 차례가 바뀔
//!   수 있으므로 앞 요청의 답을 받은 뒤에 다음 키를 보낸다. 그사이 누른 키는 모아 한 번에 보낸다. 로그인 화면은
//!   컴포지터에 닿지 못하면(세션 버스가 없다 등) 제 창의 글 칸에 바로 넣는다([`CompositorKeys::with_fallback`]).
//! - [`EditableKeys`]: 그 창의 포커스를 가진 글 칸에 바로 넣는다(잠금 화면). 잠긴 동안 컴포지터는 셸의 키를 받지
//!   않고, 잠금 화면 밖의 프로그램이 잠금 화면에 키를 넣는 길도 열지 않는다. 암호 칸에는 한글을 조합하지 않는다.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gtk4::{self as gtk, gio, glib, prelude::*};

use crate::board::Output;

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
/// 한 번에 보내는 키 수 (comp/src/osk.rs의 MAX_KEYS)
const MAX_KEYS: usize = 64;
/// 컴포지터가 답하기를 기다리는 시간 (ms). 컴포지터는 2초 안에 답한다
const CALL_TIMEOUT: i32 = 2500;
/// 답을 기다리는 동안 모아 두는 키의 상한. 컴포지터가 멈춘 동안 지우기를 누르고 있어도 끝없이 쌓지 않는다. 넘친 키는 버린다
const MAX_PENDING: usize = 256;

/// 화면 키보드가 누른 키를 받는 곳
pub trait KeySink {
    fn send(&self, output: Output);
}

/// 컴포지터에 키를 차례로 보낸다
#[derive(Clone, Default)]
pub struct CompositorKeys(Rc<Queue>);

#[derive(Default)]
struct Queue {
    /// 아직 보내지 않은 키
    pending: RefCell<Vec<Output>>,
    /// 앞 요청의 답을 기다리는 중이다
    busy: Cell<bool>,
    bus: RefCell<Option<gio::DBusConnection>>,
    /// 컴포지터에 닿지 못한 키를 대신 넣는 곳
    fallback: Option<Rc<dyn KeySink>>,
}

impl CompositorKeys {
    pub fn new() -> Self {
        Self::default()
    }

    /// 컴포지터에 닿지 못한 키(세션 버스나 컴포지터의 D-Bus가 없거나, 그 메서드를 모르거나, 셸이 아니라고 거절했다)를
    /// `fallback`에 넣는다. 로그인 화면은 그래도 암호를 칠 수 있어야 한다. 답이 늦은 것은 컴포지터가 넣었을 수 있으므로
    /// 다시 넣지 않는다
    pub fn with_fallback(fallback: Rc<dyn KeySink>) -> Self {
        Self(Rc::new(Queue {
            fallback: Some(fallback),
            ..Queue::default()
        }))
    }
}

impl KeySink for CompositorKeys {
    fn send(&self, output: Output) {
        {
            let mut pending = self.0.pending.borrow_mut();
            if pending.len() >= MAX_PENDING {
                eprintln!("bunt-keyboard: the compositor is not taking keys; dropping {output:?}");
                return;
            }
            pending.push(output);
        }
        flush(&self.0);
    }
}

/// 키를 넣지 못한 까닭
enum Failure {
    /// 세션 버스에 붙지 못했다
    NoBus(glib::Error),
    Call(glib::Error),
}

impl Failure {
    /// 컴포지터가 키를 넣지 않은 것이 확실하다
    fn unreached(&self) -> bool {
        match self {
            Failure::NoBus(_) => true,
            Failure::Call(err) => [
                gio::DBusError::ServiceUnknown,
                gio::DBusError::NameHasNoOwner,
                gio::DBusError::UnknownMethod,
                gio::DBusError::UnknownInterface,
                gio::DBusError::UnknownObject,
                gio::DBusError::AccessDenied,
                gio::DBusError::Disconnected,
            ]
            .into_iter()
            .any(|code| err.matches(code)),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::NoBus(err) => write!(f, "no session bus: {err}"),
            Failure::Call(err) => write!(f, "{err}"),
        }
    }
}

/// 기다리는 요청이 없으면 모인 키를 보낸다. 답을 받으면 그사이 모인 키를 보낸다
fn flush(queue: &Rc<Queue>) {
    if queue.busy.get() {
        return;
    }
    let keys = take_batch(&mut queue.pending.borrow_mut());
    if keys.is_empty() {
        return;
    }
    queue.busy.set(true);
    let queue = queue.clone();
    glib::spawn_future_local(async move {
        if let Err(failure) = press(&queue, &keys).await {
            eprintln!("bunt-keyboard: {INTERFACE}.PressOnScreenKeys: {failure}");
            if let Some(fallback) = queue.fallback.as_ref().filter(|_| failure.unreached()) {
                for key in keys {
                    fallback.send(key);
                }
            }
        }
        queue.busy.set(false);
        flush(&queue);
    });
}

/// 앞에서부터 한 번에 보낼 만큼 꺼낸다
fn take_batch<T>(pending: &mut Vec<T>) -> Vec<T> {
    let count = pending.len().min(MAX_KEYS);
    pending.drain(..count).collect()
}

async fn press(queue: &Queue, keys: &[Output]) -> Result<(), Failure> {
    let cached = queue.bus.borrow().clone().filter(|bus| !bus.is_closed());
    let bus = match cached {
        Some(bus) => bus,
        None => {
            let bus = gio::bus_get_future(gio::BusType::Session)
                .await
                .map_err(Failure::NoBus)?;
            queue.bus.replace(Some(bus.clone()));
            bus
        }
    };
    let names: Vec<String> = keys.iter().map(|key| key.key_name()).collect();
    bus.call_future(
        Some(NAME),
        PATH,
        INTERFACE,
        "PressOnScreenKeys",
        Some(&(names,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        CALL_TIMEOUT,
    )
    .await
    .map(|_| ())
    .map_err(Failure::Call)
}

/// 창의 포커스를 가진 글 칸에 바로 넣는다 (잠금 화면)
pub struct EditableKeys {
    window: glib::WeakRef<gtk::Window>,
}

impl EditableKeys {
    pub fn new(window: &impl IsA<gtk::Window>) -> Self {
        Self {
            window: window.upcast_ref::<gtk::Window>().downgrade(),
        }
    }
}

impl KeySink for EditableKeys {
    fn send(&self, output: Output) {
        let Some(focus) = self
            .window
            .upgrade()
            .and_then(|window| GtkWindowExt::focus(&window))
        else {
            return;
        };
        // 포커스는 글 칸 안의 GtkText에 있다. 막아 둔 칸(암호를 확인하는 동안, 기다리는 동안)에는 넣지 않는다
        let Ok(editable) = focus.clone().dynamic_cast::<gtk::Editable>() else {
            return;
        };
        if !editable.is_editable() || !focus.is_sensitive() {
            return;
        }
        match output {
            Output::Char(character) => {
                editable.delete_selection();
                let mut position = editable.position();
                editable.insert_text(&character.to_string(), &mut position);
                editable.set_position(position);
            }
            Output::Delete => {
                if editable.selection_bounds().is_some() {
                    editable.delete_selection();
                } else {
                    let position = editable.position();
                    if position > 0 {
                        editable.delete_text(position - 1, position);
                    }
                }
            }
            // 글 칸의 기본 동작 (잠금 화면: 암호 확인). GtkText는 위젯의 기본 동작 신호를 정해 두지 않아
            // gtk_widget_activate가 아무것도 하지 않는다. Return 키가 보내는 그 칸의 activate 신호를 보낸다. 글자 칸과
            // 암호 칸은 이것을 제 activate로 옮긴다
            Output::Return => {
                if focus.is::<gtk::Text>() {
                    focus.emit_by_name::<()>("activate", &[]);
                } else {
                    focus.activate();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader},
        path::PathBuf,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    use super::*;

    /// 시험 하나가 쓰는 버스 데몬 (shell/src/dbus.rs의 시험과 같다). dbus-daemon이 없으면 None이다
    struct Bus {
        daemon: Child,
        dir: PathBuf,
        address: String,
    }

    impl Bus {
        fn start(name: &str) -> Option<Self> {
            let dir = std::env::temp_dir()
                .join(format!("bunt-keyboard-test-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let spawned = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address"])
                .arg(format!("--address=unix:dir={}", dir.display()))
                .stdout(Stdio::piped())
                .spawn();
            let mut daemon = match spawned {
                Ok(daemon) => daemon,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    let _ = std::fs::remove_dir_all(&dir);
                    return None;
                }
                Err(err) => panic!("dbus-daemon: {err}"),
            };
            let mut address = String::new();
            if let Some(stdout) = daemon.stdout.take() {
                let _ = BufReader::new(stdout).read_line(&mut address);
            }
            Some(Self {
                daemon,
                dir,
                address: address.trim().to_owned(),
            })
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
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// 가짜 컴포지터: PressOnScreenKeys로 받은 묶음을 적는다. `refuse`면 셸이 아니라고 거절한다(AccessDenied)
    fn fake_compositor(
        bus: &Bus,
        refuse: bool,
    ) -> (gio::DBusConnection, Rc<RefCell<Vec<Vec<String>>>>) {
        let connection = bus.connect();
        connection
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
        let node = gio::DBusNodeInfo::for_xml(&format!(
            "<node><interface name='{INTERFACE}'><method name='PressOnScreenKeys'>\
             <arg type='as' direction='in'/></method></interface></node>"
        ))
        .unwrap();
        let batches = Rc::new(RefCell::new(Vec::new()));
        let seen = batches.clone();
        let _ = connection
            .register_object(PATH, &node.interfaces()[0])
            .method_call(move |_, _, _, _, _, parameters, invocation| {
                if refuse {
                    invocation.return_dbus_error(
                        "org.freedesktop.DBus.Error.AccessDenied",
                        "only the shell started by the compositor may call this",
                    );
                    return;
                }
                let (keys,): (Vec<String>,) = parameters.get().unwrap();
                seen.borrow_mut().push(keys);
                invocation.return_value(None);
            })
            .build()
            .unwrap();
        (connection, batches)
    }

    /// 받은 키를 적는 대신 넣는 곳 (로그인 화면의 글 칸)
    struct Record(Rc<RefCell<Vec<Output>>>);

    impl KeySink for Record {
        fn send(&self, output: Output) {
            self.0.borrow_mut().push(output);
        }
    }

    fn pump_until(context: &glib::MainContext, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "timed out");
            if !context.iteration(false) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// 셸·로그인 화면의 키가 컴포지터의 PressOnScreenKeys에 누른 차례로 닿는다. 답을 기다리는 동안 누른 키는 다음 요청에
    /// 모아 보낸다
    #[test]
    fn keys_reach_the_compositor_in_order() {
        let Some(bus) = Bus::start("order") else {
            eprintln!("dbus-daemon is not installed; skipping the compositor keys test");
            return;
        };
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (_service, batches) = fake_compositor(&bus, false);
                let keys = CompositorKeys::new();
                keys.0.bus.replace(Some(bus.connect()));
                for output in [
                    Output::Char('a'),
                    Output::Char('B'),
                    Output::Char(' '),
                    Output::Delete,
                    Output::Return,
                ] {
                    keys.send(output);
                }
                pump_until(&context, || {
                    batches.borrow().iter().map(Vec::len).sum::<usize>() == 5
                });
                // 첫 키는 바로 가고, 답을 기다리는 동안 누른 키는 한 요청에 모여 간다
                assert_eq!(
                    *batches.borrow(),
                    [
                        vec!["a".to_owned()],
                        ["B", " ", "BackSpace", "Return"].map(String::from).to_vec()
                    ]
                );
                pump_until(&context, || !keys.0.busy.get());
            })
            .unwrap();
    }

    /// 컴포지터에 닿지 못한 키(컴포지터가 버스에 없다, 셸이 아니라고 거절했다)는 대신 넣는 곳에 차례대로 넣는다
    #[test]
    fn keys_fall_back_when_the_compositor_does_not_take_them() {
        let Some(bus) = Bus::start("fallback") else {
            eprintln!("dbus-daemon is not installed; skipping the fallback test");
            return;
        };
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                for refuse in [None, Some(true)] {
                    // None: 컴포지터가 버스에 없다
                    let service = refuse.map(|refuse| fake_compositor(&bus, refuse));
                    let typed = Rc::new(RefCell::new(Vec::new()));
                    let keys = CompositorKeys::with_fallback(Rc::new(Record(typed.clone())));
                    keys.0.bus.replace(Some(bus.connect()));
                    keys.send(Output::Char('q'));
                    keys.send(Output::Delete);
                    keys.send(Output::Return);
                    pump_until(&context, || typed.borrow().len() == 3);
                    assert_eq!(
                        *typed.borrow(),
                        [Output::Char('q'), Output::Delete, Output::Return]
                    );
                    pump_until(&context, || !keys.0.busy.get());
                    drop(service);
                }
            })
            .unwrap();
    }

    #[test]
    fn keys_go_out_in_order_and_in_bounded_batches() {
        let mut pending: Vec<String> = (0..MAX_KEYS + 3).map(|index| index.to_string()).collect();
        let first = take_batch(&mut pending);
        assert_eq!(first.len(), MAX_KEYS);
        assert_eq!(first[0], "0");
        assert_eq!(pending, ["64", "65", "66"]);
        assert_eq!(take_batch(&mut pending), ["64", "65", "66"]);
        assert!(take_batch(&mut pending).is_empty());
    }

    /// 컴포지터가 답하지 않는 동안 누른 키는 상한까지만 모은다
    #[test]
    fn keys_waiting_for_a_busy_compositor_are_bounded() {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let keys = CompositorKeys::new();
                for _ in 0..MAX_PENDING + 10 {
                    keys.send(Output::Delete);
                }
                // 첫 키는 보내는 중(답을 기다린다. 이 시험은 루프를 돌리지 않는다)이고, 나머지는 상한까지만 모인다
                assert!(keys.0.busy.get());
                assert_eq!(keys.0.pending.borrow().len(), MAX_PENDING);
            })
            .unwrap();
    }

    /// 컴포지터에 닿지 못한 키만 대신 넣는다. 답이 늦거나 키를 거절한 것(넣었을 수 있거나, 넣으면 안 된다)은 넣지 않는다
    #[test]
    fn only_keys_that_never_reached_the_compositor_fall_back() {
        let error = |code: gio::DBusError| Failure::Call(glib::Error::new(code, "test"));
        assert!(Failure::NoBus(glib::Error::new(gio::IOErrorEnum::NotFound, "no bus")).unreached());
        assert!(error(gio::DBusError::ServiceUnknown).unreached());
        assert!(error(gio::DBusError::UnknownMethod).unreached());
        assert!(error(gio::DBusError::AccessDenied).unreached());
        assert!(!error(gio::DBusError::NoReply).unreached());
        assert!(!error(gio::DBusError::Timeout).unreached());
        assert!(!error(gio::DBusError::InvalidArgs).unreached());
    }
}
