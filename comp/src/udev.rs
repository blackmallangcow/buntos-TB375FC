// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRM/KMS + libinput 백엔드 (GPU 하나). Smithay anvil의 udev 백엔드 구조를 단순화했다.
//! 하드웨어 GPU가 없는 VM에서는 Mesa의 소프트웨어 렌더러(llvmpipe)로 그린다.

use std::{cmp::Reverse, collections::HashMap, error::Error, path::PathBuf, time::Duration};

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
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::{
            ImportMemWl,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                surface::render_elements_from_surface_tree,
            },
            gles::GlesRenderer,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, UdevEvent, all_gpus, primary_gpu},
    },
    desktop::{
        layer_map_for_output, space::space_render_elements, utils::send_frames_surface_tree,
    },
    input::pointer::{CursorImageStatus, CursorImageSurfaceData},
    output::{Mode as WlMode, Output, PhysicalProperties, Scale as OutputScale},
    reexports::{
        calloop::{
            EventLoop, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{Device as ControlDevice, Mode as DrmMode, ModeTypeFlags, connector, crtc},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::{Display, backend::GlobalId},
    },
    utils::{DeviceFd, IsAlive, Point, Scale, Transform},
    wayland::compositor::with_states,
};

use crate::{
    blur::Blur,
    config::{Config, parse_hex_color},
    cursor::Cursor,
    dbus::{Command, OutputInfo},
    prefs::ModePref,
    render::{self, OutputRenderElements},
    state::BuntState,
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Elements = OutputRenderElements;

/// 화면 갱신이 없을 때 다시 확인하는 간격
const IDLE_REDRAW: Duration = Duration::from_millis(16);
/// 이보다 오래 걸린 프레임은 디버그 기록에 남긴다 (RUST_LOG=bunt_comp=debug)
const SLOW_FRAME: Duration = Duration::from_millis(12);
const CLEAR_COLOR: [f32; 4] = [0.08, 0.08, 0.1, 1.0];
const FALLBACK_TOP: [u8; 3] = [0x5b, 0x7c, 0xfa];
const FALLBACK_BOTTOM: [u8; 3] = [0xf2, 0xa7, 0xc3];
/// 배율을 고를 때 남겨야 하는 가장 작은 논리 화면 크기
const MIN_LOGICAL_WIDTH: i32 = 640;
const MIN_LOGICAL_HEIGHT: i32 = 400;
const MAX_SCALE: i32 = 3;

pub struct UdevData {
    pub session: LibSeatSession,
    renderer: GlesRenderer,
    output_manager: DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>,
    outputs: HashMap<crtc::Handle, OutputSurface>,
    cursor: Cursor,
    cursor_buffer: MemoryRenderBuffer,
    /// 셸 유리 뒤 흐림 (blur.rs)
    blur: Blur,
}

impl UdevData {
    pub fn seat_name(&self) -> String {
        self.session.seat()
    }
}

struct OutputSurface {
    output: Output,
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
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd)?;

    // 안전: GBM 장치는 EGL 디스플레이보다 오래 산다 (둘 다 UdevData가 끝까지 가진다)
    let egl_display = unsafe { EGLDisplay::new(gbm.clone())? };
    let egl_context = EGLContext::new(&egl_display)?;
    let renderer = unsafe { GlesRenderer::new(egl_context)? };
    let render_formats: FormatSet = renderer.egl_context().dmabuf_render_formats().clone();
    let shm_formats: Vec<_> = renderer.shm_formats().collect();

    let output_manager = DrmOutputManager::new(
        drm,
        GbmAllocator::new(
            gbm.clone(),
            GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
        ),
        GbmFramebufferExporter::new(gbm.clone(), Some(primary)),
        Some(gbm),
        [Fourcc::Abgr8888, Fourcc::Argb8888],
        render_formats,
    );

    let cursor = Cursor::load(&config.cursor.theme, config.cursor.size);
    let image = cursor.image();
    let cursor_buffer = MemoryRenderBuffer::from_slice(
        &image.pixels_rgba,
        Fourcc::Abgr8888,
        (image.width as i32, image.height as i32),
        1,
        Transform::Normal,
        None,
    );

    let backend = UdevData {
        session,
        renderer,
        output_manager,
        outputs: HashMap::new(),
        cursor,
        cursor_buffer,
        blur: Blur::default(),
    };
    let mut state = BuntState::new(
        display,
        event_loop.handle(),
        event_loop.get_signal(),
        config,
        backend,
    )?;
    state.shm_state.update_formats(shm_formats);

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
            |event, _, state| state.process_input_event(event),
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
        let scale = saved
            .and_then(|prefs| prefs.scale)
            .filter(|scale| scales_for(wl_mode.size.w, wl_mode.size.h).contains(scale))
            .unwrap_or(1.0);
        let (width_mm, height_mm) = info.size().unwrap_or((0, 0));
        let (make, model) = edid_blob(self.backend.output_manager.device(), info.handle())
            .map(|edid| parse_edid(&edid))
            .unwrap_or_default();
        let description = if model.is_empty() {
            make.clone()
        } else {
            model.clone()
        };
        let builtin = matches!(
            info.interface(),
            connector::Interface::EmbeddedDisplayPort
                | connector::Interface::LVDS
                | connector::Interface::DSI
        );

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
                drm_output,
                connector: info.handle(),
                modes,
                mode: drm_mode,
                builtin,
                description,
                wallpaper,
                frame_pending: false,
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
        // 지금 배율로는 화면이 너무 작아지면 1배로 되돌린다
        let scale = surface.output.current_scale().fractional_scale();
        if !scales_for(wl_mode.size.w, wl_mode.size.h).contains(&scale) {
            surface
                .output
                .change_current_state(None, None, Some(OutputScale::Integer(1)), None);
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
                OutputInfo {
                    name: surface.output.name(),
                    description: surface.description.clone(),
                    builtin: surface.builtin,
                    width: mode.size.w,
                    height: mode.size.h,
                    refresh: mode.refresh,
                    scale: surface.output.current_scale().fractional_scale(),
                    modes: surface.modes.iter().map(mode_key).collect(),
                    scales: scales_for(mode.size.w, mode.size.h),
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
        self.render_output(crtc);
    }

    fn schedule_redraw(&mut self, crtc: crtc::Handle) {
        let Some(surface) = self.backend.outputs.get_mut(&crtc) else {
            return;
        };
        if surface.redraw_timer.is_some() {
            return;
        }
        let token = self.loop_handle.insert_source(
            Timer::from_duration(IDLE_REDRAW),
            move |_, _, state| {
                if let Some(surface) = state.backend.outputs.get_mut(&crtc) {
                    surface.redraw_timer = None;
                }
                state.render_output(crtc);
                TimeoutAction::Drop
            },
        );
        match token {
            Ok(token) => surface.redraw_timer = Some(token),
            Err(err) => tracing::warn!("failed to schedule redraw: {}", err.error),
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

        let UdevData {
            renderer,
            outputs,
            cursor,
            cursor_buffer,
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
                CursorImageStatus::Named(_) => {
                    let image = cursor.image();
                    let hotspot = Point::from((f64::from(image.xhot), f64::from(image.yhot)));
                    let location = (local - hotspot).to_physical(scale);
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

        match space_render_elements::<_, smithay::desktop::Window, _>(
            renderer,
            [&self.space],
            &output,
            1.0,
        ) {
            Ok(space_elements) => {
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

        let started = std::time::Instant::now();
        let rendered = surface
            .drm_output
            .render_frame(renderer, &elements, CLEAR_COLOR, flags);
        let elapsed = started.elapsed();
        if elapsed > SLOW_FRAME {
            tracing::debug!("slow frame on {}: {elapsed:?}", output.name());
        }
        let queued = match rendered {
            Ok(frame) if !frame.is_empty => match surface.drm_output.queue_frame(()) {
                Ok(()) => true,
                Err(err) => {
                    tracing::warn!("failed to queue frame: {err:?}");
                    false
                }
            },
            Ok(_) => false,
            Err(err) => {
                tracing::warn!("failed to render frame: {err:?}");
                false
            }
        };
        surface.frame_pending = queued;

        let elapsed = self.start_time.elapsed();
        for window in self.space.elements() {
            window.send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        for layer in layer_map_for_output(&output).layers() {
            layer.send_frame(&output, elapsed, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
        if let CursorImageStatus::Surface(cursor_surface) = &self.cursor_status {
            send_frames_surface_tree(
                cursor_surface,
                &output,
                elapsed,
                Some(Duration::ZERO),
                |_, _| Some(output.clone()),
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

/// 고를 수 있는 배율. 정수 배율만 쓴다 (GTK를 cairo로 그리는 동안 분수 배율은 흐리다).
/// 배율로 나눈 논리 화면이 너무 작아지지 않는 것만 둔다
fn scales_for(width: i32, height: i32) -> Vec<f64> {
    (1..=MAX_SCALE)
        .filter(|scale| {
            *scale == 1
                || (width / scale >= MIN_LOGICAL_WIDTH && height / scale >= MIN_LOGICAL_HEIGHT)
        })
        .map(f64::from)
        .collect()
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
        assert_eq!(scales_for(1280, 800), vec![1.0, 2.0]);
        assert_eq!(scales_for(1024, 768), vec![1.0]);
        assert_eq!(scales_for(3840, 2160), vec![1.0, 2.0, 3.0]);
    }
}
