// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 컴포지터끼리 화면을 넘겨주고 넘겨받는다. 로그인 화면과 사용자 세션은 서로 다른 컴포지터 프로세스이므로
//! 로그인하고 로그아웃할 때마다 화면 장치를 넘긴다. 그 사이에 CRTC를 끄면 패널이 꺼졌다 다시 켜진다(TB375FC에서
//! 0.26~0.55초). 전에는 끝나는 쪽(smithay의 정리)과 뜨는 쪽(`DrmDevice::new(fd, true)`)이 모두 CRTC를 껐다.
//!
//! - 넘겨줄 때: 화면에 있는 프레임버퍼의 핸들만 닫고(`DRM_IOCTL_MODE_CLOSEFB`, 리눅스 6.8) smithay의 정리를
//!   건너뛴다. 커널은 프로세스가 끝날 때 파일이 가진 프레임버퍼를 평면에서 내리는데, 닫은 것은 그 목록에 없어
//!   평면이 다음 세션이 그릴 때까지 마지막 화면을 계속 보인다.
//! - 넘겨받을 때: 커넥터가 쓰던 CRTC를 그대로 쓰고 쓰지 않을 것만 끈다. 첫 프레임이 모드와 커넥터를 바꾸지 않으면
//!   smithay는 페이지 플립만 하고, 모드 설정 커밋을 해도 커널은 타이밍이 같으면 CRTC를 끄지 않는다.
//!
//! 창이 남은 채 끝나면(⌃⌥⌫) 마지막 화면을 남기지 않는다. 닫은 프레임버퍼는 다음 DRM 마스터가 읽을 수 있고
//! (커널 문서 `DRM_IOCTL_MODE_CLOSEFB`), 앱의 내용을 다음 세션까지 화면에 두지 않기 위해서다.
//!
//! 커널 콘솔(fbdev 클라이언트)이 있는 기기에서는 장치의 마지막 파일이 닫힐 때(`drm_lastclose`) 콘솔이 화면을
//! 가져가므로 마지막 화면이 끝까지 남지 않는다. CRTC와 모드는 그대로다. TB375FC는 콘솔이 없다
//! (`drm_client_lib.active=`).

use std::{io, os::fd::AsRawFd};

use smithay::reexports::drm::control::{
    AtomicCommitFlags, Device as ControlDevice, ResourceHandle, atomic::AtomicModeReq, connector,
    crtc, framebuffer, plane, property,
};

/// CRTC를 고를 커넥터 하나
pub struct Wanted<K, C> {
    pub connector: K,
    /// 앞 컴포지터(또는 커널의 콘솔)가 이 커넥터를 붙여 둔 CRTC
    pub current: Option<C>,
    /// 이 커넥터가 쓸 수 있는 CRTC
    pub possible: Vec<C>,
}

/// 커넥터마다 쓸 CRTC를 고른다. 지금 쓰는 CRTC를 먼저 그대로 둔다: 다른 CRTC로 옮기면 화면을 끄고 켜야 한다.
/// 남는 CRTC가 없는 커넥터는 뺀다. 결과는 `wanted`의 차례를 따른다(화면의 왼쪽부터 놓는 차례다)
pub fn assign_crtcs<K: Copy, C: Copy + PartialEq>(
    wanted: &[Wanted<K, C>],
    used: &[C],
) -> Vec<(K, C)> {
    let mut taken = used.to_vec();
    let mut chosen: Vec<Option<C>> = wanted
        .iter()
        .map(|wanted| {
            let crtc = wanted
                .current
                .filter(|crtc| wanted.possible.contains(crtc) && !taken.contains(crtc))?;
            taken.push(crtc);
            Some(crtc)
        })
        .collect();
    for (slot, wanted) in chosen.iter_mut().zip(wanted) {
        if slot.is_none() {
            *slot = wanted
                .possible
                .iter()
                .copied()
                .find(|crtc| !taken.contains(crtc));
            taken.extend(*slot);
        }
    }
    wanted
        .iter()
        .zip(chosen)
        .filter_map(|(wanted, crtc)| Some((wanted.connector, crtc?)))
        .collect()
}

/// 넘겨받을 때 꺼야 할 것
#[derive(Debug, PartialEq, Eq)]
struct Unused<K, C, P> {
    connectors: Vec<K>,
    crtcs: Vec<C>,
    planes: Vec<P>,
}

impl<K, C, P> Unused<K, C, P> {
    fn is_empty(&self) -> bool {
        self.connectors.is_empty() && self.crtcs.is_empty() && self.planes.is_empty()
    }
}

/// 이어 쓸 (커넥터, CRTC, 주 평면)만 남기고 꺼야 할 것을 고른다. 켜진 다른 화면, 쓰지 않을 CRTC에 남은 평면,
/// 이어 쓸 CRTC에 남은 커서와 오버레이 평면이다. smithay는 자기가 쓰지 않은 평면을 건드리지 않으므로 여기서 끈다
fn unused<K: Copy + PartialEq, C: Copy + PartialEq, P: Copy + PartialEq>(
    keep: &[(K, C, P)],
    connectors: &[(K, Option<C>)],
    crtcs: &[(C, bool)],
    planes: &[(P, Option<C>)],
) -> Unused<K, C, P> {
    Unused {
        connectors: connectors
            .iter()
            .filter(|(connector, crtc)| {
                crtc.is_some_and(|crtc| {
                    !keep
                        .iter()
                        .any(|kept| kept.0 == *connector && kept.1 == crtc)
                })
            })
            .map(|(connector, _)| *connector)
            .collect(),
        crtcs: crtcs
            .iter()
            .filter(|(crtc, on)| *on && !keep.iter().any(|kept| kept.1 == *crtc))
            .map(|(crtc, _)| *crtc)
            .collect(),
        planes: planes
            .iter()
            .filter(|(plane, crtc)| {
                crtc.is_some_and(|crtc| !keep.iter().any(|kept| kept.1 == crtc && kept.2 == *plane))
            })
            .map(|(plane, _)| *plane)
            .collect(),
    }
}

/// 커넥터가 지금 붙어 있는 CRTC
pub fn current_crtc(device: &impl ControlDevice, info: &connector::Info) -> Option<crtc::Handle> {
    device.get_encoder(info.current_encoder()?).ok()?.crtc()
}

/// 이어 쓸 것만 남기고 끈다. 끌 것이 없으면 커밋하지 않는다
pub fn turn_off_unused(
    device: &impl ControlDevice,
    keep: &[(connector::Handle, crtc::Handle, plane::Handle)],
) -> io::Result<()> {
    let resources = device.resource_handles()?;
    let connectors: Vec<_> = resources
        .connectors()
        .iter()
        .map(|&connector| {
            let crtc = device
                .get_connector(connector, false)
                .ok()
                .and_then(|info| current_crtc(device, &info));
            (connector, crtc)
        })
        .collect();
    let crtcs = resources
        .crtcs()
        .iter()
        .map(|&crtc| Ok((crtc, device.get_crtc(crtc)?.mode().is_some())))
        .collect::<io::Result<Vec<_>>>()?;
    let planes = device
        .plane_handles()?
        .into_iter()
        .map(|plane| Ok((plane, device.get_plane(plane)?.crtc())))
        .collect::<io::Result<Vec<_>>>()?;
    let unused = unused(keep, &connectors, &crtcs, &planes);
    if unused.is_empty() {
        return Ok(());
    }
    tracing::info!("turning off what this session does not use: {unused:?}");

    let mut request = AtomicModeReq::new();
    for connector in unused.connectors {
        let crtc_id = property_id(device, connector, c"CRTC_ID")?;
        request.add_property(connector, crtc_id, property::Value::CRTC(None));
    }
    for crtc in unused.crtcs {
        let active = property_id(device, crtc, c"ACTIVE")?;
        let mode_id = property_id(device, crtc, c"MODE_ID")?;
        request.add_property(crtc, active, property::Value::Boolean(false));
        request.add_property(crtc, mode_id, property::Value::Blob(0));
    }
    for plane in unused.planes {
        let crtc_id = property_id(device, plane, c"CRTC_ID")?;
        let fb_id = property_id(device, plane, c"FB_ID")?;
        request.add_property(plane, crtc_id, property::Value::CRTC(None));
        request.add_property(plane, fb_id, property::Value::Framebuffer(None));
    }
    device.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, request)
}

/// 커넥터, CRTC, 평면의 속성을 이름으로 찾는다
fn property_id<T: ResourceHandle>(
    device: &impl ControlDevice,
    handle: T,
    name: &std::ffi::CStr,
) -> io::Result<property::Handle> {
    let properties = device.get_properties(handle)?;
    let (ids, _) = properties.as_props_and_values();
    ids.iter()
        .copied()
        .find(|id| {
            device
                .get_property(*id)
                .is_ok_and(|property| property.name() == name)
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no DRM property {name:?}")))
}

/// 커널의 fbdev 클라이언트(콘솔)가 있는지. 다른 GPU의 것이어도 있다고 본다: 그때는 첫 화면까지 검을 뿐이다
pub fn kernel_console_present() -> bool {
    std::path::Path::new("/sys/class/graphics/fb0").exists()
}

/// 평면에 있는 프레임버퍼의 핸들을 닫아, 이 프로세스가 끝나도 평면이 그 버퍼를 계속 보이게 한다
pub fn keep_last_frame(device: &impl ControlDevice, plane: plane::Handle) -> io::Result<()> {
    match device.get_plane(plane)?.framebuffer() {
        Some(framebuffer) => close_framebuffer(device, framebuffer),
        None => Ok(()),
    }
}

/// `struct drm_mode_closefb`
#[repr(C)]
struct CloseFramebuffer {
    fb_id: u32,
    pad: u32,
}

/// `DRM_IOCTL_MODE_CLOSEFB`: `DRM_IOCTL_MODE_RMFB`와 달리 평면과 CRTC를 끄지 않는다. drm 크레이트에 아직 없다
const CLOSE_FRAMEBUFFER: libc::Ioctl = libc::_IOWR::<CloseFramebuffer>(b'd' as u32, 0xD0);

fn close_framebuffer(
    device: &impl ControlDevice,
    framebuffer: framebuffer::Handle,
) -> io::Result<()> {
    let mut request = CloseFramebuffer {
        fb_id: framebuffer.into(),
        pad: 0,
    };
    // 안전: fd는 device가 살아 있는 동안 열려 있고, request는 이 ioctl이 읽고 쓰는 구조체 그대로다
    let result = unsafe {
        libc::ioctl(
            device.as_fd().as_raw_fd(),
            CLOSE_FRAMEBUFFER,
            &raw mut request,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(connector: u32, current: Option<u32>, possible: &[u32]) -> Wanted<u32, u32> {
        Wanted {
            connector,
            current,
            possible: possible.to_vec(),
        }
    }

    #[test]
    fn a_connector_stays_on_the_crtc_it_already_uses() {
        // 쓸 수 있는 CRTC의 첫째가 아니라도 지금 쓰는 CRTC를 고른다. 첫째를 고르면 화면을 끄고 옮겨야 한다
        assert_eq!(
            assign_crtcs(&[wanted(1, Some(11), &[10, 11])], &[]),
            vec![(1, 11)]
        );
    }

    #[test]
    fn a_new_connector_does_not_take_the_crtc_of_a_lit_one() {
        // 차례가 앞선 새 커넥터(1)가 켜진 커넥터(2)의 CRTC를 뺏지 않는다
        assert_eq!(
            assign_crtcs(
                &[wanted(1, None, &[10, 11]), wanted(2, Some(10), &[10, 11])],
                &[]
            ),
            vec![(1, 11), (2, 10)]
        );
    }

    #[test]
    fn the_crtcs_of_other_outputs_are_not_reused() {
        assert_eq!(
            assign_crtcs(&[wanted(1, None, &[10, 11])], &[10]),
            vec![(1, 11)]
        );
        // 지금 쓰는 CRTC를 이미 다른 출력이 쓰면 다른 것을 고른다
        assert_eq!(
            assign_crtcs(&[wanted(1, Some(10), &[10, 11])], &[10]),
            vec![(1, 11)]
        );
    }

    #[test]
    fn two_connectors_on_one_crtc_get_their_own() {
        // 콘솔이나 펌웨어가 두 화면을 복제로 켜 두면 하나는 다른 CRTC로 간다
        assert_eq!(
            assign_crtcs(
                &[
                    wanted(1, Some(10), &[10, 11]),
                    wanted(2, Some(10), &[10, 11])
                ],
                &[]
            ),
            vec![(1, 10), (2, 11)]
        );
    }

    #[test]
    fn a_crtc_the_connector_cannot_use_or_no_crtc_at_all() {
        assert_eq!(
            assign_crtcs(&[wanted(1, Some(12), &[10])], &[]),
            vec![(1, 10)]
        );
        assert_eq!(assign_crtcs(&[wanted(1, None, &[10])], &[10]), vec![]);
    }

    #[test]
    fn nothing_to_turn_off_when_the_session_keeps_the_only_screen() {
        // 커서 평면(101)은 비어 있다
        let unused = unused(
            &[(1, 10, 100)],
            &[(1, Some(10)), (2, None)],
            &[(10, true), (11, false)],
            &[(100, Some(10)), (101, None)],
        );
        assert!(unused.is_empty());
    }

    #[test]
    fn what_the_session_does_not_use_is_turned_off() {
        // 커넥터 2는 빠졌지만 CRTC 11에 붙어 있고, 평면 102는 그 CRTC에, 평면 101은 이어 쓸 CRTC에 남았다
        let unused = unused(
            &[(1, 10, 100)],
            &[(1, Some(10)), (2, Some(11))],
            &[(10, true), (11, true)],
            &[(100, Some(10)), (101, Some(10)), (102, Some(11))],
        );
        assert_eq!(
            unused,
            Unused {
                connectors: vec![2],
                crtcs: vec![11],
                planes: vec![101, 102],
            }
        );
    }

    #[test]
    fn a_connector_moving_to_another_crtc_is_taken_off_the_old_one() {
        let unused = unused(
            &[(1, 11, 101)],
            &[(1, Some(10))],
            &[(10, true), (11, false)],
            &[(100, Some(10)), (101, None)],
        );
        assert_eq!(
            unused,
            Unused {
                connectors: vec![1],
                crtcs: vec![10],
                planes: vec![100],
            }
        );
    }
}
