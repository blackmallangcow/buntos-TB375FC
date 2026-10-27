// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 시스템 정보 읽기와 시스템 서비스(systemd의 hostnamed, timedated) 요청

use std::{io, path::PathBuf};

use gtk4::{gio, glib, prelude::*};

/// D-Bus로 요청하는 systemd 시스템 서비스. 서비스는 요청이 오면 켜지고, 권한은 polkit이 판단한다
pub struct SystemService {
    name: &'static str,
    path: &'static str,
    interface: &'static str,
}

pub const HOSTNAME: SystemService = SystemService {
    name: "org.freedesktop.hostname1",
    path: "/org/freedesktop/hostname1",
    interface: "org.freedesktop.hostname1",
};

pub const TIMEDATE: SystemService = SystemService {
    name: "org.freedesktop.timedate1",
    path: "/org/freedesktop/timedate1",
    interface: "org.freedesktop.timedate1",
};

impl SystemService {
    pub async fn call(
        &self,
        method: &str,
        args: &glib::Variant,
    ) -> Result<glib::Variant, glib::Error> {
        let bus = gio::bus_get_future(gio::BusType::System).await?;
        bus.call_future(
            Some(self.name),
            self.path,
            self.interface,
            method,
            Some(args),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
    }

    pub async fn property(&self, name: &str) -> Result<glib::Variant, glib::Error> {
        let bus = gio::bus_get_future(gio::BusType::System).await?;
        let reply = bus
            .call_future(
                Some(self.name),
                self.path,
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&(self.interface, name).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await?;
        // 답은 (v) 형식이다
        reply.child_value(0).as_variant().ok_or_else(|| {
            glib::Error::new(gio::IOErrorEnum::InvalidData, "unexpected D-Bus reply")
        })
    }
}

/// os-release 형식(KEY=값, 값은 따옴표로 감쌀 수 있음)에서 값을 찾는다.
fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix('=')?;
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);
        Some(value.to_owned())
    })
}

pub struct OsRelease {
    /// 예: "Debian GNU/Linux forky/sid"
    pub pretty_name: Option<String>,
    /// mkosi가 적는 이미지 이름. buntOS 이미지는 "buntos"
    pub image_id: Option<String>,
}

pub fn os_release() -> OsRelease {
    let text = ["/etc/os-release", "/usr/lib/os-release"]
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    OsRelease {
        pretty_name: os_release_value(&text, "PRETTY_NAME"),
        image_id: os_release_value(&text, "IMAGE_ID"),
    }
}

pub fn kernel_release() -> Option<String> {
    let text = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok()?;
    Some(text.trim().to_owned())
}

/// 기기 모델. 장치 트리(실기기)나 SMBIOS(PC, 가상 머신)에서 읽는다
pub fn device_model() -> Option<String> {
    let read = |path: &str| {
        let text = std::fs::read_to_string(path).ok()?;
        let text = text.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        (!text.is_empty()).then(|| text.to_owned())
    };
    if let Some(model) = read("/sys/firmware/devicetree/base/model") {
        return Some(model);
    }
    let product = read("/sys/class/dmi/id/product_name")?;
    match read("/sys/class/dmi/id/sys_vendor") {
        Some(vendor) if !product.starts_with(&vendor) => Some(format!("{vendor} {product}")),
        _ => Some(product),
    }
}

/// /proc/cpuinfo의 "model name". arm64 커널은 이 항목을 주지 않는다
fn cpu_model_from(cpuinfo: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim().to_owned())
    })
}

pub fn cpu_model() -> Option<String> {
    cpu_model_from(&std::fs::read_to_string("/proc/cpuinfo").ok()?)
}

pub fn cpu_count() -> usize {
    std::thread::available_parallelism().map_or(1, |count| count.get())
}

fn memory_total_from(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib * 1024)
}

/// 전체 메모리 (바이트)
pub fn memory_total() -> Option<u64> {
    memory_total_from(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

/// 루트 파일 시스템의 (전체, 남은) 바이트
pub fn disk_space() -> Option<(u64, u64)> {
    let info = gio::File::for_path("/")
        .query_filesystem_info("filesystem::size,filesystem::free", gio::Cancellable::NONE)
        .ok()?;
    Some((
        info.attribute_uint64("filesystem::size"),
        info.attribute_uint64("filesystem::free"),
    ))
}

/// 표시 언어를 적어 두는 파일. systemd의 environment.d 형식이며, 세션을 시작하는
/// /usr/libexec/buntos/session과 사용자 서비스 관리자(systemd --user)가 읽는다
fn language_file() -> PathBuf {
    glib::user_config_dir()
        .join("environment.d")
        .join("50-buntos-language.conf")
}

fn lang_from(text: &str) -> Option<String> {
    // 같은 변수가 여러 번 나오면 마지막 값을 쓴다
    text.lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("LANG="))
        .map(str::to_owned)
}

/// 시스템 설정에서 고른 표시 언어 (예: "en_US.UTF-8"). 고른 적이 없으면 None
pub fn session_language() -> Option<String> {
    lang_from(&std::fs::read_to_string(language_file()).ok()?)
}

pub fn set_session_language(locale: &str) -> io::Result<()> {
    let path = language_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        path,
        format!("# 시스템 설정에서 고른 표시 언어. 다시 시작하면 적용된다\nLANG={locale}\n"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_values_are_unquoted() {
        let text = "PRETTY_NAME=\"Debian GNU/Linux forky/sid\"\nNAME='Debian'\nIMAGE_ID=buntos\n";
        assert_eq!(
            os_release_value(text, "PRETTY_NAME").as_deref(),
            Some("Debian GNU/Linux forky/sid")
        );
        assert_eq!(os_release_value(text, "NAME").as_deref(), Some("Debian"));
        assert_eq!(
            os_release_value(text, "IMAGE_ID").as_deref(),
            Some("buntos")
        );
        assert_eq!(os_release_value(text, "IMAGE"), None);
    }

    #[test]
    fn cpu_model_comes_from_model_name() {
        let x86 = "processor\t: 0\nmodel name\t: AMD Ryzen 7 7840U\n";
        assert_eq!(cpu_model_from(x86).as_deref(), Some("AMD Ryzen 7 7840U"));
        let arm64 = "processor\t: 0\nBogoMIPS\t: 48.00\nCPU part\t: 0xd4b\n";
        assert_eq!(cpu_model_from(arm64), None);
    }

    #[test]
    fn memory_total_is_in_bytes() {
        let meminfo = "MemTotal:        4005504 kB\nMemFree:         1000000 kB\n";
        assert_eq!(memory_total_from(meminfo), Some(4005504 * 1024));
    }

    #[test]
    fn last_lang_line_wins() {
        assert_eq!(
            lang_from("# 주석\nLANG=ko_KR.UTF-8\nLANG=en_US.UTF-8\n").as_deref(),
            Some("en_US.UTF-8")
        );
        assert_eq!(lang_from("LC_TIME=ko_KR.UTF-8\n"), None);
    }
}
