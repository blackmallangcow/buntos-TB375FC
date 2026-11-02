// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! PackageKit(org.freedesktop.PackageKit, 시스템 버스)로 .deb를 설치하고 지운다. 앱 스토어(GNOME Software)와 같은
//! 서비스를 쓴다. 권한은 PackageKit이 polkit으로 묻는다: 설치와 제거는 관리자 인증 창(bunt-shell)이 뜨고, 목록
//! 갱신과 미리 해 보기(SIMULATE)는 묻지 않는다.
//!
//! 일 하나가 트랜잭션 하나다: CreateTransaction으로 객체를 받아 신호를 듣고, 메서드 하나를 부르고, Finished를
//! 기다린다. 수는 pk-enum.h(packagekit 1.4)와 같다

use std::{cell::RefCell, rc::Rc};

use gtk4::{gio, glib, prelude::*};

const NAME: &str = "org.freedesktop.PackageKit";
const PATH: &str = "/org/freedesktop/PackageKit";
const MANAGER: &str = "org.freedesktop.PackageKit";
const TRANSACTION: &str = "org.freedesktop.PackageKit.Transaction";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// PkBitfield: 1 << PK_TRANSACTION_FLAG_ENUM_*
pub const FLAG_SIMULATE: u64 = 1 << 2;
pub const FLAG_ALLOW_REINSTALL: u64 = 1 << 4;
pub const FLAG_ALLOW_DOWNGRADE: u64 = 1 << 6;
/// PkBitfield: 1 << PK_FILTER_ENUM_INSTALLED
const FILTER_INSTALLED: u64 = 1 << 2;

/// PK_EXIT_ENUM_SUCCESS
const EXIT_SUCCESS: u32 = 1;
/// PK_EXIT_ENUM_CANCELLED
const EXIT_CANCELLED: u32 = 3;
/// PK_EXIT_ENUM_NEED_UNTRUSTED: 서명한 저장소에 없는 패키지를 믿을지 물어야 한다
const EXIT_NEED_UNTRUSTED: u32 = 8;

/// PK_ERROR_ENUM_*. 사람에게 까닭을 알릴 수 있는 것만 적는다
pub mod error {
    pub const NO_NETWORK: u32 = 2;
    pub const PACKAGE_DOWNLOAD_FAILED: u32 = 10;
    pub const DEP_RESOLUTION_FAILED: u32 = 13;
    pub const TRANSACTION_CANCELLED: u32 = 17;
    pub const CANNOT_REMOVE_SYSTEM_PACKAGE: u32 = 20;
    pub const CANNOT_GET_LOCK: u32 = 26;
    pub const FILE_CONFLICTS: u32 = 35;
    pub const PACKAGE_CONFLICTS: u32 = 36;
    pub const REPO_NOT_AVAILABLE: u32 = 37;
    pub const INVALID_PACKAGE_FILE: u32 = 38;
    pub const PACKAGE_CORRUPT: u32 = 40;
    pub const ALL_PACKAGES_ALREADY_INSTALLED: u32 = 41;
    pub const NO_MORE_MIRRORS_TO_TRY: u32 = 43;
    pub const INCOMPATIBLE_ARCHITECTURE: u32 = 45;
    pub const NO_SPACE_ON_DEVICE: u32 = 46;
    pub const NOT_AUTHORIZED: u32 = 48;
    pub const CANNOT_FETCH_SOURCES: u32 = 71;
}

/// PK_INFO_ENUM_*: 미리 해 보기에서 패키지마다 오는 할 일
pub mod info {
    pub const INSTALLED: u32 = 1;
    pub const UPDATING: u32 = 11;
    pub const INSTALLING: u32 = 12;
    pub const REMOVING: u32 = 13;
    pub const OBSOLETING: u32 = 15;
    pub const REINSTALLING: u32 = 19;
    pub const DOWNGRADING: u32 = 20;
    pub const INSTALL: u32 = 27;
    pub const REMOVE: u32 = 28;
    pub const OBSOLETE: u32 = 29;
    pub const DOWNGRADE: u32 = 30;
}

/// PK_STATUS_ENUM_*: 진행 상황의 단계
pub mod status {
    pub const WAIT: u32 = 1;
    pub const SETUP: u32 = 2;
    pub const QUERY: u32 = 4;
    pub const REMOVE: u32 = 6;
    pub const REFRESH_CACHE: u32 = 7;
    pub const DOWNLOAD: u32 = 8;
    pub const INSTALL: u32 = 9;
    pub const UPDATE: u32 = 10;
    pub const CLEANUP: u32 = 11;
    pub const DEP_RESOLVE: u32 = 13;
    pub const SIG_CHECK: u32 = 14;
    pub const COMMIT: u32 = 16;
    pub const DOWNLOAD_REPOSITORY: u32 = 20;
    pub const DOWNLOAD_PACKAGELIST: u32 = 21;
    pub const LOADING_CACHE: u32 = 27;
    pub const WAITING_FOR_LOCK: u32 = 30;
    pub const WAITING_FOR_AUTH: u32 = 31;
}

/// 일이 실패한 까닭
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// PK_ERROR_ENUM_*. 서비스에 닿지 못했으면 None
    pub code: Option<u32>,
    pub details: String,
}

impl Failure {
    fn other(details: impl Into<String>) -> Self {
        Self {
            code: None,
            details: details.into(),
        }
    }

    /// 사람이 인증 창을 닫았거나 일을 취소했다. 실패로 알리지 않는다
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self.code,
            Some(error::NOT_AUTHORIZED | error::TRANSACTION_CANCELLED)
        )
    }
}

/// 미리 해 보기나 설치에서 패키지 하나에 온 알림
#[derive(Debug, Clone, PartialEq)]
pub struct PackageLine {
    pub info: u32,
    /// name;version;arch;data
    pub id: String,
    pub summary: String,
}

impl PackageLine {
    pub fn name(&self) -> &str {
        package_name(&self.id)
    }
}

/// 패키지 ID(name;version;arch;data)에서 이름
pub fn package_name(id: &str) -> &str {
    id.split(';').next().unwrap_or(id)
}

/// 진행 상황 (Percentage의 101은 모른다는 뜻이다)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub status: u32,
    pub percentage: Option<u32>,
}

enum Event {
    Package(PackageLine),
    Error(u32, String),
    Finished(u32),
}

/// 일 하나를 끝까지 돌린다. 진행 상황은 on_progress로, 패키지 알림은 돌려준 값으로 받는다
async fn run(
    method: &'static str,
    arguments: glib::Variant,
    on_progress: Rc<dyn Fn(Progress)>,
) -> Result<Vec<PackageLine>, Failure> {
    let bus = gio::bus_get_future(gio::BusType::System)
        .await
        .map_err(|err| Failure::other(err.message()))?;
    let reply = bus
        .call_future(
            Some(NAME),
            PATH,
            MANAGER,
            "CreateTransaction",
            None,
            Some(glib::VariantTy::new("(o)").expect("valid type")),
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .map_err(|err| Failure::other(err.message()))?;
    let path = reply
        .child_value(0)
        .str()
        .ok_or_else(|| Failure::other("no transaction path"))?
        .to_owned();

    let (sender, receiver) = async_channel::unbounded();
    let on_signal = sender.clone();
    let _transaction_signals = bus.subscribe_to_signal(
        Some(NAME),
        Some(TRANSACTION),
        None,
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if let Some(event) = parse_signal(signal.signal_name, signal.parameters) {
                let _ = on_signal.try_send(event);
            }
        },
    );
    let progress = Rc::new(RefCell::new(Progress {
        status: 0,
        percentage: None,
    }));
    let shown = progress.clone();
    let report = on_progress.clone();
    let _property_signals = bus.subscribe_to_signal(
        Some(NAME),
        Some(PROPERTIES),
        Some("PropertiesChanged"),
        Some(&path),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let changed = signal.parameters.child_value(1);
            let dict = glib::VariantDict::new(Some(&changed));
            let mut progress = shown.borrow_mut();
            let mut updated = false;
            if let Some(value) = dict
                .lookup_value("Status", None)
                .and_then(|v| v.get::<u32>())
            {
                progress.status = value;
                updated = true;
            }
            if let Some(value) = dict
                .lookup_value("Percentage", None)
                .and_then(|v| v.get::<u32>())
            {
                progress.percentage = (value <= 100).then_some(value);
                updated = true;
            }
            if updated {
                report(*progress);
            }
        },
    );

    // 언어와 대화형을 먼저 알린다. 실패해도 일에는 지장이 없다
    let hints = vec![
        format!(
            "locale={}",
            std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".to_owned())
        ),
        "interactive=true".to_owned(),
        "background=false".to_owned(),
    ];
    let _ = bus
        .call_future(
            Some(NAME),
            &path,
            TRANSACTION,
            "SetHints",
            Some(&(hints,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    // 관리자 인증을 기다릴 수 있으므로 제한 시간을 두지 않는다. 메서드는 일을 맡으면 바로 돌아온다
    bus.call_future(
        Some(NAME),
        &path,
        TRANSACTION,
        method,
        Some(&arguments),
        None,
        gio::DBusCallFlags::ALLOW_INTERACTIVE_AUTHORIZATION,
        i32::MAX,
    )
    .await
    .map_err(|err| Failure::other(err.message()))?;

    let mut packages = Vec::new();
    let mut failure = None;
    while let Ok(event) = receiver.recv().await {
        match event {
            Event::Package(line) => packages.push(line),
            Event::Error(code, details) => {
                // 뒤의 오류는 앞의 오류에서 이어진 것이다
                failure.get_or_insert(Failure {
                    code: Some(code),
                    details,
                });
            }
            Event::Finished(exit) => {
                return match (exit, failure) {
                    (EXIT_SUCCESS, _) => Ok(packages),
                    (_, Some(failure)) => Err(failure),
                    (EXIT_CANCELLED, None) => Err(Failure {
                        code: Some(error::TRANSACTION_CANCELLED),
                        details: String::new(),
                    }),
                    (EXIT_NEED_UNTRUSTED, None) => {
                        Err(Failure::other("the package is not trusted"))
                    }
                    (exit, None) => Err(Failure::other(format!(
                        "the transaction ended with status {exit}"
                    ))),
                };
            }
        }
    }
    Err(Failure::other("PackageKit went away"))
}

fn parse_signal(name: &str, parameters: &glib::Variant) -> Option<Event> {
    match name {
        "Package" => {
            let (info, id, summary) = parameters.get::<(u32, String, String)>()?;
            Some(Event::Package(PackageLine { info, id, summary }))
        }
        "ErrorCode" => {
            let (code, details) = parameters.get::<(u32, String)>()?;
            Some(Event::Error(code, details))
        }
        "Finished" => {
            let (exit, _runtime) = parameters.get::<(u32, u32)>()?;
            Some(Event::Finished(exit))
        }
        _ => None,
    }
}

/// apt 패키지 목록을 받는다. 묻지 않고 된다
pub async fn refresh_cache(on_progress: Rc<dyn Fn(Progress)>) -> Result<(), Failure> {
    run("RefreshCache", (false,).to_variant(), on_progress)
        .await
        .map(|_| ())
}

/// .deb를 설치한다. FLAG_SIMULATE를 주면 하지 않고 할 일만 알려 준다
pub async fn install_files(
    flags: u64,
    path: &str,
    on_progress: Rc<dyn Fn(Progress)>,
) -> Result<Vec<PackageLine>, Failure> {
    let files = vec![path.to_owned()];
    run("InstallFiles", (flags, files).to_variant(), on_progress).await
}

/// 패키지를 지운다. allow_deps면 이 패키지에 기대는 패키지도 함께 지운다. autoremove면 이것 때문에 들어온 의존성도
/// 지운다. FLAG_SIMULATE를 주면 지우지 않고 할 일만 알려 준다
pub async fn remove_packages(
    flags: u64,
    ids: Vec<String>,
    allow_deps: bool,
    autoremove: bool,
    on_progress: Rc<dyn Fn(Progress)>,
) -> Result<Vec<PackageLine>, Failure> {
    run(
        "RemovePackages",
        (flags, ids, allow_deps, autoremove).to_variant(),
        on_progress,
    )
    .await
}

/// 설치된 패키지의 패키지 ID. 없으면 빈 목록이다
pub async fn resolve_installed(names: &[String]) -> Result<Vec<String>, Failure> {
    let lines = run(
        "Resolve",
        (FILTER_INSTALLED, names.to_vec()).to_variant(),
        Rc::new(|_| {}),
    )
    .await?;
    Ok(lines
        .into_iter()
        .filter(|line| line.info == info::INSTALLED)
        .map(|line| line.id)
        .collect())
}

/// 미리 해 보기의 알림을 사람에게 보일 할 일로 나눈다
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Plan {
    pub install: Vec<String>,
    pub update: Vec<String>,
    pub remove: Vec<String>,
}

pub fn plan(lines: &[PackageLine]) -> Plan {
    let mut plan = Plan::default();
    for line in lines {
        let list = match line.info {
            info::INSTALLING | info::INSTALL | info::REINSTALLING => &mut plan.install,
            info::UPDATING | info::DOWNGRADING | info::DOWNGRADE => &mut plan.update,
            info::REMOVING | info::REMOVE | info::OBSOLETING | info::OBSOLETE => &mut plan.remove,
            _ => continue,
        };
        let name = line.name().to_owned();
        if !list.contains(&name) {
            list.push(name);
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(info: u32, id: &str) -> PackageLine {
        PackageLine {
            info,
            id: id.to_owned(),
            summary: String::new(),
        }
    }

    #[test]
    fn names_come_from_package_ids() {
        assert_eq!(package_name("hello;1.0;arm64;installed"), "hello");
        assert_eq!(package_name("hello"), "hello");
    }

    #[test]
    fn a_plan_groups_what_will_happen() {
        let plan = plan(&[
            line(info::INSTALLING, "app;1;arm64;local"),
            line(info::INSTALLING, "libfoo;2;arm64;debian"),
            line(info::INSTALL, "libfoo;2;arm64;debian"),
            line(info::UPDATING, "libc6;3;arm64;debian"),
            line(info::REMOVING, "old;1;arm64;installed"),
            line(info::INSTALLED, "other;1;arm64;installed"),
        ]);
        assert_eq!(plan.install, ["app", "libfoo"]);
        assert_eq!(plan.update, ["libc6"]);
        assert_eq!(plan.remove, ["old"]);
    }

    #[test]
    fn a_closed_authentication_window_is_not_a_failure() {
        let closed = Failure {
            code: Some(error::NOT_AUTHORIZED),
            details: String::new(),
        };
        assert!(closed.is_cancelled());
        let failed = Failure {
            code: Some(error::NO_SPACE_ON_DEVICE),
            details: String::new(),
        };
        assert!(!failed.is_cancelled());
        assert!(!Failure::other("x").is_cancelled());
    }
}
