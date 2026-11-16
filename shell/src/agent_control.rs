// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AI 제어 표시와 AI 제어 창 (사람과 AI가 한 컴퓨터를 함께 쓰는 설계의 "사람에게 늘 보이기"). macOS가 메뉴 막대에
//! 사용 중 표시를 두고, 누르면 어느 앱이 쓰는지 보이는 것을 따른다.
//!
//! - 메뉴 막대 표시: 컴포지터가 알리는 AI 상태(GetAgentState, AgentStateChanged)를 따른다. AI가 동작하는 동안(마지막
//!   동작 뒤 5초)과 사람이 AI 제어를 멈춘 동안만 보인다. 제어 중은 AI 색 캡슐에 AI 기호, 멈춤은 빈 캡슐에 멈춤 기호다
//!   (색만으로 구별하지 않는다). 개인정보 표시처럼 사용자가 숨길 수 없다. 이 API가 없는 옛 컴포지터에서는 숨긴다.
//! - AI 제어 창: 표시를 누르면 연다. 어느 AI가 어느 앱(창)을 다루는지, [AI 제어 멈추기]·[다시 허용](SetAgentPaused)과
//!   그 단축키(⌃⌥⌘., 메뉴 항목처럼 오른쪽에), "AI 에이전트 설정…"이 있다. 메뉴 막대 표면 안의 팝오버가 아니라 따로 된 레이어 창
//!   ([`NAMESPACE`])이다. 컴포지터와 에이전트 데몬이 이 이름을 사람만 답하는 창으로 막아, 에이전트는 창을 읽거나 누르지
//!   못하고 창이 떠 있는 동안 에이전트의 입력은 거절된다. 팝오버였다면 메뉴 막대 표면이라 에이전트가 단추를 누를 수 있다.
//! - 창은 모든 앱 패널처럼 화면을 덮는 투명한 overlay 창이고, 메뉴처럼 판 바깥을 누르면 닫힌다. 표시는 메뉴 막대에
//!   있어 AI도 눌러 띄울 수 있으므로, 키보드는 사람이 창을 눌렀을 때만 받는다(AI 접근 요청 창과 같다).
//! - [AI 제어 멈추기]·[다시 허용]은 사람의 실제 누르기와 키로만 동작한다([`HumanPress`]). 접근성(AT-SPI)의 동작은 실제
//!   입력 없이 단추를 누르므로, 셸 도구를 가진 AI가 접근성 버스로 스스로 멈춤을 풀지 못하게 한다.
//! - 개발용 가짜 상태: 환경 변수 `BUNT_SHELL_FAKE_AGENT=active|paused`로 셸을 띄우면 컴포지터 대신 가짜 상태를 보이고,
//!   단추는 그 가짜 상태만 바꾼다. 새 컴포지터 없이 모양을 확인할 때 쓴다(`bunt-lock --preview`와 같은 쓰임).

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{KeyboardMode, LayerShell};

use crate::{dbus, extras, launcher, menubar, service_dialog::keep_words};

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";
/// 사람만 답하는 창의 레이어 이름이자 창의 접근성 이름. 바꾸면 comp/src/agent.rs의 AUTH_NAMESPACES와
/// agent/src/atspi.rs의 HUMAN_ONLY_WINDOWS도 함께 고친다
pub const NAMESPACE: &str = "bunt-agent-control";
/// GetAgentState의 답과 AgentStateChanged의 인자: (paused, active, client, pid, app_id, title)
const STATE_TYPE: &str = "(bbsuss)";
/// AI 에이전트 기호 (시스템 설정의 AI 에이전트 페이지와 같다)
const AGENT_ICON: &str = "org.buntos.Agent-symbolic";
const PAUSE_ICON: &str = "media-playback-pause-symbolic";
const RESUME_ICON: &str = "media-playback-start-symbolic";
/// 데스크톱 항목이 없는 앱
const APP_ICON: &str = "application-x-executable-symbolic";
/// 시스템 설정의 AI 에이전트 페이지 (`bunt-settings agent`)
const SETTINGS_PAGE: &str = "agent";
/// AI 제어를 멈추거나 다시 허용하는 컴포지터 단축키(agent-pause의 기본값). 메뉴 항목의 단축키처럼 기호로 적는다. 시스템
/// 설정에서 바꾸면 따라가지 않는다(강제 종료 창의 ⌥⌘⎋ 안내와 같다)
const SHORTCUT: &str = "⌃⌥⌘.";
/// 개발용 가짜 상태를 고르는 환경 변수
const FAKE_ENV: &str = "BUNT_SHELL_FAKE_AGENT";
/// 판과 메뉴 막대 사이. 메뉴 막대 항목의 메뉴처럼 막대 바로 아래에 붙는다(VM에서 잰 사운드 메뉴와 같다)
const GAP: i32 = 0;
/// 판이 화면 가장자리에서 떨어지는 거리
const EDGE: i32 = 8;
/// 다룬 앱의 아이콘 (메뉴 줄의 동그라미 아이콘과 같은 크기)
const APP_ICON_SIZE: i32 = 26;
/// 상태 문장의 줄 길이 (글자 수)
const TEXT_WIDTH: i32 = 34;
/// 사람의 실제 누르기나 키 뒤 이 안에 온 누름만 받는다
const HUMAN_PRESS_WINDOW: Duration = Duration::from_secs(1);

/// 컴포지터가 알리는 AI 상태
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentState {
    /// 사람이 AI 제어를 멈췄다. 사람만(이 창, 단축키) 다시 허용할 수 있다
    pub paused: bool,
    /// AI가 마지막으로 동작한 뒤 5초가 지나지 않았다
    pub active: bool,
    /// 마지막으로 동작한 AI 클라이언트의 이름
    pub client: String,
    pub pid: u32,
    /// 그 AI가 다룬 창의 앱 ID와 제목
    pub app_id: String,
    pub title: String,
}

impl AgentState {
    fn from_variant(value: &glib::Variant) -> Option<Self> {
        let (paused, active, client, pid, app_id, title) =
            value.get::<(bool, bool, String, u32, String, String)>()?;
        Some(Self {
            paused,
            active,
            client,
            pid,
            app_id,
            title,
        })
    }

    fn mode(&self) -> Mode {
        if self.paused {
            Mode::Paused
        } else if self.active {
            Mode::Active
        } else {
            Mode::Idle
        }
    }

    /// 다룬 앱이나 창을 아는지
    fn has_target(&self) -> bool {
        !self.app_id.is_empty() || !self.title.is_empty()
    }
}

/// 메뉴 막대 표시와 창의 모양
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// AI가 동작하는 중
    Active,
    /// 사람이 멈췄다. AI가 동작하려 해도 멈춤이 먼저다
    Paused,
    /// 멈추지 않았고 동작하지 않는다. 메뉴 막대 표시를 숨긴다
    Idle,
}

impl Mode {
    /// 메뉴 막대 표시의 이름(도움말, 접근성 이름). 표시를 숨기는 때는 None
    fn indicator_label(self) -> Option<&'static str> {
        match self {
            Self::Active => Some("agent-indicator-active"),
            Self::Paused => Some("agent-indicator-paused"),
            Self::Idle => None,
        }
    }

    /// 창 제목 줄 오른쪽의 지금 상태
    fn state_label(self) -> &'static str {
        match self {
            Self::Active => "agent-control-state-active",
            Self::Paused => "agent-control-state-paused",
            Self::Idle => "agent-control-state-idle",
        }
    }

    /// 다룬 앱 묶음의 머리글
    fn target_section(self) -> &'static str {
        match self {
            Self::Active => "agent-control-target",
            Self::Paused | Self::Idle => "agent-control-last-target",
        }
    }

    /// 단추의 글, 기호, 누르면 바뀔 멈춤
    fn toggle(self) -> (&'static str, &'static str, bool) {
        match self {
            Self::Paused => ("agent-control-resume", RESUME_ICON, false),
            Self::Active | Self::Idle => ("agent-control-pause", PAUSE_ICON, true),
        }
    }
}

/// 창의 상태 문장: 메시지 ID와 그 문장에 넣을 AI 이름
fn status_message(state: &AgentState) -> (&'static str, Option<&str>) {
    match state.mode() {
        Mode::Paused => ("agent-control-paused", None),
        Mode::Active if state.client.is_empty() => ("agent-control-active-unknown", None),
        Mode::Active => ("agent-control-active", Some(&state.client)),
        Mode::Idle => ("agent-control-idle", None),
    }
}

/// 다룬 앱 줄의 이름과 그 아래 창 제목. 이름은 데스크톱 항목의 이름, 없으면 앱 ID, 그것도 없으면 `unknown`이다.
/// 창 제목이 이름과 같으면 두 번 보이지 않는다
fn target_text(
    state: &AgentState,
    desktop_name: Option<String>,
    unknown: String,
) -> (String, Option<String>) {
    let name = desktop_name
        .filter(|name| !name.is_empty())
        .or_else(|| (!state.app_id.is_empty()).then(|| state.app_id.clone()))
        .unwrap_or(unknown);
    let title = (!state.title.is_empty() && state.title != name).then(|| state.title.clone());
    (name, title)
}

/// 개발용 가짜 상태(BUNT_SHELL_FAKE_AGENT의 값). 값이 없거나 모르는 값이면 None
fn fake_state(value: Option<&str>) -> Option<AgentState> {
    let base = AgentState {
        client: "claude".into(),
        app_id: "org.gnome.TextEditor".into(),
        title: "README.md".into(),
        ..AgentState::default()
    };
    match value? {
        "active" => Some(AgentState {
            active: true,
            ..base
        }),
        "paused" => Some(AgentState {
            paused: true,
            ..base
        }),
        other => {
            eprintln!("bunt-shell: {FAKE_ENV}={other}: use active or paused");
            None
        }
    }
}

/// 판의 왼쪽 끝: 표시 아래 가운데에 두되 화면 밖으로 나가지 않게 한다(메뉴 막대 항목의 메뉴와 같다)
fn panel_x(center: f64, width: i32, screen: i32) -> i32 {
    let x = (center - f64::from(width) / 2.0).round() as i32;
    x.min(screen - width - EDGE).max(EDGE)
}

/// 읽은 답(GetAgentState)과 신호(AgentStateChanged)의 차례를 맞춘다. 읽는 동안 신호가 왔으면 신호가 더 새 값이라
/// 늦게 온 답은 버린다(input_mode.rs와 같다). None은 컴포지터가 없거나 이 API가 없는 옛 컴포지터다
#[derive(Default)]
struct Latest {
    state: RefCell<Option<AgentState>>,
    /// 받은 신호 수. 읽기를 시작할 때 적어 두었다가 답이 오면 그사이 신호가 왔는지 본다
    signals: Cell<u64>,
}

impl Latest {
    fn read_started(&self) -> u64 {
        self.signals.get()
    }

    /// 읽은 답. 상태가 바뀌었으면 새 상태
    fn answered(&self, started: u64, state: Option<AgentState>) -> Option<Option<AgentState>> {
        if self.signals.get() != started {
            return None;
        }
        self.set(state)
    }

    /// 받은 신호(컴포지터가 사라진 것도 신호로 본다). 상태가 바뀌었으면 새 상태
    fn signalled(&self, state: Option<AgentState>) -> Option<Option<AgentState>> {
        self.signals.set(self.signals.get().wrapping_add(1));
        self.set(state)
    }

    fn set(&self, state: Option<AgentState>) -> Option<Option<AgentState>> {
        if *self.state.borrow() == state {
            return None;
        }
        self.state.replace(state.clone());
        Some(state)
    }
}

/// 옛 컴포지터처럼 이 API가 없어 생긴 오류인지. 이때는 기록 없이 표시를 숨긴다
fn missing_api(err: &glib::Error) -> bool {
    [
        gio::DBusError::UnknownMethod,
        gio::DBusError::UnknownInterface,
        gio::DBusError::UnknownObject,
        gio::DBusError::ServiceUnknown,
        gio::DBusError::NameHasNoOwner,
    ]
    .into_iter()
    .any(|code| err.matches(code))
}

/// 컴포지터의 AI 상태를 지켜보고 사람이 고른 멈춤을 알린다
struct Compositor {
    connection: gio::DBusConnection,
    latest: Latest,
    changed: Box<dyn Fn(Option<AgentState>)>,
}

impl Compositor {
    /// 지켜보기 시작한다. 컴포지터가 (다시) 뜨면 읽고, 바뀌면 신호로 받는다. 상태가 바뀔 때만 changed를 부른다.
    /// 시험은 개인 버스의 연결을 준다
    fn watch(
        connection: &gio::DBusConnection,
        changed: impl Fn(Option<AgentState>) + 'static,
    ) -> Rc<Self> {
        let compositor = Rc::new(Self {
            connection: connection.clone(),
            latest: Latest::default(),
            changed: Box::new(changed),
        });
        // 신호를 먼저 받는다. 읽은 뒤에 달면 그 사이에 바뀐 것을 놓친다
        let weak = Rc::downgrade(&compositor);
        let subscription = connection.subscribe_to_signal(
            Some(NAME),
            Some(INTERFACE),
            Some("AgentStateChanged"),
            Some(PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                if let Some(compositor) = weak.upgrade()
                    && let Some(state) = AgentState::from_variant(signal.parameters)
                {
                    compositor.signalled(Some(state));
                }
            },
        );
        // 셸이 끝날 때까지 받는다
        std::mem::forget(subscription);
        let (appeared, vanished) = (Rc::downgrade(&compositor), Rc::downgrade(&compositor));
        // 감시는 놓아도 멈추지 않는다 (dbus::watch_name)
        dbus::watch_name(
            connection,
            NAME,
            move |_| {
                if let Some(compositor) = appeared.upgrade() {
                    compositor.read();
                }
            },
            move || {
                if let Some(compositor) = vanished.upgrade() {
                    compositor.signalled(None);
                }
            },
        );
        compositor
    }

    fn signalled(&self, state: Option<AgentState>) {
        if let Some(state) = self.latest.signalled(state) {
            (self.changed)(state);
        }
    }

    /// 지금 상태를 묻는다
    fn read(self: &Rc<Self>) {
        let started = self.latest.read_started();
        let compositor = Rc::downgrade(self);
        self.connection.call(
            Some(NAME),
            PATH,
            INTERFACE,
            "GetAgentState",
            None,
            glib::VariantTy::new(STATE_TYPE).ok(),
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |result| {
                let Some(compositor) = compositor.upgrade() else {
                    return;
                };
                let state = match result {
                    Ok(reply) => AgentState::from_variant(&reply),
                    Err(err) => {
                        if !missing_api(&err) {
                            eprintln!("bunt-shell: {INTERFACE}.GetAgentState: {err}");
                        }
                        None
                    }
                };
                if let Some(state) = compositor.latest.answered(started, state) {
                    (compositor.changed)(state);
                }
            },
        );
    }

    /// 사람이 고른 멈춤을 알린다. 바뀐 상태는 신호로 오지만, 신호를 놓쳐도 맞도록 다시 읽는다
    fn set_paused(self: &Rc<Self>, paused: bool) {
        let compositor = Rc::downgrade(self);
        self.connection.call(
            Some(NAME),
            PATH,
            INTERFACE,
            "SetAgentPaused",
            Some(&(paused,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |result| match result {
                Ok(_) => {
                    if let Some(compositor) = compositor.upgrade() {
                        compositor.read();
                    }
                }
                Err(err) => eprintln!("bunt-shell: {INTERFACE}.SetAgentPaused: {err}"),
            },
        );
    }
}

/// AI 상태를 주는 곳
enum Backend {
    Compositor(Rc<Compositor>),
    /// BUNT_SHELL_FAKE_AGENT. 멈추기와 다시 허용은 이 가짜 상태만 바꾼다
    Fake,
}

/// 사람의 실제 누르기나 키로 단추가 눌렸는지 본다. 접근성(AT-SPI)의 동작(DoAction)은 실제 입력 없이 단추를 눌러
/// clicked를 부르므로, 같은 사용자의 다른 프로그램(셸 도구를 가진 AI 등)이 접근성 버스로 단추를 누를 수 있다. 이 창에
/// 오는 실제 입력은 사람의 것이다: 컴포지터는 사람만 답하는 창이 떠 있는 동안 에이전트의 입력을 받지 않는다
#[derive(Default)]
struct HumanPress {
    at: Cell<Option<Instant>>,
}

impl HumanPress {
    fn record(&self, now: Instant) {
        self.at.set(Some(now));
    }

    /// 방금 실제 누르기가 있었는지. 한 번 누르기로 한 번만 받는다
    fn take(&self, now: Instant) -> bool {
        self.at
            .take()
            .is_some_and(|at| now.saturating_duration_since(at) <= HUMAN_PRESS_WINDOW)
    }
}

/// 단추를 누르는 실제 입력인지: 주 단추 누르기·떼기, 터치, 단추를 누르는 키(스페이스, Return)
fn is_press(event: &gdk::Event) -> bool {
    match event.event_type() {
        gdk::EventType::ButtonPress | gdk::EventType::ButtonRelease => event
            .downcast_ref::<gdk::ButtonEvent>()
            .is_some_and(|button| button.button() == gdk::BUTTON_PRIMARY),
        gdk::EventType::TouchBegin | gdk::EventType::TouchEnd => true,
        gdk::EventType::KeyPress => event.downcast_ref::<gdk::KeyEvent>().is_some_and(|key| {
            [
                gdk::Key::space,
                gdk::Key::KP_Space,
                gdk::Key::Return,
                gdk::Key::KP_Enter,
                gdk::Key::ISO_Enter,
            ]
            .contains(&key.keyval())
        }),
        _ => false,
    }
}

/// 사람이 실제로 눌렀을 때만 act를 부른다. 단추의 이벤트를 단추보다 먼저(캡처 단계에서) 보고 적어 둔다
fn on_human_press(button: &gtk::Button, act: impl Fn() + 'static) {
    let press = Rc::new(HumanPress::default());
    let watch = gtk::EventControllerLegacy::new();
    watch.set_propagation_phase(gtk::PropagationPhase::Capture);
    let record = press.clone();
    watch.connect_event(move |_, event| {
        if is_press(event) {
            record.record(Instant::now());
        }
        glib::Propagation::Proceed
    });
    button.add_controller(watch);
    button.connect_clicked(move |_| {
        if press.take(Instant::now()) {
            act();
        } else {
            eprintln!("bunt-shell: ignored a press without real input on the AI control window");
        }
    });
}

/// 떠 있는 AI 제어 창
struct Panel {
    window: gtk::ApplicationWindow,
    card: gtk::Box,
    /// 밝게·어둡게를 따르는 처리기. 닫을 때 끊는다
    scheme: Option<(gio::Settings, glib::SignalHandlerId)>,
    /// 창을 연 뒤 AI가 제어 중이었는지 ([`shown_state`])
    seen_active: Cell<bool>,
}

/// 창에 보일 상태. 창이 떠 있는 동안 컴포지터는 에이전트의 입력을 받지 않으므로(사람만 답하는 창) AI는 몇 초 뒤 대기
/// 상태가 된다. 창을 연 사람이 보던 것이 그 때문에 바뀌지 않게, 창을 연 뒤 제어 중이었으면 제어 중으로 둔다
fn shown_state(state: &AgentState, seen_active: bool) -> AgentState {
    AgentState {
        active: state.active || seen_active,
        ..state.clone()
    }
}

/// 메뉴 막대 표시와 AI 제어 창. 셸이 끝날 때까지 산다
struct AgentControl {
    app: gtk::Application,
    tr: Rc<Translations>,
    button: gtk::Button,
    badge: gtk::Box,
    image: gtk::Image,
    /// 마지막으로 받은 상태. None이면 컴포지터가 없거나 이 API가 없다
    state: RefCell<Option<AgentState>>,
    backend: RefCell<Option<Backend>>,
    panel: RefCell<Option<Panel>>,
    /// 나타나는 애니메이션을 다시 시작하려고 번갈아 붙이는 클래스의 차례
    appear: Cell<bool>,
}

/// 메뉴 막대의 AI 표시. AI가 동작하거나 사람이 멈춘 동안만 보이고, 누르면 AI 제어 창을 연다
pub fn indicator(app: &gtk::Application, tr: &Rc<Translations>) -> gtk::Button {
    let image = gtk::Image::builder()
        .icon_name(AGENT_ICON)
        .halign(gtk::Align::Center)
        .hexpand(true)
        .build();
    // 기호는 캡슐 안 가운데에 두려고 넓히지만, 그 넓힘이 메뉴 막대로 번지면 단추가 막대의 남는 너비를 다 차지해
    // 표시가 막대 가운데로 간다. 캡슐에서 끊는다
    let badge = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(false)
        .css_classes(["agent-badge"])
        .build();
    badge.append(&image);
    let button = gtk::Button::builder()
        .child(&badge)
        .css_classes(["agent-indicator"])
        .visible(false)
        .build();
    let control = Rc::new(AgentControl {
        app: app.clone(),
        tr: tr.clone(),
        button: button.clone(),
        badge,
        image,
        state: RefCell::default(),
        backend: RefCell::default(),
        panel: RefCell::default(),
        appear: Cell::new(false),
    });
    let weak = Rc::downgrade(&control);
    button.connect_clicked(move |_| {
        if let Some(control) = weak.upgrade() {
            control.toggle_panel();
        }
    });
    match fake_state(std::env::var(FAKE_ENV).ok().as_deref()) {
        Some(state) => {
            control.backend.replace(Some(Backend::Fake));
            control.apply(Some(state));
        }
        None => {
            let control = control.clone();
            gio::bus_get(
                gio::BusType::Session,
                gio::Cancellable::NONE,
                move |result| match result {
                    Ok(connection) => {
                        let target = control.clone();
                        let compositor =
                            Compositor::watch(&connection, move |state| target.apply(state));
                        control
                            .backend
                            .replace(Some(Backend::Compositor(compositor)));
                    }
                    Err(err) => eprintln!("bunt-shell: session bus: {err}"),
                },
            );
        }
    }
    // 처리기가 약한 참조만 드므로 단추가 사는 동안(셸이 끝날 때까지) 함께 산다
    button.connect_destroy(move |_| {
        let _ = &control;
    });
    button
}

impl AgentControl {
    fn apply(self: &Rc<Self>, state: Option<AgentState>) {
        self.state.replace(state.clone());
        self.show_indicator(state.as_ref().map_or(Mode::Idle, AgentState::mode));
        match state {
            Some(state) => self.refill(&state),
            // 컴포지터가 사라졌다: 멈추거나 다시 허용할 곳이 없다
            None => self.close_panel(),
        }
    }

    fn show_indicator(&self, mode: Mode) {
        let Some(label) = mode.indicator_label() else {
            self.button.set_visible(false);
            return;
        };
        let label = self.tr.get(label);
        self.button.set_tooltip_text(Some(&label));
        self.button
            .update_property(&[gtk::accessible::Property::Label(&label)]);
        let paused = mode == Mode::Paused;
        self.image
            .set_icon_name(Some(if paused { PAUSE_ICON } else { AGENT_ICON }));
        if paused {
            self.badge.add_css_class("paused");
        } else {
            self.badge.remove_css_class("paused");
        }
        if !self.button.is_visible() {
            // 나타날 때 흐리게 시작한다. 같은 애니메이션을 다시 시작하려고 이름만 다른 두 클래스를 번갈아 붙인다
            let next = !self.appear.get();
            self.appear.set(next);
            let (add, remove) = if next {
                ("appear-a", "appear-b")
            } else {
                ("appear-b", "appear-a")
            };
            self.button.remove_css_class(remove);
            self.button.add_css_class(add);
            self.button.set_visible(true);
        }
    }

    fn toggle_panel(self: &Rc<Self>) {
        if self.panel.borrow().is_some() {
            self.close_panel();
            return;
        }
        let state = self.state.borrow().clone();
        if let Some(state) = state {
            self.open_panel(&state);
        }
    }

    fn open_panel(self: &Rc<Self>, state: &AgentState) {
        // 모든 앱 패널처럼 화면 전체를 덮어 판 바깥 어디를 눌러도 닫히게 한다(메뉴 막대 포함)
        let window = launcher::overlay_window(&self.app, NAMESPACE);
        // 표시는 AI도 누를 수 있으므로 키보드는 사람이 창을 눌렀을 때만 받는다. 사람이 다른 앱에 치던 글이 이 창으로
        // 오지 않는다(AI 접근 요청 창과 같다)
        window.set_keyboard_mode(KeyboardMode::OnDemand);
        // 화면이 여럿이면 표시가 있는 화면에 띄운다
        let monitor = self
            .button
            .root()
            .and_then(|root| root.surface())
            .and_then(|surface| WidgetExt::display(&self.button).monitor_at_surface(&surface));
        if let Some(monitor) = monitor {
            window.set_monitor(Some(&monitor));
        }
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .css_classes(["agent-control-panel", "extra-menu"])
            .build();
        extras::follow_pointer(&card);
        window.set_child(Some(&card));
        let weak = Rc::downgrade(self);
        launcher::close_on_outside_press(&window, &card, move || {
            if let Some(control) = weak.upgrade() {
                control.close_panel();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            if let Some(control) = weak.upgrade() {
                control.close_panel();
            }
            glib::Propagation::Stop
        });
        window.add_controller(keys);
        let scheme = crate::interface_settings().map(|settings| {
            follow_dark(&window, &settings);
            let target = window.downgrade();
            let handler = settings.connect_changed(Some("color-scheme"), move |settings, _| {
                if let Some(window) = target.upgrade() {
                    follow_dark(&window, settings);
                }
            });
            (settings, handler)
        });
        self.panel.replace(Some(Panel {
            window: window.clone(),
            card,
            scheme,
            seen_active: Cell::new(false),
        }));
        self.refill(state);
        window.present();
    }

    /// 떠 있는 창의 내용을 이 상태로 다시 그리고 자리를 맞춘다
    fn refill(self: &Rc<Self>, state: &AgentState) {
        let (card, shown) = {
            let panel = self.panel.borrow();
            let Some(panel) = panel.as_ref() else {
                return;
            };
            panel
                .seen_active
                .set(panel.seen_active.get() || state.active);
            (
                panel.card.clone(),
                shown_state(state, panel.seen_active.get()),
            )
        };
        self.fill(&card, &shown);
        self.place(&card);
    }

    fn fill(self: &Rc<Self>, card: &gtk::Box, state: &AgentState) {
        while let Some(child) = card.first_child() {
            card.remove(&child);
        }
        let tr = &self.tr;
        let mode = state.mode();
        // 제목 줄: AI 기호(제어 중이면 AI 색), "AI 제어", 오른쪽에 지금 상태
        let title = gtk::Box::builder()
            .spacing(8)
            .css_classes(["extra-title"])
            .build();
        let icon = gtk::Image::from_icon_name(if mode == Mode::Paused {
            PAUSE_ICON
        } else {
            AGENT_ICON
        });
        icon.add_css_class("agent-control-icon");
        if mode == Mode::Active {
            icon.add_css_class("active");
        }
        title.append(&icon);
        title.append(
            &gtk::Label::builder()
                .label(tr.get("agent-control-title"))
                .xalign(0.0)
                .hexpand(true)
                .build(),
        );
        title.append(
            &gtk::Label::builder()
                .label(tr.get(mode.state_label()))
                .css_classes(["extra-value"])
                .build(),
        );
        card.append(&title);
        let text = match status_message(state) {
            (id, Some(client)) => {
                let mut args = FluentArgs::new();
                args.set("client", client.to_owned());
                tr.get_with(id, &args)
            }
            (id, None) => tr.get(id),
        };
        card.append(&wrapped(&text));
        if state.has_target() {
            card.append(
                &gtk::Label::builder()
                    .label(tr.get(mode.target_section()))
                    .xalign(0.0)
                    .css_classes(["extra-section"])
                    .build(),
            );
            card.append(&target_row(state, tr));
        }
        card.append(&separator());
        let (label, icon, paused) = mode.toggle();
        let toggle = toggle_row(&tr.get(label), icon);
        let weak = Rc::downgrade(self);
        on_human_press(&toggle, move || {
            if let Some(control) = weak.upgrade() {
                // 바뀐 것은 메뉴 막대 표시로 보인다. 창이 떠 있는 동안에는 컴포지터가 에이전트의 입력을 받지
                // 않으므로, 다시 허용한 뒤에도 창을 남겨 두면 AI가 계속 막힌다. 누른 단추를 다시 그리지 않게
                // 창을 먼저 닫는다
                control.close_panel();
                control.set_paused(paused);
            }
        });
        card.append(&toggle);
        card.append(&separator());
        let settings = gtk::Button::builder()
            .child(
                &gtk::Label::builder()
                    .label(tr.get("agent-control-settings"))
                    .xalign(0.0)
                    .build(),
            )
            .css_classes(["extra-row"])
            .build();
        let weak = Rc::downgrade(self);
        settings.connect_clicked(move |_| {
            menubar::open_settings(Some(SETTINGS_PAGE));
            if let Some(control) = weak.upgrade() {
                control.close_panel();
            }
        });
        card.append(&settings);
    }

    /// 판을 메뉴 막대의 표시 바로 아래에 둔다. 창은 화면 전체를 덮으므로(배타 영역을 무시한다) 창의 좌표가 곧 화면의
    /// 좌표이고, 메뉴 막대도 화면 왼쪽 위에서 가로 전체를 채운다
    fn place(&self, card: &gtk::Box) {
        let Some(menubar) = self.button.root().map(|root| root.upcast::<gtk::Widget>()) else {
            return;
        };
        let Some(bounds) = self.button.compute_bounds(&menubar) else {
            return;
        };
        card.set_margin_start(0);
        let (_, width, _, _) = card.measure(gtk::Orientation::Horizontal, -1);
        let center = f64::from(bounds.x() + bounds.width() / 2.0);
        card.set_margin_start(panel_x(center, width, menubar.width()));
        card.set_margin_top(menubar.height() + GAP);
    }

    fn set_paused(self: &Rc<Self>, paused: bool) {
        let fake = match &*self.backend.borrow() {
            Some(Backend::Compositor(compositor)) => {
                compositor.set_paused(paused);
                false
            }
            Some(Backend::Fake) => true,
            None => false,
        };
        if fake {
            let mut state = self.state.borrow().clone().unwrap_or_default();
            state.paused = paused;
            state.active = !paused;
            self.apply(Some(state));
        }
    }

    fn close_panel(&self) {
        let Some(panel) = self.panel.take() else {
            return;
        };
        if let Some((settings, handler)) = panel.scheme {
            settings.disconnect(handler);
        }
        // 사람만 답하는 창이 떠 있는 동안 컴포지터는 에이전트의 입력을 받지 않으므로 바로 내린다. 창을 지우는 것은
        // 다루고 있는 누르기가 끝난 뒤에 한다
        panel.window.set_visible(false);
        let window = panel.window;
        glib::idle_add_local_once(move || window.destroy());
    }
}

/// 다룬 앱 줄: 앱 아이콘, 이름, 창 제목. 누를 수 없다
fn target_row(state: &AgentState, tr: &Translations) -> gtk::Box {
    let info = (!state.app_id.is_empty())
        .then(|| gio_unix::DesktopAppInfo::new(&format!("{}.desktop", state.app_id)))
        .flatten();
    let icon = match info.as_ref().and_then(|info| info.icon()) {
        Some(icon) => {
            let image = gtk::Image::from_gicon(&icon);
            image.set_pixel_size(APP_ICON_SIZE);
            image
        }
        None => {
            let image = gtk::Image::from_icon_name(APP_ICON);
            image.add_css_class("row-icon");
            image
        }
    };
    let (name, title) = target_text(
        state,
        info.map(|info| info.name().to_string()),
        tr.get("agent-control-unknown-app"),
    );
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    text.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(28)
            .build(),
    );
    if let Some(title) = title {
        text.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(32)
                .css_classes(["extra-subtitle"])
                .build(),
        );
    }
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["extra-row", "agent-control-app"])
        .build();
    row.append(&icon);
    row.append(&text);
    row
}

/// 멈추기·다시 허용 줄: 메뉴 항목처럼 기호, 글, 오른쪽에 단축키(macOS 로고 메뉴의 "강제 종료… ⌥⌘⎋"와 같은 꼴)
fn toggle_row(label: &str, icon: &str) -> gtk::Button {
    let content = gtk::Box::builder().spacing(8).build();
    let glyph = gtk::Image::from_icon_name(icon);
    glyph.add_css_class("agent-control-glyph");
    content.append(&glyph);
    content.append(
        &gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .hexpand(true)
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(SHORTCUT)
            .css_classes(["agent-control-shortcut"])
            .build(),
    );
    let button = gtk::Button::builder()
        .child(&content)
        .css_classes(["extra-row"])
        .build();
    button.update_property(&[
        gtk::accessible::Property::Label(label),
        gtk::accessible::Property::KeyShortcuts(SHORTCUT),
    ]);
    button
}

/// 여러 줄로 감싸는 설명 글. 띄어 쓴 낱말 가운데에서 줄을 바꾸지 않는다
fn wrapped(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(keep_words(text))
        .use_markup(true)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .max_width_chars(TEXT_WIDTH)
        .css_classes(["extra-text"])
        .build()
}

fn separator() -> gtk::Separator {
    gtk::Separator::builder()
        .css_classes(["extra-separator"])
        .build()
}

/// 시스템 설정의 밝게·어둡게를 창의 dark 클래스로 따른다(main.rs의 follow_color_scheme과 같다)
fn follow_dark(window: &gtk::ApplicationWindow, settings: &gio::Settings) {
    if settings.string("color-scheme") == "prefer-dark" {
        window.add_css_class("dark");
    } else {
        window.remove_css_class("dark");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(paused: bool, active: bool) -> AgentState {
        AgentState {
            paused,
            active,
            client: "claude".into(),
            pid: 42,
            app_id: "org.gnome.TextEditor".into(),
            title: "README.md".into(),
        }
    }

    #[test]
    fn reads_the_state_the_compositor_sends() {
        let value = (
            true,
            false,
            "claude",
            42u32,
            "org.gnome.TextEditor",
            "README.md",
        )
            .to_variant();
        assert_eq!(value.type_().as_str(), STATE_TYPE);
        assert_eq!(AgentState::from_variant(&value), Some(state(true, false)));
        // 다른 꼴의 답(옛 컴포지터나 계약과 다른 컴포지터)은 받지 않는다
        assert_eq!(AgentState::from_variant(&(true, false).to_variant()), None);
        assert_eq!(AgentState::from_variant(&("claude",).to_variant()), None);
    }

    #[test]
    fn the_indicator_shows_while_active_or_paused() {
        assert_eq!(state(false, true).mode(), Mode::Active);
        assert_eq!(state(true, false).mode(), Mode::Paused);
        // 멈춘 동안 AI가 동작하려 해도 멈춤이 먼저다
        assert_eq!(state(true, true).mode(), Mode::Paused);
        assert_eq!(state(false, false).mode(), Mode::Idle);
        assert_eq!(Mode::Idle.indicator_label(), None);
        assert!(Mode::Active.indicator_label().is_some());
        assert_ne!(
            Mode::Active.indicator_label(),
            Mode::Paused.indicator_label()
        );
    }

    #[test]
    fn the_button_resumes_only_when_paused() {
        assert_eq!(
            Mode::Paused.toggle(),
            ("agent-control-resume", RESUME_ICON, false)
        );
        assert_eq!(
            Mode::Active.toggle(),
            ("agent-control-pause", PAUSE_ICON, true)
        );
        // 지금 동작하지 않아도 미리 멈출 수 있다
        assert_eq!(
            Mode::Idle.toggle(),
            ("agent-control-pause", PAUSE_ICON, true)
        );
    }

    #[test]
    fn the_status_names_the_ai_while_it_acts() {
        assert_eq!(
            status_message(&state(false, true)),
            ("agent-control-active", Some("claude"))
        );
        let unnamed = AgentState {
            client: String::new(),
            ..state(false, true)
        };
        assert_eq!(
            status_message(&unnamed),
            ("agent-control-active-unknown", None)
        );
        assert_eq!(
            status_message(&state(true, true)),
            ("agent-control-paused", None)
        );
        assert_eq!(
            status_message(&state(false, false)),
            ("agent-control-idle", None)
        );
    }

    #[test]
    fn the_target_row_names_the_app_and_the_window() {
        let unknown = || "알 수 없는 앱".to_owned();
        let editor = state(false, true);
        assert_eq!(
            target_text(&editor, Some("텍스트 편집기".into()), unknown()),
            ("텍스트 편집기".into(), Some("README.md".into()))
        );
        // 데스크톱 항목이 없으면 앱 ID다
        assert_eq!(
            target_text(&editor, None, unknown()),
            ("org.gnome.TextEditor".into(), Some("README.md".into()))
        );
        // 앱 ID도 없으면 알 수 없는 앱과 창 제목이다
        let untitled = AgentState {
            app_id: String::new(),
            ..editor.clone()
        };
        assert!(untitled.has_target());
        assert_eq!(
            target_text(&untitled, None, unknown()),
            (unknown(), Some("README.md".into()))
        );
        // 제목이 이름과 같으면 두 번 보이지 않는다
        let same = AgentState {
            title: "Firefox".into(),
            ..editor.clone()
        };
        assert_eq!(
            target_text(&same, Some("Firefox".into()), unknown()),
            ("Firefox".into(), None)
        );
        // 다룬 앱을 모르면(사람이 미리 멈췄다) 묶음을 보이지 않는다
        let nothing = AgentState {
            paused: true,
            ..AgentState::default()
        };
        assert!(!nothing.has_target());
    }

    #[test]
    fn the_open_window_keeps_showing_control_until_paused() {
        // 창을 연 뒤 AI가 대기 상태가 되어도(창이 에이전트 입력을 막는다) 제어 중으로 보인다
        assert_eq!(shown_state(&state(false, false), true).mode(), Mode::Active);
        // 사람이 단축키로 멈추면 멈춤이 보인다
        assert_eq!(shown_state(&state(true, false), true).mode(), Mode::Paused);
        // 창을 연 뒤 한 번도 제어 중이 아니었으면 대기 중이다
        assert_eq!(shown_state(&state(false, false), false).mode(), Mode::Idle);
        assert_eq!(shown_state(&state(false, true), false).mode(), Mode::Active);
    }

    #[test]
    fn the_panel_stays_under_the_indicator_and_on_screen() {
        // 1280px 화면, 270px 판. 가운데 표시 아래에 가운데를 맞춘다
        assert_eq!(panel_x(640.0, 270, 1280), 505);
        // 오른쪽 끝 가까이면 가장자리에서 8px 안쪽에 멈춘다
        assert_eq!(panel_x(1250.0, 270, 1280), 1280 - 270 - EDGE);
        // 왼쪽도 같다
        assert_eq!(panel_x(20.0, 270, 1280), EDGE);
    }

    #[test]
    fn the_fake_state_is_only_for_known_values() {
        assert_eq!(fake_state(None), None);
        assert_eq!(fake_state(Some("")), None);
        assert_eq!(fake_state(Some("on")), None);
        let active = fake_state(Some("active")).unwrap();
        assert_eq!(active.mode(), Mode::Active);
        assert!(active.has_target());
        assert_eq!(fake_state(Some("paused")).unwrap().mode(), Mode::Paused);
    }

    #[test]
    fn a_press_counts_only_right_after_real_input() {
        let press = HumanPress::default();
        let now = Instant::now();
        // 접근성 동작처럼 실제 입력 없이 눌렸다
        assert!(!press.take(now));
        press.record(now);
        assert!(press.take(now + Duration::from_millis(300)));
        // 한 번 누르기로 한 번만 받는다
        assert!(!press.take(now + Duration::from_millis(400)));
        // 오래된 누르기는 받지 않는다
        press.record(now);
        assert!(!press.take(now + HUMAN_PRESS_WINDOW + Duration::from_millis(1)));
    }

    /// 접근성의 누르기(AT-SPI click 동작은 gtk_widget_activate다)와 프로그램이 부른 clicked처럼 실제 입력이 없는
    /// 누르기는 받지 않는다. GTK는 처음 초기화한 스레드에서만 쓸 수 있어 따로 띄운 프로세스에서 돈다(input_mode의
    /// 시험과 같다). 화면이 없으면 건너뛴다. 화면 없는 곳에서는 xvfb-run cargo test로 돌린다
    #[test]
    fn a_press_without_real_input_is_ignored() {
        const INSIDE: &str = "BUNT_SHELL_TEST_GTK_PROCESS";
        const TEST: &str = "agent_control::tests::a_press_without_real_input_is_ignored";
        if std::env::var_os(INSIDE).is_none() {
            // 출력은 파일로 받는다(input_mode의 시험: 파이프는 TestDBus의 감시 프로세스가 물려받아 멈췄다)
            let log = std::env::temp_dir().join(format!(
                "bunt-shell-agent-control-test-{}.log",
                std::process::id()
            ));
            let file = std::fs::File::create(&log).unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(INSIDE, "1")
                .env("GSK_RENDERER", "cairo")
                .stdout(file.try_clone().unwrap())
                .stderr(file)
                .status()
                .unwrap();
            let output = std::fs::read_to_string(&log).unwrap_or_default();
            let _ = std::fs::remove_file(&log);
            assert!(status.success(), "{output}");
            return;
        }
        if gtk::init().is_err() {
            eprintln!("no display; skipping the AI control press test");
            return;
        }
        let window = gtk::Window::new();
        let button = gtk::Button::with_label("AI");
        let (acted, clicked) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let count = acted.clone();
        on_human_press(&button, move || count.set(count.get() + 1));
        let count = clicked.clone();
        button.connect_clicked(move |_| count.set(count.get() + 1));
        window.set_child(Some(&button));
        window.present();
        let context = glib::MainContext::default();
        let pump = |until: Instant| {
            while Instant::now() < until {
                if !context.iteration(false) {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !button.is_realized() {
            assert!(Instant::now() < deadline, "the window did not open");
            context.iteration(false);
        }
        assert!(button.activate());
        button.emit_clicked();
        // 단추는 activate 뒤 250ms에 clicked를 낸다
        pump(Instant::now() + Duration::from_millis(600));
        assert_eq!(clicked.get(), 2, "the button was not pressed");
        assert_eq!(acted.get(), 0);
    }

    #[test]
    fn a_reply_older_than_a_signal_is_dropped() {
        let latest = Latest::default();
        // 읽는 동안 멈췄다는 신호가 먼저 왔다. 그 전에 읽은 답(제어 중)은 버린다
        let started = latest.read_started();
        let paused = Some(state(true, false));
        assert_eq!(latest.signalled(paused.clone()), Some(paused.clone()));
        assert_eq!(latest.answered(started, Some(state(false, true))), None);
        assert_eq!(*latest.state.borrow(), paused);
        // 같은 상태가 거듭 와도 한 번만 바뀐다
        assert_eq!(latest.signalled(paused.clone()), None);
        // 컴포지터가 사라졌다
        assert_eq!(latest.signalled(None), Some(None));
    }

    /// 컴포지터 노릇을 하는 객체를 개인 버스에 두고 본다: 컴포지터가 나중에 떠도 처음 상태를 읽고, 신호로 바뀐 상태를
    /// 받고, 멈춤을 알리면 컴포지터가 받고 셸이 다시 읽는다. 컴포지터가 사라지면 숨기고(None), 이 API가 없는 옛
    /// 컴포지터면 기록 없이 숨긴 채로 둔다. dbus-daemon이 없으면 건너뛴다
    #[test]
    fn follows_the_compositor_over_dbus() {
        const INTROSPECTION: &str = r#"<node>
  <interface name="org.buntos.Compositor1">
    <method name="GetAgentState">
      <arg type="b" direction="out"/><arg type="b" direction="out"/><arg type="s" direction="out"/>
      <arg type="u" direction="out"/><arg type="s" direction="out"/><arg type="s" direction="out"/>
    </method>
    <method name="SetAgentPaused"><arg type="b" direction="in"/></method>
    <signal name="AgentStateChanged">
      <arg type="b"/><arg type="b"/><arg type="s"/><arg type="u"/><arg type="s"/><arg type="s"/>
    </signal>
  </interface>
</node>"#;
        /// 이 API가 없는 옛 컴포지터
        const OLD_INTROSPECTION: &str = r#"<node>
  <interface name="org.buntos.Compositor1">
    <method name="GetInputMode"><arg type="s" direction="out"/></method>
  </interface>
</node>"#;
        fn to_variant(state: &AgentState) -> glib::Variant {
            (
                state.paused,
                state.active,
                state.client.as_str(),
                state.pid,
                state.app_id.as_str(),
                state.title.as_str(),
            )
                .to_variant()
        }
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

        let Some((_serial, bus)) = crate::service_dialog::test_bus() else {
            eprintln!("dbus-daemon is missing; skipping the D-Bus test");
            return;
        };
        let address = bus.bus_address().unwrap().to_string();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let connect = || {
                    gio::DBusConnection::for_address_sync(
                        &address,
                        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
                        None,
                        gio::Cancellable::NONE,
                    )
                    .unwrap()
                };
                let (shell, compositor) = (connect(), connect());
                let seen: Rc<RefCell<Vec<Option<AgentState>>>> = Rc::default();
                let record = seen.clone();
                let watcher =
                    Compositor::watch(&shell, move |state| record.borrow_mut().push(state));
                let wait_for = |count: usize| {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    while seen.borrow().len() < count {
                        assert!(Instant::now() < deadline, "{:?}", seen.borrow());
                        if !context.iteration(false) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                };
                let settle = || {
                    let until = Instant::now() + Duration::from_millis(300);
                    while Instant::now() < until {
                        if !context.iteration(false) {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                    }
                };

                // 셸이 먼저 뜨고 컴포지터가 나중에 이름을 얻는다. AI가 동작하는 중이었다
                let current = Rc::new(RefCell::new(state(false, true)));
                let requests: Rc<RefCell<Vec<bool>>> = Rc::default();
                let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).unwrap();
                let interface = node.lookup_interface(INTERFACE).unwrap();
                let (answer, asked) = (current.clone(), requests.clone());
                let registration = compositor
                    .register_object(PATH, &interface)
                    .method_call(
                        move |_, _, _, _, method, parameters, invocation| match method {
                            "GetAgentState" => {
                                invocation.return_value(Some(&to_variant(&answer.borrow())))
                            }
                            "SetAgentPaused" => {
                                let (paused,) = parameters.get::<(bool,)>().unwrap();
                                asked.borrow_mut().push(paused);
                                answer.borrow_mut().paused = paused;
                                invocation.return_value(None);
                            }
                            other => panic!("unexpected call {other}"),
                        },
                    )
                    .build()
                    .unwrap();
                call_bus(&compositor, "RequestName", (NAME, 0u32).to_variant());
                wait_for(1);
                assert_eq!(seen.borrow()[0], Some(state(false, true)));

                // 사람이 단축키로 멈췄다: 신호로 받는다. 같은 상태를 다시 알려도 한 번만 받는다
                current.replace(state(true, true));
                for _ in 0..2 {
                    compositor
                        .emit_signal(
                            None,
                            PATH,
                            INTERFACE,
                            "AgentStateChanged",
                            Some(&to_variant(&state(true, true))),
                        )
                        .unwrap();
                }
                wait_for(2);
                assert_eq!(seen.borrow()[1], Some(state(true, true)));

                // 창에서 다시 허용했다: 컴포지터가 받고, 셸은 신호가 없어도 다시 읽어 맞춘다
                watcher.set_paused(false);
                wait_for(3);
                assert_eq!(*requests.borrow(), [false]);
                assert_eq!(seen.borrow()[2], Some(state(false, true)));
                settle();
                assert_eq!(seen.borrow().len(), 3, "{:?}", seen.borrow());

                // 컴포지터가 끝났다: 표시를 숨긴다
                compositor.unregister_object(registration).unwrap();
                call_bus(&compositor, "ReleaseName", (NAME,).to_variant());
                wait_for(4);
                assert_eq!(seen.borrow()[3], None);

                // 이 API가 없는 옛 컴포지터가 떴다: 오류를 기록하지 않고 숨긴 채로 둔다
                let legacy = connect();
                let node = gio::DBusNodeInfo::for_xml(OLD_INTROSPECTION).unwrap();
                let interface = node.lookup_interface(INTERFACE).unwrap();
                let _old = legacy
                    .register_object(PATH, &interface)
                    .method_call(|_, _, _, _, method, _, _| panic!("unexpected call {method}"))
                    .build()
                    .unwrap();
                call_bus(&legacy, "RequestName", (NAME, 0u32).to_variant());
                let err = shell
                    .call_sync(
                        Some(NAME),
                        PATH,
                        INTERFACE,
                        "GetAgentState",
                        None,
                        None,
                        gio::DBusCallFlags::NONE,
                        -1,
                        gio::Cancellable::NONE,
                    )
                    .unwrap_err();
                assert!(missing_api(&err), "{err}");
                settle();
                assert_eq!(seen.borrow().len(), 4, "{:?}", seen.borrow());
            })
            .unwrap();
    }

    #[test]
    fn every_message_exists() {
        let mut ids = vec![
            "agent-control-title",
            "agent-control-active",
            "agent-control-active-unknown",
            "agent-control-paused",
            "agent-control-idle",
            "agent-control-unknown-app",
            "agent-control-settings",
        ];
        for mode in [Mode::Active, Mode::Paused, Mode::Idle] {
            ids.extend(mode.indicator_label());
            ids.push(mode.state_label());
            ids.push(mode.target_section());
            ids.push(mode.toggle().0);
        }
        for (language, source) in crate::TRANSLATIONS {
            let tr = Translations::from_source(language, source);
            for id in &ids {
                let text = tr.get(id);
                assert!(!text.is_empty() && text != *id, "{language}: {id}");
            }
        }
    }
}
