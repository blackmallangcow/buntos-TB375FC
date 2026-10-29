// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 앱 창 뒤 흐림(ext-background-effect-v1)과 앱의 dmabuf(linux-dmabuf)를 시험하는 작은 Wayland
//! 클라이언트. 반투명한 창을 띄우고 창 뒤를 흐리게 해 달라고 요청한다. 컴포지터가 알린 흐림 기능과
//! 보낸 버퍼의 종류를 표준 출력에 적는다. 이 프로토콜을 쓰는 앱이 아직 드물어 만들었다.
//!
//! 빌드: `cargo build -p bunt-comp --example glass-window` (target/debug/examples/glass-window)
//!
//! 사용법 (세션의 터미널에서, 또는 직렬 콘솔에서 사용자와 WAYLAND_DISPLAY를 주고):
//!   glass-window [--region=all|sidebar|none] [--toggle[=초]] [--dmabuf] [--seconds=초]
//! - --region: 흐릴 곳. all은 창 전체(기본), sidebar는 왼쪽 사이드바, none은 요청하지 않는다
//! - --toggle: 흐림 영역을 지웠다 다시 준다 (영역만 바꾼 커밋). 간격은 기본 1초
//! - --dmabuf: 컴포지터의 기본 피드백이 알린 장치에서 GBM으로 버퍼를 만들어 linux-dmabuf로 보낸다.
//!   컴포지터가 linux-dmabuf를 열지 않았으면(CPU 렌더러) 그렇다고 적고 wl_shm으로 보낸다
//! - --seconds: 이만큼 뒤에 끝난다. 주지 않으면 창을 닫을 때까지 돈다

use std::{
    error::Error,
    fs::File,
    io::{ErrorKind, Write},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    path::PathBuf,
    time::{Duration, Instant},
};

use gbm::{BufferObject, BufferObjectFlags, Device, Format};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    backend::WaylandError,
    delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_buffer::WlBuffer, wl_compositor::WlCompositor, wl_region::WlRegion,
        wl_registry::WlRegistry, wl_shm, wl_shm_pool::WlShmPool, wl_surface::WlSurface,
    },
};
use wayland_protocols::{
    ext::background_effect::v1::client::{
        ext_background_effect_manager_v1::{self, ExtBackgroundEffectManagerV1},
        ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
    },
    wp::linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1::{Flags, ZwpLinuxBufferParamsV1},
        zwp_linux_dmabuf_feedback_v1::{self, ZwpLinuxDmabufFeedbackV1},
        zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    },
    xdg::shell::client::{
        xdg_surface::{self, XdgSurface},
        xdg_toplevel::{self, XdgToplevel},
        xdg_wm_base::{self, XdgWmBase},
    },
};

/// 컴포지터가 크기를 정하지 않을 때의 창 크기 (논리 픽셀)
const DEFAULT_SIZE: (i32, i32) = (480, 320);
const SIDEBAR_WIDTH: i32 = 160;
const TITLE_HEIGHT: i32 = 28;
const TOGGLE: Duration = Duration::from_secs(1);
const USAGE: &str = "usage: glass-window [--region=all|sidebar|none] [--toggle[=SECONDS]] \
                     [--dmabuf] [--seconds=SECONDS]";

#[derive(Clone, Copy, PartialEq)]
enum Region {
    All,
    Sidebar,
    None,
}

struct Options {
    region: Region,
    /// 흐림 영역을 바꾸는 간격
    toggle: Option<Duration>,
    dmabuf: bool,
    seconds: Option<f64>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            region: Region::All,
            toggle: None,
            dmabuf: false,
            seconds: None,
        };
        let seconds = |arg: &str, value: &str| match value.parse::<f64>() {
            Ok(seconds) if seconds.is_finite() && seconds > 0.0 => Ok(seconds),
            _ => Err(format!("bad {arg}\n{USAGE}")),
        };
        for arg in args {
            match arg.as_str() {
                "--region=all" => options.region = Region::All,
                "--region=sidebar" => options.region = Region::Sidebar,
                "--region=none" => options.region = Region::None,
                "--toggle" => options.toggle = Some(TOGGLE),
                "--dmabuf" => options.dmabuf = true,
                _ => {
                    if let Some(value) = arg.strip_prefix("--toggle=") {
                        options.toggle = Some(Duration::from_secs_f64(seconds(&arg, value)?));
                    } else if let Some(value) = arg.strip_prefix("--seconds=") {
                        options.seconds = Some(seconds(&arg, value)?);
                    } else {
                        return Err(format!("unknown argument {arg}\n{USAGE}"));
                    }
                }
            }
        }
        Ok(options)
    }
}

/// 받은 이벤트
#[derive(Default)]
struct App {
    capabilities: Option<u32>,
    main_device: Option<libc::dev_t>,
    feedback_done: bool,
    /// 컴포지터가 정한 크기 (0이면 앱이 정한다)
    pending_size: Option<(i32, i32)>,
    configured: bool,
    closed: bool,
}

/// 붙인 버퍼. dmabuf는 GBM 버퍼가 버퍼를 쓰는 동안 살아 있어야 한다
struct Backing {
    buffer: WlBuffer,
    _bo: Option<BufferObject<()>>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<App>(&conn)?;
    let qh = queue.handle();
    let mut app = App::default();

    let compositor: WlCompositor = globals.bind(&qh, 4..=6, ())?;
    let shm: wl_shm::WlShm = globals.bind(&qh, 1..=1, ())?;
    let wm_base: XdgWmBase = globals.bind(&qh, 1..=6, ())?;
    let effects: Option<ExtBackgroundEffectManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
    let dmabuf: Option<ZwpLinuxDmabufV1> = options
        .dmabuf
        .then(|| globals.bind(&qh, 4..=5, ()).ok())
        .flatten();
    if effects.is_none() {
        println!("ext_background_effect_manager_v1: not offered (blur is off)");
    }
    let mut gbm = None;
    if options.dmabuf {
        match &dmabuf {
            Some(dmabuf) => {
                dmabuf.get_default_feedback(&qh, ());
                while !app.feedback_done {
                    queue.blocking_dispatch(&mut app)?;
                }
                let device = app
                    .main_device
                    .ok_or("the default feedback has no main device")?;
                let path = device_path(device)?;
                println!("linux-dmabuf: main device {}", path.display());
                gbm = Some(Device::new(
                    File::options().read(true).write(true).open(&path)?,
                )?);
            }
            None => println!("linux-dmabuf: not offered (CPU rendering), using wl_shm"),
        }
    }

    let surface = compositor.create_surface(&qh, ());
    let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg_surface.get_toplevel(&qh, ());
    toplevel.set_title("Glass test".into());
    toplevel.set_app_id("org.buntos.GlassTest".into());
    let effect = effects
        .as_ref()
        .filter(|_| options.region != Region::None)
        .map(|effects| effects.get_background_effect(&surface, &qh, ()));
    surface.commit();
    while !app.configured {
        queue.blocking_dispatch(&mut app)?;
    }
    match app.capabilities {
        Some(flags) if flags & ext_background_effect_manager_v1::Capability::Blur.bits() != 0 => {
            println!("capabilities: blur")
        }
        Some(_) => println!("capabilities: none"),
        None => {}
    }

    let mut size = (0, 0);
    let mut backing: Option<Backing> = None;
    let mut blurred = true;
    let started = Instant::now();
    let interval = options.toggle.unwrap_or(TOGGLE);
    let mut next_toggle = started + interval;
    let deadline = options
        .seconds
        .map(|seconds| started + Duration::from_secs_f64(seconds));
    loop {
        // 처음과 크기가 바뀐 configure 뒤에 다시 그린다
        if let Some(pending) = app.pending_size.take() {
            let wanted = match pending {
                (width, height) if width > 0 && height > 0 => (width, height),
                _ => DEFAULT_SIZE,
            };
            if wanted != size || backing.is_none() {
                size = wanted;
                let pixels = paint(size.0, size.1);
                let made = match &gbm {
                    Some(device) => {
                        dmabuf_buffer(device, dmabuf.as_ref().unwrap(), &qh, size, &pixels)?
                    }
                    None => shm_buffer(&shm, &qh, size, &pixels)?,
                };
                surface.attach(Some(&made.buffer), 0, 0);
                surface.damage_buffer(0, 0, size.0, size.1);
                if let Some(old) = backing.replace(made) {
                    old.buffer.destroy();
                }
                if let Some(effect) = &effect {
                    set_region(&compositor, &qh, effect, options.region, size, blurred);
                }
                println!(
                    "drew {}x{} ({})",
                    size.0,
                    size.1,
                    if gbm.is_some() { "dmabuf" } else { "wl_shm" }
                );
            }
            // 받은 configure는 바뀐 것이 없어도 커밋으로 마친다 (xdg_surface.ack_configure 뒤)
            surface.commit();
        }
        let now = Instant::now();
        if options.toggle.is_some()
            && now >= next_toggle
            && let Some(effect) = &effect
        {
            blurred = !blurred;
            set_region(&compositor, &qh, effect, options.region, size, blurred);
            surface.commit();
            println!("blur {}", if blurred { "on" } else { "off" });
            next_toggle = now + interval;
        }
        if app.closed || deadline.is_some_and(|deadline| now >= deadline) {
            break;
        }
        // 다음 할 일까지 기다리며 이벤트를 받는다
        let mut wake = deadline;
        if options.toggle.is_some() {
            wake = Some(wake.map_or(next_toggle, |at| at.min(next_toggle)));
        }
        let timeout = wake.map_or(-1, |at| {
            at.saturating_duration_since(Instant::now()).as_millis() as i32
        });
        conn.flush()?;
        if let Some(guard) = queue.prepare_read() {
            let mut poll = libc::pollfd {
                fd: guard.connection_fd().as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // 안전: pollfd 하나를 가리키고 기다리는 동안 살아 있다
            if unsafe { libc::poll(&mut poll, 1, timeout) } > 0 {
                match guard.read() {
                    Ok(_) => {}
                    // 읽은 것이 연결 안에서 처리하는 메시지(delete_id 등)뿐이면 이렇게 답한다
                    Err(WaylandError::Io(err)) if err.kind() == ErrorKind::WouldBlock => {}
                    Err(err) => return Err(err.into()),
                }
            }
        }
        queue.dispatch_pending(&mut app)?;
    }
    toplevel.destroy();
    xdg_surface.destroy();
    surface.destroy();
    conn.flush()?;
    Ok(())
}

/// 흐림 영역을 준다(`on`이 거짓이면 지운다). 영역은 표면 기준 논리 좌표다
fn set_region(
    compositor: &WlCompositor,
    qh: &QueueHandle<App>,
    effect: &ExtBackgroundEffectSurfaceV1,
    region: Region,
    (width, height): (i32, i32),
    on: bool,
) {
    let rect = match region {
        Region::All => (0, 0, width, height),
        Region::Sidebar => (0, 0, SIDEBAR_WIDTH.min(width), height),
        Region::None => return,
    };
    if !on {
        effect.set_blur_region(None);
        return;
    }
    let wl_region: WlRegion = compositor.create_region(qh, ());
    wl_region.add(rect.0, rect.1, rect.2, rect.3);
    effect.set_blur_region(Some(&wl_region));
    // 영역은 복사되므로 바로 없애도 된다
    wl_region.destroy();
}

/// 반투명한 창 그림 (Argb8888, 미리 곱한 알파, 리틀 엔디언 B, G, R, A): 흰 유리 창, 더 옅은 사이드바,
/// 더 진한 제목 막대, 1픽셀 테두리
fn paint(width: i32, height: i32) -> Vec<u8> {
    let premultiplied = |[r, g, b]: [u8; 3], alpha: f32| {
        let scale = |value: u8| (f32::from(value) * alpha).round() as u8;
        [scale(b), scale(g), scale(r), (alpha * 255.0).round() as u8]
    };
    let window = premultiplied([255, 255, 255], 0.35);
    let sidebar = premultiplied([230, 230, 236], 0.2);
    let title = premultiplied([250, 250, 250], 0.6);
    let border = premultiplied([0, 0, 0], 0.25);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let color = if x == 0 || y == 0 || x == width - 1 || y == height - 1 {
                border
            } else if y < TITLE_HEIGHT {
                title
            } else if x < SIDEBAR_WIDTH {
                sidebar
            } else {
                window
            };
            pixels.extend_from_slice(&color);
        }
    }
    pixels
}

fn shm_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<App>,
    (width, height): (i32, i32),
    pixels: &[u8],
) -> Result<Backing, Box<dyn Error>> {
    // 안전: 이름은 NUL로 끝나고, 돌려받은 fd는 여기서만 갖는다
    let fd = unsafe { libc::memfd_create(c"glass-window".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // 안전: 방금 만든 fd다
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    File::from(fd.try_clone()?).write_all(pixels)?;
    let pool: WlShmPool = shm.create_pool(fd.as_fd(), pixels.len() as i32, qh, ());
    let buffer = pool.create_buffer(
        0,
        width,
        height,
        width * 4,
        wl_shm::Format::Argb8888,
        qh,
        (),
    );
    pool.destroy();
    Ok(Backing { buffer, _bo: None })
}

/// GBM으로 선형 버퍼를 만들어 그림을 쓰고 linux-dmabuf로 보낸다
fn dmabuf_buffer(
    device: &Device<File>,
    dmabuf: &ZwpLinuxDmabufV1,
    qh: &QueueHandle<App>,
    (width, height): (i32, i32),
    pixels: &[u8],
) -> Result<Backing, Box<dyn Error>> {
    let mut bo = device.create_buffer_object::<()>(
        width as u32,
        height as u32,
        Format::Argb8888,
        BufferObjectFlags::LINEAR | BufferObjectFlags::RENDERING,
    )?;
    let row = width as usize * 4;
    bo.map_mut(0, 0, width as u32, height as u32, |mapping| {
        let stride = mapping.stride() as usize;
        let target = mapping.buffer_mut();
        for (y, source) in pixels.chunks_exact(row).enumerate() {
            target[y * stride..y * stride + row].copy_from_slice(source);
        }
    })?;
    let modifier = u64::from(bo.modifier());
    let params: ZwpLinuxBufferParamsV1 = dmabuf.create_params(qh, ());
    for plane in 0..bo.plane_count() as i32 {
        let fd = bo.fd_for_plane(plane)?;
        params.add(
            fd.as_fd(),
            plane as u32,
            bo.offset(plane),
            bo.stride_for_plane(plane),
            (modifier >> 32) as u32,
            modifier as u32,
        );
    }
    let buffer = params.create_immed(
        width,
        height,
        Format::Argb8888 as u32,
        Flags::empty(),
        qh,
        (),
    );
    params.destroy();
    println!(
        "dmabuf: {width}x{height} Argb8888, modifier {modifier:#x}, {} plane(s)",
        bo.plane_count()
    );
    Ok(Backing {
        buffer,
        _bo: Some(bo),
    })
}

/// 장치 번호의 /dev/dri 노드 (기본 피드백의 주 장치는 번호로 온다)
fn device_path(device: libc::dev_t) -> Result<PathBuf, Box<dyn Error>> {
    for entry in std::fs::read_dir("/dev/dri")? {
        let path = entry?.path();
        if std::fs::metadata(&path).is_ok_and(|meta| meta.rdev() == device) {
            return Ok(path);
        }
    }
    Err(format!("no /dev/dri node for device {device:#x}").into())
}

impl Dispatch<WlRegistry, GlobalListContents> for App {
    fn event(
        _app: &mut Self,
        _registry: &WlRegistry,
        _event: <WlRegistry as wayland_client::Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgWmBase, ()> for App {
    fn event(
        _app: &mut Self,
        wm_base: &XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for App {
    fn event(
        app: &mut Self,
        xdg_surface: &XdgSurface,
        event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            xdg_surface.ack_configure(serial);
            app.configured = true;
            app.pending_size.get_or_insert((0, 0));
        }
    }
}

impl Dispatch<XdgToplevel, ()> for App {
    fn event(
        app: &mut Self,
        _toplevel: &XdgToplevel,
        event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure { width, height, .. } => {
                app.pending_size = Some((width, height))
            }
            xdg_toplevel::Event::Close => app.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for App {
    fn event(
        app: &mut Self,
        _manager: &ExtBackgroundEffectManagerV1,
        event: ext_background_effect_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_background_effect_manager_v1::Event::Capabilities { flags } = event {
            let bits = match flags {
                WEnum::Value(flags) => flags.bits(),
                WEnum::Unknown(bits) => bits,
            };
            // 처음 뒤에 바뀌면(흐림을 끔) 알린다
            if app.capabilities.is_some_and(|known| known != bits) {
                println!("capabilities changed: {bits:#x}");
            }
            app.capabilities = Some(bits);
        }
    }
}

impl Dispatch<ZwpLinuxDmabufFeedbackV1, ()> for App {
    fn event(
        app: &mut Self,
        _feedback: &ZwpLinuxDmabufFeedbackV1,
        event: zwp_linux_dmabuf_feedback_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwp_linux_dmabuf_feedback_v1::Event::MainDevice { device } => {
                app.main_device = device
                    .as_slice()
                    .try_into()
                    .ok()
                    .map(libc::dev_t::from_ne_bytes);
            }
            zwp_linux_dmabuf_feedback_v1::Event::Done => app.feedback_done = true,
            _ => {}
        }
    }
}

impl Dispatch<ZwpLinuxBufferParamsV1, ()> for App {
    fn event(
        _app: &mut Self,
        _params: &ZwpLinuxBufferParamsV1,
        _event: <ZwpLinuxBufferParamsV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // create_immed만 쓰므로 created 이벤트는 오지 않는다. 컴포지터가 가져오지 못하면 연결을 끊는다
    }
}

delegate_noop!(App: ignore WlCompositor);
delegate_noop!(App: ignore WlSurface);
delegate_noop!(App: ignore WlRegion);
delegate_noop!(App: ignore wl_shm::WlShm);
delegate_noop!(App: ignore WlShmPool);
delegate_noop!(App: ignore WlBuffer);
delegate_noop!(App: ignore ZwpLinuxDmabufV1);
delegate_noop!(App: ignore ExtBackgroundEffectSurfaceV1);
