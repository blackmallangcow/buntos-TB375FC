// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 창 목록 (wlr-foreign-toplevel-management). 컴포지터가 셸에게만 여는 프로토콜로, 실행 중인 앱,
//! 앞에 있는 창, 최대화·전체 화면 상태를 받고 창을 앞으로 가져오거나 닫는다.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk4::{gdk, glib, prelude::*};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child,
    protocol::{wl_registry, wl_seat::WlSeat},
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// GTK가 소켓에서 읽어 둔 이 목록의 이벤트를 처리하는 간격
const DISPATCH_INTERVAL: Duration = Duration::from_millis(50);

/// 창 하나
#[derive(Clone, Debug)]
pub struct Toplevel {
    pub app_id: String,
    pub title: String,
    pub activated: bool,
    pub maximized: bool,
    pub fullscreen: bool,
    handle: ZwlrForeignToplevelHandleV1,
}

impl Toplevel {
    fn new(handle: ZwlrForeignToplevelHandleV1) -> Self {
        Self {
            app_id: String::new(),
            title: String::new(),
            activated: false,
            maximized: false,
            fullscreen: false,
            handle,
        }
    }
}

struct Entry {
    /// done 이벤트 전까지 쌓는 값
    pending: Toplevel,
    current: Option<Toplevel>,
    /// 마지막으로 활성화된 순번. 앱의 창 가운데 어느 것을 앞으로 가져올지 정할 때 쓴다
    last_activated: u64,
}

#[derive(Default)]
struct WaylandState {
    seat: Option<WlSeat>,
    manager: Option<ZwlrForeignToplevelManagerV1>,
    entries: Vec<Entry>,
    activations: u64,
    changed: bool,
}

/// 창 목록. 복제해도 같은 목록을 가리킨다
#[derive(Clone)]
pub struct Toplevels(Rc<Inner>);

type Listener = Box<dyn Fn(&Toplevels)>;

struct Inner {
    connection: Connection,
    queue: RefCell<EventQueue<WaylandState>>,
    state: RefCell<WaylandState>,
    listeners: RefCell<Vec<Listener>>,
}

impl Toplevels {
    /// GTK의 Wayland 연결에 붙는다. 컴포지터가 이 프로토콜을 주지 않으면 None이다
    /// (셸이 컴포지터가 직접 띄운 특권 클라이언트가 아닐 때).
    pub fn connect(display: &gdk::Display) -> Option<Self> {
        let wayland = display.downcast_ref::<gdk4_wayland::WaylandDisplay>()?;
        let wl_display = wayland.wl_display()?;
        let connection = Connection::from_backend(wl_display.backend().upgrade()?);
        let mut queue = connection.new_event_queue();
        let mut state = WaylandState::default();
        wl_display.get_registry(&queue.handle(), ());
        queue.roundtrip(&mut state).ok()?;
        state.manager.as_ref()?;
        // 이미 떠 있는 창을 받는다
        queue.roundtrip(&mut state).ok()?;

        let toplevels = Self(Rc::new(Inner {
            connection,
            queue: RefCell::new(queue),
            state: RefCell::new(state),
            listeners: RefCell::default(),
        }));
        let weak = Rc::downgrade(&toplevels.0);
        glib::timeout_add_local(DISPATCH_INTERVAL, move || {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            Toplevels(inner).dispatch();
            glib::ControlFlow::Continue
        });
        Some(toplevels)
    }

    fn dispatch(&self) {
        let changed = {
            let mut queue = self.0.queue.borrow_mut();
            let mut state = self.0.state.borrow_mut();
            if let Err(err) = queue.dispatch_pending(&mut state) {
                eprintln!("bunt-shell: window list: {err}");
            }
            std::mem::take(&mut state.changed)
        };
        if changed {
            for listener in self.0.listeners.borrow().iter() {
                listener(self);
            }
        }
    }

    /// 목록이 바뀔 때마다 부른다. 처음 한 번은 바로 부른다.
    pub fn subscribe(&self, listener: impl Fn(&Toplevels) + 'static) {
        listener(self);
        self.0.listeners.borrow_mut().push(Box::new(listener));
    }

    pub fn list(&self) -> Vec<Toplevel> {
        let state = self.0.state.borrow();
        state
            .entries
            .iter()
            .filter_map(|entry| entry.current.clone())
            .collect()
    }

    /// 키보드 포커스를 가진 창
    pub fn active(&self) -> Option<Toplevel> {
        self.list().into_iter().find(|toplevel| toplevel.activated)
    }

    /// 앱의 창 가운데 가장 최근에 쓴 것을 앞으로 가져온다. 창이 없으면 false
    pub fn activate_app(&self, app_id: &str) -> bool {
        let target = {
            let state = self.0.state.borrow();
            state
                .entries
                .iter()
                .filter(|entry| {
                    entry
                        .current
                        .as_ref()
                        .is_some_and(|toplevel| toplevel.app_id == app_id)
                })
                .max_by_key(|entry| entry.last_activated)
                .and_then(|entry| entry.current.clone())
        };
        match target {
            Some(toplevel) => {
                self.activate(&toplevel);
                true
            }
            None => false,
        }
    }

    pub fn activate(&self, toplevel: &Toplevel) {
        if let Some(seat) = &self.0.state.borrow().seat {
            toplevel.handle.activate(seat);
        }
        self.flush();
    }

    pub fn close(&self, toplevel: &Toplevel) {
        toplevel.handle.close();
        self.flush();
    }

    /// 앱의 모든 창을 닫는다. 창을 닫으면 끝나는 앱을 끝낼 때 쓴다
    pub fn close_app(&self, app_id: &str) {
        for toplevel in self.list().iter().filter(|t| t.app_id == app_id) {
            toplevel.handle.close();
        }
        self.flush();
    }

    pub fn set_maximized(&self, toplevel: &Toplevel, maximized: bool) {
        if maximized {
            toplevel.handle.set_maximized();
        } else {
            toplevel.handle.unset_maximized();
        }
        self.flush();
    }

    pub fn set_fullscreen(&self, toplevel: &Toplevel, fullscreen: bool) {
        if fullscreen {
            toplevel.handle.set_fullscreen(None);
        } else {
            toplevel.handle.unset_fullscreen();
        }
        self.flush();
    }

    fn flush(&self) {
        if let Err(err) = self.0.connection.flush() {
            eprintln!("bunt-shell: window list: {err}");
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _connection: &Connection,
        queue: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        else {
            return;
        };
        match interface.as_str() {
            "zwlr_foreign_toplevel_manager_v1" => {
                state.manager = Some(registry.bind(name, version.min(3), queue, ()));
            }
            "wl_seat" if state.seat.is_none() => {
                state.seat = Some(registry.bind(name, 1, queue, ()));
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for WaylandState {
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

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _manager: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        if let zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } = event {
            state.entries.push(Entry {
                pending: Toplevel::new(toplevel),
                current: None,
                last_activated: 0,
            });
        }
    }

    event_created_child!(WaylandState, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
    ) {
        use zwlr_foreign_toplevel_handle_v1::{Event, State};
        if let Event::Closed = event {
            state
                .entries
                .retain(|entry| &entry.pending.handle != handle);
            handle.destroy();
            state.changed = true;
            return;
        }
        let Some(index) = state
            .entries
            .iter()
            .position(|entry| &entry.pending.handle == handle)
        else {
            return;
        };
        let entry = &mut state.entries[index];
        match event {
            Event::Title { title } => entry.pending.title = title,
            Event::AppId { app_id } => entry.pending.app_id = app_id,
            Event::State { state: bytes } => {
                // 상태 값(u32)이 기계 바이트 순서로 이어져 있다
                let states: Vec<u32> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|chunk| u32::from_ne_bytes(*chunk))
                    .collect();
                let has = |value: State| states.contains(&(value as u32));
                entry.pending.activated = has(State::Activated);
                entry.pending.maximized = has(State::Maximized);
                entry.pending.fullscreen = has(State::Fullscreen);
            }
            Event::Done => {
                let became_active = entry.pending.activated
                    && !entry
                        .current
                        .as_ref()
                        .is_some_and(|current| current.activated);
                entry.current = Some(entry.pending.clone());
                if became_active {
                    state.activations += 1;
                    state.entries[index].last_activated = state.activations;
                }
                state.changed = true;
            }
            _ => {}
        }
    }
}
