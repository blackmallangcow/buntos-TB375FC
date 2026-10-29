// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRM/KMS + libinput 백엔드 (GPU 하나). Smithay anvil의 udev 백엔드 구조를 단순화했다.
//! 하드웨어 GPU가 없는 VM에서는 Mesa의 소프트웨어 렌더러(llvmpipe)로 그린다.

use std::{
    cell::Cell,
    cmp::Reverse,
    collections::HashMap,
    error::Error,
    path::PathBuf,
    time::{Duration, Instant},
};

use smithay::{
    backend::{
        allocator::{
            Fourcc,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
        },
        egl::{EGLContext, EGLDisplay},
        input::InputEvent,
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportDma, ImportMemWl,
            element::{
                Element, Id, Kind, RenderElementStates, default_primary_scanout_output_compare,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::SolidColorRenderElement,
                surface::render_elements_from_surface_tree,
            },
            gles::GlesRenderer,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    desktop::{
        layer_map_for_output,
        space::space_render_elements,
        utils::{
            send_frames_surface_tree, surface_primary_scanout_output,
            update_surface_primary_scanout_output, with_surfaces_surface_tree,
        },
    },
    input::pointer::{CursorIcon, CursorImageStatus, CursorImageSurfaceData},
    output::{Mode as WlMode, Output, PhysicalProperties, Scale as OutputScale},
    reexports::{
        calloop::{
            EventLoop, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::{
            ClientCapability, Device as _,
            control::{Device as ControlDevice, Mode as DrmMode, ModeTypeFlags, connector, crtc},
        },
        input::{Device as InputDevice, DeviceCapability, Libinput},
        rustix::fs::OFlags,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Display, backend::GlobalId, protocol::wl_surface::WlSurface},
    },
    utils::{DeviceFd, IsAlive, Logical, Physical, Point, Scale, Size, Transform},
    wayland::compositor::{SurfaceData, with_states},
};

use crate::{
    blur::Blur,
    config::{Config, parse_hex_color},
    cursor::Cursor,
    dbus::{Command, OutputInfo, PointerDevice},
    dmabuf::Dmabufs,
    gpu::{GSK_RENDERER, Rendering, renderer_name},
    prefs::{ModePref, PointerPrefs},
    render::{self, OutputRenderElements},
    screenshot::{self, AreaSelection},
    state::BuntState,
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Elements = OutputRenderElements;

/// 화면 갱신이 없을 때 다시 확인하는 간격
const IDLE_REDRAW: Duration = Duration::from_millis(16);
/// 이보다 오래 걸린 프레임은 디버그 기록에 남긴다 (RUST_LOG=bunt_comp=debug)
const SLOW_FRAME: Duration = Duration::from_millis(12);
/// 소프트웨어 렌더러에서 프레임 사이의 최소 간격 (60Hz)
const SOFTWARE_FRAME: Duration = Duration::from_micros(16_667);
/// 소프트웨어 렌더러에서 앞에 있지 않은 창에 프레임 알림을 보내는 간격 (30Hz)
const BACKGROUND_FRAME: Duration = Duration::from_micros(33_333);
/// 화면에 보이지 않은 표면(다른 창에 다 가려진 창, 숨은 막대)에 프레임 알림을 보내는 간격
const HIDDEN_FRAME_INTERVAL: Option<Duration> = Some(Duration::from_secs(1));
const CLEAR_COLOR: [f32; 4] = [0.08, 0.08, 0.1, 1.0];
const FALLBACK_TOP: [u8; 3] = [0x5b, 0x7c, 0xfa];
const FALLBACK_BOTTOM: [u8; 3] = [0xf2, 0xa7, 0xc3];
/// 배율을 고를 때 남겨야 하는 가장 작은 논리 화면 크기. macOS의 "더 큰 텍스트"도 1024×640보다
/// 작아지지 않는다. 이보다 작으면 메뉴바, Dock, 앱의 최소 크기가 화면을 넘는다
const MIN_LOGICAL_WIDTH: f64 = 1024.0;
const MIN_LOGICAL_HEIGHT: f64 = 640.0;
/// 배율 단계. wp_fractional_scale_v1은 1/120 단위라 0.25 단위는 그대로 전해진다
const SCALE_STEP: f64 = 0.25;
const MAX_SCALE: f64 = 4.0;
/// 시스템 설정에 보이는 배율 선택지 수 (macOS도 다섯 개를 보인다)
const SCALE_CHOICES: usize = 5;
/// 기본 배율이 맞추려는 논리 화면 밀도(ppi). 노트북·태블릿 화면은 macOS 노트북의 기본값,
/// 모니터는 macOS 5K 모니터의 기본값 가까이
const BUILTIN_PPI: f64 = 125.0;
const EXTERNAL_PPI: f64 = 110.0;

pub struct UdevData {
    pub session: LibSeatSession,
    renderer: GlesRenderer,
    output_manager: DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>,
    outputs: HashMap<crtc::Handle, OutputSurface>,
    cursor: Cursor,
    /// 컴포지터가 그리는 커서(기본 화살표, 앱이 모양으로 고른 커서). 모양, 출력 배율, 크기가 바뀌면
    /// 다시 만든다
    cursor_buffer: MemoryRenderBuffer,
    cursor_hotspot: Point<f64, Logical>,
    cursor_key: Option<(CursorIcon, i32, u32)>,
    /// 연결된 포인터 장치(마우스, 터치패드). 시스템 설정의 마우스 설정을 쓴다
    pointer_devices: Vec<InputDevice>,
    /// 셸 유리 뒤 흐림 (blur.rs)
    blur: Blur,
    /// 렌더러로 정한 그리기 방식 (gpu.rs). 전에는 CPU로 그리는지만 적었다(software)
    rendering: Rendering,
    /// 컴포지터가 띄우는 프로그램에 더할 환경 변수 (gpu.rs)
    client_env: Vec<(&'static str, &'static str)>,
    /// 앱이 GPU로 그린 버퍼 (dmabuf.rs)
    pub dmabufs: Dmabufs,
}

impl UdevData {
    pub fn seat_name(&self) -> String {
        self.session.seat()
    }

    /// 화면 밖에 따로 그릴 때 (에이전트의 창 스크린샷), 앱의 dmabuf를 가져올 때
    pub fn renderer(&mut self) -> &mut GlesRenderer {
        &mut self.renderer
    }

    /// 컴포지터가 띄우는 프로그램에 더할 환경 변수 (앱의 그리기 방식)
    pub fn client_env(&self) -> &[(&'static str, &'static str)] {
        &self.client_env
    }
}

/// 뒤에 있는 창에 마지막으로 프레임 알림을 보낸 때 (BACKGROUND_FRAME)
struct BackgroundFrame(Cell<Duration>);

struct OutputSurface {
    output: Output,
    /// 화면 크기로 고른 기본 배율 (recommended_scale)
    preferred_scale: f64,
    drm_output: DrmOutput<Allocator, Exporter, (), DrmDeviceFd>,
    connector: connector::Handle,
    /// 커넥터가 알려 준 모드. 화면이 권하는 모드가 먼저다 (sorted_modes)
    modes: Vec<DrmMode>,
    mode: DrmMode,
    /// 기기에 붙은 화면 (eDP, LVDS, DSI)
    builtin: bool,
    /// EDID의 모니터 이름이나 제조사. 모르면 빈 문자열
    description: String,
    wallpaper: MemoryRenderBuffer,
    /// 페이지 플립을 기다리는 프레임이 있다
    frame_pending: bool,
    /// 마지막으로 프레임을 내보낸 때 (소프트웨어 렌더러의 60Hz 제한)
    rendered_at: Option<Instant>,
    redraw_timer: Option<RegistrationToken>,
    global: GlobalId,
}

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let mut event_loop: EventLoop<'static, BuntState> = EventLoop::try_new()?;
    let display: Display<BuntState> = Display::new()?;

    let (mut session, session_notifier) = LibSeatSession::new()?;
    let seat_name = session.seat();

    let udev = UdevBackend::new(&seat_name)?;
    let gpu = primary_gpu(&seat_name)?
        .and_then(|path| DrmNode::from_path(path).ok())
        .or_else(|| {
            all_gpus(&seat_name)
                .ok()?
                .into_iter()
                .find_map(|path| DrmNode::from_path(path).ok())
        })
        .ok_or("no GPU found")?;
    let primary = gpu
        .node_with_type(NodeType::Primary)
        .and_then(Result::ok)
        .unwrap_or(gpu);
    let path: PathBuf = udev
        .device_list()
        .find(|(device_id, _)| *device_id == primary.dev_id())
        .map(|(_, path)| path.to_owned())
        .ok_or("GPU is not known to udev")?;
    let primary_id = primary.dev_id();
    tracing::info!("using GPU {}", path.display());

    let fd = session.open(
        &path,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
    )?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    // 가상 GPU(virtio-gpu 등)의 커널은 커서 핫스폿을 알리겠다고 한 클라이언트에게만 커서 평면을
    // 보인다. smithay는 이 기능을 켜지 않아 커서를 늘 화면에 합성했다. 이 기능은 원자적 모드를 켠
    // 뒤라야 켜지고, smithay는 DrmDevice를 만들 때 본 평면만 쓰므로 DrmDevice보다 먼저 둘 다 켠다.
    // 핫스폿 속성은 0으로 남지만, smithay가 커서 평면을 핫스폿만큼 옮겨 놓으므로 커서는 제자리에 그려진다
    if fd
        .set_client_capability(ClientCapability::Atomic, true)
        .is_ok()
        && fd
            .set_client_capability(ClientCapability::CursorPlaneHotspot, true)
            .is_ok()
    {
        tracing::info!("virtual cursor plane enabled");
    }
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;

    // 안전: GBM 장치는 EGL 디스플레이보다 오래 산다 (둘 다 UdevData가 끝까지 가진다)
    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let egl_context = EGLContext::new(&egl_display)?;
    let mut renderer = unsafe { GlesRenderer::new(egl_context)? };
    // 렌더러가 CPU로 그리는지만 보던 것을, 앱이 GPU로 그릴 수 있는지까지 정하게 바꿨다 (gpu.rs)
    let renderer_name = renderer_name(&mut renderer);
    let rendering = Rendering::from_renderer_name(&renderer_name);
    let inherited = std::env::var(GSK_RENDERER)
        .ok()
        .filter(|value| !value.is_empty());
    let client_env = rendering.client_env(inherited.as_deref());
    tracing::info!("renderer: {renderer_name}");
    match (&inherited, rendering.gpu_clients) {
        (Some(value), _) => tracing::info!("apps: {GSK_RENDERER}={value} from the session"),
        (None, true) => tracing::info!("apps: GTK picks its GPU renderer"),
        (None, false) => tracing::info!("apps: drawn with the CPU ({GSK_RENDERER}=cairo)"),
    }
    let render_formats: FormatSet = renderer.egl_context().dmabuf_render_formats().clone();
    let shm_formats: Vec<_> = renderer.shm_formats().collect();
    // 앱이 GPU로 그릴 수 있으면 화면 GPU의 렌더 노드로 linux-dmabuf를 연다
    let render_node = gpu.node_with_type(NodeType::Render).and_then(Result::ok);
    if rendering.gpu_clients && render_node.is_none() {
        tracing::info!("linux-dmabuf disabled: {gpu} has no render node");
    }
    let dmabuf_device = render_node
        .filter(|_| rendering.gpu_clients)
        .map(|node| (node, renderer.dmabuf_formats()));
    let dmabufs = Dmabufs::new::<BuntState>(&display.handle(), dmabuf_device);

    let output_manager = DrmOutputManager::new(
        drm,
        GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        ),
        // 앱 버퍼(dmabuf)를 화면 평면에 바로 올리지 않는다 (dmabuf.rs). 전에는 카드 노드를 주었지만
        // 앱 버퍼가 없어 쓰이지 않았다
        GbmFramebufferExporter::new(gbm.clone(), None),
        Some(gbm),
        [Fourcc::Abgr8888, Fourcc::Argb8888],
        render_formats,
    );

    let cursor = Cursor::load(&config.cursor.theme, config.cursor.size);
    let (cursor_buffer, cursor_hotspot) = cursor.buffer(CursorIcon::Default, 1);

    let backend = UdevData {
        session,
        renderer,
        output_manager,
        outputs: HashMap::new(),
        cursor,
        cursor_buffer,
        cursor_hotspot,
        cursor_key: None,
        pointer_devices: Vec::new(),
        blur: Blur::default(),
        rendering,
        client_env,
        dmabufs,
    };
    let mut state = BuntState::new(
        display,
        event_loop.handle(),
        event_loop.get_signal(),
        config,
        backend,
    )?;
    state.shm_state.update_formats(shm_formats);
    state.apply_pointer_prefs();

    event_loop
        .handle()
        .insert_source(drm_notifier, |event, _, state| match event {
            DrmEvent::VBlank(crtc) => state.frame_finish(crtc),
            DrmEvent::Error(err) => tracing::error!("DRM error: {err:?}"),
        })
        .map_err(|err| err.error)?;

    state.scan_connectors();
    if state.backend.outputs.is_empty() {
        return Err("no connected display".into());
    }
    state.publish_outputs();
    // 화면을 꽂거나 빼거나, 화면이 권하는 모드가 바뀌면(VM 창 크기 변경 등) 커널이 알린다
    event_loop
        .handle()
        .insert_source(udev, move |event, _, state| {
            if let UdevEvent::Changed { device_id } = event
                && device_id == primary_id
            {
                state.connectors_changed();
            }
        })
        .map_err(|err| err.error)?;

    let mut libinput = Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(
        state.backend.session.clone().into(),
    );
    libinput
        .udev_assign_seat(&seat_name)
        .map_err(|()| "failed to assign libinput seat")?;
    event_loop
        .handle()
        .insert_source(
            LibinputInputBackend::new(libinput.clone()),
            |event, _, state| {
                match &event {
                    InputEvent::DeviceAdded { device } => state.pointer_device_added(device),
                    InputEvent::DeviceRemoved { device } => state.pointer_device_removed(device),
                    _ => {}
                }
                state.process_input_event(event)
            },
        )
        .map_err(|err| err.error)?;

    event_loop
        .handle()
        .insert_source(session_notifier, move |event, _, state| match event {
            SessionEvent::PauseSession => {
                libinput.suspend();
                state.backend.output_manager.pause();
            }
            SessionEvent::ActivateSession => {
                if libinput.resume().is_err() {
                    tracing::warn!("failed to resume libinput");
                }
                if let Err(err) = state.backend.output_manager.activate(false) {
                    tracing::warn!("failed to activate DRM device: {err}");
                }
                for surface in state.backend.outputs.values_mut() {
                    surface.frame_pending = false;
                }
                state.render_all();
            }
        })
        .map_err(|err| err.error)?;

    tracing::info!("listening on {:?}", state.socket_name);
    for command in state.config.session.prepare.clone() {
        state.run_and_wait(&command);
    }
    let shell = state.config.session.shell.clone();
    state.spawn_privileged(&shell);
    state.spawn_input_method();
    for command in state.config.session.startup.clone() {
        state.spawn(&command);
    }
    state.render_all();

    event_loop.run(None, &mut state, |state| {
        state.space.refresh();
        state.popups.cleanup();
        if let Err(err) = state.display_handle.flush_clients() {
            tracing::warn!("failed to flush clients: {err}");
        }
    })?;
    Ok(())
}

impl BuntState {
    /// 연결된 커넥터 가운데 아직 출력이 없는 것에 출력을 만든다
    fn scan_connectors(&mut self) {
        let device = self.backend.output_manager.device();
        let resources = match device.resource_handles() {
            Ok(resources) => resources,
            Err(err) => {
                tracing::warn!("failed to read DRM resources: {err}");
                return;
            }
        };
        let mut used: Vec<crtc::Handle> = self.backend.outputs.keys().copied().collect();
        let known: Vec<connector::Handle> = self
            .backend
            .outputs
            .values()
            .map(|surface| surface.connector)
            .collect();
        let mut connected = Vec::new();
        for handle in resources.connectors() {
            if known.contains(handle) {
                continue;
            }
            let Ok(info) = device.get_connector(*handle, true) else {
                continue;
            };
            if info.state() != connector::State::Connected || info.modes().is_empty() {
                continue;
            }
            let crtc = info
                .encoders()
                .iter()
                .filter_map(|encoder| device.get_encoder(*encoder).ok())
                .flat_map(|encoder| resources.filter_crtcs(encoder.possible_crtcs()))
                .find(|crtc| !used.contains(crtc));
            if let Some(crtc) = crtc {
                used.push(crtc);
                connected.push((info, crtc));
            }
        }
        for (info, crtc) in connected {
            self.connector_connected(info, crtc);
        }
    }

    fn connector_connected(&mut self, info: connector::Info, crtc: crtc::Handle) {
        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        let modes = sorted_modes(&info);
        // 사용자가 고정한 모드가 있으면 그것을, 없으면 화면이 권하는 모드를 쓴다
        let saved = self.prefs.output(&name).cloned();
        let drm_mode = saved
            .as_ref()
            .and_then(|prefs| prefs.mode)
            .and_then(|pin| find_mode(&modes, pin))
            .unwrap_or(modes[0]);
        let wl_mode = WlMode::from(drm_mode);
        let (width_mm, height_mm) = info.size().unwrap_or((0, 0));
        let builtin = matches!(
            info.interface(),
            connector::Interface::EmbeddedDisplayPort
                | connector::Interface::LVDS
                | connector::Interface::DSI
        );
        let preferred_scale = recommended_scale(
            WlMode::from(modes[0]).size,
            (width_mm as i32, height_mm as i32),
            builtin,
        );
        // 사용자가 고른 배율이 없으면 화면 크기로 고른 기본 배율을 쓴다
        let scale = saved
            .and_then(|prefs| prefs.scale)
            .unwrap_or(preferred_scale);
        let scale = nearest_scale(&scales_for(wl_mode.size.w, wl_mode.size.h), scale);
        let (make, model) = edid_blob(self.backend.output_manager.device(), info.handle())
            .map(|edid| parse_edid(&edid))
            .unwrap_or_default();
        let description = if model.is_empty() {
            make.clone()
        } else {
            model.clone()
        };

        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: (width_mm as i32, height_mm as i32).into(),
                subpixel: info.subpixel().into(),
                make: if make.is_empty() {
                    "Unknown".into()
                } else {
                    make
                },
                model: if model.is_empty() {
                    "Unknown".into()
                } else {
                    model
                },
            },
        );
        let global = output.create_global::<BuntState>(&self.display_handle);
        let x = self
            .space
            .outputs()
            .filter_map(|output| self.space.output_geometry(output))
            .map(|geo| geo.loc.x + geo.size.w)
            .max()
            .unwrap_or(0);
        output.set_preferred(WlMode::from(modes[0]));
        output.change_current_state(
            Some(wl_mode),
            None,
            Some(output_scale(scale)),
            Some((x, 0).into()),
        );
        self.space.map_output(&output, (x, 0));

        let planes = self.backend.output_manager.device().planes(&crtc).ok();
        if let Some(planes) = &planes {
            tracing::info!(
                "output {name}: planes primary {}, cursor {}, overlay {}; cursor size {:?}",
                planes.primary.len(),
                planes.cursor.len(),
                planes.overlay.len(),
                self.backend.output_manager.device().cursor_size()
            );
        }
        let UdevData {
            renderer,
            output_manager,
            outputs,
            ..
        } = &mut self.backend;
        let drm_output = match output_manager.initialize_output::<_, Elements>(
            crtc,
            drm_mode,
            &[info.handle()],
            &output,
            planes,
            renderer,
            &DrmOutputRenderElements::default(),
        ) {
            Ok(drm_output) => drm_output,
            Err(err) => {
                tracing::warn!("failed to set up output {name}: {err:?}");
                self.space.unmap_output(&output);
                self.display_handle.remove_global::<BuntState>(global);
                return;
            }
        };

        let wallpaper = self.config.wallpaper_buffer(wl_mode);
        tracing::info!(
            "output {name}: {}x{} @ {} mHz, scale {scale}",
            wl_mode.size.w,
            wl_mode.size.h,
            wl_mode.refresh
        );
        outputs.insert(
            crtc,
            OutputSurface {
                output,
                preferred_scale,
                drm_output,
                connector: info.handle(),
                modes,
                mode: drm_mode,
                builtin,
                description,
                wallpaper,
                frame_pending: false,
                rendered_at: None,
                redraw_timer: None,
                global,
            },
        );
    }

    /// 화면을 꽂거나 뺐거나 화면이 권하는 모드가 바뀌었을 때(udev change 이벤트). 사용자가 모드를
    /// 고정하지 않은 출력은 화면이 새로 권하는 모드로 바꾼다
    fn connectors_changed(&mut self) {
        let mut removed = Vec::new();
        let mut changed = false;
        let crtcs: Vec<crtc::Handle> = self.backend.outputs.keys().copied().collect();
        for crtc in crtcs {
            let connector = self.backend.outputs[&crtc].connector;
            let info = match self
                .backend
                .output_manager
                .device()
                .get_connector(connector, true)
            {
                Ok(info)
                    if info.state() == connector::State::Connected && !info.modes().is_empty() =>
                {
                    info
                }
                _ => {
                    removed.push(crtc);
                    continue;
                }
            };
            let modes = sorted_modes(&info);
            let surface = self.backend.outputs.get_mut(&crtc).unwrap();
            if modes
                .iter()
                .map(mode_key)
                .eq(surface.modes.iter().map(mode_key))
            {
                continue;
            }
            changed = true;
            surface.output.set_preferred(WlMode::from(modes[0]));
            surface.modes = modes;
            let pinned = self
                .prefs
                .output(&surface.output.name())
                .and_then(|prefs| prefs.mode)
                .and_then(|pin| find_mode(&surface.modes, pin));
            let target = pinned.unwrap_or(surface.modes[0]);
            if mode_key(&target) != mode_key(&surface.mode) {
                self.apply_mode(crtc, target);
            }
        }
        for crtc in removed {
            self.remove_output(crtc);
            changed = true;
        }
        let before = self.backend.outputs.len();
        self.scan_connectors();
        if changed || self.backend.outputs.len() != before {
            self.refit_windows();
            self.publish_outputs();
        }
    }

    fn remove_output(&mut self, crtc: crtc::Handle) {
        let Some(surface) = self.backend.outputs.remove(&crtc) else {
            return;
        };
        tracing::info!("output {} disconnected", surface.output.name());
        if let Some(token) = surface.redraw_timer {
            self.loop_handle.remove(token);
        }
        for layer in layer_map_for_output(&surface.output).layers() {
            layer.layer_surface().send_close();
        }
        self.space.unmap_output(&surface.output);
        self.display_handle
            .remove_global::<BuntState>(surface.global);
    }

    /// 출력의 모드를 바꾼다. 실패하면 false
    fn apply_mode(&mut self, crtc: crtc::Handle, mode: DrmMode) -> bool {
        let UdevData {
            renderer, outputs, ..
        } = &mut self.backend;
        let Some(surface) = outputs.get_mut(&crtc) else {
            return false;
        };
        if let Err(err) = surface.drm_output.use_mode(
            mode,
            renderer,
            &DrmOutputRenderElements::<GlesRenderer, Elements>::default(),
        ) {
            tracing::warn!(
                "failed to change the mode of {}: {err:?}",
                surface.output.name()
            );
            return false;
        }
        surface.mode = mode;
        let wl_mode = WlMode::from(mode);
        surface
            .output
            .change_current_state(Some(wl_mode), None, None, None);
        tracing::info!(
            "output {}: {}x{} @ {} mHz",
            surface.output.name(),
            wl_mode.size.w,
            wl_mode.size.h,
            wl_mode.refresh
        );
        // 지금 배율로는 화면이 너무 작아지면 쓸 수 있는 가장 가까운 배율로 바꾼다
        let scale = surface.output.current_scale().fractional_scale();
        let fitted = nearest_scale(&scales_for(wl_mode.size.w, wl_mode.size.h), scale);
        if fitted != scale {
            surface
                .output
                .change_current_state(None, None, Some(output_scale(fitted)), None);
        }
        self.output_resized(crtc);
        true
    }

    /// 출력의 모드나 배율이 바뀐 뒤 배경화면, 레이어(메뉴바, Dock), 창을 새 크기에 맞춘다
    fn output_resized(&mut self, crtc: crtc::Handle) {
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return;
        };
        let output = surface.output.clone();
        if let Some(mode) = output.current_mode() {
            surface.wallpaper = self.config.wallpaper_buffer(mode);
        }
        // Space는 출력 위치만 기억하고 크기는 출력에서 읽는다. 다시 놓아 새 크기를 반영한다
        self.space.map_output(&output, output.current_location());
        layer_map_for_output(&output).arrange();
        self.refit_windows();
        self.send_scale_to_all();
        self.render_output(crtc);
    }

    /// 시스템 설정이 고른 모드로 바꾸고 기억한다. 화면이 권하는 모드를 고르면 고정을 푼다
    pub fn set_output_mode(&mut self, name: &str, width: i32, height: i32, refresh: i32) {
        let pin = ModePref {
            width,
            height,
            refresh,
        };
        let found = self.backend.outputs.iter().find_map(|(crtc, surface)| {
            (surface.output.name() == name)
                .then(|| find_mode(&surface.modes, pin).map(|mode| (*crtc, mode)))
                .flatten()
        });
        let Some((crtc, mode)) = found else {
            tracing::warn!("{name} has no mode {width}x{height}@{refresh}");
            return;
        };
        if self.apply_mode(crtc, mode) {
            let preferred = self.backend.outputs[&crtc].modes.first().map(mode_key)
                == Some((width, height, refresh));
            let scale = self.backend.outputs[&crtc]
                .output
                .current_scale()
                .fractional_scale();
            let prefs = self.prefs.output_mut(name);
            prefs.mode = (!preferred).then_some(pin);
            prefs.scale = Some(scale);
            self.prefs.save();
        }
        self.publish_outputs();
    }

    /// 시스템 설정이 고른 배율로 바꾸고 기억한다
    pub fn set_output_scale(&mut self, name: &str, scale: f64) {
        let Some(crtc) = self
            .backend
            .outputs
            .iter()
            .find_map(|(crtc, surface)| (surface.output.name() == name).then_some(*crtc))
        else {
            tracing::warn!("no output named {name}");
            return;
        };
        let output = self.backend.outputs[&crtc].output.clone();
        output.change_current_state(None, None, Some(output_scale(scale)), None);
        tracing::info!("output {name}: scale {scale}");
        self.prefs.output_mut(name).scale = Some(scale);
        self.prefs.save();
        self.output_resized(crtc);
        self.publish_outputs();
    }

    /// 시스템 설정이 읽는 출력 목록을 새로 적고 바뀌었다고 알린다
    pub fn publish_outputs(&self) {
        let mut outputs: Vec<OutputInfo> = self
            .backend
            .outputs
            .values()
            .map(|surface| {
                let mode = WlMode::from(surface.mode);
                let size = surface.output.physical_properties().size;
                let scale = surface.output.current_scale().fractional_scale();
                let scales = scales_for(mode.size.w, mode.size.h);
                let preferred_scale = nearest_scale(&scales, surface.preferred_scale);
                OutputInfo {
                    name: surface.output.name(),
                    description: surface.description.clone(),
                    builtin: surface.builtin,
                    width: mode.size.w,
                    height: mode.size.h,
                    refresh: mode.refresh,
                    scale,
                    modes: surface.modes.iter().map(mode_key).collect(),
                    scales: scale_choices(&scales, &[scale, preferred_scale]),
                    preferred_scale,
                    physical_width: size.w,
                    physical_height: size.h,
                }
            })
            .collect();
        outputs.sort_by(|a, b| a.name.cmp(&b.name));
        self.dbus.update(|snapshot| snapshot.outputs = outputs);
        self.dbus.send(Command::OutputsChanged);
    }

    fn render_all(&mut self) {
        let crtcs: Vec<_> = self.backend.outputs.keys().copied().collect();
        for crtc in crtcs {
            self.render_output(crtc);
        }
    }

    fn frame_finish(&mut self, crtc: crtc::Handle) {
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return;
        };
        if let Err(err) = surface.drm_output.frame_submitted() {
            tracing::warn!("frame submission failed: {err:?}");
        }
        surface.frame_pending = false;
        // 소프트웨어 렌더러는 화면 주사율(QEMU 창은 Mac 화면을 따라 120~144Hz)과 상관없이 60Hz로
        // 그린다. 프레임마다 CPU로 합성하므로 그보다 자주 그리면 CPU만 쓴다. 앱도 프레임 알림을
        // 그만큼 받으므로 애니메이션을 60Hz로 그린다
        let since = surface.rendered_at.map(|at| at.elapsed());
        if self.backend.rendering.cpu_composited
            && let Some(since) = since.filter(|since| *since < SOFTWARE_FRAME)
        {
            self.schedule_redraw_after(crtc, SOFTWARE_FRAME - since);
            return;
        }
        self.render_output(crtc);
    }

    fn schedule_redraw(&mut self, crtc: crtc::Handle) {
        self.schedule_redraw_after(crtc, IDLE_REDRAW);
    }

    fn schedule_redraw_after(&mut self, crtc: crtc::Handle, delay: Duration) {
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return;
        };
        if surface.redraw_timer.is_some() {
            return;
        }
        let token =
            self.loop_handle
                .insert_source(Timer::from_duration(delay), move |_, _, state| {
                    if let Some(surface) = state.backend.outputs.get_mut(&crtc) {
                        surface.redraw_timer = None;
                    }
                    state.render_output(crtc);
                    TimeoutAction::Drop
                });
        match token {
            Ok(token) => surface.redraw_timer = Some(token),
            Err(err) => tracing::warn!("failed to schedule redraw: {}", err.error),
        }
    }

    /// 포인터 장치가 연결됐다. 마우스 설정을 쓰고 시스템 설정에 알린다
    fn pointer_device_added(&mut self, device: &InputDevice) {
        if !device.has_capability(DeviceCapability::Pointer) {
            return;
        }
        let mut device = device.clone();
        apply_pointer(&mut device, self.pointer_prefs());
        tracing::info!("pointer device: {}", device.name());
        self.backend.pointer_devices.push(device);
        self.publish_pointer_devices();
    }

    fn pointer_device_removed(&mut self, device: &InputDevice) {
        let before = self.backend.pointer_devices.len();
        self.backend.pointer_devices.retain(|known| known != device);
        if self.backend.pointer_devices.len() != before {
            self.publish_pointer_devices();
        }
    }

    fn publish_pointer_devices(&mut self) {
        let devices = self
            .backend
            .pointer_devices
            .iter()
            .map(device_info)
            .collect();
        self.dbus
            .update(|snapshot| snapshot.pointer_devices = devices);
        self.dbus.send(Command::PointerDevicesChanged);
    }

    /// 시스템 설정이 마우스 설정을 바꿨다. 다음 로그인에도 쓴다
    pub fn set_pointer_prefs(&mut self, prefs: PointerPrefs) {
        self.prefs.pointer = Some(prefs);
        self.prefs.save();
        self.apply_pointer_prefs();
        tracing::info!(
            "pointer: speed {}, natural scroll {}, left handed {}, scroll {}x, cursor {}px",
            prefs.speed,
            prefs.natural_scroll,
            prefs.left_handed,
            prefs.scroll_speed,
            prefs.cursor_size
        );
    }

    /// 마우스 설정을 연결된 장치와 커서에 쓴다
    pub fn apply_pointer_prefs(&mut self) {
        let prefs = self.pointer_prefs();
        for device in &mut self.backend.pointer_devices {
            apply_pointer(device, prefs);
        }
        self.backend.cursor.set_size(prefs.cursor_size);
        self.dbus.update(|snapshot| snapshot.pointer = prefs);
    }

    /// 표면마다 이번 프레임에 그 출력에 보였는지 적는다 (프레임 알림을 줄 표면을 고를 때 쓴다)
    fn note_visible_surfaces(&self, output: &Output, states: &RenderElementStates) {
        let update = |surface: &WlSurface, data: &SurfaceData| {
            update_surface_primary_scanout_output(
                surface,
                output,
                data,
                states,
                default_primary_scanout_output_compare,
            );
        };
        for window in self.space.elements() {
            window.with_surfaces(update);
        }
        for layer in layer_map_for_output(output).layers() {
            layer.with_surfaces(update);
        }
        if let CursorImageStatus::Surface(cursor_surface) = &self.cursor_status {
            with_surfaces_surface_tree(cursor_surface, update);
        }
    }

    fn render_output(&mut self, crtc: crtc::Handle) {
        if !self.backend.session.is_active() {
            return;
        }
        if let CursorImageStatus::Surface(surface) = &self.cursor_status
            && !surface.alive()
        {
            self.cursor_status = CursorImageStatus::default_named();
        }

        // 뒤를 흐릴 셸 표면. 회전한 출력에서는 흐리지 않는다 (blur.rs는 회전하지 않은 화면만 안다)
        let blur_surfaces = match self.backend.outputs.get(&crtc) {
            Some(surface)
                if self.config.effects.blur
                    && surface.output.current_transform() == Transform::Normal =>
            {
                self.blur_surfaces(&surface.output)
            }
            _ => Vec::new(),
        };

        // 막대를 숨길지 다시 보고(앞의 창이 최대화했을 수 있다), 다 보이지 않는 막대의 자리를 정한다.
        // 숨기고 꺼낼 때의 미끄러짐은 이 렌더 루프가 프레임마다 그린다
        self.update_bars(None);
        let bar_places = match self.backend.outputs.get(&crtc) {
            Some(surface) => self.bar_places(&surface.output),
            None => Vec::new(),
        };

        let UdevData {
            renderer,
            outputs,
            cursor,
            cursor_buffer,
            cursor_hotspot,
            cursor_key,
            blur,
            ..
        } = &mut self.backend;
        let Some(surface) = outputs.get_mut(&crtc) else {
            return;
        };
        if surface.frame_pending {
            return;
        }
        let output = surface.output.clone();
        let Some(output_geo) = self.space.output_geometry(&output) else {
            return;
        };
        let scale = Scale::from(output.current_scale().fractional_scale());

        // 앞(위)에 그릴 요소부터 넣는다: 커서, 레이어와 창, 배경화면
        let mut elements: Vec<Elements> = Vec::new();

        let pointer_pos = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or_default();
        if output_geo.to_f64().contains(pointer_pos) {
            let local = pointer_pos - output_geo.loc.to_f64();
            match &self.cursor_status {
                CursorImageStatus::Hidden => {}
                CursorImageStatus::Surface(cursor_surface) => {
                    let hotspot = with_states(cursor_surface, |states| {
                        states
                            .data_map
                            .get::<CursorImageSurfaceData>()
                            .map(|data| data.lock().unwrap().hotspot)
                            .unwrap_or_default()
                    });
                    let location = (local - hotspot.to_f64()).to_physical(scale).to_i32_round();
                    elements.extend(
                        render_elements_from_surface_tree(
                            renderer,
                            cursor_surface,
                            location,
                            scale,
                            1.0,
                            Kind::Cursor,
                        )
                        .into_iter()
                        .map(OutputRenderElements::Surface),
                    );
                }
                CursorImageStatus::Named(icon) => {
                    let key = (*icon, output.current_scale().integer_scale(), cursor.size());
                    if *cursor_key != Some(key) {
                        (*cursor_buffer, *cursor_hotspot) = cursor.buffer(key.0, key.1);
                        *cursor_key = Some(key);
                    }
                    let location = (local - *cursor_hotspot).to_physical(scale);
                    match MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        location,
                        cursor_buffer,
                        None,
                        None,
                        None,
                        Kind::Cursor,
                    ) {
                        Ok(element) => elements.push(OutputRenderElements::Memory(element)),
                        Err(err) => tracing::warn!("failed to draw cursor: {err}"),
                    }
                }
            }
        }
        // 스크린샷에는 커서를 넣지 않는다
        let cursor_ids: Vec<Id> = elements
            .iter()
            .map(|element| element.id().clone())
            .collect();

        // ⇧⌘4로 고르는 영역: 커서 아래, 창과 막대 위
        if let Some(parts) = self
            .screenshot_area
            .as_mut()
            .and_then(AreaSelection::layout)
        {
            for (location, buffer) in parts {
                let location = (location - output_geo.loc).to_physical_precise_round(scale);
                elements.push(OutputRenderElements::Solid(
                    SolidColorRenderElement::from_buffer(
                        buffer,
                        location,
                        scale,
                        1.0,
                        Kind::Unspecified,
                    ),
                ));
            }
        }

        match space_render_elements::<_, smithay::desktop::Window, _>(
            renderer,
            [&self.space],
            &output,
            1.0,
        ) {
            Ok(mut space_elements) => {
                render::place_bars(renderer, &mut space_elements, &bar_places, scale);
                elements.extend(space_elements.into_iter().map(OutputRenderElements::Space))
            }
            Err(err) => tracing::warn!("failed to collect windows: {err:?}"),
        }

        // 배경화면은 물리 픽셀 크기로 만들었으므로 논리 크기를 주어 배율과 상관없이 화면을 채운다
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (0.0, 0.0),
            &surface.wallpaper,
            None,
            None,
            Some(output_geo.size),
            Kind::Unspecified,
        ) {
            Ok(element) => elements.push(OutputRenderElements::Memory(element)),
            Err(err) => tracing::warn!("failed to draw wallpaper: {err}"),
        }
        // 흐림은 화면 버퍼에 먼저 그려진 아래 장면을 복사한다. 아래 창이 오버레이 평면에 따로 올라가면
        // 복사에서 빠지므로, 흐릴 때는 오버레이 평면에 내보내지 않는다
        let flags = if blur.insert(renderer, &mut elements, &blur_surfaces, scale) {
            FrameFlags::DEFAULT.difference(FrameFlags::ALLOW_OVERLAY_PLANE_SCANOUT)
        } else {
            FrameFlags::DEFAULT
        };

        // 스크린샷: 커서를 뺀 같은 장면을 따로 그려 읽는다. 저장과 알림은 프레임을 낸 뒤에 한다
        let screenshot = match self.screenshot_capture.take() {
            Some(capture) if capture.output == output => {
                let scene: Vec<&Elements> = elements
                    .iter()
                    .filter(|element| !cursor_ids.contains(element.id()))
                    .collect();
                let image =
                    screenshot::capture_scene(renderer, &scene, &output, capture.area, CLEAR_COLOR)
                        .inspect_err(|err| tracing::warn!("failed to take a screenshot: {err}"))
                        .ok();
                Some((capture.clipboard, image))
            }
            other => {
                self.screenshot_capture = other;
                None
            }
        };

        let started = std::time::Instant::now();
        let rendered = surface
            .drm_output
            .render_frame(renderer, &elements, CLEAR_COLOR, flags);
        let elapsed = started.elapsed();
        if elapsed > SLOW_FRAME {
            tracing::debug!("slow frame on {}: {elapsed:?}", output.name());
        }
        let (queued, states) = match rendered {
            Ok(frame) => {
                let queued = !frame.is_empty
                    && match surface.drm_output.queue_frame(()) {
                        Ok(()) => true,
                        Err(err) => {
                            tracing::warn!("failed to queue frame: {err:?}");
                            false
                        }
                    };
                (queued, Some(frame.states))
            }
            Err(err) => {
                tracing::warn!("failed to render frame: {err:?}");
                (false, None)
            }
        };
        surface.frame_pending = queued;
        if queued {
            surface.rendered_at = Some(Instant::now());
        }
        if let Some(states) = &states {
            self.note_visible_surfaces(&output, states);
        }
        if let Some((clipboard, image)) = screenshot {
            self.screenshot_taken(clipboard, image);
        }

        // 이번 프레임에 보인 표면만 바로 다음 프레임 알림을 받는다. 다른 창에 다 가려진 창, 숨은
        // 막대처럼 보이지 않은 표면은 HIDDEN_FRAME_INTERVAL마다 한 번만 받아, 보이지 않는
        // 애니메이션(시스템 모니터의 그래프 등)이 계속 그려지며 CPU를 쓰지 않게 한다
        let elapsed = self.start_time.elapsed();
        for window in self.space.elements() {
            // 소프트웨어 렌더러에서는 앞에 있지 않은 창(시스템 모니터의 그래프 등)이 애니메이션을
            // 30Hz로 그리게 한다. 뒤의 창이 60Hz로 그리면 그 합성만으로 CPU 한 코어의 절반을 썼다
            if self.backend.rendering.cpu_composited
                && !window.toplevel().is_some_and(|toplevel| {
                    Self::has_state(toplevel, xdg_toplevel::State::Activated)
                })
            {
                let last = window
                    .user_data()
                    .get_or_insert(|| BackgroundFrame(Cell::new(Duration::ZERO)));
                if elapsed.saturating_sub(last.0.get()) < BACKGROUND_FRAME {
                    continue;
                }
                last.0.set(elapsed);
            }
            window.send_frame(
                &output,
                elapsed,
                HIDDEN_FRAME_INTERVAL,
                surface_primary_scanout_output,
            );
        }
        for layer in layer_map_for_output(&output).layers() {
            layer.send_frame(
                &output,
                elapsed,
                HIDDEN_FRAME_INTERVAL,
                surface_primary_scanout_output,
            );
        }
        if let CursorImageStatus::Surface(cursor_surface) = &self.cursor_status {
            send_frames_surface_tree(
                cursor_surface,
                &output,
                elapsed,
                HIDDEN_FRAME_INTERVAL,
                surface_primary_scanout_output,
            );
        }

        // 새로 보낸 프레임이 없으면 플립 이벤트가 오지 않으므로 잠시 뒤 다시 확인한다
        if !queued {
            self.schedule_redraw(crtc);
        }
    }
}

impl Config {
    /// 출력 모드 크기(물리 픽셀)의 그라데이션 배경화면
    fn wallpaper_buffer(&self, mode: WlMode) -> MemoryRenderBuffer {
        let top = parse_hex_color(&self.wallpaper.top).unwrap_or(FALLBACK_TOP);
        let bottom = parse_hex_color(&self.wallpaper.bottom).unwrap_or(FALLBACK_BOTTOM);
        render::wallpaper(top, bottom, mode.size)
    }
}

/// D-Bus와 저장 파일에서 모드를 가리키는 값: (너비, 높이, mHz)
fn mode_key(mode: &DrmMode) -> (i32, i32, i32) {
    let mode = WlMode::from(*mode);
    (mode.size.w, mode.size.h, mode.refresh)
}

fn find_mode(modes: &[DrmMode], pin: ModePref) -> Option<DrmMode> {
    modes
        .iter()
        .find(|mode| mode_key(mode) == (pin.width, pin.height, pin.refresh))
        .copied()
}

/// 커넥터의 모드를 화면이 권하는 것부터, 그다음 큰 해상도와 높은 주사율 순으로 늘어놓는다.
/// 크기와 주사율이 같은 모드가 여럿이면 앞의 것만 둔다
fn sorted_modes(info: &connector::Info) -> Vec<DrmMode> {
    let mut modes = info.modes().to_vec();
    modes.sort_by_key(|mode| {
        let (width, height) = mode.size();
        (
            !mode.mode_type().contains(ModeTypeFlags::PREFERRED),
            Reverse(u32::from(width) * u32::from(height)),
            Reverse(mode_key(mode).2),
        )
    });
    let mut seen = Vec::new();
    modes.retain(|mode| {
        let key = mode_key(mode);
        let new = !seen.contains(&key);
        seen.push(key);
        new
    });
    modes
}

/// 고를 수 있는 배율: 1배와, 배율로 나눈 논리 화면이 너무 작아지지 않는 0.25 단위 배율.
/// 소수 배율을 모르는 앱은 올림한 정수 배율로 그리고 컴포지터가 줄여 그린다 (macOS의 HiDPI와 같다)
fn scales_for(width: i32, height: i32) -> Vec<f64> {
    let fits = |scale: f64| {
        f64::from(width) / scale >= MIN_LOGICAL_WIDTH - 1e-6
            && f64::from(height) / scale >= MIN_LOGICAL_HEIGHT - 1e-6
    };
    let mut scales = vec![1.0];
    scales.extend(
        (1..)
            .map(|step| 1.0 + f64::from(step) * SCALE_STEP)
            .take_while(|scale| *scale <= MAX_SCALE && fits(*scale)),
    );
    scales
}

/// 시스템 설정에 보일 배율: 많으면 양 끝(가장 큰 텍스트, 가장 넓은 공간) 사이를 고르게 나눠 다섯 개를
/// 고른다. 거리가 같으면 고르기 쉬운 0.5 단위(150%, 200%)를 고르고, `keep`(지금 배율, 기본 배율)은
/// 늘 넣는다
fn scale_choices(scales: &[f64], keep: &[f64]) -> Vec<f64> {
    if scales.len() <= SCALE_CHOICES {
        return scales.to_vec();
    }
    let (first, last) = (scales[0], scales[scales.len() - 1]);
    let half_step = |scale: f64| (scale * 2.0).fract() == 0.0;
    let nearest = |target: f64| {
        scales
            .iter()
            .copied()
            .min_by(|a, b| {
                (a - target)
                    .abs()
                    .total_cmp(&(b - target).abs())
                    .then(half_step(*b).cmp(&half_step(*a)))
            })
            .unwrap_or(target)
    };
    let mut picks: Vec<f64> = (0..SCALE_CHOICES)
        .map(|index| nearest(first + (last - first) * index as f64 / (SCALE_CHOICES - 1) as f64))
        .collect();
    picks.dedup();
    for &scale in keep {
        if !scales.contains(&scale) || picks.contains(&scale) {
            continue;
        }
        // 양 끝과 지킬 값은 두고, 가장 가까운 자리를 바꾼다
        if let Some(slot) = picks
            .iter_mut()
            .filter(|pick| **pick != first && **pick != last && !keep.contains(pick))
            .min_by(|a, b| (**a - scale).abs().total_cmp(&(**b - scale).abs()))
        {
            *slot = scale;
        }
        picks.sort_by(f64::total_cmp);
    }
    picks
}

/// `scales` 가운데 `scale`에 가장 가까운 배율 (같으면 작은 쪽)
fn nearest_scale(scales: &[f64], scale: f64) -> f64 {
    scales
        .iter()
        .copied()
        .min_by(|a, b| (a - scale).abs().total_cmp(&(b - scale).abs()))
        .unwrap_or(1.0)
}

/// 화면의 실제 크기(mm)로 고른 기본 배율. 논리 밀도가 BUILTIN_PPI나 EXTERNAL_PPI에 가깝게 한다.
/// 크기를 모르거나(0) 믿기 어려우면(프로젝터의 센티미터 값, 가로세로 비가 픽셀과 다름) 1배
fn recommended_scale(pixels: Size<i32, Physical>, millimeters: (i32, i32), builtin: bool) -> f64 {
    let (width_mm, height_mm) = millimeters;
    if width_mm < 100 || height_mm < 60 || pixels.w <= 0 || pixels.h <= 0 {
        return 1.0;
    }
    let pixel_ratio = f64::from(pixels.w) / f64::from(pixels.h);
    let size_ratio = f64::from(width_mm) / f64::from(height_mm);
    if (pixel_ratio / size_ratio - 1.0).abs() > 0.15 {
        return 1.0;
    }
    let ppi = f64::from(pixels.w) * 25.4 / f64::from(width_mm);
    let target = if builtin { BUILTIN_PPI } else { EXTERNAL_PPI };
    nearest_scale(&scales_for(pixels.w, pixels.h), ppi / target)
}

fn output_scale(scale: f64) -> OutputScale {
    if scale.fract() == 0.0 {
        OutputScale::Integer(scale as i32)
    } else {
        OutputScale::Fractional(scale)
    }
}

/// 커넥터의 EDID
fn edid_blob(device: &impl ControlDevice, connector: connector::Handle) -> Option<Vec<u8>> {
    let properties = device.get_properties(connector).ok()?;
    let (handles, values) = properties.as_props_and_values();
    handles.iter().zip(values).find_map(|(handle, value)| {
        let info = device.get_property(*handle).ok()?;
        (info.name().to_bytes() == b"EDID")
            .then(|| device.get_property_blob(*value).ok())
            .flatten()
    })
}

/// EDID에서 제조사 코드(예: RHT)와 모니터 이름(예: QEMU Monitor)을 읽는다
fn parse_edid(edid: &[u8]) -> (String, String) {
    if edid.len() < 128 || edid[..8] != [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0] {
        return Default::default();
    }
    let id = u16::from_be_bytes([edid[8], edid[9]]);
    let letters: Option<String> = [10, 5, 0]
        .iter()
        .map(|shift| {
            let code = ((id >> shift) & 0x1f) as u8;
            (1..=26)
                .contains(&code)
                .then(|| char::from(b'A' + code - 1))
        })
        .collect();
    // 설명자 4개 가운데 모니터 이름(0xfc) 설명자
    let name = (0..4)
        .map(|index| &edid[54 + index * 18..72 + index * 18])
        .find(|descriptor| descriptor[..3] == [0, 0, 0] && descriptor[3] == 0xfc)
        .map(|descriptor| {
            String::from_utf8_lossy(&descriptor[5..])
                .trim_end_matches(['\n', ' ', '\0'])
                .to_owned()
        })
        .unwrap_or_default();
    (letters.unwrap_or_default(), name)
}

/// 장치가 지원하는 마우스 설정만 쓴다 (절대 좌표 장치에는 가속이 없다)
fn apply_pointer(device: &mut InputDevice, prefs: PointerPrefs) {
    if device.config_accel_is_available() {
        let _ = device.config_accel_set_speed(prefs.speed.clamp(-1.0, 1.0));
    }
    if device.config_scroll_has_natural_scroll() {
        let _ = device.config_scroll_set_natural_scroll_enabled(prefs.natural_scroll);
    }
    if device.config_left_handed_is_available() {
        let _ = device.config_left_handed_set(prefs.left_handed);
    }
}

/// 시스템 설정에 보일 장치 정보. 연결 방식은 udev의 ID_BUS다 (usb, bluetooth 등)
fn device_info(device: &InputDevice) -> PointerDevice {
    // 안전: udev 장치는 libinput 장치(device)가 사는 동안 쓴다
    let bus = unsafe { device.udev_device() }
        .and_then(|udev| {
            udev.property_value("ID_BUS")
                .map(|bus| bus.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    PointerDevice {
        name: device.name().to_owned(),
        vendor: device.id_vendor(),
        product: device.id_product(),
        bus,
        touchpad: device.config_tap_finger_count() > 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edid_names() {
        let mut edid = vec![0u8; 128];
        edid[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        // RHT: R=18, H=8, T=20
        edid[8..10].copy_from_slice(&((18u16 << 10) | (8 << 5) | 20).to_be_bytes());
        edid[72..77].copy_from_slice(&[0, 0, 0, 0xfc, 0]);
        edid[77..90].copy_from_slice(b"QEMU Monitor\n");
        assert_eq!(
            parse_edid(&edid),
            ("RHT".to_owned(), "QEMU Monitor".to_owned())
        );
        assert_eq!(parse_edid(&edid[..64]), Default::default());
    }

    #[test]
    fn scales_keep_a_usable_logical_size() {
        assert_eq!(scales_for(1280, 800), vec![1.0, 1.25]);
        assert_eq!(scales_for(1366, 768), vec![1.0]);
        assert_eq!(scales_for(1920, 1080), vec![1.0, 1.25, 1.5]);
        assert_eq!(
            scales_for(2560, 1600),
            vec![1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5]
        );
    }

    #[test]
    fn five_choices_keep_the_ends_and_the_current_scale() {
        let laptop = scales_for(2560, 1600);
        assert_eq!(scale_choices(&laptop, &[]), vec![1.0, 1.5, 1.75, 2.0, 2.5]);
        assert_eq!(
            scale_choices(&laptop, &[2.25, 1.75]),
            vec![1.0, 1.5, 1.75, 2.25, 2.5]
        );
        // 4K: 1920×1080처럼 보이는 200%와 2560×1440처럼 보이는 150%가 들어간다
        let monitor = scales_for(3840, 2160);
        assert_eq!(
            scale_choices(&monitor, &[1.0]),
            vec![1.0, 1.5, 2.0, 2.75, 3.25]
        );
        assert_eq!(
            scale_choices(&monitor, &[1.75, 1.0]),
            vec![1.0, 1.75, 2.0, 2.75, 3.25]
        );
        assert_eq!(scale_choices(&[1.0, 1.25], &[1.0]), vec![1.0, 1.25]);
    }

    #[test]
    fn recommended_scale_follows_the_pixel_density() {
        // QEMU의 가상 화면 (약 100ppi)
        assert_eq!(
            recommended_scale((1280, 800).into(), (320, 200), false),
            1.0
        );
        // 13.3형 2560×1600 노트북 (227ppi) → 1440×900에 가깝게
        assert_eq!(
            recommended_scale((2560, 1600).into(), (286, 179), true),
            1.75
        );
        // 27형 4K 모니터 (163ppi) → 2560×1440
        assert_eq!(
            recommended_scale((3840, 2160).into(), (597, 336), false),
            1.5
        );
        // 크기를 모르거나 믿을 수 없는 화면
        assert_eq!(recommended_scale((2560, 1600).into(), (0, 0), true), 1.0);
        assert_eq!(recommended_scale((1920, 1080).into(), (16, 9), false), 1.0);
    }
}
