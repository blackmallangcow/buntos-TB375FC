// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 화면 기록 (macOS ⇧⌘5의 "전체 화면 기록", "선택 부분 기록"). 컴포지터가 기록하는 화면을 그릴 때마다 같은
//! 장면(커서 포함. macOS의 화면 기록도 포인터를 보인다)을 따로 그려 둔 텍스처에서 기록할 영역을 읽고, 따로
//! 도는 스레드가 그 프레임을 초당 FRAME_RATE장의 고른 간격으로 부호기에 넘긴다.
//! - 부호기는 따로 띄운 GStreamer(gst-launch-1.0) 프로세스다. 원시 RGBA 프레임을 표준 입력으로 받아 VP8로
//!   부호화하고 WebM으로 묶어, 컴포지터가 열어 넘긴 파일(fd 3)에 쓴다. 부호기가 죽어도 컴포지터는 살고,
//!   파일 경로를 파이프라인 글에 넣지 않으므로 경로의 글자가 파이프라인을 바꾸지 못한다
//! - 화면이 바뀌지 않아 새로 그리지 않는 동안에는 마지막 프레임을 되풀이해 넘겨 영상의 시간이 실제 시간과
//!   같다. 부호기가 따라오지 못하면 밀린 프레임을 버린다
//! - 메뉴 막대의 기록 중단(셸)으로 멈추면 표준 입력을 닫고, 부호기가 파일을 마무리하면 셸에 알린다
//!   (RecordingSaved: 파일과 마지막 프레임의 미리 보기 그림). 화면을 잠그면 멈춘다(잠금 화면은 기록하지
//!   않는다). 기록하던 화면이 사라지거나 크기가 바뀌어도 멈춘다
//! - 파일은 스크린샷과 같은 저장 위치에 "화면 기록 2026-10-30 오후 3.12.45.webm"처럼 둔다(macOS).
//!   클립보드를 골랐으면 동영상 폴더에 둔다

use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command as Process, Stdio},
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    time::{Duration, Instant},
};

use bunt_i18n::{FluentArgs, Translations};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, ExportMem, Offscreen,
            damage::OutputDamageTracker,
            gles::{GlesRenderer, GlesTexture},
        },
    },
    output::Output,
    utils::{Logical, Physical, Rectangle, Size, Transform},
};

use crate::{
    dbus::Command,
    render::OutputRenderElements,
    screenshot::{self, Image},
    state::BuntState,
    windows,
};

/// 초당 프레임 수. 소프트웨어 렌더러(VM)에서도 따로 그리기와 부호화가 따라오게 30으로 둔다
const FRAME_RATE: u32 = 30;
/// 부호기 (Debian의 gstreamer1.0-tools). PATH에 놓인 다른 프로그램을 부르지 않게 절대 경로로 둔다
const GST_LAUNCH: &str = "/usr/bin/gst-launch-1.0";
/// 부호기가 기록 파일로 쓰는 파일 서술자
const OUTPUT_FD: i32 = 3;
/// 부호기가 늦을 때 한 번에 몰아 넘기는 프레임 수. 그보다 밀리면 밀린 만큼 버린다(영상이 그만큼 짧아진다)
const MAX_CATCH_UP: u64 = 5;
/// 컴포지터가 기록 스레드에 쌓아 두는 프레임 수. 넘으면 새 프레임을 버리고 스레드가 앞 프레임을 되풀이한다
const QUEUE: usize = 2;
/// 첫 프레임을 기다리는 동안 깨지 않는 시간. 기록을 멈추면(보내는 쪽을 버리면) 바로 깬다
const IDLE_WAIT: Duration = Duration::from_secs(3600);
/// 미리 보기 그림의 긴 변 (물리 픽셀). 셸의 미리 보기(긴 변 200)를 2배 배율로 그려도 흐리지 않다
const PREVIEW_SIZE: u32 = 400;
/// 따로 그릴 때의 바탕 (udev.rs의 CLEAR_COLOR). 배경화면이 화면을 덮으므로 보이지 않는다
const CLEAR: [f32; 4] = [0.08, 0.08, 0.1, 1.0];

/// 무엇을 기록하는가 (⇧⌘5 도구 막대의 방식)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 포인터가 있는 화면 전체
    Screen,
    /// ⇧⌘5의 선택 영역
    Area,
}

impl Mode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "screen" => Some(Self::Screen),
            "area" => Some(Self::Area),
            _ => None,
        }
    }
}

/// 기록 스레드에 넘기는 프레임: 기록하는 영역의 RGBA, 위 줄부터
type Frame = Arc<Vec<u8>>;

/// 기록하는 중
pub struct Recording {
    output: Output,
    /// 화면에서 기록하는 영역 (물리 픽셀, 위 줄부터). 부호기의 4:2:0 색이 짝수 크기를 바라므로 짝수다
    region: Rectangle<i32, Physical>,
    /// 기록을 시작할 때의 화면 크기 (물리 픽셀, 보이는 방향). 바뀌면 멈춘다
    screen: Size<i32, Physical>,
    scale: f64,
    /// 장면을 따로 그리는 텍스처와 그 손상 추적. 처음 읽을 때 만든다. 바뀐 곳만 다시 그린다
    target: Option<(GlesTexture, OutputDamageTracker)>,
    frames: SyncSender<Frame>,
    /// 마지막으로 읽은 때. FRAME_RATE보다 자주 읽지 않는다
    read_at: Option<Instant>,
    /// 너무 일러 읽지 않고 건너뛴 프레임이 있다. 그 뒤로 화면이 그대로면 그 프레임을 놓치므로 다시 그리게 한다
    pending: bool,
    /// 읽지 못했거나 기록 스레드가 끝났다. 다음 프레임 전에 멈춘다
    failed: bool,
}

impl Recording {
    pub fn output(&self) -> &Output {
        &self.output
    }

    /// 건너뛴 프레임을 읽으려면 다음 프레임을 그려야 한다 (렌더 루프 계약의 is_animating)
    pub fn wants_frame(&self) -> bool {
        self.pending && !self.failed
    }

    /// 화면 배율이 바뀌었다. 장면의 요소가 새 배율로 만들어지므로 새 배율로 다시 그린다. 화면의 물리 크기는
    /// 그대로라 영상은 이어진다
    fn rescale(&mut self, scale: f64) {
        self.scale = scale;
        self.target = None;
    }

    /// 그리는 장면(앞의 요소부터)에서 기록할 영역을 읽어 기록 스레드에 넘긴다. FRAME_RATE보다 자주 읽지
    /// 않고, 지난번에 읽은 뒤 바뀐 것이 없으면 읽지 않는다(기록 스레드가 마지막 프레임을 되풀이한다)
    pub fn capture(&mut self, renderer: &mut GlesRenderer, elements: &[&OutputRenderElements]) {
        let now = Instant::now();
        let interval = frame_interval();
        if self.failed {
            return;
        }
        if self
            .read_at
            .is_some_and(|at| now.saturating_duration_since(at) < interval)
        {
            self.pending = true;
            return;
        }
        self.pending = false;
        match self.read(renderer, elements) {
            Ok(Some(frame)) => {
                self.read_at = Some(now);
                match self.frames.try_send(Arc::new(frame)) {
                    Ok(()) | Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => self.failed = true,
                }
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!("failed to read a screen recording frame: {err}");
                self.failed = true;
            }
        }
    }

    /// 장면을 텍스처에 그리고(처음 뒤로는 바뀐 곳만) 기록할 영역을 읽는다. 바뀐 것이 없으면 None
    fn read(
        &mut self,
        renderer: &mut GlesRenderer,
        elements: &[&OutputRenderElements],
    ) -> Result<Option<Vec<u8>>, String> {
        let buffer_size = self.screen.to_logical(1).to_buffer(1, Transform::Normal);
        let first = self.target.is_none();
        if first {
            let texture: GlesTexture = renderer
                .create_buffer(Fourcc::Abgr8888, buffer_size)
                .map_err(|err| err.to_string())?;
            let damage = OutputDamageTracker::new(self.screen, self.scale, Transform::Normal);
            self.target = Some((texture, damage));
        }
        let Some((texture, damage)) = self.target.as_mut() else {
            return Ok(None);
        };
        let mapping = {
            let mut framebuffer = renderer.bind(texture).map_err(|err| err.to_string())?;
            let age = if first { 0 } else { 1 };
            let rendered = damage
                .render_output(renderer, &mut framebuffer, age, elements, CLEAR)
                .map_err(|err| format!("{err:?}"))?;
            if !first && rendered.damage.is_none() {
                return Ok(None);
            }
            renderer
                .copy_framebuffer(
                    &framebuffer,
                    Rectangle::from_size(buffer_size),
                    Fourcc::Abgr8888,
                )
                .map_err(|err| err.to_string())?
        };
        // 텍스처에 그린 것은 맨 위 줄부터 읽힌다 (screenshot.rs의 capture_scene과 같다)
        let pixels = renderer
            .map_texture(&mapping)
            .map_err(|err| err.to_string())?;
        crop(pixels, self.screen, self.region).map(Some)
    }
}

/// 프레임 사이 간격
fn frame_interval() -> Duration {
    Duration::from_secs(1) / FRAME_RATE
}

/// 화면에서 기록할 영역 (물리 픽셀). `area`(화면 안 논리 좌표)가 없으면 화면 전체다. 부호기의 4:2:0 색은
/// 짝수 크기를 바라므로 너비와 높이를 짝수로 줄인다. 화면 밖이거나 너무 작으면 None
pub fn region(
    screen: Size<i32, Physical>,
    area: Option<Rectangle<i32, Logical>>,
    scale: f64,
) -> Option<Rectangle<i32, Physical>> {
    let full = Rectangle::from_size(screen);
    let region = match area {
        Some(area) => area.to_physical_precise_round(scale).intersection(full)?,
        None => full,
    };
    let even = Size::from((region.size.w & !1, region.size.h & !1));
    (even.w >= 2 && even.h >= 2).then(|| Rectangle::new(region.loc, even))
}

/// 위 줄부터의 RGBA 화면(`screen`)에서 `region`만 잘라 낸다
fn crop(
    pixels: &[u8],
    screen: Size<i32, Physical>,
    region: Rectangle<i32, Physical>,
) -> Result<Vec<u8>, String> {
    let (width, height) = (screen.w as usize, screen.h as usize);
    let stride = width * 4;
    if pixels.len() < stride * height {
        return Err(format!("read {} bytes for {width}x{height}", pixels.len()));
    }
    if region.loc.x < 0
        || region.loc.y < 0
        || region.loc.x + region.size.w > screen.w
        || region.loc.y + region.size.h > screen.h
    {
        return Err(format!("{region:?} is outside the {width}x{height} screen"));
    }
    let row = region.size.w as usize * 4;
    let mut frame = Vec::with_capacity(row * region.size.h as usize);
    for y in region.loc.y as usize..(region.loc.y + region.size.h) as usize {
        let start = y * stride + region.loc.x as usize * 4;
        frame.extend_from_slice(&pixels[start..start + row]);
    }
    Ok(frame)
}

/// 부호기 파이프라인 (gst-launch-1.0의 인자). 원시 RGBA 프레임(`width`×`height`, 초당 FRAME_RATE장)을 표준
/// 입력으로 받아 VP8(libvpx)로 부호화하고 WebM으로 묶어 OUTPUT_FD에 쓴다. 설정은 GNOME 셸 화면 기록의
/// 소프트웨어 VP8 설정을 따랐다: 가장 빠른 실시간 부호화(cpu-used=16, deadline=1), 글자가 뭉개지지 않는
/// 양자화 한계(max-quantizer=17), 거의 바뀌지 않는 블록 건너뛰기(static-threshold). 열쇠 프레임은 10초마다
/// 두어 재생할 때 옮겨 볼 수 있게 한다. 사용자가 정하는 글(파일 경로)은 넣지 않는다
fn pipeline(width: u32, height: u32, threads: usize) -> Vec<String> {
    let frame = width as usize * height as usize * 4;
    let keyframes = FRAME_RATE * 10;
    let elements = [
        format!("fdsrc fd=0 blocksize={frame}"),
        format!(
            "rawvideoparse use-sink-caps=false width={width} height={height} format=rgba \
             framerate={FRAME_RATE}/1"
        ),
        "queue".to_owned(),
        format!(
            "videoconvert chroma-mode=none dither=none matrix-mode=output-only n-threads={threads}"
        ),
        "queue".to_owned(),
        format!(
            "vp8enc cpu-used=16 max-quantizer=17 deadline=1 keyframe-max-dist={keyframes} \
             static-threshold=1000 buffer-size=20000 threads={threads}"
        ),
        "queue".to_owned(),
        "webmmux".to_owned(),
        format!("fdsink fd={OUTPUT_FD}"),
    ];
    let mut args = vec!["-q".to_owned(), "-e".to_owned()];
    for (index, element) in elements.iter().enumerate() {
        if index > 0 {
            args.push("!".to_owned());
        }
        args.extend(element.split_whitespace().map(str::to_owned));
    }
    args
}

/// 부호기가 쓸 스레드 수: 코어의 반, 1~4. 나머지는 컴포지터(소프트웨어 렌더러)와 앱이 쓴다
fn encoder_threads() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| (cores.get() / 2).clamp(1, 4))
}

/// 부호기를 띄운다. 프레임은 표준 입력으로 받고, 기록 파일(`file`)은 fd OUTPUT_FD로 받는다
fn spawn_encoder(program: &Path, args: &[String], file: &File) -> io::Result<Child> {
    // 표준 입출력(0~2)이나 OUTPUT_FD와 겹치지 않게 그 위로 복제해 둔다. CLOEXEC이므로 부호기에는 아래에서
    // dup2한 것만 넘어간다
    // 안전: 열린 fd를 복제할 뿐이다
    let duplicate = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, OUTPUT_FD + 1) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // 안전: 방금 만든 fd이고 여기서만 갖는다
    let source = unsafe { OwnedFd::from_raw_fd(duplicate) };
    let raw = source.as_raw_fd();
    let mut command = Process::new(program);
    command
        .args(args)
        .env_clear()
        .envs(encoder_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    // 안전: fork 뒤 exec 전에는 비동기 신호에 안전한 시스템 호출(dup2, close_range)만 부른다. dup2로 만든 fd는
    // CLOEXEC가 아니다. 그 위의 fd(컴포지터가 연 장치나 소켓 가운데 CLOEXEC가 아닌 것)는 exec에서 닫히게 해
    // 부호기에 넘기지 않는다. 바로 닫으면 Rust가 exec 실패를 알리는 파이프까지 닫혀 없는 부호기도 띄운 것처럼
    // 보인다. 오래된 커널(5.11 전)이면 실패하고, 그때는 각 fd의 CLOEXEC에 맡긴다
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(raw, OUTPUT_FD) < 0 {
                return Err(io::Error::last_os_error());
            }
            libc::close_range(
                OUTPUT_FD as libc::c_uint + 1,
                libc::c_uint::MAX,
                libc::CLOSE_RANGE_CLOEXEC as libc::c_int,
            );
            Ok(())
        });
    }
    command.spawn()
}

/// 부호기의 환경. 컴포지터의 환경은 넘기지 않는다. 플러그인은 시스템 폴더에서만 읽어(사용자 폴더의 플러그인이
/// 화면 프레임을 보지 못하게), 플러그인 목록 캐시는 세션의 런타임 폴더에 둔다
fn encoder_env() -> Vec<(&'static str, OsString)> {
    let mut env = Vec::new();
    if let Some(dir) = system_plugin_dir() {
        env.push(("GST_PLUGIN_SYSTEM_PATH_1_0", dir.into_os_string()));
    }
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let registry = Path::new(&runtime).join("bunt-comp-gstreamer-registry.bin");
        env.push(("GST_REGISTRY_1_0", registry.into_os_string()));
        env.push(("XDG_RUNTIME_DIR", runtime));
    }
    if let Some(home) = std::env::var_os("HOME") {
        env.push(("HOME", home));
    }
    env
}

/// Debian의 GStreamer 플러그인 폴더 (다중 아키텍처 경로). 모르는 아키텍처이거나 없으면 None이고, 그때는
/// GStreamer가 스스로 찾는다
fn system_plugin_dir() -> Option<PathBuf> {
    let triplet = match std::env::consts::ARCH {
        "x86_64" => "x86_64-linux-gnu",
        "aarch64" => "aarch64-linux-gnu",
        _ => return None,
    };
    let dir = PathBuf::from(format!("/usr/lib/{triplet}/gstreamer-1.0"));
    dir.is_dir().then_some(dir)
}

/// 고른 간격으로 넘길 프레임 수를 센다. 첫 프레임을 받은 때부터 잰다
#[derive(Debug, Default)]
struct Pacer {
    started: Option<Instant>,
    written: u64,
}

impl Pacer {
    fn start(&mut self, now: Instant) {
        self.started.get_or_insert(now);
    }

    /// 다음 프레임을 넘길 때까지 기다릴 시간
    fn wait(&self, now: Instant, interval: Duration) -> Duration {
        match self.started {
            Some(start) => {
                let next = u32::try_from(self.written).unwrap_or(u32::MAX);
                (start + interval.saturating_mul(next)).saturating_duration_since(now)
            }
            None => IDLE_WAIT,
        }
    }

    /// 지금(`now`)까지 넘겼어야 하는데 아직 넘기지 않은 프레임 수 (MAX_CATCH_UP까지). 그보다 밀렸으면 밀린
    /// 만큼 넘긴 셈 치고 버린다
    fn due(&mut self, now: Instant, interval: Duration) -> u64 {
        let Some(start) = self.started else {
            return 0;
        };
        let elapsed = now.saturating_duration_since(start).as_nanos();
        let should = u64::try_from(elapsed / interval.as_nanos().max(1)).unwrap_or(u64::MAX) + 1;
        let behind = should.saturating_sub(self.written);
        if behind > MAX_CATCH_UP {
            self.written = should - MAX_CATCH_UP;
            return MAX_CATCH_UP;
        }
        behind
    }

    fn wrote(&mut self) {
        self.written += 1;
    }
}

/// 기록 스레드: 받은 프레임을 고른 간격으로 부호기에 넘기고, 보내는 쪽이 끝나면(기록을 멈추면) 부호기를
/// 마무리해 결과를 알린다
fn record(mut encoder: Child, frames: Receiver<Frame>, finish: Finish) {
    let interval = frame_interval();
    let mut input = encoder.stdin.take();
    let mut pacer = Pacer::default();
    let mut latest: Option<Frame> = None;
    loop {
        match frames.recv_timeout(pacer.wait(Instant::now(), interval)) {
            Ok(frame) => {
                pacer.start(Instant::now());
                latest = Some(frame);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if !write_due(&mut input, &mut pacer, latest.as_ref(), interval) {
            break;
        }
    }
    // 멈춘 때까지의 프레임을 마저 넘기고 표준 입력을 닫으면 부호기가 파일을 마무리하고 끝난다
    write_due(&mut input, &mut pacer, latest.as_ref(), interval);
    drop(input);
    let encoded = match encoder.wait() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            tracing::warn!("the screen recording encoder failed: {status}");
            false
        }
        Err(err) => {
            tracing::warn!("cannot wait for the screen recording encoder: {err}");
            false
        }
    };
    finish.report(encoded, pacer.written > 0, latest);
}

/// 지금까지 넘겼어야 할 프레임을 넘긴다. 부호기가 끝나 넘기지 못하면 false
fn write_due(
    input: &mut Option<ChildStdin>,
    pacer: &mut Pacer,
    frame: Option<&Frame>,
    interval: Duration,
) -> bool {
    let (Some(stdin), Some(frame)) = (input.as_mut(), frame) else {
        return input.is_some();
    };
    for _ in 0..pacer.due(Instant::now(), interval) {
        if let Err(err) = stdin.write_all(frame) {
            tracing::warn!("the screen recording encoder stopped taking frames: {err}");
            *input = None;
            return false;
        }
        pacer.wrote();
    }
    true
}

/// 기록을 마친 뒤 알릴 것
struct Finish {
    path: PathBuf,
    /// 프레임 크기 (미리 보기)
    size: (u32, u32),
    /// 플로팅 썸네일 보기 (스크린샷 옵션)
    thumbnail: bool,
    title: String,
    saved: String,
    failed: String,
    commands: mpsc::Sender<Command>,
}

impl Finish {
    /// 저장했으면 셸에 미리 보기를 띄우게 하거나(플로팅 썸네일) 알림을 띄우고, 아니면 실패를 알린다.
    /// `framed`는 부호기에 프레임을 하나라도 넘겼는지다
    fn report(self, encoded: bool, framed: bool, latest: Option<Frame>) {
        let command = if !encoded || !framed {
            // 프레임을 하나도 넘기지 못한 파일(머리만 있거나 빈 파일)은 남기지 않는다. 도중에 부호기가 죽은
            // 파일은 앞부분이라도 볼 수 있게 남긴다
            if !framed {
                let _ = std::fs::remove_file(&self.path);
            }
            Command::Notify {
                title: self.title,
                body: self.failed,
            }
        } else {
            // 미리 보기는 세션의 런타임 폴더에 둔다 (다음 기록이 덮어쓴다)
            let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
            let preview = latest
                .filter(|_| self.thumbnail)
                .zip(runtime)
                .and_then(|(frame, dir)| write_preview(&frame, self.size, &dir));
            match preview {
                Some(preview) => Command::RecordingSaved {
                    path: self.path.to_string_lossy().into_owned(),
                    preview: preview.to_string_lossy().into_owned(),
                },
                None => Command::Notify {
                    title: self.title,
                    body: self.saved,
                },
            }
        };
        let _ = self.commands.send(command);
    }
}

/// 마지막 프레임을 줄여 `dir`에 미리 보기 그림(PNG)으로 쓴다. 셸이 미리 보기를 띄울 때 읽는다
fn write_preview(frame: &[u8], (width, height): (u32, u32), dir: &Path) -> Option<PathBuf> {
    let path = dir.join("bunt-comp-recording-preview.png");
    let image = windows::shrink(
        Image {
            width,
            height,
            rgba: frame.to_vec(),
        },
        PREVIEW_SIZE,
    );
    let written =
        File::create(&path).and_then(|file| screenshot::write_png(&image, BufWriter::new(file)));
    match written {
        Ok(()) => Some(path),
        Err(err) => {
            tracing::warn!("failed to write the recording preview: {err}");
            None
        }
    }
}

impl BuntState {
    /// ⇧⌘5 도구 막대의 기록 (셸만). 전체 화면은 포인터가 있는 화면을, 선택 부분은 ⇧⌘5의 선택 영역을
    /// `delay`초 뒤부터 기록한다. 이미 기록 중이거나 잠긴 동안에는 하지 않는다
    pub fn start_recording(&mut self, mode: Mode, delay: u32) {
        if self.recording.is_some() || self.session_lock.is_locked() {
            return;
        }
        let area = match mode {
            Mode::Screen => None,
            Mode::Area => match self.take_selected_area() {
                Some(area) => Some(area),
                None => return,
            },
        };
        self.after(delay, move |state| state.begin_recording(area));
    }

    /// 기록을 시작한다: 파일을 만들고 부호기와 기록 스레드를 띄운다. `area`(화면 좌표)가 없으면 포인터가 있는
    /// 화면 전체다
    fn begin_recording(&mut self, area: Option<Rectangle<i32, Logical>>) {
        if self.recording.is_some() {
            return;
        }
        let tr = Translations::load(screenshot::TRANSLATIONS);
        let title = tr.get("recording-title");
        let pointer = self
            .seat
            .get_pointer()
            .map(|pointer| pointer.current_location())
            .unwrap_or_default();
        let output = match area {
            Some(area) => self.space.outputs().find(|output| {
                self.space
                    .output_geometry(output)
                    .is_some_and(|geo| geo.overlaps(area))
            }),
            None => self
                .space
                .output_under(pointer)
                .next()
                .or_else(|| self.space.outputs().next()),
        }
        .cloned();
        let Some((output, output_geo)) = output.and_then(|output| {
            let geo = self.space.output_geometry(&output)?;
            Some((output, geo))
        }) else {
            return;
        };
        let Some(mode) = output.current_mode() else {
            return;
        };
        let screen = output.current_transform().transform_size(mode.size);
        let scale = output.current_scale().fractional_scale();
        let local = area.map(|area| Rectangle::new(area.loc - output_geo.loc, area.size));
        let Some(region) = region(screen, local, scale) else {
            return;
        };
        let dir = screenshot::save_dir(
            self.screenshot_options.location,
            ("XDG_VIDEOS_DIR", "Videos"),
        );
        let name = screenshot::file_name(&tr, "recording-file");
        let path = screenshot::unique_path(&dir, &name, "webm");
        let file = std::fs::create_dir_all(&dir)
            .and_then(|()| OpenOptions::new().write(true).create_new(true).open(&path));
        let file = match file {
            Ok(file) => file,
            Err(err) => {
                tracing::warn!("failed to create {}: {err}", path.display());
                self.dbus.send(Command::Notify {
                    title,
                    body: tr.get("recording-failed"),
                });
                return;
            }
        };
        let (width, height) = (region.size.w as u32, region.size.h as u32);
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut args = FluentArgs::new();
        args.set("name", file_name);
        let finish = Finish {
            path: path.clone(),
            size: (width, height),
            thumbnail: self.screenshot_options.thumbnail,
            title: title.clone(),
            saved: tr.get_with("recording-saved", &args),
            failed: tr.get("recording-failed"),
            commands: self.dbus.sender(),
        };
        let (frames, receiver) = mpsc::sync_channel(QUEUE);
        // 기록 스레드를 먼저 띄우고 부호기는 그다음에 띄워 넘긴다. 전에는 부호기를 먼저 띄워, 스레드를 띄우지
        // 못하면 끝을 기다리는 쪽 없이 부호기 프로세스가 남았다. 부호기를 띄우지 못하면 넘길 것이 없어 스레드가 끝난다
        let (hand_over, encoder) = mpsc::channel::<Child>();
        if let Err(err) = std::thread::Builder::new()
            .name("bunt-comp-recording".into())
            .spawn(move || {
                if let Ok(encoder) = encoder.recv() {
                    record(encoder, receiver, finish);
                }
            })
        {
            tracing::warn!("failed to start the screen recording thread: {err}");
            drop(file);
            let _ = std::fs::remove_file(&path);
            self.dbus.send(Command::Notify {
                title,
                body: tr.get("recording-failed"),
            });
            return;
        }
        let args = pipeline(width, height, encoder_threads());
        let encoder = spawn_encoder(Path::new(GST_LAUNCH), &args, &file);
        drop(file);
        let encoder = match encoder {
            Ok(encoder) => encoder,
            Err(err) => {
                tracing::warn!("cannot start the screen recording encoder {GST_LAUNCH}: {err}");
                let _ = std::fs::remove_file(&path);
                // 부호기가 깔리지 않았으면 그렇다고 알린다
                let body = match err.kind() {
                    io::ErrorKind::NotFound => tr.get("recording-unavailable"),
                    _ => tr.get("recording-failed"),
                };
                self.dbus.send(Command::Notify { title, body });
                return;
            }
        };
        if let Err(mpsc::SendError(mut encoder)) = hand_over.send(encoder) {
            // 기록 스레드가 벌써 끝났다. 부호기를 끝내고 기다려 남기지 않는다
            let _ = encoder.kill();
            let _ = encoder.wait();
            let _ = std::fs::remove_file(&path);
            self.dbus.send(Command::Notify {
                title,
                body: tr.get("recording-failed"),
            });
            return;
        }
        tracing::info!(
            "recording {width}x{height} at {:?} of {} to {}",
            region.loc,
            output.name(),
            path.display()
        );
        self.recording = Some(Recording {
            output,
            region,
            screen,
            scale,
            target: None,
            frames,
            read_at: None,
            pending: false,
            failed: false,
        });
        self.dbus.update(|snapshot| snapshot.recording = true);
        self.dbus.send(Command::RecordingChanged(true));
        self.queue_redraw_all();
    }

    /// 기록을 멈춘다 (메뉴 막대의 기록 중단, 화면 잠금, 기록하던 화면이 바뀌거나 사라졌을 때). 기록 스레드가
    /// 부호기를 마무리하고 결과를 셸에 알린다
    pub fn stop_recording(&mut self) {
        if self.recording.take().is_none() {
            return;
        }
        tracing::info!("screen recording stopped");
        self.dbus.update(|snapshot| snapshot.recording = false);
        self.dbus.send(Command::RecordingChanged(false));
    }

    /// 그리기 전에 부른다 (udev.rs). 기록하던 화면이 사라졌거나 크기가 바뀌었거나 프레임을 넘기지 못했으면
    /// 멈춘다 (부호기가 받는 프레임 크기는 바꿀 수 없다)
    pub fn check_recording(&mut self) {
        let Some(recording) = &self.recording else {
            return;
        };
        let gone = !self
            .space
            .outputs()
            .any(|output| *output == recording.output);
        let screen = recording.output.current_mode().map(|mode| {
            recording
                .output
                .current_transform()
                .transform_size(mode.size)
        });
        let scale = recording.output.current_scale().fractional_scale();
        if gone || screen != Some(recording.screen) || recording.failed {
            self.stop_recording();
        } else if scale != recording.scale
            && let Some(recording) = &mut self.recording
        {
            // 전에는 배율이 바뀌어도 처음 배율로 그려, 그 뒤의 프레임이 어긋나게 그려졌다
            recording.rescale(scale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smithay::utils::Point;

    fn point(x: i32, y: i32) -> Point<i32, Logical> {
        (x, y).into()
    }

    fn screen(w: i32, h: i32) -> Size<i32, Physical> {
        (w, h).into()
    }

    #[test]
    fn modes_have_names() {
        assert_eq!(Mode::parse("screen"), Some(Mode::Screen));
        assert_eq!(Mode::parse("area"), Some(Mode::Area));
        assert_eq!(Mode::parse("window"), None);
    }

    #[test]
    fn regions_are_even_and_inside_the_screen() {
        // 화면 전체
        assert_eq!(
            region(screen(1280, 800), None, 1.0),
            Some(Rectangle::from_size((1280, 800).into()))
        );
        // 소수 배율의 선택 영역은 물리 픽셀로 옮기고 짝수로 줄인다
        let area = Rectangle::new(point(101, 51), (333, 201).into());
        assert_eq!(
            region(screen(1600, 1000), Some(area), 1.25),
            Some(Rectangle::new((126, 64).into(), (416, 250).into()))
        );
        // 화면 밖으로 나간 부분은 자른다
        let wide = Rectangle::new(point(1200, 700), (300, 300).into());
        assert_eq!(
            region(screen(1280, 800), Some(wide), 1.0),
            Some(Rectangle::new((1200, 700).into(), (80, 100).into()))
        );
        // 너무 작거나 화면 밖이면 기록하지 않는다
        let tiny = Rectangle::new(point(10, 10), (1, 40).into());
        assert_eq!(region(screen(1280, 800), Some(tiny), 1.0), None);
        let outside = Rectangle::new(point(2000, 10), (100, 100).into());
        assert_eq!(region(screen(1280, 800), Some(outside), 1.0), None);
    }

    #[test]
    fn crops_take_the_region_rows_from_the_top() {
        // 4×3 화면: 픽셀 값은 (y * 4 + x)
        let mut pixels = Vec::new();
        for y in 0..3u8 {
            for x in 0..4u8 {
                pixels.extend([y * 4 + x; 4]);
            }
        }
        let region = Rectangle::new((1, 1).into(), (2, 2).into());
        let frame = crop(&pixels, screen(4, 3), region).unwrap();
        let firsts: Vec<u8> = frame.chunks(4).map(|pixel| pixel[0]).collect();
        assert_eq!(firsts, [5, 6, 9, 10]);
        // 읽은 것이 모자라거나 영역이 화면 밖이면 실패다
        assert!(crop(&pixels[..20], screen(4, 3), region).is_err());
        let outside = Rectangle::new((3, 0).into(), (2, 2).into());
        assert!(crop(&pixels, screen(4, 3), outside).is_err());
    }

    #[test]
    fn the_pipeline_has_no_user_text_and_writes_to_the_passed_file() {
        let args = pipeline(1280, 800, 2);
        assert_eq!(&args[..2], ["-q", "-e"]);
        assert!(args.contains(&"width=1280".to_owned()) && args.contains(&"height=800".to_owned()));
        assert!(args.contains(&format!("framerate={FRAME_RATE}/1")));
        assert!(args.contains(&format!("blocksize={}", 1280 * 800 * 4)));
        assert_eq!(args.last().map(String::as_str), Some("fd=3"));
        // 요소 사이를 잇는 !의 수: 요소 아홉
        assert_eq!(args.iter().filter(|arg| *arg == "!").count(), 8);
        assert!(
            args.iter()
                .all(|arg| !arg.contains('/') || arg.ends_with("/1"))
        );
    }

    #[test]
    fn the_pacer_repeats_frames_at_an_even_rate_and_drops_what_it_cannot_catch_up() {
        let interval = Duration::from_millis(100);
        let start = Instant::now();
        let mut pacer = Pacer::default();
        // 첫 프레임 전에는 넘길 것이 없고 오래 기다린다
        assert_eq!(pacer.due(start, interval), 0);
        assert_eq!(pacer.wait(start, interval), IDLE_WAIT);
        pacer.start(start);
        // 첫 프레임은 바로, 그 뒤로는 간격마다 하나
        assert_eq!(pacer.due(start, interval), 1);
        pacer.wrote();
        assert_eq!(pacer.due(start + interval / 2, interval), 0);
        assert_eq!(pacer.wait(start + interval / 2, interval), interval / 2);
        assert_eq!(pacer.due(start + interval, interval), 1);
        pacer.wrote();
        // 화면이 오래 그대로여도 같은 프레임을 간격마다 넘겨야 하므로 밀린 만큼 넘긴다 (MAX_CATCH_UP까지)
        assert_eq!(pacer.due(start + interval * 4, interval), 3);
        for _ in 0..3 {
            pacer.wrote();
        }
        // 부호기가 오래 막혀 많이 밀렸으면 MAX_CATCH_UP만 넘기고 나머지는 버린다
        assert_eq!(pacer.due(start + interval * 100, interval), MAX_CATCH_UP);
        assert_eq!(pacer.written, 101 - MAX_CATCH_UP);
        // 다시 시작해도 처음 잰 때는 그대로다
        pacer.start(start + interval * 200);
        assert_eq!(pacer.started, Some(start));
    }

    #[test]
    fn the_encoder_gets_only_system_plugins_and_no_session_secrets() {
        let env = encoder_env();
        let names: Vec<&str> = env.iter().map(|(name, _)| *name).collect();
        assert!(names.iter().all(|name| {
            [
                "GST_PLUGIN_SYSTEM_PATH_1_0",
                "GST_REGISTRY_1_0",
                "XDG_RUNTIME_DIR",
                "HOME",
            ]
            .contains(name)
        }));
        // 이 기계의 아키텍처면 시스템 플러그인 폴더를 고른다 (Debian·Ubuntu의 다중 아키텍처 경로)
        if let Some(dir) = system_plugin_dir() {
            assert!(dir.ends_with("gstreamer-1.0") && dir.starts_with("/usr/lib"));
            assert!(names.contains(&"GST_PLUGIN_SYSTEM_PATH_1_0"));
        }
    }

    #[test]
    fn a_missing_encoder_fails_without_leaving_a_process() {
        let path =
            std::env::temp_dir().join(format!("bunt-recording-missing-{}", std::process::id()));
        let file = File::create(&path).unwrap();
        let error =
            spawn_encoder(Path::new("/nonexistent/gst-launch-1.0"), &[], &file).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        drop(file);
        std::fs::remove_file(&path).unwrap();
    }

    /// 부호기(gst-launch-1.0과 VP8, WebM 플러그인)가 있으면 프레임 서른 장으로 1초짜리 WebM을 쓰는지 본다
    #[test]
    fn the_encoder_writes_a_webm_as_long_as_its_frames() {
        if !Path::new(GST_LAUNCH).exists() {
            eprintln!("no {GST_LAUNCH}; skipping");
            return;
        }
        let path = std::env::temp_dir().join(format!("bunt-recording-{}.webm", std::process::id()));
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        let mut encoder =
            spawn_encoder(Path::new(GST_LAUNCH), &pipeline(64, 48, 1), &file).unwrap();
        drop(file);
        let mut input = encoder.stdin.take().unwrap();
        for index in 0..FRAME_RATE {
            input
                .write_all(&vec![(index * 8) as u8; 64 * 48 * 4])
                .unwrap();
        }
        drop(input);
        let status = encoder.wait().unwrap();
        let data = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(status.success(), "{status}");
        // EBML 머리와 webm 문서 형식
        assert_eq!(&data[..4], [0x1a, 0x45, 0xdf, 0xa3]);
        assert!(data.windows(4).any(|window| window == b"webm"));
        // 길이(EBML ID 0x4489, 8바이트 실수, 밀리초): 30장 / 초당 30장 = 1초. 파일이 옮겨 볼 수 있게(seekable)
        // 마무리되어야 길이가 적힌다
        let at = data
            .windows(3)
            .position(|window| window == [0x44, 0x89, 0x88])
            .expect("a duration");
        let duration = f64::from_be_bytes(data[at + 3..at + 11].try_into().unwrap());
        assert!((duration - 1000.0).abs() < 50.0, "{duration}");
    }

    /// 그린 장면에서 프레임을 읽어(llvmpipe) 기록 스레드와 부호기를 거쳐 파일이 되고 저장을 알리는지 본다
    #[test]
    fn frames_read_from_the_scene_become_a_saved_recording() {
        use smithay::backend::renderer::element::memory::MemoryRenderBuffer;

        use crate::testing;

        let _gl = testing::gl_lock();
        let Some(mut renderer) = testing::software_renderer() else {
            eprintln!("no software EGL device; skipping");
            return;
        };
        let (width, height) = (64, 48);
        let (frames, receiver) = mpsc::sync_channel(QUEUE);
        let mut recording = Recording {
            output: testing::output("test"),
            region: Rectangle::new((16, 8).into(), (32, 24).into()),
            screen: screen(width, height),
            scale: 1.0,
            target: None,
            frames,
            read_at: None,
            pending: false,
            failed: false,
        };
        // 위 절반은 초록, 아래 절반은 파랑인 화면
        let half = (width * height / 2) as usize;
        let mut pixels = testing::argb(half, 1, [10, 200, 30, 255]);
        pixels.extend(testing::argb(half, 1, [0, 0, 255, 255]));
        let picture = MemoryRenderBuffer::from_slice(
            &pixels,
            Fourcc::Argb8888,
            (width, height),
            1,
            Transform::Normal,
            None,
        );
        let background = testing::background(&mut renderer, &picture);
        // 첫 프레임은 영역(y 8~32)만 위 줄부터 RGBA로 읽는다: 처음 16줄은 초록, 나머지는 파랑
        recording.capture(&mut renderer, &[&background]);
        let frame = receiver.try_recv().expect("the first frame");
        assert_eq!(frame.len(), 32 * 24 * 4);
        let row = 32 * 4;
        assert_eq!(&frame[..4], [10, 200, 30, 255]);
        assert_eq!(&frame[15 * row..15 * row + 4], [10, 200, 30, 255]);
        assert_eq!(&frame[16 * row..16 * row + 4], [0, 0, 255, 255]);
        assert_eq!(&frame[frame.len() - 4..], [0, 0, 255, 255]);
        // 간격 안에는 다시 읽지 않고, 그 프레임을 읽으려고 다음 프레임을 그리게 한다(is_animating)
        assert!(!recording.wants_frame());
        recording.capture(&mut renderer, &[&background]);
        assert!(receiver.try_recv().is_err());
        assert!(recording.wants_frame());
        // 간격이 지나면 읽고, 바뀐 것이 없으면 넘기지 않는다. 다시 그릴 까닭도 없어진다
        recording.read_at = Some(Instant::now() - frame_interval());
        recording.capture(&mut renderer, &[&background]);
        assert!(receiver.try_recv().is_err());
        assert!(!recording.wants_frame());
        assert!(!recording.failed);
        // 배율이 바뀌면 새 배율로 처음부터 다시 그려 읽는다
        recording.rescale(2.0);
        recording.read_at = None;
        recording.capture(&mut renderer, &[&background]);
        assert!(receiver.try_recv().is_ok());

        if !Path::new(GST_LAUNCH).exists() {
            eprintln!("no {GST_LAUNCH}; skipping the encoder");
            return;
        }
        let dir = std::env::temp_dir().join(format!("bunt-recording-flow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("flow.webm");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let encoder = spawn_encoder(Path::new(GST_LAUNCH), &pipeline(32, 24, 1), &file).unwrap();
        drop(file);
        let (commands, reports) = mpsc::channel();
        let finish = Finish {
            path: path.clone(),
            size: (32, 24),
            thumbnail: false,
            title: "title".into(),
            saved: "saved".into(),
            failed: "failed".into(),
            commands,
        };
        let (frames, receiver) = mpsc::sync_channel(QUEUE);
        let writer = std::thread::spawn(move || record(encoder, receiver, finish));
        frames.send(frame).unwrap();
        // 화면이 그대로인 0.3초 동안에도 같은 프레임을 고른 간격으로 넘긴다
        std::thread::sleep(Duration::from_millis(300));
        drop(frames);
        writer.join().unwrap();
        let report = reports.recv_timeout(Duration::from_secs(10)).unwrap();
        let data = std::fs::read(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        match report {
            Command::Notify { title, body } => {
                assert_eq!((title.as_str(), body.as_str()), ("title", "saved"))
            }
            other => panic!("{other:?}"),
        }
        // 0.3초 동안 아홉 장 안팎(첫 장과 간격마다 하나): 길이 0.3초쯤
        let at = data
            .windows(3)
            .position(|window| window == [0x44, 0x89, 0x88])
            .expect("a duration");
        let duration = f64::from_be_bytes(data[at + 3..at + 11].try_into().unwrap());
        assert!((200.0..=500.0).contains(&duration), "{duration}");
    }

    #[test]
    fn previews_are_small_pngs() {
        let dir = std::env::temp_dir().join(format!("bunt-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let frame = vec![200u8; 800 * 600 * 4];
        let path = write_preview(&frame, (800, 600), &dir).unwrap();
        let data = std::fs::read(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // IHDR의 너비와 높이: 긴 변 400
        assert_eq!(&data[16..24], &[0, 0, 1, 144, 0, 0, 1, 44]);
    }
}
