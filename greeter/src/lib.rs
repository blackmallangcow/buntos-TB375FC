// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 로그인 화면(bunt-greeter)과 계정 만들기 서비스(buntd-setup)가 함께 쓰는 것: 계정 규칙과
//! greetd IPC. GTK에 기대지 않으므로 root로 도는 buntd-setup에 GTK가 들어가지 않는다

pub mod account;
pub mod greetd;

/// 처음 켤 때 계정을 만드는 시스템 버스 서비스 (buntd-setup). root와 로그인 화면 사용자만 부른다
/// (/usr/share/dbus-1/system.d/org.buntos.Setup1.conf)
pub mod setup {
    pub const BUS_NAME: &str = "org.buntos.Setup1";
    pub const OBJECT_PATH: &str = "/org/buntos/Setup1";
    pub const INTERFACE: &str = "org.buntos.Setup1";
    /// D-Bus 오류 이름의 앞부분. 뒤에 [`crate::account::Invalid::error_name`] 등이 붙는다
    pub const ERROR_PREFIX: &str = "org.buntos.Setup1.Error";
    /// 정규 계정이 이미 있어 처음 계정 만들기를 하지 않는다
    pub const ERROR_ACCOUNT_EXISTS: &str = "AccountExists";
    pub const ERROR_ACCESS_DENIED: &str = "AccessDenied";
    pub const ERROR_FAILED: &str = "Failed";
    /// 로그인 화면을 띄우는 사용자 (Debian greetd 패키지의 sysusers.d/greetd.conf)
    pub const GREETER_USER: &str = "_greetd";
}
