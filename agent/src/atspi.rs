// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 접근성 버스(AT-SPI) 읽기. 앱마다 GTK가 지닌 접근성 객체 목록(Cache.GetItems)을 한 번에 읽는다.
//! GTK4는 누군가 GetChildren으로 물어본 객체만 만들기 때문에, 목록에서 알려진 자식 수보다 자식이
//! 적은 객체는 GetChildren으로 아래를 채우고 목록을 다시 읽는다. 한 번 채운 부모에 새로 붙는
//! 위젯은 GTK가 바로 만들어 두므로, 같은 앱을 다시 읽을 때는 대개 목록 한 번으로 끝난다.

use std::{
    collections::{HashMap, HashSet},
    pin::pin,
    time::{Duration, Instant},
};

use async_io::Timer;
use futures_util::future::{Either, join, join_all, join3, select};
use serde::{Serialize, de::DeserializeOwned};
use zbus::{
    Connection, fdo,
    names::BusName,
    zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Type, Value},
};

use crate::tree::{Item, Range, Tree, iface};

const REGISTRY: &str = "org.a11y.atspi.Registry";
/// 앱 루트 객체의 경로
pub const ROOT: &str = "/org/a11y/atspi/accessible/root";
/// 셸의 관리자 인증 창: 레이어 이름이자 창의 접근성 이름. 사람만 답하는 창이라 에이전트는 읽지도
/// 조작하지도 않는다
pub const AUTH_WINDOW: &str = "bunt-polkit-agent";
/// 사람만 답하는 셸 창 모두: 관리자 인증 창, Wi-Fi 암호 창(비밀 에이전트), Bluetooth 페어링 창(페어링
/// 에이전트). 하나였던 인증 창 검사를 이 목록으로 넓혔다
pub const HUMAN_ONLY_WINDOWS: [&str; 3] = [AUTH_WINDOW, "bunt-secret-agent", "bunt-pairing-agent"];

/// 사람만 답하는 셸 창의 이름(레이어 이름이나 창의 접근성 이름)인지
pub fn is_human_only(name: &str) -> bool {
    HUMAN_ONLY_WINDOWS.contains(&name)
}

/// 잠금 화면(bunt-lock): 앱 이름이자 창의 접근성 이름. 사람만 암호를 치는 화면이라 에이전트는 읽지도 조작하지도
/// 않는다. 잠긴 동안에는 데몬이 모든 요청을 거절하므로, 이 이름은 잠금을 푼 뒤 잠금 화면이 끝나기까지의 틈을 막는다
pub const LOCK_WINDOW: &str = "bunt-lock";
/// 셸의 화면 키보드(bunt-osk): 레이어 이름이자 창의 접근성 이름 (comp/src/osk.rs). 사람만 누른다: 에이전트가 이 키보드의
/// 키를 누르면 막은 앱에도 글을 칠 수 있다. 앱 목록과 스냅샷에서 빼고 동작 대상으로 받지 않는다. 사람만 답하는 창과 달리
/// 떠 있어도 에이전트의 다른 동작은 막지 않는다. 에이전트의 키와 글은 press·type으로 넣는다
pub const KEYBOARD_WINDOW: &str = "bunt-osk";
const CACHE: &str = "/org/a11y/atspi/cache";
const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
const ACTION: &str = "org.a11y.atspi.Action";
const COMPONENT: &str = "org.a11y.atspi.Component";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// D-Bus로 뜨는 앱의 인터페이스 (데스크톱 항목 명세의 DBusActivatable)
const APPLICATION: &str = "org.freedesktop.Application";
/// AtspiCoordType의 WINDOW: 창(최상위 표면) 기준 좌표
const COORD_WINDOW: u32 = 1;
/// Component.ScrollTo의 ATSPI_SCROLL_ANYWHERE: 보이게만 한다
const SCROLL_ANYWHERE: u32 = 6;
/// linux 입력의 왼쪽 단추 (BTN_LEFT)
const BUTTON_LEFT: u32 = 0x110;
/// 한 번에 보내는 호출 수
const BATCH: usize = 64;
/// 목록을 다시 읽는 횟수 상한
const ROUNDS: usize = 8;
/// 한 앱에서 읽는 객체 수 상한
const MAX_ITEMS: usize = 20_000;
/// 앱이 호출 하나에 답하기를 기다리는 시간
const CALL_TIMEOUT: Duration = Duration::from_secs(3);
/// 컴포지터가 창을 그려 PNG로 만들기를 기다리는 시간 (소프트웨어 렌더링 VM에서 큰 창)
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);
/// Firefox는 웹 페이지 요소의 자리와 동작을 처음 물으면 모른다고(자리 (-1, -1, -1, -1), 동작 0개) 답하고
/// 콘텐츠 프로세스에서 받아 둔다(쓸 때 켜는 캐시). 그동안 다시 묻는 간격과 상한
const LAZY_STEP: Duration = Duration::from_millis(100);
const LAZY_WAIT: Duration = Duration::from_millis(600);
/// 자리를 모르는 요소의 GetExtents
const UNKNOWN_EXTENTS: (i32, i32, i32, i32) = (-1, -1, -1, -1);

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

/// GetWindows 항목: pid, 앱 ID, 제목, x, y, 너비, 높이, 키보드 포커스
type RawWindow = (u32, String, String, i32, i32, i32, i32, bool);

/// 컴포지터가 놓은 창 (GetWindows)
#[derive(Clone, Debug)]
pub struct Placed {
    pub pid: u32,
    /// Wayland 앱 ID (= 데스크톱 ID)
    pub app_id: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub focused: bool,
}

/// GetLayers 항목: pid, 레이어 이름, x, y, 너비, 높이, 그 출력의 배율
type RawLayer = (u32, String, i32, i32, i32, i32, f64);

/// 컴포지터가 놓은 레이어 표면 (GetLayers): 셸의 메뉴 막대, Dock, 모든 앱 패널 등. 셸은 레이어 창의
/// 접근성 이름을 레이어 이름으로 둔다. 사람만 답하는 창은 컴포지터가 알리지 않는다
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub pid: u32,
    pub namespace: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// 그 표면이 있는 출력의 배율. 레이어 스크린샷(CaptureLayer)의 픽셀은 논리 좌표에 이것을 곱한 것이다
    pub scale: f64,
}

/// 컴포지터가 키보드 포커스를 준 곳 (GetFocus)
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Focus {
    pub pid: u32,
    /// 창이면 앱 ID, 레이어 표면이면 그 이름
    pub app_id: String,
    /// 창 제목. 레이어 표면은 빈 문자열
    pub title: String,
    /// 셸의 레이어 표면(메뉴, 모든 앱, 인증 창 등)이다
    pub layer: bool,
}

#[derive(Clone)]
pub struct A11y {
    /// 접근성 버스
    bus: Connection,
    /// 세션 버스 (컴포지터 API)
    session: Connection,
}

/// 화면이 잠겼는지 (org.buntos.Compositor1.GetLocked). 컴포지터가 이 메서드를 모르면(화면 잠금이 없는 옛
/// 컴포지터) 잠기지 않은 것으로 본다. 컴포지터가 버스에 없거나 답이 늦는 것 같은 그 밖의 오류는 잠긴 것으로
/// 본다: 컴포지터의 D-Bus만 멈추고 화면은 잠겨 있을 수 있다
pub async fn screen_locked(session: &Connection) -> bool {
    let reply = call_on::<_, bool>(
        session,
        CALL_TIMEOUT,
        "org.buntos.Compositor",
        "/org/buntos/Compositor",
        "org.buntos.Compositor1",
        "GetLocked",
        &(),
    )
    .await;
    match reply {
        Ok(locked) => locked,
        Err(zbus::Error::MethodError(name, ..)) if no_lock_api(name.as_str()) => false,
        // 컴포지터가 버스에 없을 때(ServiceUnknown, NameHasNoOwner)도 여기로 온다
        Err(err) => {
            tracing::warn!(
                "cannot tell whether the screen is locked; treating it as locked: {err}"
            );
            true
        }
    }
}

/// 화면 잠금을 모르는 컴포지터의 오류: GetLocked가 없다(옛 컴포지터)
fn no_lock_api(error: &str) -> bool {
    error == "org.freedesktop.DBus.Error.UnknownMethod"
}

/// 보조 기술이 있다고 알린다 (org.a11y.Status.IsEnabled). Chromium·Electron·Firefox·Qt 앱은 이것을
/// 보고서야 트리를 내놓는다(GTK4는 늘 내놓는다). Chromium·Electron은 뜰 때 한 번만 읽고 바뀌어도 듣지
/// 않으므로, 이미 떠 있던 앱은 다시 띄워야 따른다. at-spi-bus-launcher는 이 값을 GSettings의
/// toolkit-accessibility에 적어 다음 로그인에도 켜 둔다
pub async fn enable_accessibility(session: &Connection) -> zbus::Result<()> {
    session
        .call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some(PROPERTIES),
            "Set",
            &("org.a11y.Status", "IsEnabled", Value::from(true)),
        )
        .await?;
    Ok(())
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
        if let Err(err) = enable_accessibility(&session).await {
            tracing::warn!("cannot turn on accessibility for other toolkits: {err}");
        }
        let bus = zbus::connection::Builder::address(address.as_str())?
            .build()
            .await?;
        Ok(Self { bus, session })
    }

    /// 컴포지터가 키보드 포커스를 준 곳 (org.buntos.Compositor1.GetFocus). 컴포지터 API가 없거나
    /// 포커스가 없으면 None
    pub async fn focus(&self) -> Option<Focus> {
        let (pid, app_id, title, layer): (u32, String, String, bool) =
            self.compositor("GetFocus", &()).await.ok()?;
        (pid != 0).then_some(Focus {
            pid,
            app_id,
            title,
            layer,
        })
    }

    /// 키보드 포커스가 사람만 답하는 셸 창(관리자 인증, Wi-Fi 암호, Bluetooth 페어링)에 있는지
    pub async fn auth_dialog_focused(&self) -> bool {
        matches!(
            self.compositor::<_, (u32, String, String, bool)>("GetFocus", &()).await,
            Ok((_, namespace, _, true)) if is_human_only(&namespace)
        )
    }

    /// 컴포지터 API (org.buntos.Compositor1). 창 목록과 입력 메서드는 buntd-agent만 부를 수 있다
    async fn compositor<B, R>(&self, method: &str, body: &B) -> zbus::Result<R>
    where
        B: Serialize + DynamicType,
        R: DeserializeOwned + Type,
    {
        call_on(
            &self.session,
            CALL_TIMEOUT,
            "org.buntos.Compositor",
            "/org/buntos/Compositor",
            "org.buntos.Compositor1",
            method,
            body,
        )
        .await
    }

    /// 컴포지터의 창 목록 (쌓인 순서, 아래부터)
    pub async fn windows(&self) -> zbus::Result<Vec<Placed>> {
        let windows: Vec<RawWindow> = self.compositor("GetWindows", &()).await?;
        Ok(windows
            .into_iter()
            .map(
                |(pid, app_id, title, x, y, width, height, focused)| Placed {
                    pid,
                    app_id,
                    title,
                    x,
                    y,
                    width,
                    height,
                    focused,
                },
            )
            .collect())
    }

    /// 컴포지터의 레이어 표면 목록 (출력마다 아래층부터)
    pub async fn layers(&self) -> zbus::Result<Vec<Layer>> {
        let layers: Vec<RawLayer> = self.compositor("GetLayers", &()).await?;
        Ok(layers
            .into_iter()
            .map(|(pid, namespace, x, y, width, height, scale)| Layer {
                pid,
                namespace,
                x,
                y,
                width,
                height,
                scale,
            })
            .collect())
    }

    /// 그 pid의 레이어 표면(이름이 같은 것) 안의 (x, y)를 왼쪽 단추로 누른다. x, y는 표면 기준 논리
    /// 좌표(셸 창의 접근성 좌표, 열린 메뉴 포함)다. 누른 화면의 논리 좌표를 돌려준다. 그 점에서 포인터가 그
    /// 표면에 가지 않으면(자동으로 숨은 막대, 가림) 오류다
    pub async fn click_layer(
        &self,
        pid: u32,
        namespace: &str,
        x: f64,
        y: f64,
    ) -> zbus::Result<(f64, f64)> {
        self.compositor("ClickLayer", &(pid, namespace, x, y, BUTTON_LEFT))
            .await
    }

    /// 그 pid의 창(제목이 같은 것 먼저)을 앞으로 가져와 키보드 포커스를 준다
    pub async fn activate_window(&self, pid: u32, title: &str) -> zbus::Result<bool> {
        self.compositor("ActivateWindow", &(pid, title)).await
    }

    /// 키보드 포커스에 키를 차례로 누른다 ("ctrl+a", "Return")
    pub async fn press_keys(&self, keys: &[String]) -> zbus::Result<()> {
        self.compositor("PressKeys", &(keys,)).await
    }

    /// 키보드 포커스를 가진 앱이 글 입력(text-input-v3)을 켜 두었는지
    pub async fn text_input_active(&self) -> zbus::Result<bool> {
        self.compositor("TextInputActive", &()).await
    }

    /// 글을 친다. 앱의 글 입력에 확정했으면 "text-input", 글자마다 키를 쳤으면 "keys"
    /// (지금 키 배열에 있는 글자만)
    pub async fn type_text(&self, text: &str) -> zbus::Result<String> {
        self.compositor("TypeText", &(text,)).await
    }

    /// 켜진 글 입력에만 글을 확정한다. 켜진 글 입력이 없으면 아무것도 치지 않고 false다
    pub async fn commit_text(&self, text: &str) -> zbus::Result<bool> {
        self.compositor("CommitText", &(text,)).await
    }

    /// 화면의 논리 좌표를 왼쪽 단추로 누른다
    pub async fn click(&self, x: f64, y: f64) -> zbus::Result<()> {
        self.compositor("Click", &(x, y, BUTTON_LEFT)).await
    }

    /// 화면의 논리 좌표로 포인터를 옮겨 휠을 돌린다. dx, dy는 눈금 수이고 오른쪽·아래가 +다
    pub async fn scroll(&self, x: f64, y: f64, dx: f64, dy: f64) -> zbus::Result<()> {
        self.compositor("Scroll", &(x, y, dx, dy)).await
    }

    /// 그 pid의 레이어 표면(이름이 같은 것) 안의 (x, y)에서 휠을 돌린다. x, y는 click_layer와 같은 표면 기준
    /// 논리 좌표이고 dx, dy는 scroll과 같은 눈금 수다. 그 점에서 포인터가 그 표면에 가지 않으면 오류다
    pub async fn scroll_layer(
        &self,
        pid: u32,
        namespace: &str,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
    ) -> zbus::Result<()> {
        self.compositor("ScrollLayer", &(pid, namespace, x, y, dx, dy))
            .await
    }

    /// 그 pid의 창(제목이 같은 것 먼저) 안의 (x, y)를 왼쪽 단추로 누른다. x, y는 그 창 스크린샷의
    /// 픽셀이다. 누른 화면의 논리 좌표를 돌려준다. 창 밖이거나 그 점 위에 다른 것이 있으면 오류다
    pub async fn click_window(
        &self,
        pid: u32,
        title: &str,
        x: f64,
        y: f64,
    ) -> zbus::Result<(f64, f64)> {
        self.compositor("ClickWindow", &(pid, title, x, y, BUTTON_LEFT))
            .await
    }

    /// 그 pid의 창(제목이 같은 것 먼저)만 그린 PNG: (너비, 높이, 배율, PNG 바이트)
    pub async fn capture_window(
        &self,
        pid: u32,
        title: &str,
    ) -> zbus::Result<(u32, u32, f64, Vec<u8>)> {
        call_on(
            &self.session,
            CAPTURE_TIMEOUT,
            "org.buntos.Compositor",
            "/org/buntos/Compositor",
            "org.buntos.Compositor1",
            "CaptureWindow",
            &(pid, title),
        )
        .await
    }

    /// 그 pid의 레이어 표면(이름이 같은 것)만 그린 PNG: (너비, 높이, 배율, PNG 바이트)
    pub async fn capture_layer(
        &self,
        pid: u32,
        namespace: &str,
    ) -> zbus::Result<(u32, u32, f64, Vec<u8>)> {
        call_on(
            &self.session,
            CAPTURE_TIMEOUT,
            "org.buntos.Compositor",
            "/org/buntos/Compositor",
            "org.buntos.Compositor1",
            "CaptureLayer",
            &(pid, namespace),
        )
        .await
    }

    /// D-Bus로 뜨는 앱(DBusActivatable)에 창을 열라고 한다 (org.freedesktop.Application.Activate).
    /// 앱이 없으면 세션 버스가 띄우고, 답은 앱이 뜬 뒤에 오므로 `wait`만큼 기다린다. `gio launch`는
    /// 이 호출이 앱에 닿기 전에 끝나 버려서, 앱은 뜨기만 하고 창을 열지 않은 채 곧 끝난다
    pub async fn activate_app(&self, id: &str, wait: Duration) -> zbus::Result<()> {
        let path = format!("/{}", id.replace('.', "/").replace('-', "_"));
        let platform: HashMap<&str, Value> = HashMap::new();
        call_on(
            &self.session,
            wait,
            id,
            &path,
            APPLICATION,
            "Activate",
            &(platform,),
        )
        .await
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
        call_on(
            &self.bus,
            CALL_TIMEOUT,
            destination,
            path,
            interface,
            method,
            body,
        )
        .await
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

    /// 요소의 지금 상태 비트 (Accessible.GetState). 읽을 수 없으면 0
    pub async fn states(&self, bus: &str, path: &str) -> u64 {
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
        let mut items = match self.items(bus).await {
            Ok(items) => items,
            // 목록(캐시)이 없는 툴킷(Firefox의 ATK 다리 등)은 트리를 직접 걷는다
            Err(zbus::Error::MethodError(name, ..))
                if name.as_str() == "org.freedesktop.DBus.Error.UnknownMethod" =>
            {
                return Ok(self.walk(bus).await);
            }
            Err(err) => return Err(err),
        };
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
        crate::tree::hide_detached(&mut items);
        Ok(items)
    }

    /// 루트부터 GetChildren으로 내려가며 객체마다 역할, 이름, 설명, 상태, 인터페이스를 묻는다.
    /// 캐시가 없는 앱에 쓴다 (객체마다 호출 여섯 번이라 GTK4의 목록보다 느리다)
    async fn walk(&self, bus: &str) -> Vec<Item> {
        let mut items = vec![Item {
            path: ROOT.into(),
            name: self.name(bus, ROOT).await,
            role: crate::tree::role::APPLICATION,
            ..Item::default()
        }];
        let mut frontier = vec![ROOT.to_string()];
        while !frontier.is_empty() && items.len() < MAX_ITEMS {
            let mut next = Vec::new();
            for batch in frontier.chunks(BATCH / 4) {
                let levels = join_all(batch.iter().map(|parent| async move {
                    let children: Vec<String> = self
                        .children(bus, parent)
                        .await
                        .into_iter()
                        .filter(|(child_bus, _)| child_bus == bus)
                        .map(|(_, path)| path.as_str().to_string())
                        .collect();
                    join_all(
                        children
                            .iter()
                            .enumerate()
                            .map(|(index, path)| self.describe(bus, parent, index, path)),
                    )
                    .await
                }))
                .await;
                for item in levels.into_iter().flatten() {
                    next.push(item.path.clone());
                    items.push(item);
                }
            }
            frontier = next;
        }
        items
    }

    /// 객체 하나의 모양 (walk)
    async fn describe(&self, bus: &str, parent: &str, index: usize, path: &str) -> Item {
        let (role, name, (states, interfaces, children)) = join3(
            self.call::<_, u32>(bus, path, ACCESSIBLE, "GetRole", &()),
            self.name(bus, path),
            join3(
                self.states(bus, path),
                self.call::<_, Vec<String>>(bus, path, ACCESSIBLE, "GetInterfaces", &()),
                self.call::<_, OwnedValue>(
                    bus,
                    path,
                    PROPERTIES,
                    "Get",
                    &(ACCESSIBLE, "ChildCount"),
                ),
            ),
        )
        .await;
        let description = self
            .call::<_, OwnedValue>(bus, path, PROPERTIES, "Get", &(ACCESSIBLE, "Description"))
            .await
            .ok()
            .and_then(|value| String::try_from(value).ok())
            .unwrap_or_default();
        Item {
            path: path.to_string(),
            parent: parent.to_string(),
            index: index as i32,
            child_count: children
                .ok()
                .and_then(|value| value.downcast_ref::<i32>().ok())
                .unwrap_or(0),
            interfaces: interfaces
                .map(|names| iface::from_names(names.iter().map(String::as_str)))
                .unwrap_or(0),
            name,
            role: role.unwrap_or(0),
            description,
            states,
            ..Item::default()
        }
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

    /// 글자 칸이나 글의 내용 (Text.GetText). 읽을 수 없으면 None
    pub async fn text(&self, bus: &str, path: &str) -> Option<String> {
        self.call(bus, path, "org.a11y.atspi.Text", "GetText", &(0i32, -1i32))
            .await
            .ok()
    }

    /// 요소가 내놓은 동작의 이름. 번호는 DoAction의 번호다. GetActions는 지역화한 이름("토글")을
    /// 주고 쓸 수 없는 동작을 빼므로, 동작 수와 GetName으로 지역화하지 않은 이름을 읽는다
    /// `web`이면(웹 페이지의 요소) 0개일 때 Firefox가 받아 오기를 LAZY_WAIT까지 기다린다
    pub async fn actions(&self, bus: &str, path: &str, web: bool) -> Vec<String> {
        let started = Instant::now();
        let count = loop {
            let count = self
                .call::<_, OwnedValue>(bus, path, PROPERTIES, "Get", &(ACTION, "NActions"))
                .await
                .map(|value| value.downcast_ref::<i32>().unwrap_or(0));
            match count {
                Ok(0) if web && started.elapsed() < LAZY_WAIT => Timer::after(LAZY_STEP).await,
                Ok(count) => break count.clamp(0, 16),
                Err(_) => break 0,
            };
        };
        join_all((0..count).map(|index| async move {
            self.call::<_, String>(bus, path, ACTION, "GetName", &(index,))
                .await
                .unwrap_or_default()
        }))
        .await
    }

    pub async fn do_action(&self, bus: &str, path: &str, index: i32) -> zbus::Result<bool> {
        self.call(bus, path, ACTION, "DoAction", &(index,)).await
    }

    /// 글자 칸의 커서를 글 끝으로 옮긴다 (Text.SetCaretOffset)
    pub async fn caret_to_end(&self, bus: &str, path: &str) -> zbus::Result<bool> {
        let count = self
            .call::<_, OwnedValue>(
                bus,
                path,
                PROPERTIES,
                "Get",
                &("org.a11y.atspi.Text", "CharacterCount"),
            )
            .await?
            .downcast_ref::<i32>()
            .map_err(zbus::Error::Variant)?;
        self.call(
            bus,
            path,
            "org.a11y.atspi.Text",
            "SetCaretOffset",
            &(count,),
        )
        .await
    }

    /// 글자 칸의 커서 자리에 글을 넣는다 (EditableText.InsertText. 길이는 GTK가 바이트로 읽는다)
    pub async fn insert_text(&self, bus: &str, path: &str, text: &str) -> zbus::Result<bool> {
        let caret = self
            .call::<_, OwnedValue>(
                bus,
                path,
                PROPERTIES,
                "Get",
                &("org.a11y.atspi.Text", "CaretOffset"),
            )
            .await?
            .downcast_ref::<i32>()
            .map_err(zbus::Error::Variant)?;
        let length = i32::try_from(text.len()).unwrap_or(i32::MAX);
        self.call(
            bus,
            path,
            "org.a11y.atspi.EditableText",
            "InsertText",
            &(caret, text, length),
        )
        .await
    }

    /// 글자 칸의 내용을 바꾼다 (EditableText.SetTextContents)
    pub async fn set_text(&self, bus: &str, path: &str, text: &str) -> zbus::Result<bool> {
        self.call(
            bus,
            path,
            "org.a11y.atspi.EditableText",
            "SetTextContents",
            &(text,),
        )
        .await
    }

    /// 막대·숫자 칸의 값을 바꾼다 (Value.CurrentValue)
    pub async fn set_value(&self, bus: &str, path: &str, value: f64) -> zbus::Result<()> {
        self.call(
            bus,
            path,
            PROPERTIES,
            "Set",
            &("org.a11y.atspi.Value", "CurrentValue", Value::from(value)),
        )
        .await
    }

    pub async fn grab_focus(&self, bus: &str, path: &str) -> zbus::Result<bool> {
        self.call(bus, path, COMPONENT, "GrabFocus", &()).await
    }

    /// 목록의 `index`번째 항목을 고른다 (Selection.SelectChild)
    pub async fn select_child(&self, bus: &str, list: &str, index: i32) -> zbus::Result<bool> {
        self.call(
            bus,
            list,
            "org.a11y.atspi.Selection",
            "SelectChild",
            &(index,),
        )
        .await
    }

    /// 요소의 자리와 크기 (창 기준 논리 좌표: x, y, 너비, 높이). 모르면 (-1, -1, -1, -1)
    pub async fn extents(&self, bus: &str, path: &str) -> zbus::Result<(i32, i32, i32, i32)> {
        let started = Instant::now();
        loop {
            let extents = self
                .call(bus, path, COMPONENT, "GetExtents", &(COORD_WINDOW,))
                .await?;
            if extents != UNKNOWN_EXTENTS || started.elapsed() >= LAZY_WAIT {
                return Ok(extents);
            }
            Timer::after(LAZY_STEP).await;
        }
    }

    /// 앱이 요소가 보이게 스스로 스크롤한다 (Component.ScrollTo). Firefox는 하고, GTK4는 지원하지
    /// 않는다(NotSupported)
    pub async fn scroll_to(&self, bus: &str, path: &str) -> zbus::Result<bool> {
        self.call(bus, path, COMPONENT, "ScrollTo", &(SCROLL_ANYWHERE,))
            .await
    }
}

/// D-Bus 호출 하나. 바쁘거나 멈춘 앱 하나가 데몬 전체를 붙잡지 않게 기다리는 시간을 둔다
async fn call_on<B, R>(
    connection: &Connection,
    timeout: Duration,
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
    match select(pin!(call), pin!(Timer::after(timeout))).await {
        Either::Left((reply, _)) => reply?.body().deserialize(),
        Either::Right(_) => Err(zbus::Error::Failure(format!(
            "{destination} did not answer {interface}.{method} within {}s",
            timeout.as_secs()
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
    fn only_a_missing_lock_api_counts_as_unlocked() {
        assert!(no_lock_api("org.freedesktop.DBus.Error.UnknownMethod"));
        // 컴포지터의 D-Bus만 사라져도 화면은 잠겨 있을 수 있으므로 잠긴 것으로 본다
        assert!(!no_lock_api("org.freedesktop.DBus.Error.ServiceUnknown"));
        assert!(!no_lock_api("org.freedesktop.DBus.Error.NameHasNoOwner"));
        assert!(!no_lock_api("org.freedesktop.DBus.Error.AccessDenied"));
        assert!(!no_lock_api("org.freedesktop.DBus.Error.NoReply"));
    }

    #[test]
    fn state_words_make_one_set() {
        // GTK 창: SENSITIVE, SHOWING, VISIBLE (접근성 버스에서 읽은 값)
        let set = states(&[1124073472, 0]);
        assert_eq!(set, state::SENSITIVE | state::SHOWING | state::VISIBLE);
        assert_eq!(states(&[0, 1 << 11]), state::READ_ONLY);
    }

    #[test]
    fn human_only_windows_cover_every_secret_dialog() {
        // 셸의 레이어 이름 (polkit_agent.rs, secret_agent.rs, pairing_agent.rs)
        for name in [
            "bunt-polkit-agent",
            "bunt-secret-agent",
            "bunt-pairing-agent",
        ] {
            assert!(is_human_only(name), "{name}");
        }
        assert!(!is_human_only("bunt-banner"));
        assert!(!is_human_only("bunt-polkit"));
        assert!(!is_human_only(""));
    }
}
