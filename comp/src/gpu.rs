// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터가 얻은 GL 렌더러로 정하는 그리기 방식 (설계안 8.2절).
//!
//! 어떤 렌더러를 얻을지는 화면 GPU에 EGL을 만들어 봐야 안다. 그래서 컴포지터가 뜰 때 정하고, 띄우는
//! 프로그램(셸, 입력기, 시작 프로그램, 터미널)과 prepare 명령에 환경 변수로 알린다. prepare의
//! `dbus-update-activation-environment --systemd`가 그 값을 D-Bus 활성화 환경과 systemd 사용자 환경에
//! 옮기므로, D-Bus로 뜨는 앱과 에이전트가 여는 앱도 같은 방식으로 그린다.
//!
//! - CPU 렌더러(llvmpipe, softpipe, SwiftShader): 앱에도 GPU가 없다. GTK는 GPU 렌더러가 없으면 그
//!   소프트웨어 래스터라이저로 GL·Vulkan을 쓰는데, 메모리를 3배 넘게 쓴다(시스템 설정 475MB 대 130MB).
//!   그래서 GTK가 cairo로 그리게 하고(`GSK_RENDERER=cairo`), linux-dmabuf도 열지 않는다(wl_shm만 받는다)
//! - GPU: GTK가 렌더러를 고르게 둔다(Vulkan이나 GL). 앱은 dmabuf로 보내고 컴포지터는 복사 없이 그린다
//! - 가상 GPU(virgl): 앱의 GL을 호스트가 그린다. 호스트가 CPU로 그려도 이 기기의 앱은 virgl 드라이버로
//!   그려 dmabuf로 보내므로 GPU처럼 다룬다. 다만 합성도 호스트의 CPU가 하므로 프레임 수는 CPU 렌더러처럼
//!   줄인다
//! - 컴포지터의 환경에 GSK_RENDERER가 이미 있으면(세션을 시험할 때) 그 값을 그대로 물려준다

use std::ffi::CStr;

use smithay::backend::renderer::gles::{GlesRenderer, ffi};

/// 이 이름이 들어 있으면 CPU로 그리는 렌더러다 (작은 글자로 견준다)
const SOFTWARE_RENDERERS: [&str; 3] = ["llvmpipe", "softpipe", "swiftshader"];
/// Mesa의 virgl 드라이버. 이름은 "virgl" 또는 "virgl (호스트 렌더러)"이다
const VIRTUAL_GPU: &str = "virgl";
/// GTK 4의 렌더러를 고르는 환경 변수
pub const GSK_RENDERER: &str = "GSK_RENDERER";

/// 렌더러 이름으로 정한 그리기 방식
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rendering {
    /// 화면을 CPU가 합성한다. 프레임 수를 줄인다 (udev.rs의 SOFTWARE_FRAME, BACKGROUND_FRAME)
    pub cpu_composited: bool,
    /// 앱이 GPU로 그릴 수 있다. linux-dmabuf를 열고 GTK가 렌더러를 고르게 둔다
    pub gpu_clients: bool,
}

impl Rendering {
    pub fn from_renderer_name(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        let software = SOFTWARE_RENDERERS
            .iter()
            .any(|renderer| name.contains(renderer));
        let virtual_gpu = name.starts_with(VIRTUAL_GPU);
        Self {
            cpu_composited: software,
            gpu_clients: !software || virtual_gpu,
        }
    }

    /// 컴포지터가 띄우는 프로그램에 더할 환경 변수. `inherited`는 컴포지터가 물려받은 GSK_RENDERER다.
    /// 있으면 아무것도 더하지 않아 그 값이 그대로 간다. 빈 값은 GTK가 모르는 렌더러로 보므로 없는 것으로 친다
    pub fn client_env(self, inherited: Option<&str>) -> Vec<(&'static str, &'static str)> {
        let inherited = inherited.is_some_and(|value| !value.is_empty());
        if self.gpu_clients || inherited {
            Vec::new()
        } else {
            vec![(GSK_RENDERER, "cairo")]
        }
    }
}

/// 컴포지터 GL 컨텍스트의 GL_RENDERER (예: "llvmpipe (LLVM 19.1.7, 128 bits)"). 모르면 빈 문자열
pub fn renderer_name(renderer: &mut GlesRenderer) -> String {
    renderer
        .with_context(|gl| unsafe {
            // 안전: 컴포지터의 컨텍스트가 현재이고, 돌려받은 문자열은 컨텍스트가 가진다
            let name = gl.GetString(ffi::RENDERER);
            (!name.is_null()).then(|| CStr::from_ptr(name.cast()).to_string_lossy().into_owned())
        })
        .ok()
        .flatten()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendering(name: &str) -> (bool, bool) {
        let rendering = Rendering::from_renderer_name(name);
        (rendering.cpu_composited, rendering.gpu_clients)
    }

    #[test]
    fn software_rasterizers_keep_apps_on_the_cpu() {
        // QEMU virtio-gpu(3D 없음)에서 컴포지터가 얻는 렌더러
        assert_eq!(rendering("llvmpipe (LLVM 19.1.7, 128 bits)"), (true, false));
        assert_eq!(rendering("softpipe"), (true, false));
        assert_eq!(rendering("SwiftShader Device (LLVM 10.0.0)"), (true, false));
        // Vulkan 위의 GL도 그 Vulkan이 lavapipe면 CPU다
        assert_eq!(
            rendering("zink Vulkan 1.3(llvmpipe (LLVM 19.1.7, 256 bits))"),
            (true, false)
        );
    }

    #[test]
    fn gpus_let_apps_draw_with_the_gpu() {
        assert_eq!(rendering("Mali-G610 (Panfrost)"), (false, true));
        assert_eq!(
            rendering("Mesa Intel(R) UHD Graphics 620 (KBL GT2)"),
            (false, true)
        );
        // LLVM으로 셰이더를 만드는 하드웨어 드라이버는 CPU 렌더러가 아니다
        assert_eq!(
            rendering("AMD Radeon Graphics (radeonsi, renoir, LLVM 15.0.7, DRM 3.54, 6.5.0)"),
            (false, true)
        );
        // 렌더러 이름을 읽지 못했을 때는 지금까지처럼 GPU로 본다
        assert_eq!(rendering(""), (false, true));
    }

    #[test]
    fn virgl_apps_use_the_gpu_and_a_software_host_still_limits_frames() {
        assert_eq!(
            rendering("virgl (Mesa Intel(R) UHD Graphics 620 (KBL GT2))"),
            (false, true)
        );
        // 호스트(빌드 VM)에 GPU가 없으면 호스트가 llvmpipe로 그린다. 대문자로 오기도 한다
        assert_eq!(
            rendering("virgl (LLVMPIPE (LLVM 19.1.7, 256 bits))"),
            (true, true)
        );
        assert_eq!(
            rendering("virgl (llvmpipe (LLVM 19.1.7, 256 bits))"),
            (true, true)
        );
        // 오래된 호스트는 호스트 렌더러를 알리지 않는다
        assert_eq!(rendering("virgl"), (false, true));
    }

    #[test]
    fn only_cpu_sessions_get_the_cairo_renderer() {
        let cpu = Rendering::from_renderer_name("llvmpipe (LLVM 19.1.7, 128 bits)");
        let gpu = Rendering::from_renderer_name("Mali-G610 (Panfrost)");
        assert_eq!(cpu.client_env(None), vec![(GSK_RENDERER, "cairo")]);
        assert!(gpu.client_env(None).is_empty());
        // 세션이 정한 값은 덮어쓰지 않는다
        assert!(cpu.client_env(Some("gl")).is_empty());
        assert!(gpu.client_env(Some("cairo")).is_empty());
        // 빈 값은 정하지 않은 것이다
        assert_eq!(cpu.client_env(Some("")), vec![(GSK_RENDERER, "cairo")]);
    }
}
