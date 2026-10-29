// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 시험 도우미. GPU 없이 dmabuf 가져오기, 흐림, 앱의 흐림 요청을 끝까지 시험한다.
//! - 렌더러: Mesa의 소프트웨어 EGL 장치(llvmpipe). llvmpipe는 memfd를 dmabuf처럼 가져온다
//! - 서버: 컴포지터와 같은 처리(커밋, linux-dmabuf, ext-background-effect)를 하는 작은 Wayland 서버
//! - 클라이언트: 같은 프로세스에서 소켓 쌍으로 붙는 wayland-client. 서버와 번갈아 돌린다(pump)
//!
//! 소프트웨어 EGL 장치가 없는 기계에서는 GL 시험이 알리고 건너뛴다

use std::{
    io::Write,
    os::{
        fd::{AsFd, FromRawFd, OwnedFd},
        unix::net::UnixStream,
    },
    sync::{Arc, Mutex, MutexGuard},
};

use smithay::{
    backend::{
        allocator::{Fourcc, dmabuf::Dmabuf, format::FormatSet},
        egl::{EGLContext, EGLDevice, EGLDisplay},
        renderer::{
            Bind, ExportMem, ImportDma, Offscreen,
            damage::OutputDamageTracker,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::{GlesRenderer, GlesTexture},
            utils::on_commit_buffer_handler,
        },
    },
    delegate_compositor, delegate_dmabuf, delegate_shm,
    reexports::wayland_server::{
        Client, Display,
        backend::{ClientData, ClientId, DisconnectReason},
        protocol::{wl_buffer::WlBuffer, wl_surface::WlSurface},
    },
    utils::{Physical, Point, Scale, Size, Transform},
    wayland::{
        buffer::BufferHandler,
        compositor::{CompositorClientState, CompositorHandler, CompositorState},
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        shm::{ShmHandler, ShmState},
    },
};
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, WEnum,
    backend::WaylandError,
    delegate_noop, event_created_child,
    protocol::{wl_buffer, wl_compositor, wl_region, wl_registry, wl_shm, wl_shm_pool, wl_surface},
};
use wayland_protocols::{
    ext::background_effect::v1::client::{
        ext_background_effect_manager_v1, ext_background_effect_surface_v1,
    },
    wp::linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
    },
};

use crate::{
    background_effect::{self, BackgroundEffectHandler, BackgroundEffectState},
    dmabuf::{self, Dmabufs},
    render::OutputRenderElements,
    shmdiff,
};

/// 시험의 기본 피드백에 적는 주 장치 (실제 장치는 없다)
pub const MAIN_DEVICE: libc::dev_t = 0xe280;

/// EGL 디스플레이는 장치마다 하나라, 시험끼리 함께 쓰면 한쪽이 끝낼 때 다른 쪽이 깨진다. 하나씩 돈다
static GL: Mutex<()> = Mutex::new(());

pub fn gl_lock() -> MutexGuard<'static, ()> {
    GL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// llvmpipe GLES 렌더러. 소프트웨어 EGL 장치가 없으면 None
pub fn software_renderer() -> Option<GlesRenderer> {
    let device = EGLDevice::enumerate().ok()?.find(EGLDevice::is_software)?;
    // 안전: 장치는 디스플레이를 만드는 동안만 쓰고, 디스플레이는 컨텍스트가 붙들고 있다
    let display = unsafe { EGLDisplay::new(device) }.ok()?;
    let context = EGLContext::new(&display).ok()?;
    // 안전: 컨텍스트를 이 렌더러만 쓴다
    unsafe { GlesRenderer::new(context) }.ok()
}

/// 내용을 담은 memfd. 시험 클라이언트의 wl_shm 풀과 dmabuf 평면으로 쓴다
pub fn memfd(contents: &[u8]) -> OwnedFd {
    // 안전: 이름은 NUL로 끝나고, 돌려받은 fd는 여기서만 갖는다
    let fd = unsafe { libc::memfd_create(c"bunt-test".as_ptr(), libc::MFD_CLOEXEC) };
    assert!(fd >= 0, "memfd_create failed");
    // 안전: 방금 만든 fd다
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let mut file = std::fs::File::from(fd.try_clone().expect("dup memfd"));
    file.write_all(contents).expect("write memfd");
    fd
}

/// 한 가지 색으로 채운 Argb8888 그림 (리틀 엔디언 B, G, R, A)
pub fn argb(width: usize, height: usize, [r, g, b, a]: [u8; 4]) -> Vec<u8> {
    [b, g, r, a].repeat(width * height)
}

/// 세로 줄무늬 Argb8888 그림. 흐리면 줄이 섞여 가운데 회색이 된다
pub fn stripes(width: usize, height: usize, period: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width * height * 4);
    for _ in 0..height {
        for x in 0..width {
            let value = if (x / period).is_multiple_of(2) {
                0
            } else {
                255
            };
            pixels.extend_from_slice(&[value, value, value, 255]);
        }
    }
    pixels
}

/// 시험 서버의 상태
pub struct ServerState {
    pub compositor: CompositorState,
    pub shm: ShmState,
    pub dmabufs: Dmabufs,
    pub effects: BackgroundEffectState,
    /// dmabuf를 가져올 렌더러. 없으면 가져오기가 늘 실패한다
    pub renderer: Option<GlesRenderer>,
    /// 버퍼를 커밋한 표면 (처음 커밋한 차례)
    pub surfaces: Vec<WlSurface>,
}

#[derive(Default)]
struct TestClientData {
    compositor: CompositorClientState,
}

impl ClientData for TestClientData {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

impl CompositorHandler for ServerState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<TestClientData>().unwrap().compositor
    }

    /// 컴포지터(state.rs)와 같이 SHM 손상을 줄이고 버퍼를 받는다
    fn commit(&mut self, surface: &WlSurface) {
        shmdiff::narrow_damage(surface);
        on_commit_buffer_handler::<Self>(surface);
        if !self.surfaces.contains(surface) {
            self.surfaces.push(surface.clone());
        }
    }
}

impl BufferHandler for ServerState {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for ServerState {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl DmabufHandler for ServerState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        self.dmabufs.state_mut()
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        match self.renderer.as_mut() {
            Some(renderer) => {
                dmabuf::import::<Self>(renderer, self.dmabufs.node(), dmabuf, notifier)
            }
            None => notifier.failed(),
        }
    }
}

impl BackgroundEffectHandler for ServerState {
    fn background_effect_state(&mut self) -> &mut BackgroundEffectState {
        &mut self.effects
    }
}

delegate_compositor!(ServerState);
delegate_shm!(ServerState);
delegate_dmabuf!(ServerState);
background_effect::delegate_background_effect!(ServerState);

/// 시험 서버를 만들 때 고르는 것
pub struct ServerOptions {
    /// linux-dmabuf 전역을 열고 이 형식을 알린다. None이면 열지 않는다 (CPU 렌더러의 컴포지터)
    pub dmabuf_formats: Option<FormatSet>,
    /// ext-background-effect 전역을 연다
    pub blur: bool,
}

pub struct Server {
    pub display: Display<ServerState>,
    pub state: ServerState,
}

impl Server {
    pub fn new(renderer: Option<GlesRenderer>, options: ServerOptions) -> Self {
        let display: Display<ServerState> = Display::new().expect("wayland display");
        let dh = display.handle();
        let dmabufs = match options.dmabuf_formats {
            Some(formats) => Dmabufs::open::<ServerState>(&dh, MAIN_DEVICE, None, formats),
            None => Dmabufs::new::<ServerState>(&dh, None),
        };
        let state = ServerState {
            compositor: CompositorState::new::<ServerState>(&dh),
            shm: ShmState::new::<ServerState>(&dh, vec![]),
            dmabufs,
            effects: BackgroundEffectState::new::<ServerState>(&dh, options.blur),
            renderer,
            surfaces: Vec::new(),
        };
        Self { display, state }
    }

    /// 새 클라이언트를 붙이고 전역 목록까지 받는다
    pub fn connect(&mut self) -> TestClient {
        let (ours, theirs) = UnixStream::pair().expect("socket pair");
        ours.set_nonblocking(true).unwrap();
        theirs.set_nonblocking(true).unwrap();
        self.display
            .handle()
            .insert_client(ours, Arc::new(TestClientData::default()))
            .expect("insert client");
        let conn = Connection::from_socket(theirs).expect("client connection");
        let queue = conn.new_event_queue();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());
        let mut client = TestClient {
            conn,
            queue,
            qh,
            data: ClientEvents::default(),
        };
        self.pump(&mut client);
        client
    }

    /// 서버와 클라이언트를 몇 번 번갈아 돌려 보낸 요청과 이벤트를 다 처리한다
    pub fn pump(&mut self, client: &mut TestClient) {
        for _ in 0..6 {
            let _ = client.conn.flush();
            self.display
                .dispatch_clients(&mut self.state)
                .expect("server dispatch");
            self.display.flush_clients().expect("server flush");
            client.read();
        }
    }

    /// 렌더러를 빌려 쓴다 (dmabuf를 가져올 때와 같은 렌더러)
    pub fn renderer(&mut self) -> &mut GlesRenderer {
        self.state.renderer.as_mut().expect("a renderer")
    }

    /// 커밋한 n번째 표면을 `location`에 그리는 요소 (위에서 아래 순)
    pub fn surface_elements(
        &mut self,
        index: usize,
        location: Point<i32, Physical>,
        scale: Scale<f64>,
    ) -> Vec<OutputRenderElements> {
        let surface = self.state.surfaces[index].clone();
        render_elements_from_surface_tree::<_, WaylandSurfaceRenderElement<GlesRenderer>>(
            self.renderer(),
            &surface,
            location,
            scale,
            1.0,
            Kind::Unspecified,
        )
        .into_iter()
        .map(OutputRenderElements::Surface)
        .collect()
    }
}

/// 시험 클라이언트가 받은 것
#[derive(Default)]
pub struct ClientEvents {
    /// (이름, 인터페이스, 판)
    pub globals: Vec<(u32, String, u32)>,
    pub main_device: Option<Vec<u8>>,
    pub format_table: Option<(OwnedFd, u32)>,
    pub tranche_formats: Vec<Vec<u16>>,
    pub feedback_done: bool,
    pub created: Vec<wl_buffer::WlBuffer>,
    pub failed: usize,
    pub capabilities: Vec<u32>,
}

pub struct TestClient {
    pub conn: Connection,
    pub queue: EventQueue<ClientEvents>,
    pub qh: QueueHandle<ClientEvents>,
    pub data: ClientEvents,
}

impl TestClient {
    /// 소켓에 온 이벤트를 읽어 처리한다 (기다리지 않는다)
    fn read(&mut self) {
        if let Some(guard) = self.conn.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                // 서버가 프로토콜 오류로 끊었다. 시험이 protocol_error()로 본다
                Err(_) => {}
            }
        }
        let _ = self.queue.dispatch_pending(&mut self.data);
    }

    /// 전역을 묶는다. 서버에 없으면 None
    pub fn bind<I>(&self, interface: &str, version: u32) -> Option<I>
    where
        I: wayland_client::Proxy + 'static,
        ClientEvents: Dispatch<I, ()>,
    {
        let registry = self.registry();
        self.data
            .globals
            .iter()
            .find(|(_, name, _)| name == interface)
            .map(|(name, _, offered)| {
                registry.bind::<I, _, _>(*name, version.min(*offered), &self.qh, ())
            })
    }

    pub fn global_version(&self, interface: &str) -> Option<u32> {
        self.data
            .globals
            .iter()
            .find(|(_, name, _)| name == interface)
            .map(|(_, _, version)| *version)
    }

    fn registry(&self) -> wl_registry::WlRegistry {
        // 연결할 때 하나 만들었다. 같은 이름으로 다시 묶어도 서버는 상관하지 않는다
        self.conn.display().get_registry(&self.qh, ())
    }

    /// 서버가 끊으며 보낸 프로토콜 오류: 코드와 오류를 낸 객체의 인터페이스
    pub fn protocol_error(&self) -> Option<(u32, String)> {
        self.conn
            .protocol_error()
            .map(|error| (error.code, error.object_interface))
    }

    /// 한 가지 내용의 wl_shm Argb8888 버퍼
    pub fn shm_buffer(
        &self,
        shm: &wl_shm::WlShm,
        width: i32,
        height: i32,
        pixels: &[u8],
    ) -> wl_buffer::WlBuffer {
        let fd = memfd(pixels);
        let pool = shm.create_pool(fd.as_fd(), pixels.len() as i32, &self.qh, ());
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            width * 4,
            wl_shm::Format::Argb8888,
            &self.qh,
            (),
        );
        pool.destroy();
        buffer
    }

    /// memfd 한 평면으로 만든 dmabuf 버퍼 (create_immed). llvmpipe가 가져온다
    pub fn dmabuf_buffer(
        &self,
        dmabuf: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        (width, height): (i32, i32),
        format: Fourcc,
        modifier: u64,
        pixels: &[u8],
        flags: zwp_linux_buffer_params_v1::Flags,
    ) -> wl_buffer::WlBuffer {
        let fd = memfd(pixels);
        let params = dmabuf.create_params(&self.qh, ());
        params.add(
            fd.as_fd(),
            0,
            0,
            width as u32 * 4,
            (modifier >> 32) as u32,
            modifier as u32,
        );
        let buffer = params.create_immed(width, height, format as u32, flags, &self.qh, ());
        params.destroy();
        buffer
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for ClientEvents {
    fn event(
        state: &mut Self,
        _registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && !state.globals.iter().any(|(known, ..)| *known == name)
        {
            state.globals.push((name, interface, version));
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, ()> for ClientEvents {
    fn event(
        state: &mut Self,
        _feedback: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use zwp_linux_dmabuf_feedback_v1::Event;
        match event {
            Event::MainDevice { device } => state.main_device = Some(device),
            Event::FormatTable { fd, size } => state.format_table = Some((fd, size)),
            Event::TrancheFormats { indices } => state.tranche_formats.push(
                indices
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u16::from_ne_bytes(*pair))
                    .collect(),
            ),
            Event::Done => state.feedback_done = true,
            _ => {}
        }
    }
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for ClientEvents {
    fn event(
        state: &mut Self,
        _params: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        event: zwp_linux_buffer_params_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_buffer_params_v1::Event::Created { buffer } => state.created.push(buffer),
            zwp_linux_buffer_params_v1::Event::Failed => state.failed += 1,
            _ => {}
        }
    }

    event_created_child!(ClientEvents, zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, [
        zwp_linux_buffer_params_v1::EVT_CREATED_OPCODE => (wl_buffer::WlBuffer, ()),
    ]);
}

impl Dispatch<ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1, ()> for ClientEvents {
    fn event(
        state: &mut Self,
        _manager: &ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            state.capabilities.push(match flags {
                WEnum::Value(flags) => flags.bits(),
                WEnum::Unknown(bits) => bits,
            });
        }
    }
}

delegate_noop!(ClientEvents: ignore wl_compositor::WlCompositor);
delegate_noop!(ClientEvents: ignore wl_surface::WlSurface);
delegate_noop!(ClientEvents: ignore wl_region::WlRegion);
delegate_noop!(ClientEvents: ignore wl_shm::WlShm);
delegate_noop!(ClientEvents: ignore wl_shm_pool::WlShmPool);
delegate_noop!(ClientEvents: ignore wl_buffer::WlBuffer);
delegate_noop!(ClientEvents: ignore zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1);
delegate_noop!(ClientEvents: ignore ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1);

/// 화면 하나를 흉내 낸 오프스크린 그리기. 같은 텍스처에 잇달아 그려(나이 1) 손상 추적을 시험한다
pub struct Frames {
    damage: OutputDamageTracker,
    target: GlesTexture,
    size: Size<i32, Physical>,
    age: usize,
}

impl Frames {
    pub fn new(renderer: &mut GlesRenderer, size: Size<i32, Physical>, scale: f64) -> Self {
        let target = renderer
            .create_buffer(
                Fourcc::Abgr8888,
                size.to_logical(1).to_buffer(1, Transform::Normal),
            )
            .expect("offscreen target");
        Self {
            damage: OutputDamageTracker::new(size, scale, Transform::Normal),
            target,
            size,
            age: 0,
        }
    }

    /// 요소(위에서 아래 순)를 그리고 화면을 RGBA로 읽는다 (맨 위 줄부터)
    pub fn render(
        &mut self,
        renderer: &mut GlesRenderer,
        elements: &[OutputRenderElements],
    ) -> Vec<u8> {
        let mut framebuffer = renderer.bind(&mut self.target).expect("bind target");
        self.damage
            .render_output(
                renderer,
                &mut framebuffer,
                self.age,
                elements,
                [0.0, 0.0, 0.0, 1.0],
            )
            .expect("render");
        self.age = 1;
        let region = smithay::utils::Rectangle::from_size(
            self.size.to_logical(1).to_buffer(1, Transform::Normal),
        );
        let mapping = renderer
            .copy_framebuffer(&framebuffer, region, Fourcc::Abgr8888)
            .expect("read back");
        renderer.map_texture(&mapping).expect("map").to_vec()
    }
}

/// 배경으로 깔 Argb8888 그림 요소
pub fn background(
    renderer: &mut GlesRenderer,
    buffer: &MemoryRenderBuffer,
) -> OutputRenderElements {
    OutputRenderElements::Memory(
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (0.0, 0.0),
            buffer,
            None,
            None,
            None,
            Kind::Unspecified,
        )
        .expect("background element"),
    )
}

/// RGBA 화면의 한 점
pub fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let at = (y * width + x) * 4;
    [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
}

/// 렌더러가 가져올 수 있는 dmabuf 형식 (시험 서버가 알린다)
pub fn texture_formats(renderer: &GlesRenderer) -> FormatSet {
    renderer.dmabuf_formats()
}
