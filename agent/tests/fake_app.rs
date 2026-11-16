// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 가짜 접근성 앱을 사설 D-Bus(dbus-daemon)에 띄우고 데몬의 접근성 코드(bunt_agent::atspi)로 읽어 본다. 데스크톱
//! 없이 돈다. dbus-daemon이 없으면 건너뛴다.
//!
//! 이슈 #1 P0: at-spi2-core 2.62는 Value 인터페이스의 Properties.GetAll을 받다 속성 하나를 못 읽으면 앱을
//! abort시킨다(Firefox가 이렇게 끝났다). 가짜 앱은 Value에 GetAll이 오면 적어 두고, 시험은 그런 호출이 하나라도
//! 왔으면 실패한다. 가짜 앱은 버스의 org.a11y.Bus도 맡아 접근성 버스로 자기 버스를 알려 준다.
//!
//! 환경 변수(DBUS_SESSION_BUS_ADDRESS)를 바꾸므로 이 파일에는 시험을 하나만 둔다.

use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use bunt_agent::{
    atspi::{self, A11y},
    tree::{Item, Range, iface, role},
};
use futures_util::StreamExt;
use zbus::{message::Type, zvariant::Value};

const VALUE: &str = "org.a11y.atspi.Value";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// 값을 다 아는 막대 (`<input type="range">`)
const SLIDER: &str = "/org/a11y/atspi/accessible/slider";
/// 값을 모르는 진행 막대 (값 없는 `role="progressbar"`). Firefox는 NaN을 주고, at-spi2-core는 CurrentValue와
/// MaximumValue는 그래도 숫자로 답하지만 MinimumValue는 Get에 오류로 답한다(GetAll이면 앱이 끝난다)
const UNKNOWN: &str = "/org/a11y/atspi/accessible/progress";

/// 가짜 앱이 받은 호출: (경로, 인터페이스, 메서드, 인자)
type Received = Arc<Mutex<Vec<(String, String, String, String)>>>;

/// 시험 디렉터리의 사설 세션 버스
struct Bus {
    child: Child,
    dir: PathBuf,
    address: String,
}

impl Bus {
    fn start() -> Option<Bus> {
        let dir = std::env::temp_dir().join(format!("bunt-fake-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let spawned = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address"])
            .arg(format!("--address=unix:path={}", dir.join("bus").display()))
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(err) => {
                eprintln!("skipped: cannot start dbus-daemon: {err}");
                let _ = std::fs::remove_dir_all(&dir);
                return None;
            }
        };
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Some(Bus {
            child,
            dir,
            address: address.trim().to_string(),
        })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// 가짜 앱: 받은 호출을 적고 답한다. 준비되면 자기 버스 이름을 보낸다
fn fake_app(address: String, received: Received, ready: mpsc::Sender<String>) {
    async_io::block_on(async move {
        let connection = zbus::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        // ObjectServer를 쓰지 않는다: 모든 호출에 이 함수가 답한다
        connection.request_name("org.a11y.Bus").await.unwrap();
        let mut messages = zbus::MessageStream::from(&connection);
        ready
            .send(connection.unique_name().unwrap().to_string())
            .unwrap();
        while let Some(Ok(message)) = messages.next().await {
            let header = message.header();
            if header.message_type() != Type::MethodCall {
                continue;
            }
            let text = |part: Option<String>| part.unwrap_or_default();
            let path = text(header.path().map(|path| path.to_string()));
            let interface = text(header.interface().map(|name| name.to_string()));
            let method = text(header.member().map(|name| name.to_string()));
            let body = message.body();
            let (arguments, number) = match (interface.as_str(), method.as_str()) {
                (PROPERTIES, "Get") => {
                    let (wanted, property): (String, String) = body.deserialize().unwrap();
                    let number = match (path.as_str(), property.as_str()) {
                        (SLIDER, "CurrentValue") => Some(0.4),
                        (SLIDER, "MinimumValue") => Some(0.0),
                        (SLIDER, "MaximumValue") => Some(1.0),
                        (UNKNOWN, "CurrentValue") => Some(0.0),
                        (UNKNOWN, "MaximumValue") => Some(100.0),
                        _ => None,
                    };
                    (format!("{wanted}, {property}"), number)
                }
                (PROPERTIES, "GetAll") => {
                    let (wanted,): (String,) = body.deserialize().unwrap();
                    (wanted, None)
                }
                _ => (String::new(), None),
            };
            received.lock().unwrap().push((
                path.clone(),
                interface.clone(),
                method.clone(),
                arguments.clone(),
            ));
            let replied = match (interface.as_str(), method.as_str()) {
                ("org.a11y.Bus", "GetAddress") => {
                    connection.reply(&header, &(address.as_str(),)).await
                }
                // 접근성을 켜는 호출 (org.a11y.Status.IsEnabled)
                (PROPERTIES, "Set") => connection.reply(&header, &()).await,
                (PROPERTIES, "Get") => match number {
                    Some(number) => connection.reply(&header, &Value::from(number)).await,
                    // at-spi2-core의 droute.c: 읽지 못한 속성의 Get은 오류로 답한다
                    None => {
                        connection
                            .reply_error(
                                &header,
                                "org.freedesktop.DBus.Error.Failed",
                                &("Get failed",),
                            )
                            .await
                    }
                },
                _ => {
                    connection
                        .reply_error(
                            &header,
                            "org.freedesktop.DBus.Error.UnknownMethod",
                            &(format!("the fake app has no {interface}.{method}"),),
                        )
                        .await
                }
            };
            replied.unwrap();
        }
    });
}

#[test]
fn values_are_read_one_property_at_a_time() {
    let Some(bus) = Bus::start() else {
        return;
    };
    // 안전: 이 프로그램의 시험은 이것 하나이고, 환경을 읽는 다른 스레드가 뜨기 전에 바꾼다
    unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &bus.address) };
    let received = Received::default();
    let (ready, app) = mpsc::channel();
    {
        let (address, received) = (bus.address.clone(), received.clone());
        std::thread::spawn(move || fake_app(address, received, ready));
    }
    let app = app.recv_timeout(Duration::from_secs(10)).unwrap();
    let a11y = async_io::block_on(A11y::connect()).unwrap();
    let mut items = vec![
        Item {
            path: SLIDER.into(),
            role: role::SLIDER,
            interfaces: iface::VALUE,
            ..Item::default()
        },
        Item {
            path: UNKNOWN.into(),
            role: role::PROGRESS_BAR,
            interfaces: iface::VALUE,
            ..Item::default()
        },
    ];
    async_io::block_on(a11y.enrich(&app, &mut items, vec![0, 1]));

    let received = received.lock().unwrap().clone();
    let get_all_on_value: Vec<_> = received
        .iter()
        .filter(|(_, interface, method, arguments)| {
            interface == PROPERTIES && method == "GetAll" && arguments == VALUE
        })
        .collect();
    assert!(
        get_all_on_value.is_empty(),
        "Properties.GetAll on {VALUE} aborts apps with at-spi2-core 2.62: {get_all_on_value:?}"
    );
    assert_eq!(
        items[0].value,
        Some(Range {
            current: 0.4,
            min: 0.0,
            max: 1.0
        })
    );
    // MinimumValue를 읽지 못해도 앱은 남고 나머지 값은 읽힌다
    assert_eq!(
        items[1].value,
        Some(Range {
            current: 0.0,
            min: 0.0,
            max: 100.0
        })
    );
    let gets = |path: &str| {
        let mut properties: Vec<&str> = received
            .iter()
            .filter(|(at, interface, method, _)| {
                at == path && interface == PROPERTIES && method == "Get"
            })
            .map(|(_, _, _, arguments)| arguments.as_str())
            .collect();
        properties.sort_unstable();
        properties
    };
    let expected = [
        "org.a11y.atspi.Value, CurrentValue",
        "org.a11y.atspi.Value, MaximumValue",
        "org.a11y.atspi.Value, MinimumValue",
    ];
    assert_eq!(gets(SLIDER), expected);
    assert_eq!(gets(UNKNOWN), expected);

    // 호출 기록 (이슈 #1 P1-3): 버스, 경로, 인터페이스, 메서드, 인자, 오류가 남는다
    let calls: Vec<atspi::Call> = atspi::recent_calls()
        .into_iter()
        .filter(|call| call.destination == app)
        .collect();
    assert_eq!(calls.len(), 6, "{calls:?}");
    let failed: Vec<&atspi::Call> = calls.iter().filter(|call| call.error.is_some()).collect();
    assert_eq!(failed.len(), 1, "{calls:?}");
    assert_eq!(failed[0].path, UNKNOWN);
    assert_eq!(failed[0].interface, PROPERTIES);
    assert_eq!(failed[0].method, "Get");
    assert_eq!(failed[0].arguments, "org.a11y.atspi.Value, MinimumValue");
    assert!(
        failed[0]
            .error
            .as_deref()
            .unwrap()
            .contains("org.freedesktop.DBus.Error.Failed"),
        "{failed:?}"
    );
    // 덤프는 로그만 쓴다
    atspi::dump_calls(Some(&app), "the fake app went away");
}
