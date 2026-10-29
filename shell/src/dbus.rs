// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! D-Bus 도우미. 메뉴 막대 항목이 시스템 서비스(UPower, BlueZ, NetworkManager)와 세션
//! 서비스(fcitx5)의 속성을 지켜보고 바꾸는 데 쓴다.

use std::{cell::Cell, rc::Rc};

use gtk4::{gio, glib, prelude::*};

/// 서비스의 객체 하나를 지켜본다. 처음 연결되면 한 번, 그 뒤로는 속성이 바뀌거나 서비스가
/// 켜지고 꺼질 때마다 changed를 부른다. 서비스가 꺼져 있으면 프록시의 name_owner()가 None이다.
/// 프록시는 셸이 끝날 때까지 산다.
pub fn watch(
    bus: gio::BusType,
    name: &str,
    path: &str,
    interface: &str,
    changed: impl Fn(&gio::DBusProxy) + 'static,
) {
    let service = name.to_owned();
    gio::DBusProxy::for_bus(
        bus,
        gio::DBusProxyFlags::NONE,
        None,
        name,
        path,
        interface,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(proxy) => {
                let changed = Rc::new(changed);
                let on_change = changed.clone();
                // 처리기가 프록시를 붙들어 프록시가 사라지지 않게 한다
                let keep = proxy.clone();
                proxy.connect_local("g-properties-changed", false, move |_| {
                    on_change(&keep);
                    None
                });
                let on_owner = changed.clone();
                proxy.connect_notify_local(Some("g-name-owner"), move |proxy, _| on_owner(proxy));
                changed(&proxy);
            }
            Err(err) => eprintln!("bunt-shell: {service}: {err}"),
        },
    );
}

/// 버스 연결에서 서비스 이름을 셸이 끝날 때까지 지켜본다. 서비스가 (다시) 뜨면 appeared를, 꺼지거나
/// 연결이 닫히면 vanished를 부른다. 연결은 부르는 쪽이 gio::bus_get_future로 먼저 얻는다.
///
/// gio-rs의 이름 감시는 GLib이 사라짐 처리기에 연결 대신 NULL을 주면 unwrap에서 셸을 죽인다(gio 0.22의
/// vanished_closure). GLib은 버스에 붙지 못했을 때(gio::bus_watch_name)와 지켜보던 연결이 닫혔을 때
/// (버스 데몬이 끝나거나 다시 시작할 때) NULL을 준다. 앞의 것은 연결을 먼저 얻어 피하고, 뒤의 것은
/// 연결이 닫히면 GLib보다 먼저 감시를 멈춰 피한다. 공유 버스 연결은 닫히면 GLib이 셸을 SIGTERM으로
/// 끝낸다(exit-on-close).
pub fn watch_name(
    bus: &gio::DBusConnection,
    name: &str,
    appeared: impl Fn(gio::DBusConnection) + 'static,
    vanished: impl Fn() + 'static,
) {
    let vanished = Rc::new(vanished);
    let watcher = Rc::new(Cell::new(None));
    let (stop, on_close) = (watcher.clone(), vanished.clone());
    // 신호 처리기는 단 순서대로 불리므로, GLib이 감시를 시작하며 다는 closed 처리기보다 먼저 단다
    bus.connect_local("closed", false, move |_| {
        if let Some(id) = stop.take() {
            gio::bus_unwatch_name(id);
            on_close();
        }
        None
    });
    let id = gio::bus_watch_name_on_connection(
        bus,
        name,
        gio::BusNameWatcherFlags::NONE,
        move |connection, _, _| appeared(connection),
        move |_, _| vanished(),
    );
    watcher.set(Some(id));
}

/// 캐시된 속성 값
pub fn property<T: glib::variant::FromVariant>(proxy: &gio::DBusProxy, name: &str) -> Option<T> {
    proxy.cached_property(name)?.get()
}

/// 서비스가 떠 있는지
pub fn is_running(proxy: &gio::DBusProxy) -> bool {
    proxy.name_owner().is_some()
}

/// 속성을 바꾼다 (org.freedesktop.DBus.Properties.Set). 바뀐 값은 속성 변경 알림으로 돌아온다
pub fn set_property(proxy: &gio::DBusProxy, name: &str, value: glib::Variant) {
    let args = set_property_args(proxy.interface_name().as_str(), name, value);
    call(proxy, "org.freedesktop.DBus.Properties.Set", Some(&args));
}

/// Properties.Set의 인자 (ssv). 튜플의 to_variant가 값을 v로 한 번 감싼다. 예전에는 값을 미리
/// Variant::from_variant로 감싸 v 안에 v가 들어갔고, 서비스가 "Expected type 'b' but got 'v'"로
/// 거절했다(Bluetooth 켜기, Wi-Fi 켜기, 전원 모드)
fn set_property_args(interface: &str, name: &str, value: glib::Variant) -> glib::Variant {
    (interface, name, value).to_variant()
}

/// 메서드를 부르고 결과는 기다리지 않는다. 실패하면 기록만 남긴다
pub fn call(proxy: &gio::DBusProxy, method: &str, args: Option<&glib::Variant>) {
    let label = format!("{}.{method}", proxy.interface_name());
    proxy.call(
        method,
        args,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| {
            if let Err(err) = result {
                eprintln!("bunt-shell: {label}: {err}");
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        io::{BufRead, BufReader},
        path::PathBuf,
        process::{Child, Command, Stdio},
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Instant},
    };

    use super::*;

    const NAME: &str = "org.buntos.test.WatchName";
    /// 버스 데몬마다 소켓 폴더를 따로 둔다 (Drop이 폴더를 지운다)
    static NEXT_BUS: AtomicUsize = AtomicUsize::new(0);

    /// 시험 하나가 쓰는 버스 데몬. dbus-daemon이 설치되지 않았으면 None이다
    struct Bus {
        daemon: Child,
        dir: PathBuf,
        address: String,
    }

    impl Bus {
        fn start() -> Option<Self> {
            let dir = std::env::temp_dir().join(format!(
                "bunt-shell-test-{}-{}",
                std::process::id(),
                NEXT_BUS.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let spawned = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--print-address"])
                .arg(format!("--address=unix:dir={}", dir.display()))
                .stdout(Stdio::piped())
                .spawn();
            let mut daemon = match spawned {
                Ok(daemon) => daemon,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    let _ = std::fs::remove_dir_all(&dir);
                    return None;
                }
                Err(err) => panic!("dbus-daemon: {err}"),
            };
            let mut address = String::new();
            if let Some(stdout) = daemon.stdout.take() {
                let _ = BufReader::new(stdout).read_line(&mut address);
            }
            let bus = Self {
                daemon,
                dir,
                address: address.trim().to_owned(),
            };
            assert!(!bus.address.is_empty(), "dbus-daemon did not start");
            Some(bus)
        }

        /// 연결은 이것을 부른 스레드의 기본 컨텍스트로 알림(닫힘 포함)을 보낸다
        fn connect(&self) -> gio::DBusConnection {
            gio::DBusConnection::for_address_sync(
                &self.address,
                gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                    | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                None,
                gio::Cancellable::NONE,
            )
            .unwrap()
        }

        /// 데몬을 SIGKILL로 끝낸다. 이름 주인이 바뀐 것을 알리지 못한 채 연결이 닫힌다
        fn kill(&mut self) {
            self.daemon.kill().unwrap();
            self.daemon.wait().unwrap();
        }
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.daemon.kill();
            let _ = self.daemon.wait();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// org.freedesktop.DBus의 메서드 (RequestName, ReleaseName)
    fn call_bus(connection: &gio::DBusConnection, method: &str, args: glib::Variant) {
        connection
            .call_sync(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                method,
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
            )
            .unwrap();
    }

    #[test]
    fn watch_name_follows_restarts_and_survives_losing_the_bus() {
        let Some(mut bus) = Bus::start() else {
            eprintln!("dbus-daemon is not installed; skipping the name watch test");
            return;
        };
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (watcher, service) = (bus.connect(), bus.connect());
                let events = Rc::new(RefCell::new(Vec::new()));
                let (appeared, vanished) = (events.clone(), events.clone());
                watch_name(
                    &watcher,
                    NAME,
                    move |_| appeared.borrow_mut().push("appeared"),
                    move || vanished.borrow_mut().push("vanished"),
                );
                let wait_for = |expected: &[&str]| {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while events.borrow().as_slice() != expected {
                        assert!(
                            Instant::now() < deadline,
                            "{:?} != {expected:?}",
                            events.borrow()
                        );
                        if !context.iteration(false) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                };
                // 처음에는 서비스가 없다
                wait_for(&["vanished"]);
                call_bus(&service, "RequestName", (NAME, 0u32).to_variant());
                wait_for(&["vanished", "appeared"]);
                // 서비스가 다시 시작한다
                call_bus(&service, "ReleaseName", (NAME,).to_variant());
                wait_for(&["vanished", "appeared", "vanished"]);
                call_bus(&service, "RequestName", (NAME, 0u32).to_variant());
                wait_for(&["vanished", "appeared", "vanished", "appeared"]);
                // 서비스가 떠 있는 채로 버스 데몬이 갑자기 끝난다. GLib이 사라짐 처리기에 NULL을 주는
                // 때라, 감시를 먼저 멈추지 않으면 gio-rs의 unwrap이 시험 프로세스를 죽인다
                bus.kill();
                let all = ["vanished", "appeared", "vanished", "appeared", "vanished"];
                wait_for(&all);
                assert!(watcher.is_closed());
                while context.iteration(false) {}
                assert_eq!(events.borrow().as_slice(), all);
            })
            .unwrap();
    }

    #[test]
    fn property_values_are_wrapped_once() {
        let args = set_property_args("org.bluez.Adapter1", "Powered", true.to_variant());
        assert_eq!(args.type_().as_str(), "(ssv)");
        assert_eq!(
            args.print(true),
            "('org.bluez.Adapter1', 'Powered', <true>)"
        );
    }
}
