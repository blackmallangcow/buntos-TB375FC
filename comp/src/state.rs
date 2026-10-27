// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터 상태와 Wayland 프로토콜 처리

use std::{
    cell::{Cell, RefCell},
    error::Error,
    ffi::OsString,
    sync::Arc,
    time::Instant,
};

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_data_device, delegate_layer_shell, delegate_output,
    delegate_seat, delegate_shm, delegate_xdg_shell,
    desktop::{
        LayerSurface, PopupKeyboardGrab, PopupKind, PopupManager, PopupPointerGrab,
        PopupUngrabStrategy, Space, Window, WindowSurfaceType, find_popup_root_surface,
        get_popup_toplevel_coords, layer_map_for_output,
    },
    input::{
        Seat, SeatHandler, SeatState,
        keyboard::XkbConfig,
        pointer::{CursorImageStatus, Focus, GrabStartData as PointerGrabStartData},
    },
    output::Output,
    reexports::{
        calloop::{Interest, LoopHandle, LoopSignal, Mode, PostAction, generic::Generic},
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            Client, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, Rectangle, Serial},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, get_parent,
            is_sync_subsurface, with_states,
        },
        output::{OutputHandler, OutputManagerState},
        selection::{
            SelectionHandler,
            data_device::{
                ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
                set_data_device_focus,
            },
        },
        shell::{
            wlr_layer::{
                Layer, LayerSurface as WlrLayerSurface, LayerSurfaceData, WlrLayerShellHandler,
                WlrLayerShellState,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
    },
};

use crate::{
    config::Config,
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab, handle_resize_commit},
    udev::UdevData,
};

/// 새 창을 계단식으로 놓을 때의 간격 (논리 픽셀)
const CASCADE_STEP: i32 = 32;
const CASCADE_COUNT: i32 = 8;

/// 최대화하기 전 창의 위치와 크기. 창의 사용자 데이터에 둔다.
#[derive(Default)]
struct RestoreGeometry(RefCell<Option<Rectangle<i32, Logical>>>);

/// 아직 자리를 정하지 않은 새 창. 첫 버퍼가 와서 크기를 알면 자리를 정한다.
struct PendingPlacement(Cell<bool>);

pub struct BuntState {
    pub config: Config,
    pub start_time: Instant,
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, BuntState>,
    pub loop_signal: LoopSignal,
    pub socket_name: OsString,

    pub space: Space<Window>,
    pub popups: PopupManager,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub layer_shell_state: WlrLayerShellState,
    pub shm_state: ShmState,
    pub _output_manager_state: OutputManagerState,
    pub seat_state: SeatState<BuntState>,
    pub data_device_state: DataDeviceState,
    pub seat: Seat<BuntState>,
    pub cursor_status: CursorImageStatus,

    pub backend: UdevData,
}

impl BuntState {
    pub fn new(
        display: Display<BuntState>,
        loop_handle: LoopHandle<'static, BuntState>,
        loop_signal: LoopSignal,
        config: Config,
        backend: UdevData,
    ) -> Result<Self, Box<dyn Error>> {
        let dh = display.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        // 최소화, 전체 화면, 창 메뉴는 아직 없으므로 앱이 해당 버튼을 보이지 않게 한다
        let xdg_shell_state = XdgShellState::new_with_capabilities::<Self>(
            &dh,
            [xdg_toplevel::WmCapabilities::Maximize],
        );
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, backend.seat_name());
        seat.add_keyboard(
            XkbConfig {
                layout: &config.keyboard.layout,
                ..Default::default()
            },
            config.keyboard.repeat_delay,
            config.keyboard.repeat_rate,
        )?;
        seat.add_pointer();

        let socket = ListeningSocketSource::new_auto()?;
        let socket_name = socket.socket_name().to_os_string();
        loop_handle
            .insert_source(socket, |stream, _, state| {
                if let Err(err) = state
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    tracing::warn!("failed to add client: {err}");
                }
            })
            .map_err(|err| err.error)?;
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // 안전: 이벤트 소스가 살아 있는 동안 display를 버리지 않는다
                    unsafe {
                        display.get_mut().dispatch_clients(state).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|err| err.error)?;

        Ok(Self {
            config,
            start_time: Instant::now(),
            display_handle: dh,
            loop_handle,
            loop_signal,
            socket_name,
            space: Space::default(),
            popups: PopupManager::default(),
            compositor_state,
            xdg_shell_state,
            layer_shell_state,
            shm_state,
            _output_manager_state: output_manager_state,
            seat_state,
            data_device_state,
            seat,
            cursor_status: CursorImageStatus::default_named(),
            backend,
        })
    }

    /// 명령을 실행하고 끝날 때까지 기다린다. 그동안 컴포지터가 멈추므로 금방 끝나는 명령에만 쓴다.
    pub fn run_and_wait(&self, command: &[String]) {
        let Some((program, args)) = command.split_first() else {
            return;
        };
        match std::process::Command::new(program)
            .args(args)
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .status()
        {
            Ok(status) if !status.success() => tracing::warn!("{program} failed: {status}"),
            Ok(_) => {}
            Err(err) => tracing::warn!("failed to run {program}: {err}"),
        }
    }

    /// 이 컴포지터에 붙는 프로그램을 실행한다.
    pub fn spawn(&self, command: &[String]) {
        let Some((program, args)) = command.split_first() else {
            return;
        };
        match std::process::Command::new(program)
            .args(args)
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .spawn()
        {
            Ok(mut child) => {
                // 끝난 자식 프로세스를 거둔다
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(err) => tracing::warn!("failed to start {program}: {err}"),
        }
    }

    pub fn window_for_surface(&self, surface: &WlSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|window| {
                window
                    .toplevel()
                    .is_some_and(|toplevel| toplevel.wl_surface() == surface)
            })
            .cloned()
    }

    /// 화면 좌표 아래의 표면. 위층(overlay, top) 레이어, 창, 아래층(bottom, background) 순으로 찾는다.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.space.output_under(pos).next()?;
        let output_geo = self.space.output_geometry(output)?;
        let layers = layer_map_for_output(output);
        let relative = pos - output_geo.loc.to_f64();

        let layer_surface_under = |layer: Layer| {
            let surface = layers.layer_under(layer, relative)?;
            let geo = layers.layer_geometry(surface)?;
            surface
                .surface_under(relative - geo.loc.to_f64(), WindowSurfaceType::ALL)
                .map(|(surface, loc)| (surface, (loc + geo.loc + output_geo.loc).to_f64()))
        };

        layer_surface_under(Layer::Overlay)
            .or_else(|| layer_surface_under(Layer::Top))
            .or_else(|| {
                self.space
                    .element_under(pos)
                    .and_then(|(window, location)| {
                        window
                            .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                            .map(|(surface, loc)| (surface, (loc + location).to_f64()))
                    })
            })
            .or_else(|| layer_surface_under(Layer::Bottom))
            .or_else(|| layer_surface_under(Layer::Background))
    }

    /// 좌표가 메뉴바·Dock 같은 위층 레이어 위인지
    pub fn is_over_top_layer(&self, pos: Point<f64, Logical>) -> bool {
        let Some(output) = self.space.output_under(pos).next() else {
            return false;
        };
        let Some(output_geo) = self.space.output_geometry(output) else {
            return false;
        };
        let layers = layer_map_for_output(output);
        let relative = pos - output_geo.loc.to_f64();
        [Layer::Overlay, Layer::Top]
            .into_iter()
            .any(|layer| layers.layer_under(layer, relative).is_some())
    }

    pub fn focus_window(&mut self, window: &Window, serial: Serial) {
        self.space.raise_element(window, true);
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(
                self,
                window.toplevel().map(|t| t.wl_surface().clone()),
                serial,
            );
        }
        self.space.elements().for_each(|window| {
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_pending_configure();
            }
        });
    }

    pub fn clear_focus(&mut self, serial: Serial) {
        self.space.elements().for_each(|window| {
            window.set_activated(false);
            if let Some(toplevel) = window.toplevel() {
                toplevel.send_pending_configure();
            }
        });
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, serial);
        }
    }

    /// 새 창의 첫 버퍼가 오면 자리를 정한다. 부모가 있는 창(대화 상자)은 부모의 가운데에,
    /// 나머지는 쓸 수 있는 영역의 가운데에 놓되 창이 여럿이면 조금씩 비켜 놓는다.
    /// 창이 영역을 벗어나지 않게 하고, 영역보다 크면 왼쪽 위에 맞춘다.
    fn place_new_window(&mut self, window: &Window) {
        let Some(pending) = window.user_data().get::<PendingPlacement>() else {
            return;
        };
        let size = window.geometry().size;
        if !pending.0.get() || size.is_empty() {
            return;
        }
        pending.0.set(false);
        let (Some(toplevel), Some(area)) = (window.toplevel(), self.usable_area()) else {
            return;
        };
        if Self::is_maximized(toplevel) {
            return;
        }

        let parent = toplevel
            .parent()
            .and_then(|parent| self.window_for_surface(&parent))
            .and_then(|parent| self.space.element_geometry(&parent));
        let (center, offset) = match parent {
            Some(parent) => (parent.loc + parent.size.downscale(2).to_point(), 0),
            None => {
                let others = self
                    .space
                    .elements()
                    .filter(|other| *other != window)
                    .count();
                let index = others as i32 % CASCADE_COUNT;
                (
                    area.loc + area.size.downscale(2).to_point(),
                    CASCADE_STEP * index,
                )
            }
        };
        let fit = |center: i32, length: i32, start: i32, available: i32| {
            (center - length / 2).clamp(start, start + (available - length).max(0))
        };
        let location = Point::from((
            fit(center.x + offset, size.w, area.loc.x, area.size.w),
            fit(center.y + offset, size.h, area.loc.y, area.size.h),
        ));
        self.space.map_element(window.clone(), location, false);
    }

    fn ensure_initial_configure(&mut self, surface: &WlSurface) {
        if let Some(window) = self.window_for_surface(surface) {
            let sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .is_none_or(|data| data.lock().unwrap().initial_configure_sent)
            });
            if !sent && let Some(toplevel) = window.toplevel() {
                toplevel.send_configure();
            }
            return;
        }

        if let Some(popup) = self.popups.find_popup(surface) {
            if let PopupKind::Xdg(ref xdg) = popup
                && !xdg.is_initial_configure_sent()
            {
                // 첫 configure는 언제나 허용된다
                let _ = xdg.send_configure();
            }
            return;
        }

        let output = self
            .space
            .outputs()
            .find(|output| {
                layer_map_for_output(output)
                    .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                    .is_some()
            })
            .cloned();
        if let Some(output) = output {
            let sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<LayerSurfaceData>()
                    .is_none_or(|data| data.lock().unwrap().initial_configure_sent)
            });
            let mut map = layer_map_for_output(&output);
            // 클라이언트가 요청한 크기를 반영해 배치한 뒤 configure를 보낸다
            map.arrange();
            if !sent
                && let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
            {
                layer.layer_surface().send_configure();
            }
        }
    }

    /// 팝업이 화면 밖으로 나가지 않게 위치를 고친다. 부모는 창이거나 메뉴바 같은 레이어 표면이다.
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let Some((output_geo, root_location)) = self.popup_root_location(&root) else {
            return;
        };

        // 위치 지정은 부모 표면 기준이므로 출력 영역을 그 좌표계로 옮긴다
        let mut target = output_geo;
        target.loc -= get_popup_toplevel_coords(&kind);
        target.loc -= root_location;

        popup.with_pending_state(|state| {
            state.geometry = state.positioner.get_unconstrained_geometry(target);
        });
    }

    /// 팝업의 최상위 부모가 놓인 출력의 영역과 부모의 전역 위치
    fn popup_root_location(
        &self,
        root: &WlSurface,
    ) -> Option<(Rectangle<i32, Logical>, Point<i32, Logical>)> {
        if let Some(window) = self.window_for_surface(root) {
            let output = self.space.outputs().next()?;
            let output_geo = self.space.output_geometry(output)?;
            return Some((output_geo, self.space.element_geometry(&window)?.loc));
        }
        self.space.outputs().find_map(|output| {
            let output_geo = self.space.output_geometry(output)?;
            let map = layer_map_for_output(output);
            let layer = map.layer_for_surface(root, WindowSurfaceType::TOPLEVEL)?;
            Some((output_geo, output_geo.loc + map.layer_geometry(layer)?.loc))
        })
    }

    /// 메뉴바·Dock이 차지하지 않는 화면 영역 (전역 좌표)
    fn usable_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        let output_geo = self.space.output_geometry(output)?;
        let mut zone = layer_map_for_output(output).non_exclusive_zone();
        zone.loc += output_geo.loc;
        Some(zone)
    }

    fn is_maximized(surface: &ToplevelSurface) -> bool {
        surface.with_pending_state(|state| state.states.contains(xdg_toplevel::State::Maximized))
    }

    /// 창을 쓸 수 있는 화면 영역에 맞추거나, 최대화하기 전의 위치와 크기로 되돌린다.
    fn set_maximized(&mut self, surface: &ToplevelSurface, maximized: bool) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let restore = window.user_data().get_or_insert(RestoreGeometry::default);
        if maximized {
            let Some(area) = self.usable_area() else {
                return;
            };
            if !Self::is_maximized(surface) {
                *restore.0.borrow_mut() = self.space.element_geometry(&window);
            }
            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Maximized);
                state.size = Some(area.size);
            });
            self.space.map_element(window.clone(), area.loc, true);
        } else {
            let previous = restore.0.borrow_mut().take();
            surface.with_pending_state(|state| {
                state.states.unset(xdg_toplevel::State::Maximized);
                // 크기를 비우면 앱이 스스로 크기를 고른다
                state.size = previous.map(|geo| geo.size).filter(|size| !size.is_empty());
            });
            if let Some(geo) = previous {
                self.space.map_element(window.clone(), geo.loc, true);
            }
        }
        // 첫 configure 전이면 첫 configure에 함께 실린다
        if surface.is_initial_configure_sent() {
            surface.send_pending_configure();
        }
    }
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

impl CompositorHandler for BuntState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(window) = self.window_for_surface(&root) {
                window.on_commit();
                self.place_new_window(&window);
            }
        }
        self.popups.commit(surface);
        handle_resize_commit(&mut self.space, surface);
        self.ensure_initial_configure(surface);
    }
}

impl BufferHandler for BuntState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for BuntState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl XdgShellHandler for BuntState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        // 앱이 화면에 맞는 기본 크기를 고르도록 쓸 수 있는 영역의 크기를 알린다 (configure_bounds)
        let area = self.usable_area();
        if let Some(area) = area {
            surface.with_pending_state(|state| state.bounds = Some(area.size));
        }
        let window = Window::new_wayland_window(surface);
        window
            .user_data()
            .get_or_insert(|| PendingPlacement(Cell::new(true)));
        // 크기를 알기 전에는 아무 데나 두고, 첫 버퍼가 오면 place_new_window가 옮긴다
        let origin = area.map(|area| area.loc).unwrap_or_default();
        self.space.map_element(window.clone(), origin, true);
        self.focus_window(&window, smithay::utils::SERIAL_COUNTER.next_serial());
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        self.unconstrain_popup(&surface);
        if let Err(err) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            tracing::warn!("failed to track popup: {err}");
        }
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|state| {
            state.geometry = positioner.get_geometry();
            state.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<Self>::from_resource(&seat) else {
            return;
        };
        let wl_surface = surface.wl_surface();
        let Some(start_data) = check_grab(&seat, wl_surface, serial) else {
            return;
        };
        let (Some(pointer), Some(window)) =
            (seat.get_pointer(), self.window_for_surface(wl_surface))
        else {
            return;
        };
        let Some(mut initial_window_location) = self.space.element_location(&window) else {
            return;
        };

        // 최대화된 창을 끌면 원래 크기로 되돌리고, 포인터가 제목 표시줄의 같은 비율 위치에 오게 한다
        if Self::is_maximized(&surface) {
            let maximized_width = window.geometry().size.w.max(1);
            let restored_width = window
                .user_data()
                .get::<RestoreGeometry>()
                .and_then(|restore| *restore.0.borrow())
                .map_or(maximized_width, |geo| geo.size.w);
            self.set_maximized(&surface, false);
            let ratio = (start_data.location.x - f64::from(initial_window_location.x))
                / f64::from(maximized_width);
            initial_window_location.x =
                (start_data.location.x - ratio * f64::from(restored_width)).round() as i32;
            self.space
                .map_element(window.clone(), initial_window_location, true);
        }

        let grab = MoveSurfaceGrab {
            start_data,
            window,
            initial_window_location,
        };
        pointer.set_grab(self, grab, serial, Focus::Clear);
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let Some(seat) = Seat::<Self>::from_resource(&seat) else {
            return;
        };
        let wl_surface = surface.wl_surface();
        let Some(start_data) = check_grab(&seat, wl_surface, serial) else {
            return;
        };
        let (Some(pointer), Some(window)) =
            (seat.get_pointer(), self.window_for_surface(wl_surface))
        else {
            return;
        };
        let Some(initial_rect) = self.space.element_geometry(&window) else {
            return;
        };
        surface.with_pending_state(|state| {
            state.states.set(xdg_toplevel::State::Resizing);
        });
        surface.send_pending_configure();
        if let Some(grab) = ResizeSurfaceGrab::start(start_data, window, edges.into(), initial_rect)
        {
            pointer.set_grab(self, grab, serial, Focus::Clear);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        self.set_maximized(&surface, true);
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        self.set_maximized(&surface, false);
    }

    /// 메뉴 같은 팝업이 입력을 독점한다. 팝업 밖을 누르면 팝업이 닫힌다.
    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<Self>::from_resource(&seat) else {
            return;
        };
        let kind = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&kind) else {
            return;
        };
        let Ok(mut grab) = self.popups.grab_popup(root, kind, &seat, serial) else {
            return;
        };

        // 다른 그랩이 진행 중이면 팝업 그랩을 거절한다. 이어지는 하위 메뉴의 그랩은 허용한다
        if let Some(keyboard) = seat.get_keyboard() {
            if keyboard.is_grabbed()
                && !(keyboard.has_grab(serial)
                    || keyboard.has_grab(grab.previous_serial().unwrap_or(serial)))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            keyboard.set_focus(self, grab.current_grab(), serial);
            keyboard.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        if let Some(pointer) = seat.get_pointer() {
            if pointer.is_grabbed()
                && !(pointer.has_grab(serial)
                    || pointer.has_grab(grab.previous_serial().unwrap_or_else(|| grab.serial())))
            {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            pointer.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
        }
    }
}

/// 요청한 표면이 실제로 클릭 그랩을 가지고 있을 때만 그랩 시작 정보를 돌려준다.
fn check_grab(
    seat: &Seat<BuntState>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<PointerGrabStartData<BuntState>> {
    let pointer = seat.get_pointer()?;
    if !pointer.has_grab(serial) {
        return None;
    }
    let start_data = pointer.grab_start_data()?;
    let (focus, _) = start_data.focus.as_ref()?;
    if !focus.id().same_client_as(&surface.id()) {
        return None;
    }
    Some(start_data)
}

impl WlrLayerShellHandler for BuntState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: WlrLayerSurface,
        wl_output: Option<wl_output::WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let Some(output) = wl_output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned())
        else {
            return;
        };
        let mut map = layer_map_for_output(&output);
        if let Err(err) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            tracing::warn!("failed to map layer surface: {err}");
        }
    }

    fn new_popup(&mut self, _parent: WlrLayerSurface, popup: PopupSurface) {
        self.unconstrain_popup(&popup);
    }

    fn layer_destroyed(&mut self, surface: WlrLayerSurface) {
        for output in self.space.outputs() {
            let mut map = layer_map_for_output(output);
            let layer = map
                .layers()
                .find(|layer| layer.layer_surface() == &surface)
                .cloned();
            if let Some(layer) = layer {
                map.unmap_layer(&layer);
            }
        }
    }
}

impl SeatHandler for BuntState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<BuntState> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let client = focused.and_then(|surface| self.display_handle.get_client(surface.id()).ok());
        set_data_device_focus(&self.display_handle, seat, client);
    }
}

impl SelectionHandler for BuntState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for BuntState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for BuntState {}
impl ServerDndGrabHandler for BuntState {}

impl OutputHandler for BuntState {}

delegate_compositor!(BuntState);
delegate_shm!(BuntState);
delegate_xdg_shell!(BuntState);
delegate_layer_shell!(BuntState);
delegate_seat!(BuntState);
delegate_data_device!(BuntState);
delegate_output!(BuntState);
