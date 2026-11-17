// SPDX-FileCopyrightText: 2017 Victor Berger and Victoria Brekenfeld
// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later AND MIT
//
// Smithay anvil의 udev.rs(MIT)를 바탕으로 했다.

//! DRM/KMS + libinput 백엔드 (GPU 하나). Smithay anvil의 udev 백엔드 구조를 단순화했다.
//! 하드웨어 GPU가 없는 VM에서는 Mesa의 소프트웨어 렌더러(llvmpipe)로 그린다.

use std::{
    cell::Cell,
    cmp::Reverse,
    collections::HashMap,
    error::Error,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use smithay::{
    backend::{
        allocator::{
            Format, Fourcc, Modifier,
            dmabuf::AsDmabuf,
            format::FormatSet,
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
        },
        drm::{
            DrmDevice, DrmDeviceFd, DrmError, DrmEvent, DrmNode, NodeType, Planes,
            compositor::{DrmCompositor, FrameError, FrameFlags, PrimaryPlaneElement},
            exporter::{ExportFramebuffer, gbm::GbmFramebufferExporter},
        },
        egl::{EGLContext, EGLDisplay, context::ContextPriority},
        input::InputEvent,
        libinput::LibinputSessionInterface,
        renderer::{
            Color32F, ImportDma, ImportMemWl,
            element::{
                Element, Id, Kind, RenderElementStates, default_primary_scanout_output_compare,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::render_elements_from_surface_tree,
            },
            gles::GlesRenderer,
            utils::with_renderer_surface_state,
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
            control::{
                Device as ControlDevice, Mode as DrmMode, ModeTypeFlags, connector, crtc, plane,
            },
        },
        input::{ClickMethod, Device as InputDevice, DeviceCapability, Libinput, TapButtonMap},
        rustix::fs::OFlags,
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{Display, backend::GlobalId, protocol::wl_surface::WlSurface},
    },
    utils::{DeviceFd, IsAlive, Logical, Physical, Point, Scale, Size, Transform},
    wayland::{
        compositor::{SurfaceAttributes, SurfaceData, with_states},
        shell::wlr_layer::Layer,
    },
};

use crate::{
    background_effect::BackgroundEffectState,
    blur::{Blur, BlurTarget},
    config::{Config, parse_hex_color},
    cursor::Cursor,
    dbus::{Command, OutputInfo, PointerDevice},
    dmabuf::{self, Dmabufs},
    frame_floor::{GpuFloor, IDLE_RELEASE, INTERACTIVE_FOR},
    gpu::{GSK_RENDERER, Rendering, renderer_name},
    handover::{self, Wanted},
    prefs::{ModePref, PointerPrefs, SecondaryClick, TouchpadPrefs},
    render::{self, OutputRenderElements},
    screenshot,
    state::BuntState,
    texture_shader,
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Compositor = DrmCompositor<Allocator, Exporter, (), DrmDeviceFd>;
type Buffer = <Allocator as smithay::backend::allocator::Allocator>::Buffer;
/// `Compositor`의 오류
type CompositorError = FrameError<
    <Allocator as smithay::backend::allocator::Allocator>::Error,
    <Buffer as AsDmabuf>::Error,
    <Exporter as ExportFramebuffer<Buffer>>::Error,
>;
type Elements = OutputRenderElements;

/// 화면 버퍼의 색 형식. 앞의 것부터 시험한다
const COLOR_FORMATS: [Fourcc; 2] = [Fourcc::Abgr8888, Fourcc::Argb8888];

/// 화면의 주사율을 모를 때 프레임 사이의 간격 (60Hz)
const DEFAULT_FRAME: Duration = Duration::from_micros(16_667);
/// 바뀐 것 없는 프레임이 이만큼 이어지면 렌더 루프가 쉰다. 다음 커밋, 입력, 요청이 깨운다(queue_redraw_all).
/// 전에는 바뀐 것이 없어도 16ms마다 다시 그려, 가만히 둔 화면에서도 1초에 약 62번 깨어났다
const IDLE_AFTER: u32 = 2;
/// 소프트웨어 렌더러에서 프레임 사이의 최소 간격 (60Hz)
const SOFTWARE_FRAME: Duration = Duration::from_micros(16_667);
/// 소프트웨어 렌더러에서 앞에 있지 않은 창에 프레임 알림을 보내는 간격 (30Hz)
const BACKGROUND_FRAME: Duration = Duration::from_micros(33_333);
/// 화면에 보이지 않은 표면(다른 창에 다 가려진 창, 숨은 막대)에 프레임 알림을 보내는 간격
const HIDDEN_FRAME_INTERVAL: Option<Duration> = Some(Duration::from_secs(1));
/// 세션이 시작될 때 셸이 첫 화면을 낼 때까지 기다리는 시간. 그때까지 화면을 그리지 않는다. 셸이 더 늦으면
/// 설정의 배경화면을 그린다
const FIRST_FRAME_WAIT: Duration = Duration::from_secs(5);
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
    /// 화면 장치. 전에는 smithay의 DrmOutputManager가 가졌는데, 그것은 출력을 만들 때 검은 프레임을 바로
    /// 화면에 올려서 앞 세션의 마지막 화면이 사라졌다 (handover.rs)
    drm: DrmDevice,
    allocator: Allocator,
    exporter: Exporter,
    gbm: GbmDevice<DrmDeviceFd>,
    /// 렌더러가 그릴 수 있는 형식
    render_formats: Vec<Format>,
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
    /// 앱 창 뒤 흐림 요청 (background_effect.rs)
    pub background_effects: BackgroundEffectState,
    /// 프레임이 떨어지면 GPU의 최소 클럭을 올린다 (frame_floor.rs). GPU가 받지 않으면 None
    frame_floor: Option<GpuFloor>,
    /// 최소 클럭을 건 동안 움직임이 멈추는 것을 보는 타이머
    frame_floor_timer: Option<RegistrationToken>,
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

    /// 출력 하나를 만든다. 화면에는 아직 아무것도 올리지 않는다: 첫 프레임을 그릴 때까지 앞 세션이 남긴 화면이
    /// 그대로 있다. 형식과 모드는 커널에 시험해 보므로 쓸 수 없는 조합이면 여기서 실패한다
    fn new_compositor(
        &mut self,
        crtc: crtc::Handle,
        mode: DrmMode,
        connector: connector::Handle,
        output: &Output,
        planes: Option<Planes>,
    ) -> Result<Compositor, CompositorError> {
        let create = |backend: &mut Self, implicit: bool| {
            let surface = backend
                .drm
                .create_surface(crtc, mode, &[connector])
                .map_err(FrameError::DrmError)?;
            let formats: Vec<Format> = backend
                .render_formats
                .iter()
                .copied()
                .filter(|format| !implicit || format.modifier == Modifier::Invalid)
                .collect();
            Compositor::new(
                output,
                surface,
                planes.clone(),
                backend.allocator.clone(),
                backend.exporter.clone(),
                COLOR_FORMATS,
                formats,
                backend.drm.cursor_size(),
                Some(backend.gbm.clone()),
            )
        };
        create(self, false).or_else(|err| {
            tracing::warn!("failed to set up crtc {crtc:?}, lowering the bandwidth: {err:?}");
            self.with_less_bandwidth(create)
        })
    }

    /// 출력의 모드를 바꾼다. 다음 프레임에 화면에 오른다
    fn use_mode(&mut self, crtc: crtc::Handle, mode: DrmMode) -> Result<(), CompositorError> {
        let change = |backend: &mut Self, _implicit: bool| match backend.outputs.get_mut(&crtc) {
            Some(surface) => surface.compositor.use_mode(mode),
            None => Ok(()),
        };
        match change(self, false) {
            Err(err @ FrameError::DrmError(DrmError::TestFailed(_))) => {
                tracing::warn!("failed to set the mode of {crtc:?}, lowering the bandwidth: {err}");
                self.with_less_bandwidth(change)
            }
            result => result,
        }
    }

    /// 화면을 켜거나 모드를 바꾸는 `attempt`가 실패했을 때 대역폭을 줄이고 다시 해 본다. 평면과 커서를 내려 보고,
    /// 그래도 안 되면 모두 암묵적 modifier로 바꿔 본다(`attempt`의 둘째 인자). 전에 smithay의 DrmOutputManager가
    /// 하던 차례 그대로다
    fn with_less_bandwidth<T>(
        &mut self,
        attempt: impl Fn(&mut Self, bool) -> Result<T, CompositorError>,
    ) -> Result<T, CompositorError> {
        for surface in self.outputs.values_mut() {
            blank_frame(&mut surface.compositor, &mut self.renderer);
        }
        match attempt(self, false) {
            Ok(value) => return Ok(value),
            Err(err) => tracing::warn!("still failing, trying implicit modifiers: {err:?}"),
        }
        self.set_implicit_modifiers(true);
        let result = attempt(self, true);
        if result.is_err() {
            self.set_implicit_modifiers(false);
        }
        result
    }

    /// 모든 화면의 버퍼를 암묵적 modifier로, 또는 렌더러가 고를 수 있는 modifier로 다시 만든다
    fn set_implicit_modifiers(&mut self, implicit: bool) {
        for surface in self.outputs.values_mut() {
            let code = surface.compositor.format();
            let modifiers: Vec<Modifier> = if implicit {
                vec![Modifier::Invalid]
            } else {
                self.render_formats
                    .iter()
                    .filter(|format| format.code == code)
                    .map(|format| format.modifier)
                    .collect()
            };
            if let Err(err) = surface
                .compositor
                .set_format(self.allocator.clone(), code, modifiers)
            {
                tracing::warn!("failed to change the buffer modifiers: {err:?}");
                continue;
            }
            blank_frame(&mut surface.compositor, &mut self.renderer);
        }
    }

    /// 세션이 끝날 때 화면을 다음 컴포지터에 넘긴다. CRTC를 켜 둔 채 끝나고, `keep_last_frame`이면 마지막 화면도
    /// 남겨 둔다 (handover.rs). 다른 VT에 가 있으면 화면이 이미 다른 세션의 것이므로 아무것도 하지 않는다
    fn hand_over_screens(&mut self, keep_last_frame: bool) {
        if !self.drm.is_active() {
            return;
        }
        if keep_last_frame {
            for surface in self.outputs.values() {
                // 6.8보다 오래된 커널에서는 실패한다. 그때는 다음 세션이 그릴 때까지 화면이 검다
                if let Err(err) = handover::keep_last_frame(&self.drm, surface.compositor.plane()) {
                    tracing::warn!("failed to keep the last frame: {err}");
                }
            }
        }
        // 꺼 두면 smithay가 끝날 때 CRTC를 끄지 않고, 처음 본 상태로 되돌리지도 않는다
        self.drm.pause();
        // 닫은 프레임버퍼를 smithay가 다시 지우면 경고만 남는다. 곧 프로세스가 끝나고 커널이 나머지를 치운다
        std::mem::forget(std::mem::take(&mut self.outputs));
    }
}

/// 화면 버퍼 하나에 검은 화면을 그려 바로 올린다. 평면을 쓰지 않아 대역폭이 가장 적다
fn blank_frame(compositor: &mut Compositor, renderer: &mut GlesRenderer) {
    let frame =
        compositor.render_frame::<_, Elements>(renderer, &[], Color32F::BLACK, FrameFlags::empty());
    match frame {
        Ok(frame) => {
            if frame.needs_sync()
                && let PrimaryPlaneElement::Swapchain(primary) = frame.primary_element
            {
                let _ = primary.sync.wait();
            }
        }
        Err(err) => {
            tracing::warn!("failed to render a blank frame: {err:?}");
            return;
        }
    }
    match compositor.commit_frame() {
        Ok(()) | Err(FrameError::EmptyFrame) => {}
        Err(err) => tracing::warn!("failed to show a blank frame: {err:?}"),
    }
}

/// 뒤에 있는 창에 마지막으로 프레임 알림을 보낸 때 (BACKGROUND_FRAME)
struct BackgroundFrame(Cell<Duration>);

struct OutputSurface {
    output: Output,
    /// 화면 크기로 고른 기본 배율 (recommended_scale)
    preferred_scale: f64,
    compositor: Compositor,
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
    /// 마지막으로 그려 본 때(바뀐 것이 없어 내보내지 않은 프레임 포함). 다시 그리기를 한 프레임 간격 넘게 띄운다
    tried_at: Option<Instant>,
    /// 바뀐 것 없이 이어진 프레임 수 (IDLE_AFTER)
    idle_frames: u32,
    /// 잡아 둔 다시 그리기와 그 시각
    redraw_timer: Option<(RegistrationToken, Instant)>,
    /// 셸의 첫 화면을 이때까지 기다린다. 첫 프레임을 그린 뒤에는 None이다
    first_frame_deadline: Option<Instant>,
    global: GlobalId,
}

impl OutputSurface {
    /// 프레임 사이의 간격: 화면의 주사율. 소프트웨어 렌더러는 60Hz보다 자주 그리지 않는다
    fn frame_interval(&self, cpu_composited: bool) -> Duration {
        let interval = u64::try_from(WlMode::from(self.mode).refresh)
            .ok()
            .filter(|millihertz| *millihertz > 0)
            .map_or(DEFAULT_FRAME, |millihertz| {
                Duration::from_nanos(1_000_000_000_000 / millihertz)
            });
        if cpu_composited {
            interval.max(SOFTWARE_FRAME)
        } else {
            interval
        }
    }
}

pub fn run(config: Config) -> Result<(), Box<dyn Error>> {
    let mut event_loop: EventLoop<'static, BuntState> = EventLoop::try_new()?;
    let display: Display<BuntState> = Display::new()?;

    let (mut session, session_notifier) = LibSeatSession::new()?;
    let seat_name = session.seat();

    let udev = UdevBackend::new(&seat_name)?;
    // SoC에서는 화면 장치와 GPU가 따로라서 카드가 둘이다. 그리기만 하는 GPU(panthor 등)에는 커넥터가 없으므로
    // 커넥터가 있는 카드를 먼저 고른다. Mesa가 화면 장치에서 렌더 노드를 찾아 준다
    let gpus = all_gpus(&seat_name).unwrap_or_default();
    let gpu = primary_gpu(&seat_name)?
        .filter(|path| has_connectors(path))
        .or_else(|| gpus.iter().find(|path| has_connectors(path)).cloned())
        .or_else(|| gpus.first().cloned())
        .and_then(|path| DrmNode::from_path(path).ok())
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
    // 켜진 CRTC를 끄지 않는다. 쓰지 않을 것은 출력을 만들 때 끈다 (take_over_screens, handover.rs)
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), false)?;
    let gbm = GbmDevice::new(fd)?;

    // 안전: GBM 장치는 EGL 디스플레이보다 오래 산다 (둘 다 UdevData가 끝까지 가진다)
    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    // 앱의 GPU 작업이 많을 때도 컴포지터의 그리기가 먼저 돌게 높은 우선순위를 달라고 한다. EGL_IMG_context_priority가
    // 없거나 드라이버가 권한(CAP_SYS_NICE 등)을 보고 거절하면 보통 우선순위로 만든다
    let egl_context = EGLContext::new_with_priority(&egl_display, ContextPriority::High)?;
    let mut renderer = unsafe { GlesRenderer::new(egl_context)? };
    // 렌더러가 CPU로 그리는지만 보던 것을, 앱이 GPU로 그릴 수 있는지까지 정하게 바꿨다 (gpu.rs)
    let renderer_name = renderer_name(&mut renderer);
    let mut rendering = Rendering::from_renderer_name(&renderer_name);
    tracing::info!("renderer: {renderer_name}");
    let render_formats: FormatSet = renderer.egl_context().dmabuf_render_formats().clone();
    let shm_formats: Vec<_> = renderer.shm_formats().collect();
    // 앱이 GPU로 그릴 수 있으면 컴포지터가 그리는 GPU의 렌더 노드로 linux-dmabuf를 연다. 화면 장치의 렌더
    // 노드만 보던 것을 EGL이 알리는 장치부터 보게 바꿨다(화면 장치와 GPU가 다른 SoC, dmabuf.rs)
    let render_node = dmabuf::render_node(&egl_display, Some(gpu));
    if rendering.gpu_clients && render_node.is_none() {
        tracing::info!("linux-dmabuf disabled: no render node for {gpu}");
    }
    let dmabuf_device = render_node
        .filter(|_| rendering.gpu_clients)
        .map(|node| (node, renderer.dmabuf_formats()));
    let dmabufs = Dmabufs::new::<BuntState>(&display.handle(), dmabuf_device);
    // linux-dmabuf를 열지 못했으면(렌더 노드나 형식이 없다) 앱이 GPU로 그린 버퍼를 보낼 곳이 없으므로
    // GTK도 CPU로 그리게 한다
    rendering.gpu_clients &= dmabufs.enabled();
    let inherited = std::env::var(GSK_RENDERER)
        .ok()
        .filter(|value| !value.is_empty());
    let client_env = rendering.client_env(inherited.as_deref());
    match (&inherited, rendering.gpu_clients) {
        (Some(value), _) => tracing::info!("apps: {GSK_RENDERER}={value} from the session"),
        (None, true) => tracing::info!("apps: GTK picks its GPU renderer"),
        (None, false) => tracing::info!("apps: drawn with the CPU ({GSK_RENDERER}=cairo)"),
    }
    let background_effects =
        BackgroundEffectState::new::<BuntState>(&display.handle(), config.effects.blur);
    // 프레임 바닥: 컴포지터가 그리는 GPU가 최소 클럭을 받을 때만 (frame_floor.rs)
    let frame_floor = render_node.and_then(|node| {
        GpuFloor::open(
            node.major(),
            node.minor(),
            config.gpu.min_fps,
            config.gpu.min_fps_max_mhz,
        )
    });
    if let Some(floor) = &frame_floor {
        tracing::info!(
            "frame floor: {} frames a second while the person moves the screen, GPU up to {} MHz",
            config.gpu.min_fps,
            floor.top_mhz()
        );
    }

    let allocator = GbmAllocator::new(
        gbm.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );
    // 앱 버퍼(dmabuf)를 화면 평면에 바로 올리지 않는다 (dmabuf.rs). 전에는 카드 노드를 주었지만
    // 앱 버퍼가 없어 쓰이지 않았다
    let exporter = GbmFramebufferExporter::new(gbm.clone(), None);

    let cursor = Cursor::load(&config.cursor.theme, config.cursor.size);
    let (cursor_buffer, cursor_hotspot) = cursor.buffer(CursorIcon::Default, 1);

    let backend = UdevData {
        session,
        renderer,
        drm,
        allocator,
        exporter,
        gbm,
        render_formats: render_formats.into_iter().collect(),
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
        background_effects,
        frame_floor,
        frame_floor_timer: None,
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

    state.take_over_screens();
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
    // 좌석을 붙이며 쌓인 장치 추가 이벤트는 fd를 읽히게 하지 않으므로 첫 루프에서 꺼내게 깨운다
    // (libinput_source.rs). 세션이 돌아와 libinput을 다시 켤 때도 같다
    let (input, wake_input) = crate::libinput_source::LibinputSource::new(libinput.clone())?;
    wake_input.ping();
    event_loop
        .handle()
        .insert_source(input, |event, _, state| {
            // 장치가 붙고 떨어지면 입력 모드를 다시 정한다 (input_mode.rs)
            state.track_input_device(&event);
            match &event {
                InputEvent::DeviceAdded { device } => state.pointer_device_added(device),
                InputEvent::DeviceRemoved { device } => state.pointer_device_removed(device),
                _ => {}
            }
            state.process_input_event(event);
            // 키와 단추는 컴포지터가 그리는 것(스크린샷 영역, 막대, 창 자리)도 바꾼다
            state.queue_redraw_all();
        })
        .map_err(|err| err.error)?;

    event_loop
        .handle()
        .insert_source(session_notifier, move |event, _, state| match event {
            SessionEvent::PauseSession => {
                libinput.suspend();
                state.backend.drm.pause();
                // 다른 가상 터미널로 가면 누르고 있던 키와 단추를 떼는 이벤트가 오지 않는다 (agent_guard.rs)
                state.agent_guard.human.release_all();
                if let Some(floor) = &mut state.backend.frame_floor {
                    floor.release(Instant::now());
                }
            }
            SessionEvent::ActivateSession => {
                if libinput.resume().is_err() {
                    tracing::warn!("failed to resume libinput");
                }
                wake_input.ping();
                if let Err(err) = state.backend.drm.activate(false) {
                    tracing::warn!("failed to activate DRM device: {err}");
                }
                for surface in state.backend.outputs.values_mut() {
                    if let Err(err) = surface.compositor.reset_state() {
                        tracing::warn!("failed to reset DRM surface state: {err}");
                    }
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
    // 셸이 끝나면 [session] exit-with-shell을 따른다 (session.rs)
    state.spawn_shell();
    state.spawn_input_method();
    for command in state.config.session.startup.clone() {
        state.spawn(&command);
    }
    state.render_all();

    event_loop.run(None, &mut state, |state| {
        state.space.refresh();
        state.popups.cleanup();
        // 글 입력이 켜지거나 꺼졌으면 셸의 화면 키보드에 알린다 (osk.rs). 앱의 요청, 포커스 바꾸기, 잠금이 모두 바꾼다
        state.publish_text_input();
        if let Err(err) = state.display_handle.flush_clients() {
            tracing::warn!("failed to flush clients: {err}");
        }
    })?;
    state.hand_over_screens();
    Ok(())
}

impl BuntState {
    /// 컴포지터가 끝날 때. 창이 남은 채 끝나면(⌃⌥⌫) 앱의 내용을 다음 세션까지 화면에 두지 않는다. 잠긴 동안에는
    /// 잠금 화면이 보이므로 남겨도 된다 (handover.rs)
    fn hand_over_screens(&mut self) {
        let keep_last_frame = self.session_lock.is_locked() || self.all_windows().next().is_none();
        self.backend.hand_over_screens(keep_last_frame);
    }

    /// 컴포지터가 뜰 때: 앞 컴포지터가 켜 둔 화면을 끄지 않고 넘겨받고, 쓰지 않을 것만 끈다 (handover.rs)
    fn take_over_screens(&mut self) {
        let connected = self.connected_screens();
        let UdevData { drm, .. } = &mut self.backend;
        let result = if drm.is_atomic() {
            let mut kept: Vec<plane::Handle> = Vec::new();
            let keep: Vec<_> = connected
                .iter()
                .filter_map(|(info, crtc)| {
                    let planes = drm.planes(crtc).ok()?;
                    let primary = planes
                        .primary
                        .iter()
                        .map(|plane| plane.handle)
                        .find(|plane| !kept.contains(plane))?;
                    kept.push(primary);
                    Some((info.handle(), *crtc, primary))
                })
                .collect();
            handover::turn_off_unused(drm, &keep).map_err(|err| err.to_string())
        } else {
            Err("this driver has no atomic modesetting".to_owned())
        };
        // 켜진 것을 하나하나 끄지 못하면 전처럼 다 끄고 시작한다. 패널이 다시 켜지지만 화면은 맞다
        if let Err(err) = result {
            tracing::warn!("resetting the display: {err}");
            if let Err(err) = drm.reset_state() {
                tracing::warn!("failed to reset the display: {err}");
            }
        }
        for (info, crtc) in connected {
            self.connector_connected(info, crtc);
        }
        // 커널 콘솔이 있으면 앞 세션의 마지막 화면은 이미 없다: 앞 세션의 파일이 닫힐 때 콘솔이 화면을 가져갔다.
        // 첫 화면을 낼 때까지 콘솔 글자가 보이지 않게 검게 덮는다. CRTC는 끄지 않는다
        if handover::kernel_console_present() {
            let UdevData {
                outputs, renderer, ..
            } = &mut self.backend;
            for surface in outputs.values_mut() {
                blank_frame(&mut surface.compositor, renderer);
            }
        }
    }

    /// 연결된 커넥터 가운데 아직 출력이 없는 것에 출력을 만든다
    fn scan_connectors(&mut self) {
        for (info, crtc) in self.connected_screens() {
            self.connector_connected(info, crtc);
        }
    }

    /// 연결된 커넥터 가운데 아직 출력이 없는 것과 쓸 CRTC
    fn connected_screens(&self) -> Vec<(connector::Info, crtc::Handle)> {
        let device = &self.backend.drm;
        let resources = match device.resource_handles() {
            Ok(resources) => resources,
            Err(err) => {
                tracing::warn!("failed to read DRM resources: {err}");
                return Vec::new();
            }
        };
        let used: Vec<crtc::Handle> = self.backend.outputs.keys().copied().collect();
        let known: Vec<connector::Handle> = self
            .backend
            .outputs
            .values()
            .map(|surface| surface.connector)
            .collect();
        let mut infos = Vec::new();
        let mut wanted = Vec::new();
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
            wanted.push(Wanted {
                connector: infos.len(),
                current: handover::current_crtc(device, &info),
                possible: info
                    .encoders()
                    .iter()
                    .filter_map(|encoder| device.get_encoder(*encoder).ok())
                    .flat_map(|encoder| resources.filter_crtcs(encoder.possible_crtcs()))
                    .collect(),
            });
            infos.push(info);
        }
        handover::assign_crtcs(&wanted, &used)
            .into_iter()
            .map(|(index, crtc)| (infos[index].clone(), crtc))
            .collect()
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
        let (make, model) = edid_blob(&self.backend.drm, info.handle())
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

        let planes = self.backend.drm.planes(&crtc).ok();
        if let Some(planes) = &planes {
            tracing::info!(
                "output {name}: planes primary {}, cursor {}, overlay {}; cursor size {:?}",
                planes.primary.len(),
                planes.cursor.len(),
                planes.overlay.len(),
                self.backend.drm.cursor_size()
            );
        }
        let compositor =
            match self
                .backend
                .new_compositor(crtc, drm_mode, info.handle(), &output, planes)
            {
                Ok(compositor) => compositor,
                Err(err) => {
                    tracing::warn!("failed to set up output {name}: {err:?}");
                    self.space.unmap_output(&output);
                    self.display_handle.remove_global::<BuntState>(global);
                    return;
                }
            };
        // 앞 컴포지터의 모드를 그대로 쓰면 첫 프레임이 페이지 플립이다. e2e 1번이 이 기록을 본다
        tracing::info!(
            "output {name}: {}",
            if compositor.surface().commit_pending() {
                "the first frame sets the mode"
            } else {
                "keeping the mode of the previous compositor"
            }
        );

        let wallpaper = self.config.wallpaper_buffer(wl_mode);
        tracing::info!(
            "output {name}: {}x{} @ {} mHz, scale {scale}",
            wl_mode.size.w,
            wl_mode.size.h,
            wl_mode.refresh
        );
        self.backend.outputs.insert(
            crtc,
            OutputSurface {
                output,
                preferred_scale,
                compositor,
                connector: info.handle(),
                modes,
                mode: drm_mode,
                builtin,
                description,
                wallpaper,
                frame_pending: false,
                rendered_at: None,
                tried_at: None,
                idle_frames: 0,
                redraw_timer: None,
                first_frame_deadline: Some(Instant::now() + FIRST_FRAME_WAIT),
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
            let info = match self.backend.drm.get_connector(connector, true) {
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
        // 새로 꽂은 화면은 아직 그린 적이 없다
        self.queue_redraw_all();
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
        if let Some((token, _)) = surface.redraw_timer {
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
        let Some(name) = self
            .backend
            .outputs
            .get(&crtc)
            .map(|surface| surface.output.name())
        else {
            return false;
        };
        if let Err(err) = self.backend.use_mode(crtc, mode) {
            tracing::warn!("failed to change the mode of {name}: {err:?}");
            return false;
        }
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return false;
        };
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
        self.frame_floor_frame(crtc);
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return;
        };
        if let Err(err) = surface.compositor.frame_submitted() {
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

    /// 프레임이 화면에 나갔다: 프레임 바닥에 알린다. 사람이 방금 입력했거나 컴포지터 애니메이션이 도는 동안만
    /// 프레임을 지킨다 (frame_floor.rs)
    fn frame_floor_frame(&mut self, crtc: crtc::Handle) {
        if self.backend.frame_floor.is_none() {
            return;
        }
        let now = Instant::now();
        let interactive =
            self.agent_guard.human.active_within(INTERACTIVE_FOR, now) || self.is_animating();
        let Some(refresh) = self
            .backend
            .outputs
            .get(&crtc)
            .map(|surface| surface.frame_interval(false))
        else {
            return;
        };
        let Some(floor) = &mut self.backend.frame_floor else {
            return;
        };
        let held = floor.frame(u64::from(u32::from(crtc)), refresh, now, interactive);
        if !held || self.backend.frame_floor_timer.is_some() {
            return;
        }
        // 최소 클럭을 건 채 움직임이 멈추면 프레임이 더 오지 않는다. 타이머로 보고 푼다
        let token =
            self.loop_handle
                .insert_source(Timer::from_duration(IDLE_RELEASE), |_, _, state| {
                    let left = state
                        .backend
                        .frame_floor
                        .as_mut()
                        .and_then(|floor| floor.idle(Instant::now()));
                    match left {
                        Some(left) => TimeoutAction::ToDuration(left),
                        None => {
                            state.backend.frame_floor_timer = None;
                            TimeoutAction::Drop
                        }
                    }
                });
        match token {
            Ok(token) => self.backend.frame_floor_timer = Some(token),
            Err(err) => tracing::warn!("failed to watch the frame floor: {}", err.error),
        }
    }

    /// `delay` 뒤에 그 화면을 그린다. 그보다 이르게 잡아 둔 다시 그리기가 있으면 그것을 쓰고, 늦게 잡아 둔
    /// 것(가려진 창의 프레임 알림)은 앞당긴다
    fn schedule_redraw_after(&mut self, crtc: crtc::Handle, delay: Duration) {
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return;
        };
        let deadline = Instant::now() + delay;
        if let Some((token, at)) = surface.redraw_timer {
            if at <= deadline {
                return;
            }
            self.loop_handle.remove(token);
            surface.redraw_timer = None;
        }
        let token =
            self.loop_handle
                .insert_source(Timer::from_deadline(deadline), move |_, _, state| {
                    if let Some(surface) = state.backend.outputs.get_mut(&crtc) {
                        surface.redraw_timer = None;
                    }
                    state.render_output(crtc);
                    TimeoutAction::Drop
                });
        match token {
            Ok(token) => surface.redraw_timer = Some((token, deadline)),
            Err(err) => tracing::warn!("failed to schedule redraw: {}", err.error),
        }
    }

    /// 포인터 장치가 연결됐다. 마우스 설정을 쓰고 시스템 설정에 알린다. 터치 화면은 포인터를 함께 알려도 마우스로
    /// 보지 않는다(입력 모드의 나누기와 같다, input_mode.rs)
    fn pointer_device_added(&mut self, device: &InputDevice) {
        if !device.has_capability(DeviceCapability::Pointer)
            || device.has_capability(DeviceCapability::Touch)
        {
            return;
        }
        let mut device = device.clone();
        apply_pointer(&mut device, self.pointer_prefs(), self.touchpad_prefs());
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

    /// 시스템 설정이 트랙패드 설정을 바꿨다. 다음 로그인에도 쓴다. 자연스러운 스크롤은 마우스와 같은
    /// 값이라 마우스 설정에도 쓴다(macOS와 같다)
    pub fn set_touchpad_prefs(&mut self, prefs: TouchpadPrefs, natural_scroll: bool) {
        let mut pointer = self.pointer_prefs();
        pointer.natural_scroll = natural_scroll;
        self.prefs.pointer = Some(pointer);
        self.prefs.touchpad = Some(prefs);
        self.prefs.save();
        self.apply_pointer_prefs();
        tracing::info!(
            "touchpad: tap to click {}, secondary click {}, speed {}, natural scroll {}",
            prefs.tap_to_click,
            prefs.secondary_click.name(),
            prefs.speed,
            natural_scroll
        );
    }

    /// 마우스와 트랙패드 설정을 연결된 장치와 커서에 쓴다
    pub fn apply_pointer_prefs(&mut self) {
        let prefs = self.pointer_prefs();
        let touchpad = self.touchpad_prefs();
        for device in &mut self.backend.pointer_devices {
            apply_pointer(device, prefs, touchpad);
        }
        self.backend.cursor.set_size(prefs.cursor_size);
        self.dbus.update(|snapshot| {
            snapshot.pointer = prefs;
            snapshot.touchpad = touchpad;
        });
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

    /// 셸의 배경화면(Background 레이어)이나 창이 버퍼를 올렸는지, 또는 화면이 잠겼는지
    fn output_has_content(&self, output: &Output) -> bool {
        let drawn = |surface: &WlSurface| {
            with_renderer_surface_state(surface, |state| state.buffer().is_some()).unwrap_or(false)
        };
        self.session_lock.is_locked()
            || layer_map_for_output(output)
                .layers()
                .any(|layer| layer.layer() == Layer::Background && drawn(layer.wl_surface()))
            || self.space.elements().any(|window| {
                window
                    .toplevel()
                    .is_some_and(|toplevel| drawn(toplevel.wl_surface()))
            })
    }

    /// 이 화면을 지금 그릴지 정한다. 기다리는 동안 앞 세션이 남긴 화면이나 부팅 화면이 그대로 있다
    fn hold_decision(&mut self, crtc: crtc::Handle) -> Hold {
        let Some(output) = self
            .backend
            .outputs
            .get(&crtc)
            .map(|surface| surface.output.clone())
        else {
            return Hold::Draw;
        };
        let has_content = self.output_has_content(&output);
        let exit_with_shell = self.config.session.exit_with_shell;
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return Hold::Draw;
        };
        let hold = hold_for(
            surface.first_frame_deadline,
            Instant::now(),
            has_content,
            exit_with_shell,
        );
        if hold == Hold::Draw {
            surface.first_frame_deadline = None;
        }
        hold
    }

    fn render_output(&mut self, crtc: crtc::Handle) {
        if !self.backend.session.is_active() {
            return;
        }
        match self.hold_decision(crtc) {
            Hold::Draw => {}
            Hold::Until(wait) => {
                self.schedule_redraw_after(crtc, wait);
                return;
            }
            Hold::Stay => return,
        }
        if let CursorImageStatus::Surface(surface) = &self.cursor_status
            && !surface.alive()
        {
            self.cursor_status = CursorImageStatus::default_named();
        }

        // 뒤를 흐릴 표면: 셸 유리와, 앱이 요청한 영역(background_effect.rs. 셸 유리로 흐리는 표면은 뺀다).
        // 회전한 출력에서는 흐리지 않는다 (blur.rs는 회전하지 않은 화면만 안다). 전에는 셸 표면만 모았다
        let blur_output = self
            .backend
            .outputs
            .get(&crtc)
            .map(|surface| surface.output.clone())
            .filter(|output| {
                self.config.effects.blur && output.current_transform() == Transform::Normal
            });
        let blur_targets: Vec<BlurTarget> = match blur_output {
            Some(output) => {
                let glass = self.blur_surfaces(&output);
                let apps = self.backend.background_effects.targets();
                glass
                    .iter()
                    .cloned()
                    .map(BlurTarget::glass)
                    .chain(
                        apps.into_iter()
                            .filter(|target| !glass.contains(&target.surface)),
                    )
                    .collect()
            }
            None => Vec::new(),
        };

        // 막대를 숨길지 다시 보고(앞의 창이 최대화했을 수 있다), 다 보이지 않는 막대의 자리를 정한다.
        // 숨기고 꺼낼 때의 미끄러짐은 이 렌더 루프가 프레임마다 그린다. 최소화 효과는 칸의 자리를 새로 본다
        self.update_bars(None);
        self.update_minimize_effects();
        self.check_recording();
        let bar_places = match self.backend.outputs.get(&crtc) {
            Some(surface) => self.bar_places(&surface.output),
            None => Vec::new(),
        };
        // AI 대상 강조 (agent_target.rs): 창의 지금 자리를 따르고, 그 창의 쌓인 자리에 그린다. 잠긴 동안에는 그리지 않는다
        let agent_target = if self.session_lock.is_locked() {
            None
        } else {
            self.agent_target_rect()
        };
        let agent_target_ids = if agent_target.is_some() {
            self.agent_target_ids()
        } else {
            Vec::new()
        };

        let UdevData {
            renderer,
            outputs,
            cursor,
            cursor_buffer,
            cursor_hotspot,
            cursor_key,
            blur,
            background_effects,
            rendering,
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
        if self.input_devices.cursor_shown() && output_geo.to_f64().contains(pointer_pos) {
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
        // AI 대상 강조 테두리. 창과 막대를 모은 뒤 대상 창의 자리에 끼운다
        let highlight =
            self.agent_target
                .element(renderer, &output.name(), output_geo, agent_target, scale.x);
        // 스크린샷에는 커서를 넣지 않는다 (AI 대상 강조 테두리도 아래에서 더한다)
        let mut cursor_ids: Vec<Id> = elements
            .iter()
            .map(|element| element.id().clone())
            .collect();

        // ⇧⌘4로 고르는 영역과 ⇧⌘5의 선택 영역: 창과 막대 위, ⇧⌘5 도구 막대 아래(screenshot.rs의
        // place_overlay). 전에는 커서 바로 아래에 넣었는데, ⇧⌘5의 선택 영역 위에는 도구 막대가 남아야 한다
        let selection = self
            .screenshot_area
            .as_mut()
            .map(|selection| selection.elements(renderer, output_geo, scale))
            .unwrap_or_default();

        // 잠긴 동안에는 잠금 표면과 검은 바탕만 그린다. 창과 셸 막대는 넣지 않는다 (session_lock.rs)
        let lock_scene = self
            .session_lock
            .scene(renderer, &output, output_geo.size, scale);
        let locked = lock_scene.is_some();
        elements.extend(lock_scene.into_iter().flatten());
        // 최소화 효과 (minimize.rs): 창과 막대 위. 잠긴 동안에는 그리지 않는다
        if !locked {
            elements.extend(self.minimize_effects.elements(output_geo, scale));
        }
        let space_start = elements.len();
        match space_render_elements::<_, smithay::desktop::Window, _>(
            renderer,
            [&self.space],
            &output,
            1.0,
        ) {
            Ok(_) if locked => {}
            Ok(mut space_elements) => {
                render::place_bars(renderer, &mut space_elements, &bar_places, scale);
                // 최소화 칸에서 나오는 창은 효과가 그리는 동안 공간에서 빼 둔다
                self.minimize_effects.hide_restoring(&mut space_elements);
                elements.extend(space_elements.into_iter().map(OutputRenderElements::Space))
            }
            Err(err) => tracing::warn!("failed to collect windows: {err:?}"),
        }
        if !locked {
            screenshot::place_overlay(&mut elements, space_start, selection, &output);
        }
        // AI 대상 강조 테두리: 대상 창의 맨 앞 표면(열린 메뉴 포함) 바로 위에 끼운다. 그 창을 덮은 창과 막대가 테두리도
        // 덮어, 사람이 쓰는 창 위에 AI 테두리가 뜨지 않는다. 입력을 받지 않는 그림일 뿐이다. 커서처럼 스크린샷에는 넣지
        // 않고 화면 기록에는 넣는다. 에이전트의 창 그림(capture_window)은 창만 그려 들지 않는다. 그 창이 이 화면에 없으면
        // 그리지 않는다
        let space_start = space_start.min(elements.len());
        if let Some(element) = highlight
            && let Some(at) = elements[space_start..]
                .iter()
                .position(|element| agent_target_ids.contains(element.id()))
        {
            cursor_ids.push(element.id().clone());
            elements.insert(space_start + at, OutputRenderElements::Highlight(element));
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
        let flags = if blur.insert(renderer, &mut elements, &blur_targets, &output, scale) {
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
        // 화면 기록 (recording.rs): 기록하는 화면이면 커서까지 넣은 같은 장면을 따로 그려 읽는다
        if let Some(recording) = self
            .recording
            .as_mut()
            .filter(|recording| *recording.output() == output)
        {
            let scene: Vec<&Elements> = elements.iter().collect();
            recording.capture(renderer, &scene);
        }

        let elements = texture_shader::precise(renderer, &elements);

        // 한 프레임 간격보다 오래 걸린 프레임은 디버그 기록에 남긴다 (RUST_LOG=bunt_comp=debug)
        let interval = surface.frame_interval(rendering.cpu_composited);
        let started = std::time::Instant::now();
        let rendered = surface
            .compositor
            .render_frame(renderer, &elements, CLEAR_COLOR, flags);
        // 렌더 펜스를 커밋에 실을 수 없으면(simpledrm처럼 DRM syncobj가 없는 드라이버) 커널이 렌더링을
        // 기다려 주지 않는다. llvmpipe는 스레드에서 그리고 simpledrm은 커밋 때 버퍼를 화면으로 한 번 복사하므로,
        // 기다리지 않으면 덜 그린 버퍼가 화면에 남는다
        if let Ok(frame) = &rendered
            && frame.needs_sync()
            && let PrimaryPlaneElement::Swapchain(primary) = &frame.primary_element
            && let Err(err) = primary.sync.wait()
        {
            tracing::warn!("failed to wait for rendering: {err:?}");
        }
        let elapsed = started.elapsed();
        if elapsed > interval {
            tracing::debug!("slow frame on {}: {elapsed:?}", output.name());
        }
        // 흐림 셰이더를 만들지 못했으면 앱에도 흐림이 없다고 알린다
        if blur.broken() {
            background_effects.set_blur(false);
        }
        let (queued, states) = match rendered {
            Ok(frame) => {
                let queued = !frame.is_empty
                    && match surface.compositor.queue_frame(()) {
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
        surface.tried_at = Some(Instant::now());
        if queued {
            surface.rendered_at = surface.tried_at;
            surface.idle_frames = 0;
        } else {
            surface.idle_frames = surface.idle_frames.saturating_add(1);
        }
        let idle_frames = surface.idle_frames;
        if let Some(states) = &states {
            self.note_visible_surfaces(&output, states);
        }
        // 잠긴 장면을 모든 화면에 낸 뒤에야 잠금 화면에 잠겼다고 알린다 (session_lock.rs)
        self.lock_frame_done(&output, queued);
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
        self.session_lock.send_frames(&output, elapsed);
        if let CursorImageStatus::Surface(cursor_surface) = &self.cursor_status {
            send_frames_surface_tree(
                cursor_surface,
                &output,
                elapsed,
                HIDDEN_FRAME_INTERVAL,
                surface_primary_scanout_output,
            );
        }

        // 새로 보낸 프레임이 없으면 플립 이벤트가 오지 않는다. 애니메이션이 돌거나 방금 바뀐 것이 있었으면
        // 한 프레임 뒤에 다시 그리고, 아니면 쉰다. 쉬는 동안에도 프레임 알림을 기다리는 표면이 있으면 알림을
        // 보낼 때 깨어난다
        if !queued {
            if self.is_animating() || idle_frames < IDLE_AFTER {
                self.schedule_redraw_after(crtc, interval);
            } else if let Some(wait) = self.frame_callback_wait(&output) {
                self.schedule_redraw_after(crtc, wait);
            }
        }
    }

    /// 프레임 알림을 기다리는 표면이 남았으면 다음 알림까지 기다릴 시간. 이번 프레임에 보인 표면은 늘 알림을
    /// 받으므로, 남는 것은 소프트웨어 렌더러에서 앞에 있지 않은 창(BACKGROUND_FRAME)과 가려진 표면
    /// (HIDDEN_FRAME_INTERVAL)이다
    fn frame_callback_wait(&self, output: &Output) -> Option<Duration> {
        let (mut visible, mut hidden) = (false, false);
        let mut waiting = |surface: &WlSurface, states: &SurfaceData| {
            let pending = !states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .frame_callbacks
                .is_empty();
            if !pending {
                return;
            }
            if surface_primary_scanout_output(surface, states).as_ref() == Some(output) {
                visible = true;
            } else {
                hidden = true;
            }
        };
        for window in self.space.elements() {
            window.with_surfaces(&mut waiting);
        }
        for layer in layer_map_for_output(output).layers() {
            layer.with_surfaces(&mut waiting);
        }
        if visible {
            Some(BACKGROUND_FRAME)
        } else if hidden {
            HIDDEN_FRAME_INTERVAL
        } else {
            None
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

/// 이 카드 노드(`/dev/dri/cardN`)에 커넥터가 있는지. 커널은 sysfs의 `cardN/cardN-<커넥터>`로 알린다. 장치를 열지 않고 본다
fn has_connectors(card: &Path) -> bool {
    has_connectors_in(Path::new("/sys/class/drm"), card)
}

fn has_connectors_in(class_drm: &Path, card: &Path) -> bool {
    let Some(name) = card.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let prefix = format!("{name}-");
    std::fs::read_dir(class_drm.join(name)).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
    })
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

/// 장치가 지원하는 설정만 쓴다 (절대 좌표 장치에는 가속이 없다). 탭을 아는 장치는 트랙패드다. 트랙패드는
/// 트랙패드의 이동 속도와 클릭 설정을 쓰고, 마우스의 보조 클릭(왼손잡이)은 쓰지 않는다(macOS와 같다).
/// 자연스러운 스크롤은 함께 쓴다
fn apply_pointer(device: &mut InputDevice, prefs: PointerPrefs, touchpad: TouchpadPrefs) {
    let is_touchpad = device.config_tap_finger_count() > 0;
    if device.config_accel_is_available() {
        let speed = if is_touchpad {
            touchpad.speed
        } else {
            prefs.speed
        };
        let _ = device.config_accel_set_speed(speed.clamp(-1.0, 1.0));
    }
    if device.config_scroll_has_natural_scroll() {
        let _ = device.config_scroll_set_natural_scroll_enabled(prefs.natural_scroll);
    }
    let left_handed = prefs.left_handed && !is_touchpad;
    if device.config_left_handed_is_available() {
        let _ = device.config_left_handed_set(left_handed);
    }
    if is_touchpad {
        apply_touchpad(device, touchpad);
    }
}

/// 탭하여 클릭하기와 보조 클릭. 탭은 한 손가락이 클릭, 두 손가락이 보조 클릭, 세 손가락이 가운데
/// 클릭이다. 누르는 트랙패드(clickpad)는 보조 클릭 방식에 따라 두 손가락으로 누르거나 오른쪽 아래
/// 모서리를 누르면 보조 클릭이다. libinput은 누르는 트랙패드에서 탭을 기본으로 끈다
fn apply_touchpad(device: &mut InputDevice, prefs: TouchpadPrefs) {
    let _ = device.config_tap_set_enabled(prefs.tap_to_click);
    let _ = device.config_tap_set_button_map(TapButtonMap::LeftRightMiddle);
    let method = match prefs.secondary_click {
        SecondaryClick::TwoFingers => ClickMethod::Clickfinger,
        SecondaryClick::BottomRight => ClickMethod::ButtonAreas,
    };
    if device.config_click_methods().contains(&method) {
        let _ = device.config_click_set_method(method);
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

impl BuntState {
    /// 화면이 바뀌었다(앱의 커밋, 입력, 셸과 에이전트의 요청, 창과 막대의 변화). 화면마다 다음 프레임을 그린다.
    /// 프레임을 낸 화면은 플립 뒤(frame_finish)에 이어 그리고, 쉬던 화면은 한 프레임 간격을 띄워 그린다.
    /// 컴포지터가 직접 그리는 것(선택 사각형, 효과)이 바뀌어도 부른다
    pub fn queue_redraw_all(&mut self) {
        let crtcs: Vec<_> = self.backend.outputs.keys().copied().collect();
        for crtc in crtcs {
            self.queue_redraw(crtc);
        }
    }

    fn queue_redraw(&mut self, crtc: crtc::Handle) {
        let cpu_composited = self.backend.rendering.cpu_composited;
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return;
        };
        surface.idle_frames = 0;
        if surface.frame_pending {
            return;
        }
        // 바로 앞에 그려 본 때부터 한 프레임 간격을 띄운다. 그리는 것 없이 프레임 알림만 바라는 앱이 빈
        // 프레임을 쉬지 않고 그리게 하지 않는다
        let delay = surface.tried_at.map_or(Duration::ZERO, |at| {
            surface
                .frame_interval(cpu_composited)
                .saturating_sub(at.elapsed())
        });
        self.schedule_redraw_after(crtc, delay);
    }

    /// 컴포지터가 그리는 애니메이션(막대 미끄러짐, 최소화 효과)이 도는 중이거나, 화면 기록이 건너뛴 프레임을
    /// 읽으려고 다음 프레임을 그려야 한다. 그동안은 프레임마다 그린다
    pub fn is_animating(&self) -> bool {
        self.bars_sliding()
            || self.minimize_effects.is_running()
            || self
                .recording
                .as_ref()
                .is_some_and(crate::recording::Recording::wants_frame)
    }
}

/// 화면을 그릴지 정한 결과
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hold {
    Draw,
    /// 셸의 첫 화면을 기다린다. 커밋이 오면 그때 다시 정하고, 이만큼 지나면 그냥 그린다
    Until(Duration),
    /// 로그인 화면이 끝나는 중이다. 다음 세션이 그릴 때까지 마지막 화면을 둔다
    Stay,
}

/// 세션이 시작될 때 설정의 배경화면이 잠깐 보이지 않게 셸의 첫 화면을 기다린다. 로그인 화면(`exit_with_shell`)은
/// 창이 사라지면 곧 끝나므로, 그 사이에 빈 배경화면을 내지 않는다
fn hold_for(
    deadline: Option<Instant>,
    now: Instant,
    has_content: bool,
    exit_with_shell: bool,
) -> Hold {
    if has_content {
        return Hold::Draw;
    }
    match deadline {
        Some(deadline) if now < deadline => Hold::Until(deadline - now),
        Some(_) => Hold::Draw,
        None if exit_with_shell => Hold::Stay,
        None => Hold::Draw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_frame_waits_for_the_shell_until_the_deadline() {
        let now = Instant::now();
        let later = now + Duration::from_secs(2);
        assert_eq!(
            hold_for(Some(later), now, false, false),
            Hold::Until(Duration::from_secs(2))
        );
        assert_eq!(hold_for(Some(later), now, true, false), Hold::Draw);
        assert_eq!(hold_for(Some(now), later, false, false), Hold::Draw);
    }

    #[test]
    fn the_login_screen_keeps_its_last_frame_when_its_window_goes_away() {
        let now = Instant::now();
        assert_eq!(hold_for(None, now, false, true), Hold::Stay);
        assert_eq!(hold_for(None, now, true, true), Hold::Draw);
        // 사용자 세션은 창이 다 닫혀도 배경화면을 그린다
        assert_eq!(hold_for(None, now, false, false), Hold::Draw);
    }

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
    fn only_a_card_with_connectors_can_show_anything() {
        let sys = std::env::temp_dir().join(format!("bunt-comp-drm-{}", std::process::id()));
        // 화면 드라이버는 커넥터가 있고, 그리기만 하는 GPU(panthor)는 카드 폴더에 다른 것만 있다
        std::fs::create_dir_all(sys.join("card2/card2-DSI-1")).unwrap();
        std::fs::create_dir_all(sys.join("card1/device")).unwrap();
        assert!(has_connectors_in(&sys, Path::new("/dev/dri/card2")));
        assert!(!has_connectors_in(&sys, Path::new("/dev/dri/card1")));
        // 이름이 앞부분만 같은 다른 카드의 폴더에 속지 않고, 없는 카드도 없는 것으로 본다
        std::fs::create_dir_all(sys.join("card1/card10-DSI-1")).unwrap();
        assert!(!has_connectors_in(&sys, Path::new("/dev/dri/card1")));
        assert!(!has_connectors_in(&sys, Path::new("/dev/dri/card9")));
        std::fs::remove_dir_all(&sys).unwrap();
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
