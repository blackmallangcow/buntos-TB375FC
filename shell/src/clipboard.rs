// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 클립보드 기록 서비스: 새 복사를 받아 기록(clipboard_history.rs)에 남기고, 고른 항목을 클립보드에 다시 내놓는다.
//! 화면은 모든 앱 패널의 클립보드 보기(clipboard_panel.rs)다. macOS 26 Spotlight의 클립보드 기록을 따른다.
//!
//! - 통로: 컴포지터가 여는 ext_data_control_v1 (comp/src/clipboard.rs). 포커스 없이 복사를 알리고 읽는다.
//! - 켜기: macOS처럼 처음 쓸 때 사람이 허용해야 켜진다(org.buntos.shell clipboard-history). 꺼져 있으면 컴포지터에 붙지도
//!   않아 복사를 보지 않는다. 끄면 연결을 끊고 기록을 지운다.
//! - 연결: GTK의 연결과 따로 둔다. GTK의 연결에 얹으면 GTK가 읽어 둔 이벤트를 처리할 때를 알 수 없어 창 목록(toplevels.rs)처럼
//!   주기적으로 깨어야 한다. 따로 두면 복사가 없을 때 깨지 않는다.
//! - 잠긴 동안에는 복사를 남기지 않고, 읽던 복사도 버린다. 패널은 잠그면 닫힌다.
//! - 기록을 읽는 길은 이 프로세스의 패널뿐이다. D-Bus에도, GAction에도 내놓지 않는다. 패널은 에이전트가 읽거나 누르지 못하는 창이고
//!   글이 접근성 트리에 들지 않는다(clipboard_panel.rs).
//! - 복사의 내용을 기록(로그)에 적지 않는다.

use std::{
    cell::{Cell, RefCell},
    io::pipe,
    os::fd::{AsFd, OwnedFd},
    rc::{Rc, Weak},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use gtk4::{gio, glib, prelude::*};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child,
    protocol::{wl_registry, wl_seat::WlSeat},
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

use crate::clipboard_history::{
    Decision, History, Item, Kind, NewItem, OWN_MIME, decide, keep_for,
};

/// org.buntos.shell 키: 켜고 끄기, 보관 기간, 시스템 설정의 지우기 단추(늘리면 지운다)
const ENABLED_KEY: &str = "clipboard-history";
const PERIOD_KEY: &str = "clipboard-history-period";
const CLEAR_KEY: &str = "clipboard-history-clear";

const COMPOSITOR: &str = "org.buntos.Compositor";
const COMPOSITOR_PATH: &str = "/org/buntos/Compositor";
const COMPOSITOR_INTERFACE: &str = "org.buntos.Compositor1";

/// 한 번에 읽는 크기
const READ_CHUNK: usize = 64 * 1024;
/// 한 번에 쓰는 크기: 파이프에 원자적으로 쓰는 크기(PIPE_BUF). GIO의 비동기 쓰기는 쓸 수 있는지 본 뒤 쓰므로, 이보다 크게 쓰면
/// 읽는 쪽이 늦을 때 셸이 멈출 수 있다
const WRITE_CHUNK: usize = 4096;
/// 앱이 이 시간 안에 복사를 내주지 않거나 앱이 붙여넣기를 다 읽지 않으면 그만둔다
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
/// 보관 기간을 다시 보는 가장 긴 간격. 타이머는 잠자는 동안 멈추므로, 깨어난 뒤 오래 기다리지 않게 한다
const EXPIRY_RECHECK: Duration = Duration::from_secs(600);

type Listener = Box<dyn Fn()>;

/// 복제해도 같은 기록을 가리킨다
#[derive(Clone)]
pub struct ClipboardHistory(Rc<Inner>);

struct Inner {
    settings: Option<gio::Settings>,
    history: RefCell<History>,
    link: RefCell<Option<Rc<Link>>>,
    locked: Cell<bool>,
    /// 켜고 끄거나 잠그거나 지울 때마다 는다. 그 전에 읽기 시작한 복사는 남기지 않는다
    epoch: Cell<u64>,
    /// 선택이 바뀔 때마다 는다. 읽는 중에 다음 복사가 오면 앞의 앱이 내주기를 그만두므로 잘린 내용을 남기지 않는다
    selections: Cell<u64>,
    listeners: RefCell<Vec<Listener>>,
    expiry: RefCell<Option<glib::SourceId>>,
}

impl ClipboardHistory {
    pub fn start() -> Self {
        let settings = crate::shell_settings().filter(|settings| {
            settings
                .settings_schema()
                .is_some_and(|schema| schema.has_key(ENABLED_KEY))
        });
        let history = Self(Rc::new(Inner {
            settings,
            history: RefCell::default(),
            link: RefCell::default(),
            locked: Cell::new(false),
            epoch: Cell::new(0),
            selections: Cell::new(0),
            listeners: RefCell::default(),
            expiry: RefCell::default(),
        }));
        if let Some(settings) = &history.0.settings {
            // 변경 알림은 설정 객체가 살아 있을 때만 온다. Inner가 셸이 끝날 때까지 객체를 붙든다
            let weak = Rc::downgrade(&history.0);
            settings.connect_changed(Some(ENABLED_KEY), move |_, _| {
                with(&weak, |history| history.follow_setting());
            });
            let weak = Rc::downgrade(&history.0);
            settings.connect_changed(Some(PERIOD_KEY), move |_, _| {
                with(&weak, |history| history.expire());
            });
            let weak = Rc::downgrade(&history.0);
            settings.connect_changed(Some(CLEAR_KEY), move |_, _| {
                with(&weak, |history| history.clear());
            });
            // 변경 알림은 처리기를 단 뒤 한 번 읽어야 온다. 켜고 끄는 키는 follow_setting이 읽는다
            settings.string(PERIOD_KEY);
            settings.uint(CLEAR_KEY);
        }
        history.follow_lock();
        history.follow_setting();
        history
    }

    /// 사람이 허용했는지 (시스템 설정의 스위치, 패널의 허용 단추)
    pub fn enabled(&self) -> bool {
        self.0
            .settings
            .as_ref()
            .is_some_and(|settings| settings.boolean(ENABLED_KEY))
    }

    /// 켜졌는데 컴포지터가 프로토콜을 주지 않아 쓸 수 없는지 (컴포지터가 없는 개발 환경 등). 설정 스키마가 없어도 그렇다
    pub fn unavailable(&self) -> bool {
        self.0.settings.is_none()
            || (self.enabled() && !self.0.locked.get() && self.0.link.borrow().is_none())
    }

    pub fn allow(&self) {
        if let Some(settings) = &self.0.settings
            && let Err(err) = settings.set_boolean(ENABLED_KEY, true)
        {
            eprintln!("bunt-shell: {ENABLED_KEY}: {err}");
        }
    }

    pub fn locked(&self) -> bool {
        self.0.locked.get()
    }

    /// 기록, 최근 것이 먼저. 보관 기간이 지난 것은 뺀다. 잠자는 동안은 타이머가 돌지 않아 지난 항목이 남아 있을 수 있다
    pub fn items(&self) -> Vec<Rc<Item>> {
        self.expire();
        self.0.history.borrow().items().to_vec()
    }

    /// 기록이나 잠긴 상태가 바뀔 때마다 부른다
    pub fn subscribe(&self, listener: impl Fn() + 'static) {
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    /// 고른 항목을 클립보드에 넣는다. 지금 복사한 것으로 쳐 맨 앞으로 온다
    pub fn pick(&self, id: u64) {
        let Some(item) = self.0.history.borrow().get(id) else {
            return;
        };
        let Some(link) = self.0.link.borrow().clone() else {
            return;
        };
        link.offer(item);
        self.0.history.borrow_mut().touch(id, SystemTime::now());
        self.changed();
    }

    pub fn clear(&self) {
        self.0.epoch.set(self.0.epoch.get() + 1);
        self.0.history.borrow_mut().clear();
        self.changed();
    }

    fn changed(&self) {
        self.arm_expiry();
        for listener in self.0.listeners.borrow().iter() {
            listener();
        }
    }

    /// 켜고 끄는 설정을 따른다. 켜졌는데 연결이 없으면 붙고, 꺼졌으면 끊고 지운다
    fn follow_setting(&self) {
        if self.enabled() {
            self.connect();
            self.changed();
        } else {
            self.0.link.replace(None);
            self.clear();
        }
    }

    fn connect(&self) {
        // 잠긴 동안에는 컴포지터가 새 연결에 프로토콜을 보이지 않는다. 풀리면 다시 온다
        if self.0.link.borrow().is_some() || self.0.locked.get() {
            return;
        }
        let weak = Rc::downgrade(&self.0);
        match Link::connect(weak) {
            // 컴포지터가 곧 지금 클립보드의 내용을 알린다. 허용하기 바로 전에 복사한 것이 기록의 첫 항목이 된다
            Ok(link) => {
                self.0.link.replace(Some(link));
            }
            Err(err) => eprintln!("bunt-shell: clipboard history is unavailable: {err}"),
        }
    }

    /// Wayland 소켓에 읽을 것이 있다. 연결이 끊겼으면 false
    fn pump(&self) -> bool {
        let Some(link) = self.0.link.borrow().clone() else {
            return false;
        };
        match link.pump() {
            Ok(events) => {
                for event in events {
                    self.handle(&link, event);
                }
                true
            }
            Err(err) => {
                eprintln!("bunt-shell: clipboard history: {err}");
                self.0.link.replace(None);
                self.changed();
                false
            }
        }
    }

    fn handle(&self, link: &Link, event: Event) {
        match event {
            Event::Selection(selection) => {
                self.0.selections.set(self.0.selections.get() + 1);
                if let Some((offer, mimes)) = selection
                    && !self.0.locked.get()
                    && let Decision::Read { mime, kind } = decide(&mimes)
                {
                    self.receive(link, &offer, mime, kind);
                }
            }
            Event::Paste { fd, data } => {
                glib::spawn_future_local(write_all(fd, data));
            }
        }
    }

    /// 앱에 복사의 내용을 청한다. 읽는 동안 셸은 멈추지 않는다
    fn receive(&self, link: &Link, offer: &ExtDataControlOfferV1, mime: String, kind: Kind) {
        let Ok((reader, writer)) = pipe() else {
            return;
        };
        offer.receive(mime, writer.as_fd());
        // 파이프의 쓰는 쪽은 앱에 넘기고 우리 것은 닫아야 앱이 다 쓴 때 끝을 안다
        link.flush();
        drop(writer);
        let copied_at = SystemTime::now();
        let (epoch, selection) = (self.0.epoch.get(), self.0.selections.get());
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let Some(data) = read_all(reader.into(), kind.max_bytes()).await else {
                return;
            };
            if !weak
                .upgrade()
                .is_some_and(|inner| inner.selections.get() == selection)
            {
                return;
            }
            let Some(item) = make_item(kind, data).await else {
                return;
            };
            if let Some(inner) = weak.upgrade()
                && inner.epoch.get() == epoch
                && !inner.locked.get()
            {
                inner.history.borrow_mut().add(item, copied_at);
                Self(inner).changed();
            }
        });
    }

    fn keep(&self) -> Duration {
        let period = self.0.settings.as_ref().map(|s| s.string(PERIOD_KEY));
        keep_for(period.as_deref().unwrap_or_default())
    }

    /// 보관 기간이 지난 항목을 버린다
    fn expire(&self) {
        let expired = self
            .0
            .history
            .borrow_mut()
            .expire(self.keep(), SystemTime::now());
        if expired {
            self.changed();
        } else {
            self.arm_expiry();
        }
    }

    /// 다음으로 항목이 지워질 때 깨어나게 한다. 항목이 없으면 깨지 않는다
    fn arm_expiry(&self) {
        if let Some(armed) = self.0.expiry.take() {
            armed.remove();
        }
        let Some(at) = self.0.history.borrow().next_expiry(self.keep()) else {
            return;
        };
        let wait = at
            .duration_since(SystemTime::now())
            .unwrap_or_default()
            .min(EXPIRY_RECHECK);
        let weak = Rc::downgrade(&self.0);
        // 초 단위 타이머는 다른 일과 함께 깨어 전력을 아낀다. 1초를 더해 지난 뒤에 깬다
        let armed = glib::timeout_add_seconds_local_once(wait.as_secs() as u32 + 1, move || {
            if let Some(inner) = weak.upgrade() {
                // 다 돈 타이머를 지우면 GLib이 경고한다
                inner.expiry.take();
                Self(inner).expire();
            }
        });
        self.0.expiry.replace(Some(armed));
    }

    /// 화면 잠금을 따른다 (comp/src/session_lock.rs)
    fn follow_lock(&self) {
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else {
                return;
            };
            let signal_target = weak.clone();
            let subscription = bus.subscribe_to_signal(
                Some(COMPOSITOR),
                Some(COMPOSITOR_INTERFACE),
                Some("LockedChanged"),
                Some(COMPOSITOR_PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if let Some((locked,)) = signal.parameters.get::<(bool,)>() {
                        with(&signal_target, |history| history.set_locked(locked));
                    }
                },
            );
            // 셸이 끝날 때까지 받는다
            std::mem::forget(subscription);
            // 신호를 받기 시작하기 전에 잠겼을 수 있다
            let answer = bus
                .call_future(
                    Some(COMPOSITOR),
                    COMPOSITOR_PATH,
                    COMPOSITOR_INTERFACE,
                    "GetLocked",
                    None,
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            if let Some((locked,)) = answer.ok().and_then(|answer| answer.get::<(bool,)>()) {
                with(&weak, |history| history.set_locked(locked));
            }
        });
    }

    fn set_locked(&self, locked: bool) {
        if self.0.locked.replace(locked) == locked {
            return;
        }
        if locked {
            self.0.epoch.set(self.0.epoch.get() + 1);
        } else if self.enabled() {
            self.connect();
        }
        self.changed();
    }
}

fn with(weak: &Weak<Inner>, run: impl FnOnce(ClipboardHistory)) {
    if let Some(inner) = weak.upgrade() {
        run(ClipboardHistory(inner));
    }
}

/// 읽은 바이트로 항목을 만든다. 그림은 풀어야 하므로 따로 도는 스레드에서 만든다
async fn make_item(kind: Kind, data: Vec<u8>) -> Option<NewItem> {
    match kind {
        Kind::Text => NewItem::text(data),
        Kind::Files => NewItem::files(data),
        Kind::Image => gio::spawn_blocking(move || NewItem::image(data))
            .await
            .ok()
            .flatten(),
    }
}

/// 파이프를 끝까지 읽는다. 상한을 넘거나 제한 시간이 지나면 None이다
async fn read_all(fd: OwnedFd, limit: usize) -> Option<Vec<u8>> {
    let stream = gio_unix::InputStream::take_fd(fd);
    let read = async {
        let mut data = Vec::new();
        loop {
            let chunk = stream
                .read_bytes_future(READ_CHUNK, glib::Priority::DEFAULT)
                .await
                .ok()?;
            if chunk.is_empty() {
                return Some(data);
            }
            if data.len() + chunk.len() > limit {
                return None;
            }
            data.extend_from_slice(&chunk);
        }
    };
    glib::future_with_timeout(TRANSFER_TIMEOUT, read)
        .await
        .ok()
        .flatten()
}

/// 붙여 넣는 앱에 내용을 쓴다. 파이프를 닫으면 끝이다
async fn write_all(fd: OwnedFd, data: glib::Bytes) {
    let stream = gio_unix::OutputStream::take_fd(fd);
    let write = async {
        let mut offset = 0;
        while offset < data.len() {
            let end = (offset + WRITE_CHUNK).min(data.len());
            let chunk = glib::Bytes::from_bytes(&data, offset..end);
            match stream
                .write_bytes_future(&chunk, glib::Priority::DEFAULT)
                .await
            {
                Ok(written) if written > 0 => offset += written as usize,
                _ => return,
            }
        }
    };
    // 앱이 읽다 말면 그만둔다. 스트림을 버리면 파이프가 닫힌다
    let _ = glib::future_with_timeout(TRANSFER_TIMEOUT, write).await;
}

/// 컴포지터에서 온 일. 큐를 빌린 동안에는 다른 일을 시작하지 않으려고 모아서 처리한다
enum Event {
    /// 선택이 바뀌었다: 새 제안과 그 형식, 또는 비었다
    Selection(Option<(ExtDataControlOfferV1, Vec<String>)>),
    Paste {
        fd: OwnedFd,
        data: glib::Bytes,
    },
}

/// 컴포지터 연결. 버리면 소켓이 닫혀 컴포지터가 이 연결의 객체를 모두 없앤다
struct Link {
    connection: Connection,
    queue: RefCell<EventQueue<LinkState>>,
    state: RefCell<LinkState>,
    /// 소켓을 지켜보는 소스
    watch: glib::Source,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.watch.destroy();
    }
}

impl Link {
    fn connect(owner: Weak<Inner>) -> Result<Rc<Self>, String> {
        let connection = Connection::connect_to_env().map_err(|err| err.to_string())?;
        let mut queue = connection.new_event_queue();
        let mut state = LinkState::default();
        let handle = queue.handle();
        connection.display().get_registry(&handle, ());
        queue.roundtrip(&mut state).map_err(|err| err.to_string())?;
        let (Some(manager), Some(seat)) = (state.manager.as_ref(), state.seat.as_ref()) else {
            return Err("the compositor does not offer ext_data_control_v1".into());
        };
        state.device = Some(manager.get_data_device(seat, &handle, ()));
        queue.flush().map_err(|err| err.to_string())?;

        // 소켓을 GLib에 맡기려고 복제한다. 복제본은 지켜보기만 하고 읽지 않는다
        let fd = connection
            .backend()
            .poll_fd()
            .try_clone_to_owned()
            .map_err(|err| err.to_string())?;
        let stream = gio_unix::InputStream::take_fd(fd);
        let watch = stream.create_source(
            gio::Cancellable::NONE,
            Some("bunt-clipboard-wayland"),
            glib::Priority::DEFAULT,
            move |_| {
                let alive = owner
                    .upgrade()
                    .is_some_and(|inner| ClipboardHistory(inner).pump());
                if alive {
                    glib::ControlFlow::Continue
                } else {
                    glib::ControlFlow::Break
                }
            },
        );
        watch.attach(None);
        Ok(Rc::new(Self {
            connection,
            queue: RefCell::new(queue),
            state: RefCell::new(state),
            watch,
        }))
    }

    /// 소켓을 읽고 이벤트를 처리해 일을 돌려준다
    fn pump(&self) -> Result<Vec<Event>, String> {
        let mut queue = self.queue.borrow_mut();
        let mut state = self.state.borrow_mut();
        if let Some(guard) = queue.prepare_read() {
            guard.read().map_err(|err| err.to_string())?;
        }
        queue
            .dispatch_pending(&mut state)
            .map_err(|err| err.to_string())?;
        queue.flush().map_err(|err| err.to_string())?;
        Ok(std::mem::take(&mut state.events))
    }

    fn flush(&self) {
        if let Err(err) = self.connection.flush() {
            eprintln!("bunt-shell: clipboard history: {err}");
        }
    }

    /// 항목을 클립보드에 내놓는다. 앱이 붙여 넣을 때 `send`가 오고, 다른 복사가 오면 `cancelled`가 온다
    fn offer(&self, item: Rc<Item>) {
        let queue = self.queue.borrow();
        let mut state = self.state.borrow_mut();
        let (Some(manager), Some(device)) = (state.manager.as_ref(), state.device.as_ref()) else {
            return;
        };
        let source = manager.create_data_source(&queue.handle(), ());
        for mime in item.mime_types() {
            source.offer((*mime).to_owned());
        }
        source.offer(OWN_MIME.to_owned());
        device.set_selection(Some(&source));
        state.sources.push((source, item));
        drop(state);
        self.flush();
    }
}

#[derive(Default)]
struct LinkState {
    seat: Option<WlSeat>,
    manager: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
    /// 지금 선택의 제안. 새 선택이 오면 없앤다 (프로토콜의 규칙)
    offer: Option<ExtDataControlOfferV1>,
    /// 셸이 내놓은 소스와 그 항목. 지워진 항목도 클립보드에 있는 동안은 붙여 넣을 수 있다
    sources: Vec<(ExtDataControlSourceV1, Rc<Item>)>,
    events: Vec<Event>,
}

/// 제안이 밝힌 형식. 객체의 사용자 데이터는 Send + Sync여야 한다
#[derive(Default)]
struct OfferMimes(Mutex<Vec<String>>);

impl Dispatch<wl_registry::WlRegistry, ()> for LinkState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _connection: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name, interface, ..
        } = event
        else {
            return;
        };
        if interface == WlSeat::interface().name {
            state.seat = Some(registry.bind(name, 1, queue, ()));
        } else if interface == ExtDataControlManagerV1::interface().name {
            state.manager = Some(registry.bind(name, 1, queue, ()));
        }
    }
}

impl Dispatch<WlSeat, ()> for LinkState {
    fn event(
        _state: &mut Self,
        _seat: &WlSeat,
        _event: <WlSeat as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlManagerV1, ()> for LinkState {
    fn event(
        _state: &mut Self,
        _manager: &ExtDataControlManagerV1,
        _event: <ExtDataControlManagerV1 as Proxy>::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for LinkState {
    fn event(
        state: &mut Self,
        _device: &ExtDataControlDeviceV1,
        event: ext_data_control_device_v1::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        use ext_data_control_device_v1::Event as Device;
        match event {
            Device::Selection { id } => {
                if let Some(old) = state.offer.take() {
                    old.destroy();
                }
                state.offer.clone_from(&id);
                state.events.push(Event::Selection(id.map(|offer| {
                    let mimes = offer
                        .data::<OfferMimes>()
                        .and_then(|mimes| mimes.0.lock().ok().map(|mimes| mimes.clone()))
                        .unwrap_or_default();
                    (offer, mimes)
                })));
            }
            // 기본 선택은 쓰지 않는다 (comp/src/clipboard.rs). 받으면 없애기만 한다
            Device::PrimarySelection { id: Some(offer) } => offer.destroy(),
            Device::Finished => state.device = None,
            _ => {}
        }
    }

    event_created_child!(LinkState, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, OfferMimes::default()),
    ]);
}

impl Dispatch<ExtDataControlOfferV1, OfferMimes> for LinkState {
    fn event(
        _state: &mut Self,
        _offer: &ExtDataControlOfferV1,
        event: ext_data_control_offer_v1::Event,
        mimes: &OfferMimes,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        if let ext_data_control_offer_v1::Event::Offer { mime_type } = event
            && let Ok(mut mimes) = mimes.0.lock()
        {
            mimes.push(mime_type);
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for LinkState {
    fn event(
        state: &mut Self,
        source: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        use ext_data_control_source_v1::Event as Source;
        match event {
            Source::Send { mime_type, fd } => {
                if let Some((_, item)) = state.sources.iter().find(|(known, _)| known == source) {
                    state.events.push(Event::Paste {
                        fd,
                        data: item.payload(&mime_type),
                    });
                }
            }
            Source::Cancelled => {
                state.sources.retain(|(known, _)| known != source);
                source.destroy();
            }
            _ => {}
        }
    }
}
