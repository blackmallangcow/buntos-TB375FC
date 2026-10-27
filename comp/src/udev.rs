// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRM/KMS + libinput 백엔드 (GPU 하나). Smithay anvil의 udev 백엔드 구조를 단순화했다.
//! 하드웨어 GPU가 없는 VM에서는 Mesa의 소프트웨어 렌더러(llvmpipe)로 그린다.

use std::{collections::HashMap, error::Error, path::PathBuf, time::Duration};

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
                surface::{WaylandSurfaceRenderElement, render_elements_from_surface_tree},
            },
            gles::GlesRenderer,
        },
        session::{Event as SessionEvent, Session, libseat::LibSeatSession},
        udev::{UdevBackend, all_gpus, primary_gpu},
    },
    desktop::{
        layer_map_for_output, space::space_render_elements, utils::send_frames_surface_tree,
    },
    input::pointer::{CursorImageStatus, CursorImageSurfaceData},
    output::{Mode as WlMode, Output, PhysicalProperties},
    reexports::{
        calloop::{
            EventLoop, RegistrationToken,
            timer::{TimeoutAction, Timer},
        },
        drm::control::{Device as _, ModeTypeFlags, connector, crtc},
        input::Libinput,
        rustix::fs::OFlags,
        wayland_server::{Display, backend::GlobalId},
    },
    utils::{DeviceFd, IsAlive, Point, Scale, Transform},
    wayland::compositor::with_states,
};

use crate::{
    config::{Config, parse_hex_color},
    cursor::Cursor,
    render::{self, OutputRenderElements},
    state::BuntState,
};

type Allocator = GbmAllocator<DrmDeviceFd>;
type Exporter = GbmFramebufferExporter<DrmDeviceFd>;
type Elements = OutputRenderElements<GlesRenderer, WaylandSurfaceRenderElement<GlesRenderer>>;

/// 화면 갱신이 없을 때 다시 확인하는 간격
const IDLE_REDRAW: Duration = Duration::from_millis(16);
const CLEAR_COLOR: [f32; 4] = [0.08, 0.08, 0.1, 1.0];
const FALLBACK_TOP: [u8; 3] = [0x5b, 0x7c, 0xfa];
const FALLBACK_BOTTOM: [u8; 3] = [0xf2, 0xa7, 0xc3];

pub struct UdevData {
    pub session: LibSeatSession,
    renderer: GlesRenderer,
    output_manager: DrmOutputManager<Allocator, Exporter, (), DrmDeviceFd>,
    outputs: HashMap<crtc::Handle, OutputSurface>,
    cursor: Cursor,
    cursor_buffer: MemoryRenderBuffer,
}

impl UdevData {
    pub fn seat_name(&self) -> String {
        self.session.seat()
    }
}

struct OutputSurface {
    output: Output,
    drm_output: DrmOutput<Allocator, Exporter, (), DrmDeviceFd>,
    wallpaper: MemoryRenderBuffer,
    /// 페이지 플립을 기다리는 프레임이 있다
    frame_pending: bool,
    redraw_timer: Option<RegistrationToken>,
    _global: GlobalId,
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
    fn scan_connectors(&mut self) {
        let device = self.backend.output_manager.device();
        let resources = match device.resource_handles() {
            Ok(resources) => resources,
            Err(err) => {
                tracing::warn!("failed to read DRM resources: {err}");
                return;
            }
        };
        let mut used = Vec::new();
        let mut connected = Vec::new();
        for handle in resources.connectors() {
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
        let mode_index = info
            .modes()
            .iter()
            .position(|mode| mode.mode_type().contains(ModeTypeFlags::PREFERRED))
            .unwrap_or(0);
        let drm_mode = info.modes()[mode_index];
        let wl_mode = WlMode::from(drm_mode);
        let name = format!("{}-{}", info.interface().as_str(), info.interface_id());
        let (width_mm, height_mm) = info.size().unwrap_or((0, 0));

        let output = Output::new(
            name.clone(),
            PhysicalProperties {
                size: (width_mm as i32, height_mm as i32).into(),
                subpixel: info.subpixel().into(),
                make: "Unknown".into(),
                model: "Unknown".into(),
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
        output.set_preferred(wl_mode);
        output.change_current_state(Some(wl_mode), None, None, Some((x, 0).into()));
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
                return;
            }
        };

        let top = parse_hex_color(&self.config.wallpaper.top).unwrap_or(FALLBACK_TOP);
        let bottom = parse_hex_color(&self.config.wallpaper.bottom).unwrap_or(FALLBACK_BOTTOM);
        let wallpaper = render::wallpaper(top, bottom, wl_mode.size);
        tracing::info!(
            "output {name}: {}x{} @ {} mHz",
            wl_mode.size.w,
            wl_mode.size.h,
            wl_mode.refresh
        );
        outputs.insert(
            crtc,
            OutputSurface {
                output,
                drm_output,
                wallpaper,
                frame_pending: false,
                redraw_timer: None,
                _global: global,
            },
        );
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

        let UdevData {
            renderer,
            outputs,
            cursor,
            cursor_buffer,
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

        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (0.0, 0.0),
            &surface.wallpaper,
            None,
            None,
            None,
            Kind::Unspecified,
        ) {
            Ok(element) => elements.push(OutputRenderElements::Memory(element)),
            Err(err) => tracing::warn!("failed to draw wallpaper: {err}"),
        }

        let queued = match surface.drm_output.render_frame(
            renderer,
            &elements,
            CLEAR_COLOR,
            FrameFlags::DEFAULT,
        ) {
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
