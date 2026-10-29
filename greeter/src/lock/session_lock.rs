// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! gtk4-layer-shell의 세션 잠금 API (GtkSessionLockInstance, ext-session-lock-v1). GTK 창을 화면마다의 잠금
//! 표면으로 만든다.
//!
//! 빌드 VM(Debian 13)의 gtk4-layer-shell은 1.0.4라 헤더에 이 API(1.1부터)가 없고, 이미지(Debian forky)의
//! 라이브러리는 1.3이다. 그래서 라이브러리는 gtk4-layer-shell 크레이트로 링크하고, 잠금 함수는 실행할 때 찾는다.
//! 링크는 꼭 해야 한다: 라이브러리의 libwayland-shim이 GTK의 Wayland 호출을 가로채 창을 잠금 표면으로 바꾸므로
//! libwayland보다 먼저 올라와 있어야 한다(실행 중에 dlopen하면 가로채지 못한다).
//! 빌드 VM에 1.2 이상이 생기면 이 파일 대신 gtk4-session-lock 크레이트(MIT)를 쓴다.

use std::ffi::{CStr, c_void};

use gtk4::{
    self as gtk, gdk,
    glib::{self, prelude::*, translate::*},
};

/// 쓰는 기능의 최소 판: `monitor` 신호와 창을 알아서 띄우는 것은 1.2부터다
const REQUIRED: (u32, u32) = (1, 2);

type GObjectPointer = *mut glib::gobject_ffi::GObject;
type IsSupported = unsafe extern "C" fn() -> glib::ffi::gboolean;
type InstanceNew = unsafe extern "C" fn() -> GObjectPointer;
type InstanceLock = unsafe extern "C" fn(GObjectPointer) -> glib::ffi::gboolean;
type InstanceUnlock = unsafe extern "C" fn(GObjectPointer);
type AssignWindow =
    unsafe extern "C" fn(GObjectPointer, *mut gtk::ffi::GtkWindow, *mut gdk::ffi::GdkMonitor);

/// 화면 잠금 하나 (GtkSessionLockInstance)
pub struct SessionLock {
    instance: glib::Object,
    lock: InstanceLock,
    unlock: InstanceUnlock,
    assign: AssignWindow,
}

impl SessionLock {
    /// 라이브러리가 세션 잠금을 지원하고 컴포지터가 이 프로세스에 잠금 프로토콜을 보일 때만 만든다. 컴포지터는
    /// 자기가 띄운 잠금 화면에만 보이므로 직접 실행하면 실패한다
    pub fn new() -> Result<Self, String> {
        let version = (
            gtk4_layer_shell::major_version(),
            gtk4_layer_shell::minor_version(),
        );
        if version < REQUIRED {
            return Err(format!(
                "gtk4-layer-shell {}.{} cannot lock the screen; {}.{} or later is needed",
                version.0, version.1, REQUIRED.0, REQUIRED.1
            ));
        }
        let supported: IsSupported = symbol(c"gtk_session_lock_is_supported")?;
        let new: InstanceNew = symbol(c"gtk_session_lock_instance_new")?;
        let lock = symbol(c"gtk_session_lock_instance_lock")?;
        let unlock = symbol(c"gtk_session_lock_instance_unlock")?;
        let assign = symbol(c"gtk_session_lock_instance_assign_window_to_monitor")?;
        // 안전: gtk4-session-lock.h의 gboolean gtk_session_lock_is_supported(void). GTK를 연 뒤에 부른다
        if unsafe { supported() } == glib::ffi::GFALSE {
            return Err(
                "the compositor does not let this program lock the screen (ext-session-lock-v1)"
                    .into(),
            );
        }
        // 안전: GtkSessionLockInstance* gtk_session_lock_instance_new(void)는 새 참조를 넘긴다(transfer full)
        let instance: glib::Object = unsafe { from_glib_full(new()) };
        Ok(Self {
            instance,
            lock,
            unlock,
            assign,
        })
    }

    /// 화면을 잠근다. 결과는 `locked`나 `failed` 신호로 온다. 바로 실패하면 false
    pub fn lock(&self) -> bool {
        // 안전: gboolean gtk_session_lock_instance_lock(GtkSessionLockInstance*). 인스턴스는 self가 붙든다
        unsafe { (self.lock)(self.instance.as_ptr()) != glib::ffi::GFALSE }
    }

    /// 잠금을 푼다 (unlock_and_destroy). 사람이 친 암호를 PAM이 확인한 뒤에만 부른다
    pub fn unlock(&self) {
        // 안전: void gtk_session_lock_instance_unlock(GtkSessionLockInstance*)
        unsafe { (self.unlock)(self.instance.as_ptr()) }
    }

    /// 창을 그 화면의 잠금 표면으로 띄운다. 창은 아직 띄우지 않은(realize하지 않은) 것이어야 하고, 잠금이
    /// 끝나거나 화면이 없어지면 라이브러리가 닫는다
    pub fn assign_window_to_monitor(&self, window: &impl IsA<gtk::Window>, monitor: &gdk::Monitor) {
        // 안전: void gtk_session_lock_instance_assign_window_to_monitor(GtkSessionLockInstance*, GtkWindow*,
        // GdkMonitor*). 두 객체는 부르는 동안 살아 있다(빌린 참조, transfer none)
        unsafe {
            (self.assign)(
                self.instance.as_ptr(),
                window.as_ref().to_glib_none().0,
                monitor.to_glib_none().0,
            );
        }
    }

    /// 잠그는 때와 잠금 중에 생긴 화면마다 한 번 (`monitor` 신호)
    pub fn connect_monitor(&self, handler: impl Fn(&gdk::Monitor) + 'static) {
        self.instance
            .connect_local("monitor", false, move |values| {
                if let Some(Ok(monitor)) = values.get(1).map(|value| value.get::<gdk::Monitor>()) {
                    handler(&monitor);
                }
                None
            });
    }

    /// 컴포지터가 잠겼다고 알렸다
    pub fn connect_locked(&self, handler: impl Fn() + 'static) {
        self.connect_plain("locked", handler);
    }

    /// 컴포지터가 잠금을 받지 않았다
    pub fn connect_failed(&self, handler: impl Fn() + 'static) {
        self.connect_plain("failed", handler);
    }

    /// 잠금이 풀렸다
    pub fn connect_unlocked(&self, handler: impl Fn() + 'static) {
        self.connect_plain("unlocked", handler);
    }

    fn connect_plain(&self, signal: &str, handler: impl Fn() + 'static) {
        self.instance.connect_local(signal, false, move |_| {
            handler();
            None
        });
    }
}

/// libgtk4-layer-shell의 함수를 찾는다. `T`는 그 함수의 C 형식(gtk4-session-lock.h)이어야 한다
fn symbol<T: Copy>(name: &CStr) -> Result<T, String> {
    // 안전: dlsym은 이름을 읽기만 한다. RTLD_DEFAULT는 이미 올라온 라이브러리(링크한 libgtk4-layer-shell)에서 찾는다
    let pointer: *mut c_void = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    if pointer.is_null() {
        return Err(format!(
            "libgtk4-layer-shell has no {}; version 1.2 or later is needed",
            name.to_string_lossy()
        ));
    }
    assert_eq!(size_of::<T>(), size_of::<*mut c_void>());
    // 안전: 함수 포인터와 데이터 포인터는 이 플랫폼에서 크기가 같고(위에서 확인), 부르는 쪽이 T를 그 함수의
    // 시그니처로 정한다
    Ok(unsafe { std::mem::transmute_copy::<*mut c_void, T>(&pointer) })
}
