// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 앱이 GPU로 그린 버퍼를 받는다 (linux-dmabuf v5, 기본 피드백 포함. 설계안 8.2절).
//!
//! 컴포지터가 GPU로 그릴 때만 연다 (gpu.rs). CPU 렌더러에서는 지금처럼 wl_shm만 받는다.
//! - 기본 피드백: 주 장치는 컴포지터가 그리는 GPU의 렌더 노드이고, 형식은 컴포지터의 EGL이 텍스처로 읽을 수
//!   있는 형식과 수정자(modifier)다. 앱(Mesa의 EGL, GTK의 Vulkan)은 피드백을 보고 그 장치에서 버퍼를 만든다.
//!   화면 장치와 GPU가 다른 SoC(mediatek-drm과 Panthor처럼 Mesa의 kmsro로 그리는 기기)는 화면 장치에
//!   렌더 노드가 없으므로 EGL이 알리는 장치(render_node)로 고른다.
//!   표면마다 다른 피드백(스캔아웃 평면의 형식)은 직접 스캔아웃을 켤 때 준다
//! - 받기: 평면이 버퍼(fd) 안에 드는지는 smithay가 본다. 컴포지터는 버퍼를 만들 때 렌더러로 가져와 보고,
//!   안 되면 앱에 실패로 알린다. 가져온 텍스처는 렌더러가 버퍼별로 기억해 그릴 때 다시 쓴다(복사 없음)
//! - 앱 버퍼를 화면 평면에 바로 올리는 직접 스캔아웃은 아직 하지 않는다 (udev.rs의 GbmFramebufferExporter).
//!   흐림(blur.rs)은 화면 버퍼에 먼저 그린 장면을 복사하므로, 평면에 따로 올린 창은 흐림에서 빠진다.
//!   하드웨어에서 확인한 뒤 켠다

use smithay::{
    backend::{
        allocator::{Buffer, dmabuf::Dmabuf, format::FormatSet},
        drm::{DrmNode, NodeType},
        egl::{EGLDevice, EGLDisplay},
        renderer::{ImportDma, gles::GlesRenderer},
    },
    delegate_dmabuf,
    reexports::{
        wayland_protocols::wp::linux_dmabuf::zv1::server::{
            zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
            zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        },
        wayland_server::{Dispatch, DisplayHandle, GlobalDispatch, protocol::wl_buffer::WlBuffer},
    },
    wayland::{
        buffer::BufferHandler,
        dmabuf::{
            DmabufFeedbackBuilder, DmabufGlobal, DmabufGlobalData, DmabufHandler, DmabufParamsData,
            DmabufState, ImportNotifier,
        },
    },
};

use crate::state::BuntState;

/// 앱이 버퍼를 만들 GPU의 렌더 노드 (기본 피드백의 주 장치). 없으면 None
///
/// 컴포지터의 EGL이 그리는 장치를 먼저 본다. Mesa는 화면 전용 장치(kmsro)로 연 EGL에 짝이 되는 렌더
/// 장치를 알린다(Mesa 25.2 `dri2_setup_device`). 화면 장치의 렌더 노드만 보면 그런 기기에서는 렌더 노드가
/// 없어 linux-dmabuf를 열지 못했다. 렌더 노드가 없는 장치(주 노드뿐)는 앱의 Mesa가 열지 않으므로 고르지
/// 않는다. EGL이 장치를 알리지 않으면 화면 장치(`display`)의 렌더 노드를 쓴다
pub fn render_node(egl_display: &EGLDisplay, display: Option<DrmNode>) -> Option<DrmNode> {
    EGLDevice::device_for_display(egl_display)
        .ok()
        .and_then(|device| device.try_get_render_node().ok().flatten())
        .filter(|node| node.ty() == NodeType::Render)
        .or_else(|| display.and_then(|node| node.node_with_type(NodeType::Render)?.ok()))
}

/// linux-dmabuf 상태. 전역이 없으면(CPU 렌더러) 앱은 이 프로토콜을 보지 못한다
pub struct Dmabufs {
    state: DmabufState,
    global: Option<DmabufGlobal>,
    /// 앱 버퍼를 만드는 장치 (기본 피드백의 주 장치). 가져온 버퍼에 적는다
    node: Option<DrmNode>,
}

impl Dmabufs {
    /// `device`가 없으면 전역을 만들지 않는다. `device`는 렌더 노드와 렌더러가 텍스처로 읽는 형식이다
    pub fn new<D>(display: &DisplayHandle, device: Option<(DrmNode, FormatSet)>) -> Self
    where
        D: GlobalDispatch<ZwpLinuxDmabufV1, DmabufGlobalData>
            + BufferHandler
            + DmabufHandler
            + 'static,
    {
        match device {
            Some((node, formats)) => Self::open::<D>(display, node.dev_id(), Some(node), formats),
            None => Self::closed(),
        }
    }

    /// 전역 없이 (앱은 wl_shm으로만 보낸다)
    fn closed() -> Self {
        Self {
            state: DmabufState::new(),
            global: None,
            node: None,
        }
    }

    /// `main_device`를 기본 피드백의 주 장치로 알리는 전역을 연다. 시험에서는 DRM 장치 없이 연다(node 없음)
    pub(crate) fn open<D>(
        display: &DisplayHandle,
        main_device: libc::dev_t,
        node: Option<DrmNode>,
        formats: FormatSet,
    ) -> Self
    where
        D: GlobalDispatch<ZwpLinuxDmabufV1, DmabufGlobalData>
            + BufferHandler
            + DmabufHandler
            + 'static,
    {
        let mut dmabufs = Self::closed();
        let count = formats.iter().count();
        // 형식이 없는 전역을 보면 앱(Mesa의 EGL)이 GPU로 그리려다 버퍼를 보내지 못한다. 열지 않으면
        // wl_shm으로 보낸다
        if count == 0 {
            tracing::warn!("linux-dmabuf disabled: the renderer imports no dmabuf format");
            return dmabufs;
        }
        match DmabufFeedbackBuilder::new(main_device, formats).build() {
            Ok(feedback) => {
                let global = dmabufs
                    .state
                    .create_global_with_default_feedback::<D>(display, &feedback);
                match node {
                    Some(node) => {
                        tracing::info!("linux-dmabuf: main device {node}, {count} formats")
                    }
                    None => tracing::info!(
                        "linux-dmabuf: main device {main_device:#x}, {count} formats"
                    ),
                }
                dmabufs.global = Some(global);
                dmabufs.node = node;
            }
            // 형식 표를 담을 메모리 파일을 만들지 못했다. 앱은 wl_shm으로 보낸다
            Err(err) => tracing::warn!("linux-dmabuf disabled: {err}"),
        }
        dmabufs
    }

    /// 앱이 linux-dmabuf를 볼 수 있다
    pub fn enabled(&self) -> bool {
        self.global.is_some()
    }

    pub fn state_mut(&mut self) -> &mut DmabufState {
        &mut self.state
    }

    /// 가져온 버퍼에 적을 장치
    pub fn node(&self) -> Option<DrmNode> {
        self.node
    }
}

/// 앱이 만든 버퍼를 렌더러로 가져와 본다. 되면 버퍼를 만들고, 안 되면 앱에 실패로 알린다
pub fn import<D>(
    renderer: &mut GlesRenderer,
    node: Option<DrmNode>,
    dmabuf: Dmabuf,
    notifier: ImportNotifier,
) where
    D: Dispatch<ZwpLinuxBufferParamsV1, DmabufParamsData>
        + Dispatch<WlBuffer, Dmabuf>
        + BufferHandler
        + DmabufHandler
        + 'static,
{
    match renderer.import_dmabuf(&dmabuf, None) {
        Ok(_) => {
            dmabuf.set_node(node);
            // 앱이 그새 끊겼으면 버퍼를 만들 곳이 없다. 할 일이 없다
            let _ = notifier.successful::<D>();
        }
        Err(err) => {
            tracing::debug!(
                "failed to import a {}x{} {:?}/{:?} dmabuf: {err}",
                dmabuf.width(),
                dmabuf.height(),
                dmabuf.format().code,
                dmabuf.format().modifier
            );
            notifier.failed();
        }
    }
}

impl DmabufHandler for BuntState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        self.backend.dmabufs.state_mut()
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        let node = self.backend.dmabufs.node();
        import::<Self>(self.backend.renderer(), node, dmabuf, notifier);
    }
}
delegate_dmabuf!(BuntState);

#[cfg(test)]
mod tests {
    use std::{io::Read, os::fd::AsFd};

    use smithay::{
        backend::allocator::{Format, Fourcc, Modifier, format::FormatSet},
        utils::Scale,
    };
    use wayland_client::protocol::{wl_compositor::WlCompositor, wl_shm::WlShm};
    use wayland_protocols::wp::linux_dmabuf::zv1::client::{
        zwp_linux_buffer_params_v1::{self, Flags},
        zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
    };

    use crate::testing::{
        Frames, MAIN_DEVICE, Server, ServerOptions, argb, gl_lock, memfd, pixel, software_renderer,
        texture_formats,
    };

    const DMABUF: &str = "zwp_linux_dmabuf_v1";
    /// GBM이 흔히 쓰는 인텔 X 타일. llvmpipe는 가져오지 못한다
    const I915_X_TILED: u64 = 0x0100_0000_0000_0001;

    fn formats(list: &[(Fourcc, Modifier)]) -> FormatSet {
        list.iter()
            .map(|&(code, modifier)| Format { code, modifier })
            .collect()
    }

    #[test]
    fn a_device_without_drm_nodes_gives_apps_no_render_node() {
        let _gl = gl_lock();
        let Some(renderer) = software_renderer() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        // Mesa의 소프트웨어 EGL 장치에는 DRM 노드가 없다. 화면 장치도 없으면 linux-dmabuf를 열 곳이 없다
        assert_eq!(
            super::render_node(renderer.egl_context().display(), None),
            None
        );
    }

    #[test]
    fn cpu_rendering_keeps_apps_on_wl_shm() {
        let mut server = Server::new(
            None,
            ServerOptions {
                dmabuf_formats: None,
                blur: true,
            },
        );
        let client = server.connect();
        assert!(client.global_version("wl_shm").is_some());
        assert_eq!(client.global_version(DMABUF), None);
    }

    #[test]
    fn a_renderer_without_dmabuf_formats_keeps_apps_on_wl_shm() {
        let mut server = Server::new(
            None,
            ServerOptions {
                dmabuf_formats: Some(FormatSet::default()),
                blur: false,
            },
        );
        let client = server.connect();
        assert_eq!(client.global_version(DMABUF), None);
        // 컴포지터는 이때 GTK도 CPU로 그리게 한다 (udev.rs)
        assert!(!server.state.dmabufs.enabled());
    }

    #[test]
    fn default_feedback_names_the_main_device_and_the_texture_formats() {
        let offered = formats(&[
            (Fourcc::Argb8888, Modifier::Linear),
            (Fourcc::Xrgb8888, Modifier::Linear),
            (Fourcc::Argb8888, Modifier::Invalid),
        ]);
        let mut server = Server::new(
            None,
            ServerOptions {
                dmabuf_formats: Some(offered.clone()),
                blur: false,
            },
        );
        let mut client = server.connect();
        // 기본 피드백은 v4부터다. smithay는 v5를 연다
        assert!(
            client
                .global_version(DMABUF)
                .is_some_and(|version| version >= 4)
        );
        let dmabuf: ZwpLinuxDmabufV1 = client.bind(DMABUF, 5).unwrap();
        dmabuf.get_default_feedback(&client.qh, ());
        server.pump(&mut client);

        assert!(client.data.feedback_done);
        assert_eq!(
            client.data.main_device.as_deref(),
            Some(&MAIN_DEVICE.to_ne_bytes()[..])
        );
        // 형식 표: 한 칸에 형식(u32), 비움(u32), 수정자(u64)
        let (fd, size) = client.data.format_table.take().unwrap();
        let mut table = Vec::new();
        std::fs::File::from(fd).read_to_end(&mut table).unwrap();
        assert_eq!(table.len(), size as usize);
        let listed: FormatSet = table
            .as_chunks::<16>()
            .0
            .iter()
            .map(|entry| Format {
                code: Fourcc::try_from(u32::from_ne_bytes(entry[0..4].try_into().unwrap()))
                    .unwrap(),
                modifier: Modifier::from(u64::from_ne_bytes(entry[8..16].try_into().unwrap())),
            })
            .collect();
        assert_eq!(listed.indexset(), offered.indexset());
        // 묶음 하나에 모든 형식이 든다
        let indices: Vec<u16> = client.data.tranche_formats.concat();
        assert_eq!(indices.len(), offered.iter().count());
    }

    #[test]
    fn app_dmabufs_are_imported_and_drawn_without_a_copy() {
        let _gl = gl_lock();
        let Some(renderer) = software_renderer() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let offered = texture_formats(&renderer);
        let mut server = Server::new(
            Some(renderer),
            ServerOptions {
                dmabuf_formats: Some(offered),
                blur: false,
            },
        );
        let mut client = server.connect();
        let compositor: WlCompositor = client.bind("wl_compositor", 4).unwrap();
        let dmabuf: ZwpLinuxDmabufV1 = client.bind(DMABUF, 5).unwrap();
        let surface = compositor.create_surface(&client.qh, ());
        let (width, height) = (64, 32);
        let buffer = client.dmabuf_buffer(
            &dmabuf,
            (width, height),
            Fourcc::Argb8888,
            Modifier::Linear.into(),
            &argb(width as usize, height as usize, [192, 32, 16, 255]),
            Flags::empty(),
        );
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, width, height);
        surface.commit();
        server.pump(&mut client);
        assert_eq!(client.protocol_error(), None);
        assert_eq!(server.state.surfaces.len(), 1);

        let scale = Scale::from(1.0);
        let elements = server.surface_elements(0, (0, 0).into(), scale);
        assert_eq!(elements.len(), 1);
        let mut frames = Frames::new(server.renderer(), (width, height).into(), 1.0);
        let pixels = frames.render(server.renderer(), &elements);
        assert_eq!(pixel(&pixels, width as usize, 0, 0), [192, 32, 16, 255]);
        assert_eq!(
            pixel(
                &pixels,
                width as usize,
                width as usize - 1,
                height as usize - 1
            ),
            [192, 32, 16, 255]
        );
    }

    #[test]
    fn buffers_the_renderer_cannot_import_fail_without_killing_the_app() {
        let _gl = gl_lock();
        let Some(renderer) = software_renderer() else {
            eprintln!("skipped: no software EGL device");
            return;
        };
        let offered = texture_formats(&renderer);
        let mut server = Server::new(
            Some(renderer),
            ServerOptions {
                dmabuf_formats: Some(offered),
                blur: false,
            },
        );
        let mut client = server.connect();
        let dmabuf: ZwpLinuxDmabufV1 = client.bind(DMABUF, 5).unwrap();
        let (width, height) = (16, 16);
        let fd = memfd(&argb(width, height, [0, 0, 0, 255]));
        let params = dmabuf.create_params(&client.qh, ());
        params.add(
            fd.as_fd(),
            0,
            0,
            width as u32 * 4,
            (I915_X_TILED >> 32) as u32,
            I915_X_TILED as u32,
        );
        // create(실패할 수 있는 만들기)는 실패를 이벤트로 알린다
        params.create(
            width as i32,
            height as i32,
            Fourcc::Argb8888 as u32,
            Flags::empty(),
        );
        server.pump(&mut client);
        assert_eq!(client.data.failed, 1);
        assert!(client.data.created.is_empty());
        assert_eq!(client.protocol_error(), None);
        // 앱은 그대로 붙어 있어 wl_shm으로 다시 그릴 수 있다
        let shm: WlShm = client.bind("wl_shm", 1).unwrap();
        let _buffer = client.shm_buffer(&shm, 16, 16, &argb(16, 16, [0, 0, 0, 255]));
        server.pump(&mut client);
        assert_eq!(client.protocol_error(), None);
    }

    #[test]
    fn planes_beyond_the_buffer_are_refused() {
        // 평면이 fd 크기를 넘으면 smithay가 프로토콜 오류로 끊는다 (컴포지터가 남의 메모리를 읽지 않게)
        let mut server = Server::new(
            None,
            ServerOptions {
                dmabuf_formats: Some(formats(&[(Fourcc::Argb8888, Modifier::Linear)])),
                blur: false,
            },
        );
        let mut client = server.connect();
        let dmabuf: ZwpLinuxDmabufV1 = client.bind(DMABUF, 5).unwrap();
        // 64x64 버퍼라고 하면서 한 줄만 담은 fd
        let _buffer = client.dmabuf_buffer(
            &dmabuf,
            (64, 64),
            Fourcc::Argb8888,
            Modifier::Linear.into(),
            &argb(64, 1, [0, 0, 0, 255]),
            Flags::empty(),
        );
        server.pump(&mut client);
        // 오류 코드만 본다. 작업 공간을 함께 빌드하면 시험 클라이언트가 libwayland(wayland-backend의
        // client_system)로 돌고, libwayland는 이미 지운 객체(params)의 오류를 인터페이스 없이 @0으로 알린다
        assert_eq!(
            client.protocol_error().map(|(code, _)| code),
            Some(zwp_linux_buffer_params_v1::Error::OutOfBounds as u32)
        );
    }
}
