// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터 상태와 Wayland 프로토콜 처리

use std::{
    cell::{Cell, RefCell},
    error::Error,
    ffi::OsString,
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::Arc,
    time::Instant,
};

use smithay::{
    backend::renderer::utils::{on_commit_buffer_handler, with_renderer_surface_state},
    delegate_compositor, delegate_data_device, delegate_fractional_scale, delegate_output,
    delegate_seat, delegate_shm, delegate_viewporter, delegate_xdg_shell,
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
    output::{Output, Scale as OutputScale},
    reexports::{
        calloop::{
            Interest, LoopHandle, LoopSignal, Mode, PostAction, RegistrationToken,
            channel::{self, Channel},
            generic::Generic,
            timer::{TimeoutAction, Timer},
        },
        rustix::io::{FdFlags, fcntl_setfd},
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_protocols_wlr::layer_shell::v1::server::{
            zwlr_layer_shell_v1::ZwlrLayerShellV1,
            zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, Display, DisplayHandle, Resource,
            backend::{ClientData, ClientId, DisconnectReason},
            delegate_dispatch, delegate_global_dispatch,
            protocol::{wl_buffer, wl_output, wl_seat, wl_surface::WlSurface},
        },
    },
    utils::{Logical, Point, Rectangle, Serial, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            CompositorClientState, CompositorHandler, CompositorState, SurfaceData,
            TraversalAction, get_parent, is_sync_subsurface, send_surface_state, with_states,
            with_surface_tree_downward,
        },
        fractional_scale::{
            FractionalScaleHandler, FractionalScaleManagerState, with_fractional_scale,
        },
        input_method::{InputMethodManagerState, InputMethodSeat},
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
                Anchor, ExclusiveZone, KeyboardInteractivity, Layer,
                LayerSurface as WlrLayerSurface, LayerSurfaceCachedState, LayerSurfaceData,
                WlrLayerShellGlobalData, WlrLayerShellHandler, WlrLayerShellState,
                WlrLayerSurfaceUserData,
            },
            xdg::{
                PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
                XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
        socket::ListeningSocketSource,
        text_input::TextInputManagerState,
        viewporter::ViewporterState,
        virtual_keyboard::VirtualKeyboardManagerState,
    },
};

use crate::{
    config::Config,
    dbus::{self, Request},
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab, handle_resize_commit},
    ime::InputMethodProcess,
    panels::{self, Bar, BarInput, Edge, Fill, Panels, Reveal},
    prefs::{KeyRepeat, Prefs},
    render,
    toplevels::ForeignToplevelState,
    udev::UdevData,
};

/// 새 창을 계단식으로 놓을 때의 간격 (논리 픽셀)
const CASCADE_STEP: i32 = 32;
const CASCADE_COUNT: i32 = 8;
/// 셸 메뉴바 레이어의 이름 (shell/src/menubar.rs). 메뉴 막대 배경을 켰을 때만 흐린다
const MENUBAR_NAMESPACE: &str = "bunt-menubar";
/// 셸 Dock 레이어의 이름 (shell/src/dock.rs)
const DOCK_NAMESPACE: &str = "bunt-dock";
/// 막대의 독점 영역이 바뀐 뒤 최대화·전체 화면 창을 다시 맞추기까지 기다리는 시간.
/// Dock 크기 슬라이더를 끄는 동안 창 크기가 계속 바뀌지 않게 한다
const REFILL_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

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
    pub foreign_toplevels: ForeignToplevelState,
    pub seat: Seat<BuntState>,
    pub cursor_status: CursorImageStatus,
    /// 컴포지터가 가로챈 단축키의 키. 뗄 때도 앱에 보내지 않는다
    pub suppressed_keys: Vec<u32>,

    /// 시스템 설정에서 고른 값 (다음 로그인에도 쓴다)
    pub prefs: Prefs,
    /// 메뉴 막대와 Dock을 언제 가리는지 (셸이 알려 준다)
    pub panels: Panels,
    /// 메뉴 막대와 Dock이 보이는 정도 (Bar::index 순서)
    bars: [Reveal; 2],
    /// 꺼낸 막대를 다시 숨길 때를 기다리는 타이머와 그때
    bar_timer: Option<(RegistrationToken, Instant)>,
    /// 마지막으로 본 창 영역 (보통 창, 최대화, 전체 화면). 막대가 바뀌면 달라진다
    window_areas: Option<[Rectangle<i32, Logical>; 3]>,
    refill_timer: Option<RegistrationToken>,
    pub dbus: dbus::Service,
    pub input_method_process: InputMethodProcess,
    pub input_method_exited: channel::Sender<()>,

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

        // v6: 표면마다 알맞은 버퍼 배율과 회전을 알린다 (wl_surface.preferred_buffer_scale)
        let compositor_state = CompositorState::new_v6::<Self>(&dh);
        // 최소화와 창 메뉴는 아직 없으므로 앱이 해당 버튼을 보이지 않게 한다
        let xdg_shell_state = XdgShellState::new_with_capabilities::<Self>(
            &dh,
            [
                xdg_toplevel::WmCapabilities::Maximize,
                xdg_toplevel::WmCapabilities::Fullscreen,
            ],
        );
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let foreign_toplevels = ForeignToplevelState::new(&dh);
        // 입력기: 앱 쪽(text-input)은 모두에게, 입력기 쪽은 컴포지터가 띄운 입력기에만 연다
        TextInputManagerState::new::<Self>(&dh);
        InputMethodManagerState::new::<Self, _>(&dh, is_input_method_client);
        VirtualKeyboardManagerState::new::<Self, _>(&dh, is_input_method_client);
        // 소수 배율(125%, 150% …): 앱은 알려 준 배율로 그리고 viewporter로 논리 크기를 정한다.
        // Firefox는 배율이 1이 아니면 viewporter 없이는 본문을 제 크기로 보이지 못한다
        ViewporterState::new::<Self>(&dh);
        FractionalScaleManagerState::new::<Self>(&dh);

        let prefs = Prefs::load();
        let repeat = prefs.keyboard.unwrap_or(KeyRepeat {
            delay: config.keyboard.repeat_delay.max(0) as u32,
            rate: config.keyboard.repeat_rate.max(0) as u32,
        });
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, backend.seat_name());
        seat.add_keyboard(
            XkbConfig {
                layout: &config.keyboard.layout,
                ..Default::default()
            },
            repeat.delay as i32,
            repeat.rate as i32,
        )?;
        seat.add_pointer();

        let (requests, request_channel): (_, Channel<Request>) = channel::channel();
        loop_handle
            .insert_source(request_channel, |event, _, state| {
                if let channel::Event::Msg(request) = event {
                    state.handle_request(request);
                }
            })
            .map_err(|err| err.error)?;
        let dbus = dbus::Service::start(requests);
        dbus.update(|snapshot| {
            snapshot.repeat_delay = repeat.delay;
            snapshot.repeat_rate = repeat.rate;
        });
        let (input_method_exited, exited_channel) = channel::channel();
        loop_handle
            .insert_source(exited_channel, |event, _, state| {
                if let channel::Event::Msg(()) = event {
                    state.input_method_exited();
                }
            })
            .map_err(|err| err.error)?;

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
            foreign_toplevels,
            seat,
            cursor_status: CursorImageStatus::default_named(),
            suppressed_keys: Vec::new(),
            prefs,
            panels: Panels::default(),
            bars: [Reveal::default(); 2],
            bar_timer: None,
            window_areas: None,
            refill_timer: None,
            dbus,
            input_method_process: InputMethodProcess::default(),
            input_method_exited,
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

    /// 셸을 특권 클라이언트로 실행한다. 이 연결에만 창 목록·창 조작 프로토콜이 보인다 (toplevels.rs).
    pub fn spawn_privileged(&mut self, command: &[String]) {
        let client = ClientState {
            privileged: true,
            ..ClientState::default()
        };
        if let Some(mut child) = self.spawn_trusted(command, client) {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }

    /// 미리 만든 연결을 WAYLAND_SOCKET으로 넘겨 프로그램을 실행한다. `client`에 적은 권한
    /// (창 목록, 입력기 프로토콜)은 이 연결에만 주어진다. 끝난 프로세스는 부른 쪽이 거둔다.
    pub fn spawn_trusted(
        &mut self,
        command: &[String],
        client: ClientState,
    ) -> Option<std::process::Child> {
        let (program, args) = command.split_first()?;
        let (ours, theirs) = match UnixStream::pair() {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!("failed to create a socket for {program}: {err}");
                return None;
            }
        };
        if let Err(err) = self.display_handle.insert_client(ours, Arc::new(client)) {
            tracing::warn!("failed to add {program} as a client: {err}");
            return None;
        }
        // 자식 프로세스가 연결을 물려받도록 close-on-exec를 끈다. 부모 쪽 사본은 실행 뒤 닫힌다
        if let Err(err) = fcntl_setfd(&theirs, FdFlags::empty()) {
            tracing::warn!("failed to pass the socket to {program}: {err}");
            return None;
        }
        match std::process::Command::new(program)
            .args(args)
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .env("WAYLAND_SOCKET", theirs.as_raw_fd().to_string())
            .spawn()
        {
            Ok(child) => Some(child),
            Err(err) => {
                tracing::warn!("failed to start {program}: {err}");
                None
            }
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
        let relative = pos - output_geo.loc.to_f64();

        // upper_layer_under도 레이어 맵을 잠그므로 쓸 때만 잠근다
        let layer_surface_under = |layer: Layer| {
            let layers = layer_map_for_output(output);
            let surface = layers.layer_under(layer, relative)?;
            let geo = layers.layer_geometry(surface)?;
            surface
                .surface_under(relative - geo.loc.to_f64(), WindowSurfaceType::ALL)
                .map(|(surface, loc)| (surface, (loc + geo.loc + output_geo.loc).to_f64()))
        };

        self.upper_layer_under(pos)
            .map(|(_, surface, loc)| (surface, loc))
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

    /// 좌표가 메뉴바·Dock 같은 위층 레이어 위인지. 숨은 막대와 레이어의 입력 영역 밖은 세지
    /// 않으므로, 그 자리를 누르면 아래 창이 앞으로 온다
    pub fn is_over_top_layer(&self, pos: Point<f64, Logical>) -> bool {
        self.upper_layer_under(pos).is_some()
    }

    /// 창을 맨 앞으로 가져와 활성화하고 키보드 포커스를 준다. 키보드를 독점하는 레이어 표면(모든 앱
    /// 패널 등)이 떠 있으면 키보드는 그 표면에 그대로 두고, 표면이 사라질 때 이 창에 준다
    pub fn focus_window(&mut self, window: &Window, serial: Serial) {
        self.space.raise_element(window, true);
        let exclusive = self.exclusive_layer().is_some();
        if let Some(keyboard) = self.seat.get_keyboard().filter(|_| !exclusive) {
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
        self.refresh_all_windows();
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
        self.refresh_all_windows();
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
        if Self::has_state(toplevel, xdg_toplevel::State::Maximized)
            || Self::has_state(toplevel, xdg_toplevel::State::Fullscreen)
        {
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
        // 영역보다 큰 창은 영역에 맞게 줄여 달라고 한다 (최소 크기가 더 크면 앱이 따르지 않는다)
        let size = Size::from((size.w.min(area.size.w), size.h.min(area.size.h)));
        if size != window.geometry().size {
            toplevel.with_pending_state(|state| state.size = Some(size));
            toplevel.send_pending_configure();
        }
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
        let output_geo = self
            .space
            .outputs()
            .next()
            .and_then(|output| self.space.output_geometry(output))?;
        Some((output_geo, self.layer_origin(root)?))
    }

    /// 레이어 표면 왼쪽 위의 전역 위치
    pub fn layer_origin(&self, surface: &WlSurface) -> Option<Point<i32, Logical>> {
        self.space.outputs().find_map(|output| {
            let output_geo = self.space.output_geometry(output)?;
            let map = layer_map_for_output(output);
            let layer = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)?;
            Some(output_geo.loc + map.layer_geometry(layer)?.loc)
        })
    }

    /// 보통 창이 쓰는 영역: 메뉴바·Dock이 차지하지 않는 화면 영역 (전역 좌표). 늘 숨는 막대의
    /// 자리는 보통 창도 쓴다
    fn usable_area(&self) -> Option<Rectangle<i32, Logical>> {
        self.window_area(Fill::Normal)
    }

    /// 창이 놓일 영역 (전역 좌표). 출력 전체에서 그 창이 덮지 않는 레이어의 독점 영역을 뺀다.
    /// 메뉴 막대와 Dock은 자동 가리기 설정(panels)을 따르고, 다른 레이어는 전체 화면 창만 덮는다
    fn window_area(&self, fill: Fill) -> Option<Rectangle<i32, Logical>> {
        let output = self.space.outputs().next()?;
        let mut area = self.space.output_geometry(output)?;
        let map = layer_map_for_output(output);
        for layer in map.layers() {
            let state = layer.cached_state();
            let ExclusiveZone::Exclusive(amount) = state.exclusive_zone else {
                continue;
            };
            let Some(edge) = Edge::of(state.anchor) else {
                continue;
            };
            if !self.covered(layer, fill) {
                let amount = i32::try_from(amount).unwrap_or(i32::MAX);
                panels::reserve(
                    &mut area,
                    edge,
                    amount.saturating_add(edge.margin(&state.margin)),
                );
            }
        }
        Some(area)
    }

    /// 이렇게 놓이는 창이 이 레이어 자리까지 쓰는지
    fn covered(&self, layer: &LayerSurface, fill: Fill) -> bool {
        match self.bar_of(layer) {
            Some(bar) => self.bar_mode(bar).covered_by(fill),
            None => fill == Fill::Fullscreen,
        }
    }

    /// 셸의 막대(메뉴 막대, Dock)인지
    fn bar_of(&self, layer: &LayerSurface) -> Option<Bar> {
        let bar = match layer.namespace() {
            MENUBAR_NAMESPACE => Bar::MenuBar,
            DOCK_NAMESPACE => Bar::Dock,
            _ => return None,
        };
        self.is_privileged(layer.wl_surface()).then_some(bar)
    }

    fn bar_mode(&self, bar: Bar) -> panels::Autohide {
        match bar {
            Bar::MenuBar => self.panels.menu_bar,
            Bar::Dock => self.panels.dock,
        }
    }

    /// 앞에 있는(키보드를 받는) 창이 (최대화, 전체 화면)인지
    fn front_window_fills(&self) -> (bool, bool) {
        self.space
            .elements()
            .find_map(|window| {
                let toplevel = window.toplevel()?;
                Self::has_state(toplevel, xdg_toplevel::State::Activated).then(|| {
                    (
                        Self::has_state(toplevel, xdg_toplevel::State::Maximized),
                        Self::has_state(toplevel, xdg_toplevel::State::Fullscreen),
                    )
                })
            })
            .unwrap_or((false, false))
    }

    /// 막대를 숨길지 꺼낼지 다시 정한다 (panels.rs). 포인터가 움직일 때(`pointer`는 새 위치), 화면을
    /// 그릴 때, 숨기기를 기다리던 때가 됐을 때 부른다
    pub fn update_bars(&mut self, pointer: Option<Point<f64, Logical>>) {
        let Some(output) = self.space.outputs().next().cloned() else {
            return;
        };
        let Some(output_geo) = self.space.output_geometry(&output) else {
            return;
        };
        let pointer = pointer
            .or_else(|| {
                self.seat
                    .get_pointer()
                    .map(|pointer| pointer.current_location())
            })
            .unwrap_or_default();
        let (maximized, fullscreen) = self.front_window_fills();
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        for bar in Bar::ALL {
            let found = {
                let map = layer_map_for_output(&output);
                map.layers()
                    .find(|layer| self.bar_of(layer) == Some(bar))
                    .and_then(|layer| {
                        let mut geo = map.layer_geometry(layer)?;
                        geo.loc += output_geo.loc;
                        let menu_open = PopupManager::popups_for_surface(layer.wl_surface())
                            .next()
                            .is_some();
                        Some((geo, menu_open))
                    })
            };
            let Some((geo, menu_open)) = found else {
                continue;
            };
            let at_edge = match bar {
                Bar::MenuBar => pointer.y < f64::from(output_geo.loc.y) + panels::EDGE,
                Bar::Dock => {
                    pointer.y >= f64::from(output_geo.loc.y + output_geo.size.h) - panels::EDGE
                        && pointer.x >= f64::from(geo.loc.x)
                        && pointer.x < f64::from(geo.loc.x + geo.size.w)
                }
            };
            let input = BarInput {
                covered: self.bar_mode(bar).hides(maximized, fullscreen),
                at_edge,
                inside: geo.to_f64().contains(pointer),
                menu_open,
            };
            if let Some(at) = panels::decide(&mut self.bars[bar.index()], input, now) {
                next = Some(next.map_or(at, |next| next.min(at)));
            }
        }
        if let Some(at) = next {
            self.schedule_bar_check(at);
        }
    }

    /// `at`에 막대를 다시 본다 (꺼낸 막대를 떠난 뒤 숨기기)
    fn schedule_bar_check(&mut self, at: Instant) {
        if let Some((token, when)) = self.bar_timer {
            if when == at {
                return;
            }
            self.loop_handle.remove(token);
            self.bar_timer = None;
        }
        let token = self
            .loop_handle
            .insert_source(Timer::from_deadline(at), |_, _, state| {
                state.bar_timer = None;
                state.update_bars(None);
                TimeoutAction::Drop
            });
        self.bar_timer = token.ok().map(|token| (token, at));
    }

    /// 레이어를 그릴 자리의 옮김. `geo`는 출력 안 자리이고 `output_h`는 출력 높이다. 숨는 중인 막대는
    /// 미끄러진 만큼 화면 밖으로 옮기고, 다 숨은 막대는 None이다 (그리지 않고 입력도 보내지 않는다)
    fn layer_offset(
        &self,
        layer: &LayerSurface,
        geo: Rectangle<i32, Logical>,
        output_h: i32,
        now: Instant,
    ) -> Option<Point<i32, Logical>> {
        let Some(bar) = self.bar_of(layer) else {
            return Some(Point::default());
        };
        let value = self.bars[bar.index()].value(now);
        if value <= 0.0 {
            return None;
        }
        // 가장자리와 막대 사이의 여백까지 지나야 다 숨는다
        let (distance, direction) = match bar {
            Bar::MenuBar => (geo.loc.y + geo.size.h, -1),
            Bar::Dock => (output_h - geo.loc.y, 1),
        };
        let moved = ((1.0 - value) * f64::from(distance.max(0))).round() as i32;
        Some((0, direction * moved).into())
    }

    /// 다 보이지 않는 막대와 그 막대를 그릴 자리 (출력 안 좌표). 자리가 None이면 숨었다
    pub fn bar_places(&self, output: &Output) -> Vec<render::BarPlace> {
        let Some(output_h) = self.space.output_geometry(output).map(|geo| geo.size.h) else {
            return Vec::new();
        };
        let now = Instant::now();
        let map = layer_map_for_output(output);
        map.layers()
            .filter_map(|layer| {
                self.bar_of(layer)?;
                let geo = map.layer_geometry(layer)?;
                let offset = self.layer_offset(layer, geo, output_h, now);
                (offset != Some(Point::default())).then(|| render::BarPlace {
                    layer: layer.clone(),
                    at: offset.map(|offset| geo.loc + offset),
                })
            })
            .collect()
    }

    /// 위층(overlay, top) 레이어 가운데 좌표 아래의 표면과 그 표면의 전역 위치. 숨은 막대는 건너뛰고
    /// 미끄러지는 막대는 옮긴 자리로 본다
    fn upper_layer_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(LayerSurface, WlSurface, Point<f64, Logical>)> {
        let output = self.space.output_under(pos).next()?;
        let output_geo = self.space.output_geometry(output)?;
        let map = layer_map_for_output(output);
        let relative = pos - output_geo.loc.to_f64();
        let now = Instant::now();
        [Layer::Overlay, Layer::Top].into_iter().find_map(|kind| {
            map.layers_on(kind).rev().find_map(|layer| {
                let geo = map.layer_geometry(layer)?;
                let loc = geo.loc + self.layer_offset(layer, geo, output_geo.size.h, now)?;
                let (surface, at) =
                    layer.surface_under(relative - loc.to_f64(), WindowSurfaceType::ALL)?;
                Some((layer.clone(), surface, (at + loc + output_geo.loc).to_f64()))
            })
        })
    }

    /// 셸이 막대 설정을 바꿨다. 화면을 채우는 창을 새 영역에 맞춘다
    fn set_panels(&mut self, panels: Panels) {
        if panels == self.panels {
            return;
        }
        self.panels = panels;
        tracing::info!(
            "menu bar hides {:?}, Dock hides {:?}, menu bar background {}",
            panels.menu_bar,
            panels.dock,
            panels.menu_bar_background
        );
        self.schedule_refill();
        self.update_bars(None);
    }

    /// 레이어가 바뀐 뒤 부른다. 창 영역이 달라졌으면(막대의 독점 영역, 자동 가리기) 조금 뒤
    /// 화면을 채우는 창을 다시 맞춘다
    fn watch_window_areas(&mut self) {
        let areas = match (
            self.usable_area(),
            self.window_area(Fill::Maximized),
            self.window_area(Fill::Fullscreen),
        ) {
            (Some(usable), Some(maximized), Some(fullscreen)) => [usable, maximized, fullscreen],
            _ => return,
        };
        if self
            .window_areas
            .replace(areas)
            .is_some_and(|old| old != areas)
        {
            self.schedule_refill();
        }
    }

    /// REFILL_DELAY 뒤 refill_windows를 부른다. 그 사이에 또 바뀌면 다시 기다린다
    fn schedule_refill(&mut self) {
        if let Some(token) = self.refill_timer.take() {
            self.loop_handle.remove(token);
        }
        let token =
            self.loop_handle
                .insert_source(Timer::from_duration(REFILL_DELAY), |_, _, state| {
                    state.refill_timer = None;
                    state.refill_windows();
                    TimeoutAction::Drop
                });
        match token {
            Ok(token) => self.refill_timer = Some(token),
            Err(err) => {
                tracing::warn!("failed to schedule a window refit: {err}");
                self.refill_windows();
            }
        }
    }

    /// 막대가 바뀐 뒤 최대화·전체 화면 창을 새 영역에 맞춘다. 보통 창은 그대로 둔다
    /// (macOS에서도 Dock 크기를 바꿔도 창은 움직이지 않는다)
    fn refill_windows(&mut self) {
        self.window_areas = None;
        let Some(usable) = self.usable_area() else {
            return;
        };
        let windows: Vec<Window> = self.space.elements().cloned().collect();
        for window in &windows {
            let Some(toplevel) = window.toplevel() else {
                continue;
            };
            toplevel.with_pending_state(|state| state.bounds = Some(usable.size));
            if let Some(area) = self.filled_area(toplevel) {
                toplevel.with_pending_state(|state| state.size = Some(area.size));
                self.space.map_element(window.clone(), area.loc, false);
            }
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        for window in &windows {
            self.space.raise_element(window, false);
        }
        self.watch_window_areas();
    }

    /// 최대화·전체 화면 창이 채울 영역. 둘 다 아니면 None
    fn filled_area(&self, toplevel: &ToplevelSurface) -> Option<Rectangle<i32, Logical>> {
        if Self::has_state(toplevel, xdg_toplevel::State::Fullscreen) {
            self.window_area(Fill::Fullscreen)
        } else if Self::has_state(toplevel, xdg_toplevel::State::Maximized) {
            self.window_area(Fill::Maximized)
        } else {
            None
        }
    }

    pub fn has_state(surface: &ToplevelSurface, state: xdg_toplevel::State) -> bool {
        surface.with_pending_state(|pending| pending.states.contains(state))
    }

    /// 창의 최대화·전체 화면 상태를 바꾸고 그에 맞게 크기와 자리를 정한다. 최대화·전체 화면 창은
    /// window_area를 채우고, 둘 다 아니면 원래 자리로 되돌린다.
    pub fn set_window_state(
        &mut self,
        surface: &ToplevelSurface,
        state: xdg_toplevel::State,
        on: bool,
    ) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        let restore = window.user_data().get_or_insert(RestoreGeometry::default);
        let normal = !Self::has_state(surface, xdg_toplevel::State::Maximized)
            && !Self::has_state(surface, xdg_toplevel::State::Fullscreen);
        if on && normal {
            *restore.0.borrow_mut() = self.space.element_geometry(&window);
        }
        surface.with_pending_state(|pending| {
            if on {
                pending.states.set(state);
            } else {
                pending.states.unset(state);
            }
        });

        match self.filled_area(surface) {
            Some(area) => {
                surface.with_pending_state(|pending| pending.size = Some(area.size));
                self.space.map_element(window.clone(), area.loc, true);
            }
            None => {
                let previous = restore.0.borrow_mut().take();
                surface.with_pending_state(|pending| {
                    // 크기를 비우면 앱이 스스로 크기를 고른다
                    pending.size = previous.map(|geo| geo.size).filter(|size| !size.is_empty());
                });
                if let Some(geo) = previous {
                    self.space.map_element(window.clone(), geo.loc, true);
                }
            }
        }
        // 첫 configure 전이면 첫 configure에 함께 실린다
        if surface.is_initial_configure_sent() {
            surface.send_pending_configure();
        }
        self.refresh_window_info(&window);
    }
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    /// 컴포지터가 직접 띄운 셸. 창 목록·창 조작 프로토콜을 쓸 수 있다
    pub privileged: bool,
    /// 컴포지터가 직접 띄운 입력기. 키보드를 잡고 글자와 키를 대신 보낼 수 있다
    pub input_method: bool,
}

fn is_input_method_client(client: &Client) -> bool {
    client
        .get_data::<ClientState>()
        .is_some_and(|data| data.input_method)
}

/// 표면 하나에 배율과 회전을 알린다. 소수 배율을 아는 앱(wp_fractional_scale_v1)은 그 배율로 그리고,
/// 모르는 앱은 올림한 정수 배율로 그린 것을 컴포지터가 줄여 그린다
fn send_scale_state(
    surface: &WlSurface,
    data: &SurfaceData,
    scale: OutputScale,
    transform: Transform,
) {
    send_surface_state(surface, data, scale.integer_scale(), transform);
    with_fractional_scale(data, |fractional| {
        fractional.set_preferred_scale(scale.fractional_scale())
    });
}

/// 창을 영역 안에 들인다. 들어가면 옮기기만 하고, 영역보다 크면 그 방향을 영역 크기로 줄인다
fn fit_into(
    rect: Rectangle<i32, Logical>,
    area: Rectangle<i32, Logical>,
) -> Rectangle<i32, Logical> {
    let size = Size::from((rect.size.w.min(area.size.w), rect.size.h.min(area.size.h)));
    let x = rect
        .loc
        .x
        .min(area.loc.x + area.size.w - size.w)
        .max(area.loc.x);
    let y = rect
        .loc
        .y
        .min(area.loc.y + area.size.h - size.h)
        .max(area.loc.y);
    Rectangle::new((x, y).into(), size)
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
                self.announce_window(&window);
            }
        }
        self.popups.commit(surface);
        // 레이어 표면이 나타나거나 사라지거나 키보드 설정을 바꾸면 키보드 포커스를 다시 정한다
        if self.is_layer_surface(surface) {
            self.refresh_layer_focus();
        }
        // 입력기 팝업은 크기가 바뀌면 화면 안에 들도록 다시 놓는다
        if let Some(PopupKind::InputMethod(popup)) = self.popups.find_popup(surface) {
            self.place_input_method_popup(&popup);
        }
        handle_resize_commit(&mut self.space, surface);
        self.ensure_initial_configure(surface);
        self.send_scale(surface);
        // 막대의 독점 영역이 바뀌었으면(Dock 크기, 자동 가리기) 화면을 채우는 창을 다시 맞춘다
        if self.is_layer_surface(surface) {
            self.watch_window_areas();
        }
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
        if Self::has_state(&surface, xdg_toplevel::State::Maximized) {
            let maximized_width = window.geometry().size.w.max(1);
            let restored_width = window
                .user_data()
                .get::<RestoreGeometry>()
                .and_then(|restore| *restore.0.borrow())
                .map_or(maximized_width, |geo| geo.size.w);
            self.set_window_state(&surface, xdg_toplevel::State::Maximized, false);
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
        self.set_window_state(&surface, xdg_toplevel::State::Maximized, true);
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        self.set_window_state(&surface, xdg_toplevel::State::Maximized, false);
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<wl_output::WlOutput>,
    ) {
        self.set_window_state(&surface, xdg_toplevel::State::Fullscreen, true);
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        self.set_window_state(&surface, xdg_toplevel::State::Fullscreen, false);
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            self.refresh_window_info(&window);
        }
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        if let Some(window) = self.window_for_surface(surface.wl_surface()) {
            self.refresh_window_info(&window);
        }
    }

    /// 창이 닫히면 셸에 알리고, 활성 창이 없어졌으면 맨 위 창을 활성화한다.
    /// (메뉴바의 메뉴로 앱을 끝내면 키보드 포커스가 메뉴바에 있는 채로 창이 닫힌다)
    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some(window) = self.window_for_surface(surface.wl_surface()) else {
            return;
        };
        self.forget_window(&window);
        self.space.unmap_elem(&window);
        if self.active_window().is_none() {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            match self.space.elements().last().cloned() {
                Some(next) => self.focus_window(&next, serial),
                None => self.clear_focus(serial),
            }
        }
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

        // 다른 그랩이 진행 중이면 팝업 그랩을 거절한다. 이어지는 하위 메뉴의 그랩은 허용한다.
        // 입력기(fcitx5)가 입력 칸 때문에 잡은 키보드는 예외다. 포커스가 팝업으로 옮겨 가면 입력기가
        // 놓는다 (막으면 입력 칸에 포커스가 있을 때 메뉴가 키보드를 받지 못해 Esc로 닫히지 않는다)
        if let Some(keyboard) = seat.get_keyboard() {
            if keyboard.is_grabbed()
                && !seat.input_method().keyboard_grabbed()
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
        // 숨겼다 다시 띄우는 표면이면 layer_surface_destroyed에서 채운 기준 가장자리를 비운다
        with_states(surface.wl_surface(), |states| {
            states
                .cached_state
                .get::<LayerSurfaceCachedState>()
                .pending()
                .anchor = Anchor::empty();
        });
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
        self.refresh_layer_focus();
        self.watch_window_areas();
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
        // 키보드를 받지 않는 레이어(메뉴바, Dock)로 포커스가 왔다면 그 레이어의 팝업 메뉴가
        // 닫힌 것이다. 키보드를 독점하는 레이어나 앞에 있던 창에 돌려준다. 포커스를 바꾸는 중이므로
        // 다음 차례에 한다
        if focused.is_some_and(|surface| self.is_passive_layer(surface)) {
            self.loop_handle
                .insert_idle(|state| state.refresh_layer_focus());
        }
    }
}

impl BuntState {
    /// 키보드 입력을 받지 않는 레이어 표면인지
    fn is_passive_layer(&self, surface: &WlSurface) -> bool {
        self.space.outputs().any(|output| {
            layer_map_for_output(output)
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .is_some_and(|layer| {
                    layer.cached_state().keyboard_interactivity == KeyboardInteractivity::None
                })
        })
    }

    /// 활성 상태인 창
    fn active_window(&self) -> Option<Window> {
        self.space
            .elements()
            .rev()
            .find(|window| {
                window.toplevel().is_some_and(|toplevel| {
                    Self::has_state(toplevel, xdg_toplevel::State::Activated)
                })
            })
            .cloned()
    }

    /// 레이어 표면 (최상위 표면)인지
    fn is_layer_surface(&self, surface: &WlSurface) -> bool {
        self.layer_keyboard(surface).is_some()
    }

    /// 레이어 표면의 키보드 설정과 버퍼가 붙어 보이는지. 레이어 표면이 아니면 None
    fn layer_keyboard(&self, surface: &WlSurface) -> Option<(KeyboardInteractivity, bool)> {
        self.space.outputs().find_map(|output| {
            layer_map_for_output(output)
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .map(|layer| {
                    (
                        layer.cached_state().keyboard_interactivity,
                        has_buffer(layer.wl_surface()),
                    )
                })
        })
    }

    /// 키보드를 독점하는(exclusive) 위층(overlay, top) 레이어 표면 가운데 맨 위의 것.
    /// 버퍼가 붙어 보이는 것만 센다 (wlr-layer-shell)
    pub fn exclusive_layer(&self) -> Option<WlSurface> {
        self.space.outputs().find_map(|output| {
            let map = layer_map_for_output(output);
            [Layer::Overlay, Layer::Top].into_iter().find_map(|layer| {
                map.layers_on(layer)
                    .rev()
                    .find(|layer| {
                        layer.cached_state().keyboard_interactivity
                            == KeyboardInteractivity::Exclusive
                            && has_buffer(layer.wl_surface())
                    })
                    .map(|layer| layer.wl_surface().clone())
            })
        })
    }

    /// 표면이 속한 최상위 표면. 하위 표면과 xdg 팝업을 거슬러 올라간다
    fn root_surface(&self, surface: &WlSurface) -> WlSurface {
        let mut root = surface.clone();
        while let Some(parent) = get_parent(&root) {
            root = parent;
        }
        self.popups
            .find_popup(&root)
            .and_then(|popup| find_popup_root_surface(&popup).ok())
            .unwrap_or(root)
    }

    /// 키보드 포커스를 둘 수 없는 최상위 표면인지: 사라졌거나, 창도 아니고 키보드를 받는 보이는
    /// 레이어 표면도 아니다 (레이어 역할을 잃은 표면 포함)
    fn cannot_hold_focus(&self, root: &WlSurface) -> bool {
        if !root.is_alive() {
            return true;
        }
        if self.window_for_surface(root).is_some() {
            return false;
        }
        match self.layer_keyboard(root) {
            Some((interactivity, mapped)) => {
                interactivity == KeyboardInteractivity::None || !mapped
            }
            None => true,
        }
    }

    /// wlr-layer-shell 명세대로 레이어 표면의 키보드 포커스를 맞춘다. 키보드를 독점하는 위층 레이어
    /// 표면(모든 앱 패널 등)이 보이면 그 표면(이나 그 팝업)에 포커스를 두고, 그런 표면이 없는데 포커스가
    /// 사라진 표면이나 키보드를 받지 않는 레이어 표면(팝업 메뉴를 닫은 메뉴바 포함)에 남았으면 활성 창
    /// (없으면 맨 위 창)에 돌려준다. 열려 있는 팝업 메뉴의 포커스는 건드리지 않는다
    pub fn refresh_layer_focus(&mut self) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let focus = keyboard.current_focus();
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        if let Some(layer) = self.exclusive_layer() {
            let inside = focus
                .as_ref()
                .is_some_and(|focus| focus.is_alive() && self.root_surface(focus) == layer);
            if !inside {
                keyboard.set_focus(self, Some(layer), serial);
            }
            return;
        }
        // 팝업 메뉴는 그랩하는 동안 키보드를 가진다. 메뉴바처럼 키보드를 받지 않는 레이어의 팝업도
        // 그렇다 (그래야 Esc와 화살표가 메뉴로 간다)
        let stale = focus.as_ref().is_some_and(|focus| {
            !(focus.is_alive() && self.popups.find_popup(focus).is_some())
                && self.cannot_hold_focus(&self.root_surface(focus))
        });
        if stale {
            match self
                .active_window()
                .or_else(|| self.space.elements().last().cloned())
            {
                Some(window) => self.focus_window(&window, serial),
                None => self.clear_focus(serial),
            }
        }
    }

    /// 뒤를 흐릴 셸 표면: 특권 셸 클라이언트의 위층(overlay, top) 레이어 표면과 그 xdg 팝업.
    /// 메뉴바는 macOS 26처럼 흐리지 않는다(투명하거나 검은색). "메뉴 막대 배경 보기"를 켜면 흐린다.
    /// 메뉴와 제어 센터 팝업은 늘 흐린다
    pub fn blur_surfaces(&self, output: &Output) -> Vec<WlSurface> {
        let output_h = self
            .space
            .output_geometry(output)
            .map_or(0, |geo| geo.size.h);
        let map = layer_map_for_output(output);
        let now = Instant::now();
        let mut surfaces = Vec::new();
        for layer in map.layers() {
            let surface = layer.wl_surface();
            if !matches!(layer.layer(), Layer::Overlay | Layer::Top) || !self.is_privileged(surface)
            {
                continue;
            }
            // 다 숨은 막대는 그리지 않는다
            if let Some(geo) = map.layer_geometry(layer)
                && self.layer_offset(layer, geo, output_h, now).is_none()
            {
                continue;
            }
            if layer.namespace() != MENUBAR_NAMESPACE || self.panels.menu_bar_background {
                surfaces.push(surface.clone());
            }
            surfaces.extend(
                PopupManager::popups_for_surface(surface)
                    .filter(|(popup, _)| matches!(popup, PopupKind::Xdg(_)))
                    .map(|(popup, _)| popup.wl_surface().clone()),
            );
        }
        surfaces
    }

    /// 컴포지터가 직접 띄운 셸의 표면인지
    fn is_privileged(&self, surface: &WlSurface) -> bool {
        self.display_handle
            .get_client(surface.id())
            .ok()
            .and_then(|client| client.get_data::<ClientState>().map(|data| data.privileged))
            .unwrap_or(false)
    }

    /// 누른 자리의 위층 레이어 표면이 키보드를 받으면(on_demand, exclusive) 그 표면을 돌려준다
    pub fn keyboard_layer_under(&self, pos: Point<f64, Logical>) -> Option<WlSurface> {
        self.upper_layer_under(pos)
            .filter(|(layer, ..)| {
                layer.cached_state().keyboard_interactivity != KeyboardInteractivity::None
            })
            .map(|(layer, ..)| layer.wl_surface().clone())
    }

    /// 레이어 표면에 키보드 포커스를 준다 (on_demand 레이어를 눌렀을 때)
    pub fn focus_layer(&mut self, surface: &WlSurface, serial: Serial) {
        if let Some(keyboard) = self.seat.get_keyboard()
            && keyboard
                .current_focus()
                .is_none_or(|focus| self.root_surface(&focus) != *surface)
        {
            keyboard.set_focus(self, Some(surface.clone()), serial);
        }
    }

    /// 표면에 알릴 배율과 회전 (출력이 여럿이면 첫 출력 기준)
    fn preferred_scale(&self) -> Option<(OutputScale, Transform)> {
        let output = self.space.outputs().next()?;
        Some((output.current_scale(), output.current_transform()))
    }

    /// 표면에 출력의 배율과 회전을 알린다. 바뀐 값만 보낸다
    fn send_scale(&self, surface: &WlSurface) {
        if let Some((scale, transform)) = self.preferred_scale() {
            with_states(surface, |data| {
                send_scale_state(surface, data, scale, transform)
            });
        }
    }

    /// 배율이 바뀐 뒤 모든 창, 레이어 표면, 팝업에 새 배율을 알린다
    pub fn send_scale_to_all(&self) {
        let Some((scale, transform)) = self.preferred_scale() else {
            return;
        };
        let mut roots: Vec<WlSurface> = self
            .space
            .elements()
            .filter_map(|window| window.toplevel().map(|t| t.wl_surface().clone()))
            .collect();
        for output in self.space.outputs() {
            roots.extend(
                layer_map_for_output(output)
                    .layers()
                    .map(|layer| layer.wl_surface().clone()),
            );
        }
        let popups: Vec<WlSurface> = roots
            .iter()
            .flat_map(|root| {
                PopupManager::popups_for_surface(root).map(|(popup, _)| popup.wl_surface().clone())
            })
            .collect();
        // 표면 상태는 순회하는 동안 잠겨 있으므로 with_states를 다시 부르지 않고 받은 상태를 쓴다
        for root in roots.iter().chain(&popups) {
            with_surface_tree_downward(
                root,
                (),
                |_, _, _| TraversalAction::DoChildren(()),
                |surface, data, _| send_scale_state(surface, data, scale, transform),
                |_, _, _| true,
            );
        }
    }

    /// 화면 크기나 배율이 바뀐 뒤 창을 새 영역에 맞춘다. 레이어(메뉴바, Dock)는 먼저 배치해 둔다.
    /// 최대화·전체 화면 창은 새 영역을 채운다. 나머지 창은 macOS처럼 화면 안으로 옮기고, 그래도
    /// 넘치면 영역에 맞게 줄인다. 최대화를 풀 때 돌아갈 자리도 새 영역에 맞춘다
    pub fn refit_windows(&mut self) {
        let Some(usable) = self.usable_area() else {
            return;
        };
        let windows: Vec<Window> = self.space.elements().cloned().collect();
        for window in &windows {
            let Some(toplevel) = window.toplevel() else {
                continue;
            };
            toplevel.with_pending_state(|state| state.bounds = Some(usable.size));
            if let Some(restore) = window.user_data().get::<RestoreGeometry>() {
                let mut restore = restore.0.borrow_mut();
                if let Some(geo) = restore.as_mut() {
                    *geo = fit_into(*geo, usable);
                }
            }
            match self.filled_area(toplevel) {
                Some(area) => {
                    toplevel.with_pending_state(|state| state.size = Some(area.size));
                    self.space.map_element(window.clone(), area.loc, false);
                }
                None => {
                    let Some(geo) = self.space.element_geometry(window) else {
                        continue;
                    };
                    let fitted = fit_into(geo, usable);
                    if fitted.size != geo.size {
                        toplevel.with_pending_state(|state| state.size = Some(fitted.size));
                    }
                    if fitted.loc != geo.loc {
                        self.space.map_element(window.clone(), fitted.loc, false);
                    }
                }
            }
            if toplevel.is_initial_configure_sent() {
                toplevel.send_pending_configure();
            }
        }
        // map_element는 옮긴 창을 맨 위로 올린다. 원래 쌓인 순서(아래부터)대로 다시 올려 되돌린다
        for window in &windows {
            self.space.raise_element(window, false);
        }
        self.window_areas = None;
        self.watch_window_areas();
    }

    pub fn handle_request(&mut self, request: Request) {
        match request {
            Request::OutputMode {
                name,
                width,
                height,
                refresh,
            } => self.set_output_mode(&name, width, height, refresh),
            Request::OutputScale { name, scale } => self.set_output_scale(&name, scale),
            Request::KeyboardRepeat { delay, rate } => self.set_keyboard_repeat(delay, rate),
            Request::Panels(panels) => self.set_panels(panels),
        }
    }

    fn set_keyboard_repeat(&mut self, delay: u32, rate: u32) {
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.change_repeat_info(rate as i32, delay as i32);
        }
        self.prefs.keyboard = Some(KeyRepeat { delay, rate });
        self.prefs.save();
        self.dbus.update(|snapshot| {
            snapshot.repeat_delay = delay;
            snapshot.repeat_rate = rate;
        });
        tracing::info!("key repeat: {delay} ms, {rate}/s");
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

impl FractionalScaleHandler for BuntState {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        self.send_scale(&surface);
    }
}

delegate_compositor!(BuntState);
delegate_shm!(BuntState);
delegate_xdg_shell!(BuntState);
// 레이어 셸은 smithay의 처리에 맡기고, 레이어 표면 객체가 없어질 때만 손본다
delegate_dispatch!(BuntState: [ZwlrLayerShellV1: ()] => WlrLayerShellState);
delegate_global_dispatch!(BuntState: [ZwlrLayerShellV1: WlrLayerShellGlobalData] => WlrLayerShellState);

impl Dispatch<ZwlrLayerSurfaceV1, WlrLayerSurfaceUserData> for BuntState {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &ZwlrLayerSurfaceV1,
        request: zwlr_layer_surface_v1::Request,
        data: &WlrLayerSurfaceUserData,
        display: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        <WlrLayerShellState as Dispatch<ZwlrLayerSurfaceV1, WlrLayerSurfaceUserData, Self>>::request(
            state, client, resource, request, data, display, data_init,
        );
    }

    /// smithay(0.7)는 레이어 표면 객체가 없어지면 표면의 레이어 상태를 기본값(크기 0, 기준 가장자리
    /// 없음)으로 되돌리지만, 크기를 확인하는 커밋 훅은 표면에 남겨 둔다. 그래서 창을 숨기려고 객체를
    /// 없앤 뒤 빈 버퍼를 커밋하는 클라이언트(gtk4-layer-shell)가 이미 없는 객체의 프로토콜 오류로
    /// 연결이 끊긴다. 기준 가장자리를 모두 채워 그 확인을 통과시킨다. 같은 표면에 레이어를 다시
    /// 만들면 new_layer_surface에서 비운다.
    fn destroyed(
        state: &mut Self,
        client: ClientId,
        resource: &ZwlrLayerSurfaceV1,
        data: &WlrLayerSurfaceUserData,
    ) {
        let surface = state
            .layer_shell_state
            .layer_surfaces()
            .find(|layer| layer.shell_surface() == resource)
            .map(|layer| layer.wl_surface().clone());
        <WlrLayerShellState as Dispatch<ZwlrLayerSurfaceV1, WlrLayerSurfaceUserData, Self>>::destroyed(
            state, client, resource, data,
        );
        if let Some(surface) = surface.filter(Resource::is_alive) {
            with_states(&surface, |states| {
                states
                    .cached_state
                    .get::<LayerSurfaceCachedState>()
                    .pending()
                    .anchor = Anchor::all();
            });
        }
    }
}

delegate_seat!(BuntState);
delegate_data_device!(BuntState);
delegate_output!(BuntState);
delegate_viewporter!(BuntState);
delegate_fractional_scale!(BuntState);

/// 표면에 버퍼가 붙어 화면에 보이는지
fn has_buffer(surface: &WlSurface) -> bool {
    with_renderer_surface_state(surface, |state| state.buffer().is_some()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn windows_move_back_inside_a_smaller_screen() {
        let area = rect(0, 28, 1024, 508);
        // 들어가는 창은 그대로
        assert_eq!(
            fit_into(rect(100, 100, 600, 400), area),
            rect(100, 100, 600, 400)
        );
        // 오른쪽 아래로 넘친 창은 크기를 지키고 안으로 옮긴다
        assert_eq!(
            fit_into(rect(800, 300, 600, 400), area),
            rect(424, 136, 600, 400)
        );
        // 메뉴바 위로 올라간 창은 메뉴바 아래로
        assert_eq!(
            fit_into(rect(10, 0, 600, 400), area),
            rect(10, 28, 600, 400)
        );
        // 영역보다 큰 창은 영역에 맞게 줄인다
        assert_eq!(fit_into(rect(0, 28, 1280, 668), area), area);
        assert_eq!(
            fit_into(rect(300, 40, 900, 640), area),
            rect(124, 28, 900, 508)
        );
    }
}
