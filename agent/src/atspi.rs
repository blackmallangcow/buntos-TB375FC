// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 접근성 버스(AT-SPI) 읽기. 앱마다 GTK가 지닌 접근성 객체 목록(Cache.GetItems)을 한 번에 읽는다.
//! GTK4는 누군가 GetChildren으로 물어본 객체만 만들기 때문에, 목록에서 알려진 자식 수보다 자식이
//! 적은 객체는 GetChildren으로 아래를 채우고 목록을 다시 읽는다. 한 번 채운 부모에 새로 붙는
//! 위젯은 GTK가 바로 만들어 두므로, 같은 앱을 다시 읽을 때는 대개 목록 한 번으로 끝난다.

use std::{
    collections::{HashMap, HashSet},
    pin::pin,
    time::Duration,
};

use async_io::Timer;
use futures_util::future::{Either, join, join_all, join3, select};
use serde::{Serialize, de::DeserializeOwned};
use zbus::{
    Connection, fdo,
    names::BusName,
    zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Type},
};

use crate::tree::{Item, Range, Tree, iface};

const REGISTRY: &str = "org.a11y.atspi.Registry";
/// 앱 루트 객체의 경로
pub const ROOT: &str = "/org/a11y/atspi/accessible/root";
const CACHE: &str = "/org/a11y/atspi/cache";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// 한 번에 보내는 호출 수
const BATCH: usize = 64;
/// 목록을 다시 읽는 횟수 상한
const ROUNDS: usize = 8;
/// 한 앱에서 읽는 객체 수 상한
const MAX_ITEMS: usize = 20_000;
/// 앱이 호출 하나에 답하기를 기다리는 시간
const CALL_TIMEOUT: Duration = Duration::from_secs(3);

/// (버스 이름, 객체 경로)
type ObjectRef = (String, OwnedObjectPath);
/// Cache.GetItems 항목: 객체, 앱, 부모, 부모 안의 번호, 자식 수, 인터페이스, 이름, 역할, 설명, 상태
type RawItem = (
    ObjectRef,
    ObjectRef,
    ObjectRef,
    i32,
    i32,
    Vec<String>,
    String,
    u32,
    String,
    Vec<u32>,
);

/// 접근성 버스에 붙은 앱
pub struct AppRef {
    pub bus: String,
    pub name: String,
    pub pid: Option<u32>,
    pub windows: Vec<WindowRef>,
}

pub struct WindowRef {
    pub path: String,
    pub title: String,
    pub states: u64,
}

#[derive(Clone)]
pub struct A11y {
    /// 접근성 버스
    bus: Connection,
    /// 세션 버스 (컴포지터 API)
    session: Connection,
}

impl A11y {
    /// 세션 버스에서 접근성 버스의 주소를 받아 붙는다. 접근성 버스는 이때 뜬다(at-spi2-core)
    pub async fn connect() -> zbus::Result<Self> {
        let session = Connection::session().await?;
        let reply = session
            .call_method(
                Some("org.a11y.Bus"),
                "/org/a11y/bus",
                Some("org.a11y.Bus"),
                "GetAddress",
                &(),
            )
            .await?;
        let address: String = reply.body().deserialize()?;
        let bus = zbus::connection::Builder::address(address.as_str())?
            .build()
            .await?;
        Ok(Self { bus, session })
    }

    /// 컴포지터가 키보드 포커스를 준 클라이언트의 pid (org.buntos.Compositor1.GetFocus).
    /// 컴포지터 API가 없거나 포커스가 없으면 None
    pub async fn focused_pid(&self) -> Option<u32> {
        let (pid, ..): (u32, String, String, bool) = call_on(
            &self.session,
            "org.buntos.Compositor",
            "/org/buntos/Compositor",
            "org.buntos.Compositor1",
            "GetFocus",
            &(),
        )
        .await
        .ok()?;
        (pid != 0).then_some(pid)
    }

    async fn call<B, R>(
        &self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> zbus::Result<R>
    where
        B: Serialize + DynamicType,
        R: DeserializeOwned + Type,
    {
        call_on(&self.bus, destination, path, interface, method, body).await
    }

    /// 앱 목록과 그 창 (이름, 상태)
    pub async fn apps(&self) -> zbus::Result<Vec<AppRef>> {
        let apps: Vec<ObjectRef> = self
            .call(REGISTRY, ROOT, ACCESSIBLE, "GetChildren", &())
            .await?;
        Ok(join_all(apps.iter().map(|(bus, _)| self.app(bus))).await)
    }

    async fn app(&self, bus: &str) -> AppRef {
        let (name, windows, pid) = join3(
            self.name(bus, ROOT),
            self.children(bus, ROOT),
            self.pid(bus),
        )
        .await;
        let windows = join_all(
            windows
                .iter()
                .filter(|(window_bus, _)| window_bus == bus)
                .map(|(_, path)| async move {
                    let (title, states) = join(self.name(bus, path), self.states(bus, path)).await;
                    WindowRef {
                        path: path.as_str().to_string(),
                        title,
                        states,
                    }
                }),
        )
        .await;
        AppRef {
            bus: bus.to_string(),
            name,
            pid,
            windows,
        }
    }

    async fn name(&self, bus: &str, path: &str) -> String {
        self.call::<_, OwnedValue>(bus, path, PROPERTIES, "Get", &(ACCESSIBLE, "Name"))
            .await
            .ok()
            .and_then(|value| String::try_from(value).ok())
            .unwrap_or_default()
    }

    async fn states(&self, bus: &str, path: &str) -> u64 {
        self.call::<_, Vec<u32>>(bus, path, ACCESSIBLE, "GetState", &())
            .await
            .map_or(0, |words| states(&words))
    }

    async fn children(&self, bus: &str, path: &str) -> Vec<ObjectRef> {
        self.call(bus, path, ACCESSIBLE, "GetChildren", &())
            .await
            .unwrap_or_default()
    }

    pub async fn pid(&self, bus: &str) -> Option<u32> {
        let dbus = fdo::DBusProxy::new(&self.bus).await.ok()?;
        dbus.get_connection_unix_process_id(BusName::try_from(bus).ok()?)
            .await
            .ok()
    }

    /// GTK가 지금 지닌 접근성 객체 목록
    async fn items(&self, bus: &str) -> zbus::Result<Vec<Item>> {
        let raw: Vec<RawItem> = self
            .call(bus, CACHE, "org.a11y.atspi.Cache", "GetItems", &())
            .await?;
        Ok(raw
            .into_iter()
            .filter(|(object, ..)| object.0 == bus)
            .map(
                |(
                    object,
                    _,
                    parent,
                    index,
                    child_count,
                    interfaces,
                    name,
                    role,
                    description,
                    words,
                )| {
                    Item {
                        path: object.1.as_str().to_string(),
                        parent: if parent.0 == bus {
                            parent.1.as_str().to_string()
                        } else {
                            String::new()
                        },
                        index,
                        child_count,
                        interfaces: iface::from_names(interfaces.iter().map(String::as_str)),
                        name,
                        role,
                        description,
                        states: states(&words),
                        value: None,
                        text: None,
                        placeholder: None,
                        level: None,
                    }
                },
            )
            .collect())
    }

    /// 앱의 접근성 객체를 모두 읽는다
    pub async fn tree(&self, bus: &str) -> zbus::Result<Vec<Item>> {
        // 루트는 자식 수를 0으로 알리므로 창은 늘 직접 물어본다
        self.children(bus, ROOT).await;
        let mut items = self.items(bus).await?;
        for _ in 0..ROUNDS {
            let missing: Vec<String> = Tree::new(&items)
                .missing()
                .into_iter()
                .map(String::from)
                .collect();
            if missing.is_empty() || items.len() >= MAX_ITEMS {
                break;
            }
            let known = items.iter().map(|item| item.path.clone()).collect();
            self.realize(bus, missing, known).await;
            items = self.items(bus).await?;
        }
        Ok(items)
    }

    /// `start` 아래를 GetChildren으로 물어 GTK가 접근성 객체를 만들게 한다. 처음 보는 자식은 그
    /// 아래까지 내려간다 (한 층씩 한꺼번에 보낸다)
    async fn realize(&self, bus: &str, start: Vec<String>, mut known: HashSet<String>) {
        let mut frontier = start;
        while !frontier.is_empty() && known.len() < MAX_ITEMS {
            let mut next = Vec::new();
            for batch in frontier.chunks(BATCH) {
                for children in join_all(batch.iter().map(|path| self.children(bus, path))).await {
                    for (child_bus, child_path) in children {
                        let path = child_path.as_str().to_string();
                        if child_bus == bus && known.insert(path.clone()) {
                            next.push(path);
                        }
                    }
                }
            }
            frontier = next;
        }
    }

    /// `wanted` 번째 객체의 값(막대, 숫자 칸), 내용(글자 칸, 이름 없는 글), 속성(안내 글, 제목
    /// 단계)을 읽어 채운다
    pub async fn enrich(&self, bus: &str, items: &mut [Item], wanted: Vec<usize>) {
        let jobs: Vec<(usize, String, [bool; 3])> = wanted
            .into_iter()
            .map(|at| {
                let item = &items[at];
                let wants = [
                    item.wants_value(),
                    item.wants_text(),
                    item.wants_attributes(),
                ];
                (at, item.path.clone(), wants)
            })
            .collect();
        for batch in jobs.chunks(BATCH) {
            let results = join_all(batch.iter().map(|(at, path, wants)| async move {
                let (value, text, attributes) = join3(
                    async {
                        if wants[0] {
                            self.value(bus, path).await
                        } else {
                            None
                        }
                    },
                    async {
                        if wants[1] {
                            self.text(bus, path).await
                        } else {
                            None
                        }
                    },
                    async {
                        if wants[2] {
                            self.attributes(bus, path).await
                        } else {
                            HashMap::new()
                        }
                    },
                )
                .await;
                (*at, value, text, attributes)
            }))
            .await;
            for (at, value, text, mut attributes) in results {
                let item = &mut items[at];
                item.value = value;
                item.text = text;
                item.placeholder = attributes.remove("placeholder-text");
                item.level = attributes.remove("level");
            }
        }
    }

    async fn attributes(&self, bus: &str, path: &str) -> HashMap<String, String> {
        self.call(bus, path, ACCESSIBLE, "GetAttributes", &())
            .await
            .unwrap_or_default()
    }

    async fn value(&self, bus: &str, path: &str) -> Option<Range> {
        let all: HashMap<String, OwnedValue> = self
            .call(bus, path, PROPERTIES, "GetAll", &("org.a11y.atspi.Value",))
            .await
            .ok()?;
        let number = |key: &str| {
            all.get(key)
                .and_then(|value| value.downcast_ref::<f64>().ok())
        };
        Some(Range {
            current: number("CurrentValue")?,
            min: number("MinimumValue").unwrap_or(0.0),
            max: number("MaximumValue").unwrap_or(0.0),
        })
    }

    async fn text(&self, bus: &str, path: &str) -> Option<String> {
        self.call(bus, path, "org.a11y.atspi.Text", "GetText", &(0i32, -1i32))
            .await
            .ok()
    }
}

/// D-Bus 호출 하나. 바쁘거나 멈춘 앱 하나가 데몬 전체를 붙잡지 않게 기다리는 시간을 둔다
async fn call_on<B, R>(
    connection: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<R>
where
    B: Serialize + DynamicType,
    R: DeserializeOwned + Type,
{
    let call = connection.call_method(Some(destination), path, Some(interface), method, body);
    match select(pin!(call), pin!(Timer::after(CALL_TIMEOUT))).await {
        Either::Left((reply, _)) => reply?.body().deserialize(),
        Either::Right(_) => Err(zbus::Error::Failure(format!(
            "{destination} did not answer {interface}.{method} within {}s",
            CALL_TIMEOUT.as_secs()
        ))),
    }
}

/// GetState의 두 낱말(32비트씩)을 64비트 상태로
fn states(words: &[u32]) -> u64 {
    words
        .iter()
        .take(2)
        .enumerate()
        .fold(0, |set, (at, word)| set | (u64::from(*word) << (32 * at)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::state;

    #[test]
    fn state_words_make_one_set() {
        // GTK 창: SENSITIVE, SHOWING, VISIBLE (접근성 버스에서 읽은 값)
        let set = states(&[1124073472, 0]);
        assert_eq!(set, state::SENSITIVE | state::SHOWING | state::VISIBLE);
        assert_eq!(states(&[0, 1 << 11]), state::READ_ONLY);
    }
}
