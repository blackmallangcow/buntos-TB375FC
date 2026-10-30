// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 스크린샷 화면 (macOS의 스크린샷): ⇧⌘5 도구 막대와, 찍은 뒤 화면 오른쪽 아래에 뜨는 미리
//! 보기(플로팅 썸네일). 찍기와 화면 기록은 컴포지터가 한다(org.buntos.Compositor1.Screenshot, Record).
//! 기록 중에는 메뉴 막대에 기록 중단 단추가 뜬다(recording.rs). 저장한 화면 기록도 마지막 프레임으로
//! 미리 보기를 띄운다(RecordingSaved). 도구 막대에서 고른
//! 방식, 저장 위치, 타이머, 썸네일 보기는 셸 설정(screenshot-*)에 둔다. 저장 위치와 썸네일 보기는
//! 컴포지터에 알려 단축키(⇧⌘3, ⇧⌘4)로 찍은 것에도 쓴다(SetScreenshotOptions). 컴포지터가 파일을
//! 저장했다고 알리면(ScreenshotSaved) 미리 보기를 5초 띄운다. 누르면 기본 앱으로 열고, 끌어서 다른
//! 앱에 놓을 수 있다. 포인터가 올라가 있는 동안은 사라지지 않는다.
//! 선택 부분을 고른 막대가 떠 있는 동안 컴포지터가 고칠 수 있는 선택 영역을 띄운다(SelectScreenshotArea).
//! 막대는 그 영역 위에 남고, 캡처(Enter)가 그 영역을 찍는다.

use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use bunt_i18n::Translations;
use gtk4::{self as gtk, gdk, gio, glib, prelude::*};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::{clip_input_to_child, dbus, layer_window, shell_settings};

const NAME: &str = "org.buntos.Compositor";
const PATH: &str = "/org/buntos/Compositor";
const INTERFACE: &str = "org.buntos.Compositor1";

/// 셸 설정 키 (data/org.buntos.shell.gschema.xml)
const MODE_KEY: &str = "screenshot-mode";
const LOCATION_KEY: &str = "screenshot-location";
const TIMER_KEY: &str = "screenshot-timer";
const THUMBNAIL_KEY: &str = "screenshot-thumbnail";
/// 선택 영역을 쓰는 방식 (MODES의 설정 값): 선택 부분 캡처, 선택 부분 기록. 고르면 컴포지터가 선택 영역을 띄운다
const AREA_MODES: [&str; 2] = ["area", "record-area"];
/// 도구 막대의 방식: (설정 값, 아이콘, 이름). 앞의 CAPTURE_MODES개는 캡처, 뒤는 화면 기록이다(macOS의 차례)
const MODES: [(&str, &str, &str); 5] = [
    (
        "screen",
        "org.buntos.CaptureScreen-symbolic",
        "screenshot-mode-screen",
    ),
    (
        "window",
        "org.buntos.CaptureWindow-symbolic",
        "screenshot-mode-window",
    ),
    (
        "area",
        "org.buntos.CaptureSelection-symbolic",
        "screenshot-mode-area",
    ),
    (
        "record-screen",
        "org.buntos.RecordScreen-symbolic",
        "screenshot-mode-record-screen",
    ),
    (
        "record-area",
        "org.buntos.RecordSelection-symbolic",
        "screenshot-mode-record-area",
    ),
];
/// 캡처 방식의 수 (MODES의 앞부분). 그 뒤의 화면 기록 방식과는 구분선으로 나눈다
const CAPTURE_MODES: usize = 3;
/// 옵션 메뉴의 저장 위치 (설정 값, 이름). macOS의 차례다
const LOCATIONS: [(&str, &str); 4] = [
    ("desktop", "screenshot-location-desktop"),
    ("documents", "screenshot-location-documents"),
    ("pictures", "screenshot-location-pictures"),
    ("clipboard", "screenshot-location-clipboard"),
];
/// 옵션 메뉴의 타이머 (초, 이름)
const TIMERS: [(i32, &str); 3] = [
    (0, "screenshot-timer-none"),
    (5, "screenshot-timer-5"),
    (10, "screenshot-timer-10"),
];
/// 도구 막대를 숨긴 뒤 찍기까지 기다리는 시간. 막대가 화면에서 사라진 뒤에 찍는다
const HIDE_BEFORE_CAPTURE: Duration = Duration::from_millis(200);
/// 미리 보기가 떠 있는 시간 (macOS)
const THUMBNAIL_SECONDS: u32 = 5;
/// 미리 보기의 긴 변 (논리 픽셀)
const THUMBNAIL_SIZE: f64 = 200.0;
/// 미리 보기 창 안의 그림자 여백: 옆 10px, 위 8px, 아래 12px (style.css의 .screenshot-thumbnail)
const THUMBNAIL_CHROME: (i32, i32) = (2 * 10, 8 + 12);

/// 컴포지터에 저장 위치와 썸네일 보기를 알린다. 컴포지터가 (다시) 뜰 때와 값이 바뀔 때마다 보낸다
pub fn share_options(settings: &gio::Settings) {
    let settings = settings.clone();
    gio::DBusProxy::for_bus(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES | gio::DBusProxyFlags::DO_NOT_CONNECT_SIGNALS,
        None,
        NAME,
        PATH,
        INTERFACE,
        gio::Cancellable::NONE,
        move |result| {
            let proxy = match result {
                Ok(proxy) => proxy,
                Err(err) => {
                    eprintln!("bunt-shell: {NAME}: {err}");
                    return;
                }
            };
            for key in [LOCATION_KEY, THUMBNAIL_KEY] {
                // 처리기가 프록시와 설정 객체를 붙들어 셸이 끝날 때까지 둔다
                let proxy = proxy.clone();
                settings
                    .connect_changed(Some(key), move |settings, _| send_options(&proxy, settings));
            }
            let keep_alive = settings.clone();
            proxy.connect_notify_local(Some("g-name-owner"), move |proxy, _| {
                send_options(proxy, &keep_alive);
            });
            // 변경 알림은 처리기를 단 뒤 한 번 읽어야 온다
            send_options(&proxy, &settings);
        },
    );
}

fn send_options(proxy: &gio::DBusProxy, settings: &gio::Settings) {
    let args = (
        settings.string(LOCATION_KEY).to_string(),
        settings.boolean(THUMBNAIL_KEY),
    )
        .to_variant();
    if dbus::is_running(proxy) {
        dbus::call(proxy, "SetScreenshotOptions", Some(&args));
    }
}

/// 컴포지터에 찍게 한다: screen(`delay`초 뒤 화면), window(창 고르기), area(띄워 둔 선택 영역)
fn capture(mode: &str, delay: u32) {
    call_compositor("Screenshot", (mode, delay).to_variant());
}

/// 컴포지터에 화면을 기록하게 한다: screen(포인터가 있는 화면), area(띄워 둔 선택 영역). `delay`초 뒤부터다
fn record(mode: &str, delay: u32) {
    call_compositor("Record", (mode, delay).to_variant());
}

/// 화면 기록 방식이면 컴포지터의 기록 방식 (record-screen은 screen, record-area는 area)
fn recording_mode(mode: &str) -> Option<&str> {
    mode.strip_prefix("record-")
}

thread_local! {
    /// 마지막으로 보낸 SelectScreenshotArea의 번호
    static AREA_SERIAL: Cell<u64> = const { Cell::new(0) };
}

/// 컴포지터가 고칠 수 있는 선택 영역을 띄우거나(`show`) 거두게 한다. 막대를 빨리 숨겼다 띄우면 두 부름이
/// 컴포지터에서 뒤바뀌어 닿을 수 있어, 늘 커지는 번호(단조 시계)를 붙여 늦게 닿은 옛 부름을 버리게 한다
fn select_area(show: bool) {
    let serial = AREA_SERIAL.with(|last| {
        let next = u64::try_from(glib::monotonic_time())
            .unwrap_or(0)
            .max(last.get() + 1);
        last.set(next);
        next
    });
    call_compositor("SelectScreenshotArea", (show, serial).to_variant());
}

/// 컴포지터의 메서드를 부른다. 답은 기다리지 않고 실패만 알린다. 같은 연결로 보내므로 부른 차례대로 닿는다
fn call_compositor(method: &'static str, args: glib::Variant) {
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let Ok(connection) = result else {
                return;
            };
            connection.call(
                Some(NAME),
                PATH,
                INTERFACE,
                method,
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                move |result| {
                    if let Err(err) = result {
                        eprintln!("bunt-shell: {INTERFACE}.{method}: {err}");
                    }
                },
            );
        },
    );
}

struct ToolbarInner {
    window: gtk::ApplicationWindow,
    settings: Option<gio::Settings>,
    /// 방식 단추 (MODES 차례)
    modes: Vec<gtk::ToggleButton>,
    /// 실행 단추와 그 글: 캡처 방식이면 캡처, 화면 기록 방식이면 기록
    shoot: gtk::Button,
    labels: (String, String),
}

/// ⇧⌘5 도구 막대: 화면 아래 가운데의 유리 막대. [닫기] | [전체 화면 캡처] [선택한 윈도우 캡처]
/// [선택 부분 캡처] | [전체 화면 기록] [선택 부분 기록] | [옵션] [캡처 또는 기록]. 선택 부분을 고른 채
/// 떠 있는 동안 컴포지터가 선택 영역을 띄운다
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub fn new(app: &gtk::Application, tr: &Translations) -> Self {
        let window = layer_window(app, "bunt-screenshot");
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Bottom, true);
        window.set_margin(Edge::Bottom, 12);
        // Esc는 닫고 Enter는 찍는다
        window.set_keyboard_mode(KeyboardMode::Exclusive);
        window.set_resizable(false);
        clip_input_to_child(&window);
        let settings = shell_settings();

        let bar = gtk::Box::builder()
            .spacing(4)
            .css_classes(["screenshot-toolbar"])
            .build();
        let close = icon_button("window-close-symbolic", &tr.get("screenshot-close"));
        close.add_css_class("screenshot-close");
        bar.append(&close);
        bar.append(&separator());
        let mut modes: Vec<gtk::ToggleButton> = Vec::new();
        for (index, (_, icon, label)) in MODES.into_iter().enumerate() {
            if index == CAPTURE_MODES {
                bar.append(&separator());
            }
            let button = gtk::ToggleButton::builder()
                .icon_name(icon)
                .tooltip_text(tr.get(label))
                .css_classes(["screenshot-mode"])
                .build();
            button.update_property(&[gtk::accessible::Property::Label(&tr.get(label))]);
            button.set_group(modes.first());
            bar.append(&button);
            modes.push(button);
        }
        bar.append(&separator());
        let options = gtk::MenuButton::builder()
            .label(tr.get("screenshot-options"))
            .menu_model(&options_menu(tr))
            .always_show_arrow(true)
            .css_classes(["screenshot-options"])
            .build();
        bar.append(&options);
        let shoot = gtk::Button::builder()
            .label(tr.get("screenshot-capture"))
            .css_classes(["screenshot-capture"])
            .build();
        bar.append(&shoot);
        window.set_child(Some(&bar));

        // 옵션 메뉴의 항목은 셸 설정을 바로 바꾸는 동작이다 (라디오, 체크)
        if let Some(settings) = &settings {
            let actions = gio::SimpleActionGroup::new();
            for key in [LOCATION_KEY, TIMER_KEY, THUMBNAIL_KEY] {
                actions.add_action(&settings.create_action(key));
            }
            window.insert_action_group("screenshot", Some(&actions));
        }

        let toolbar = Self(Rc::new(ToolbarInner {
            window,
            settings,
            modes,
            shoot: shoot.clone(),
            labels: (tr.get("screenshot-capture"), tr.get("screenshot-record")),
        }));
        for (index, button) in toolbar.0.modes.iter().enumerate() {
            let weak = Rc::downgrade(&toolbar.0);
            button.connect_toggled(move |button| {
                let (true, Some(inner)) = (button.is_active(), weak.upgrade()) else {
                    return;
                };
                let mode = MODES[index].0;
                // 떠 있는 막대에서 방식을 바꾸면 선택 영역을 띄우거나 거둔다 (macOS)
                if inner.window.is_visible() {
                    select_area(AREA_MODES.contains(&mode));
                }
                let (capture, record) = &inner.labels;
                let label = if recording_mode(mode).is_some() {
                    record
                } else {
                    capture
                };
                inner.shoot.set_label(label);
                if let Some(settings) = &inner.settings
                    && let Err(err) = settings.set_string(MODE_KEY, MODES[index].0)
                {
                    eprintln!("bunt-shell: {MODE_KEY}: {err}");
                }
            });
        }
        // 선택 부분을 고른 막대가 뜨면 선택 영역을 띄우고, 막대가 숨으면(닫기, Esc, 캡처, ⇧⌘5) 거둔다.
        // 캡처할 때는 거둔 영역을 컴포지터가 기억했다가 찍는다
        let weak = Rc::downgrade(&toolbar.0);
        toolbar.0.window.connect_map(move |_| {
            if let Some(inner) = weak.upgrade() {
                select_area(AREA_MODES.contains(&Toolbar(inner).mode()));
            }
        });
        toolbar.0.window.connect_unmap(|_| select_area(false));
        toolbar.close_when_locked();
        let weak = Rc::downgrade(&toolbar.0);
        close.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.window.set_visible(false);
            }
        });
        let weak = Rc::downgrade(&toolbar.0);
        shoot.connect_clicked(move |_| {
            if let Some(inner) = weak.upgrade() {
                Toolbar(inner).capture();
            }
        });
        // 키는 포커스를 가진 단추보다 먼저(캡처 단계) 받는다. 막대가 뜨면 닫기 단추가 포커스를 가져, 버블
        // 단계에서는 그 단추가 Return을 먼저 받아 찍지 않고 막대를 닫았다(macOS의 Return은 캡처다). 옵션 메뉴가
        // 열려 있으면 Return과 Esc는 메뉴의 것이다
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&toolbar.0);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(inner) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let in_menu = GtkWindowExt::focus(&inner.window)
                .is_some_and(|focus| focus.ancestor(gtk::Popover::static_type()).is_some());
            if in_menu {
                return glib::Propagation::Proceed;
            }
            match key {
                gdk::Key::Escape => inner.window.set_visible(false),
                gdk::Key::Return | gdk::Key::KP_Enter => Toolbar(inner).capture(),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        toolbar.0.window.add_controller(keys);
        toolbar
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    /// 화면을 잠그면 막대를 닫는다. 잠글 때 컴포지터가 선택 영역을 거두므로, 풀었을 때 막대만 남아 보이지 않는
    /// 영역을 찍거나 기록하지 않게 한다
    fn close_when_locked(&self) {
        let weak = Rc::downgrade(&self.0);
        gio::bus_get(
            gio::BusType::Session,
            gio::Cancellable::NONE,
            move |result| {
                let Ok(connection) = result else {
                    return;
                };
                let subscription = connection.subscribe_to_signal(
                    Some(NAME),
                    Some(INTERFACE),
                    Some("LockedChanged"),
                    Some(PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let (Some((true,)), Some(inner)) =
                            (signal.parameters.get::<(bool,)>(), weak.upgrade())
                        {
                            inner.window.set_visible(false);
                        }
                    },
                );
                // 셸이 끝날 때까지 받는다
                std::mem::forget(subscription);
            },
        );
    }

    /// ⇧⌘5: 열려 있으면 닫고, 아니면 마지막으로 고른 방식으로 연다
    pub fn toggle(&self) {
        let inner = &self.0;
        if inner.window.is_visible() {
            inner.window.set_visible(false);
            return;
        }
        let mode = inner
            .settings
            .as_ref()
            .map(|settings| settings.string(MODE_KEY).to_string())
            .unwrap_or_default();
        let index = MODES
            .iter()
            .position(|(name, _, _)| *name == mode)
            .unwrap_or(2);
        inner.modes[index].set_active(true);
        inner.window.present();
    }

    /// 막대에서 고른 찍기 방식 (MODES의 설정 값)
    fn mode(&self) -> &'static str {
        let index = self
            .0
            .modes
            .iter()
            .position(|button| button.is_active())
            .unwrap_or(2);
        MODES[index].0
    }

    /// 막대를 숨기고, 화면에서 사라진 뒤에 고른 방식과 타이머로 찍거나 기록을 시작한다. 선택 부분은 막대가
    /// 숨으며 거둔 선택 영역을 찍거나 기록한다
    fn capture(&self) {
        let inner = &self.0;
        let mode = self.mode();
        let delay = inner
            .settings
            .as_ref()
            .map_or(0, |settings| settings.int(TIMER_KEY).max(0) as u32);
        inner.window.set_visible(false);
        glib::timeout_add_local_once(HIDE_BEFORE_CAPTURE, move || match recording_mode(mode) {
            Some(mode) => record(mode, delay),
            None => capture(mode, delay),
        });
    }
}

/// 옵션 메뉴: 저장 위치, 타이머, 옵션 (macOS의 차례). 화면 기록과 다른 앱으로 보내기는 아직 없다
fn options_menu(tr: &Translations) -> gio::Menu {
    let menu = gio::Menu::new();
    let locations = gio::Menu::new();
    for (value, label) in LOCATIONS {
        let item = gio::MenuItem::new(Some(&tr.get(label)), None);
        item.set_action_and_target_value(
            Some(&format!("screenshot.{LOCATION_KEY}")),
            Some(&value.to_variant()),
        );
        locations.append_item(&item);
    }
    menu.append_section(Some(&tr.get("screenshot-save-to")), &locations);
    let timers = gio::Menu::new();
    for (seconds, label) in TIMERS {
        let item = gio::MenuItem::new(Some(&tr.get(label)), None);
        item.set_action_and_target_value(
            Some(&format!("screenshot.{TIMER_KEY}")),
            Some(&seconds.to_variant()),
        );
        timers.append_item(&item);
    }
    menu.append_section(Some(&tr.get("screenshot-timer")), &timers);
    let options = gio::Menu::new();
    options.append(
        Some(&tr.get("screenshot-show-thumbnail")),
        Some(&format!("screenshot.{THUMBNAIL_KEY}")),
    );
    menu.append_section(Some(&tr.get("screenshot-options")), &options);
    menu
}

fn icon_button(icon: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(label)
        .build();
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

fn separator() -> gtk::Separator {
    gtk::Separator::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["screenshot-separator"])
        .build()
}

struct ThumbnailInner {
    window: gtk::ApplicationWindow,
    card: gtk::Box,
    picture: gtk::Picture,
    /// 지금 보이는 스크린샷이나 화면 기록 파일 (누르면 열고 끌면 건넨다)
    file: RefCell<Option<gio::File>>,
    /// 접근성 이름: 스크린샷 미리 보기, 화면 기록 미리 보기
    labels: (String, String),
    /// 사라지기까지 남은 초. 포인터가 올라가 있는 동안은 세지 않는다
    left: Cell<u32>,
    hovered: Cell<bool>,
    countdown: RefCell<Option<glib::SourceId>>,
}

/// 찍은 뒤 화면 오른쪽 아래(Dock 위)에 뜨는 미리 보기 (macOS의 플로팅 썸네일)
#[derive(Clone)]
pub struct Thumbnail(Rc<ThumbnailInner>);

impl Thumbnail {
    pub fn new(app: &gtk::Application, tr: &Translations) -> Self {
        let window = layer_window(app, "bunt-screenshot-preview");
        window.set_layer(Layer::Overlay);
        window.set_anchor(Edge::Bottom, true);
        window.set_anchor(Edge::Right, true);
        // 카드의 그림자 여백(style.css)과 합쳐 화면 가장자리에서 20px 안쪽. Dock이 차지한 자리 위에 뜬다
        window.set_margin(Edge::Right, 10);
        window.set_margin(Edge::Bottom, 8);
        clip_input_to_child(&window);
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Fill)
            .can_shrink(true)
            .build();
        let card = gtk::Box::builder()
            .css_classes(["screenshot-thumbnail"])
            .overflow(gtk::Overflow::Hidden)
            .build();
        card.append(&picture);
        card.update_property(&[gtk::accessible::Property::Label(
            &tr.get("screenshot-preview"),
        )]);
        window.set_child(Some(&card));
        let thumbnail = Self(Rc::new(ThumbnailInner {
            window,
            card,
            picture,
            file: RefCell::default(),
            labels: (tr.get("screenshot-preview"), tr.get("recording-preview")),
            left: Cell::new(0),
            hovered: Cell::new(false),
            countdown: RefCell::default(),
        }));
        thumbnail.follow_pointer();
        thumbnail.follow_saved();
        thumbnail
    }

    pub fn window(&self) -> &gtk::ApplicationWindow {
        &self.0.window
    }

    /// 누르면 기본 앱으로 열고, 끌면 파일을 건넨다. 포인터가 올라가 있는 동안은 사라지지 않는다
    fn follow_pointer(&self) {
        let inner = &self.0;
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(inner);
        motion.connect_enter(move |_, _, _| {
            if let Some(inner) = weak.upgrade() {
                inner.hovered.set(true);
            }
        });
        let weak = Rc::downgrade(inner);
        motion.connect_leave(move |_| {
            if let Some(inner) = weak.upgrade() {
                inner.hovered.set(false);
            }
        });
        inner.card.add_controller(motion);

        let click = gtk::GestureClick::new();
        let weak = Rc::downgrade(inner);
        click.connect_released(move |_, _, _, _| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if let Some(file) = inner.file.borrow().as_ref()
                && let Err(err) = gio::AppInfo::launch_default_for_uri(
                    &file.uri(),
                    None::<&gio::AppLaunchContext>,
                )
            {
                eprintln!("bunt-shell: cannot open the screenshot: {err}");
            }
            Thumbnail(inner).hide();
        });
        inner.card.add_controller(click);

        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::COPY);
        let weak = Rc::downgrade(inner);
        drag.connect_prepare(move |drag, _, _| {
            let inner = weak.upgrade()?;
            let file = inner.file.borrow().clone()?;
            if let Some(paintable) = inner.picture.paintable() {
                drag.set_icon(Some(&paintable), 0, 0);
            }
            Some(gdk::ContentProvider::for_value(
                &gdk::FileList::from_array(&[file]).to_value(),
            ))
        });
        let weak = Rc::downgrade(inner);
        drag.connect_drag_end(move |_, _, _| {
            if let Some(inner) = weak.upgrade() {
                Thumbnail(inner).hide();
            }
        });
        inner.card.add_controller(drag);
    }

    /// 컴포지터가 스크린샷이나 화면 기록을 저장했다고 알리면 띄운다. 화면 기록은 마지막 프레임의 그림을 보인다
    fn follow_saved(&self) {
        let (thumbnail, recordings) = (self.clone(), self.clone());
        gio::bus_get(
            gio::BusType::Session,
            gio::Cancellable::NONE,
            move |result| {
                let connection = match result {
                    Ok(connection) => connection,
                    Err(err) => {
                        eprintln!("bunt-shell: session bus: {err}");
                        return;
                    }
                };
                let subscription = connection.subscribe_to_signal(
                    Some(NAME),
                    Some(INTERFACE),
                    Some("ScreenshotSaved"),
                    Some(PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let Some((path,)) = signal.parameters.get::<(String,)>() {
                            let path = PathBuf::from(path);
                            thumbnail.show(&path, path.clone(), false);
                        }
                    },
                );
                let recorded = connection.subscribe_to_signal(
                    Some(NAME),
                    Some(INTERFACE),
                    Some("RecordingSaved"),
                    Some(PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |signal| {
                        if let Some((path, preview)) = signal.parameters.get::<(String, String)>() {
                            recordings.show(Path::new(&preview), PathBuf::from(path), true);
                        }
                    },
                );
                // 셸이 끝날 때까지 받는다
                std::mem::forget(subscription);
                std::mem::forget(recorded);
            },
        );
    }

    /// 그림(`picture`)을 THUMBNAIL_SECONDS초 띄운다. 누르면 `path`를 열고 끌면 건넨다(스크린샷은 그림
    /// 자신, 화면 기록은 동영상). 이미 떠 있으면 새 것으로 바꾼다. 전에는 스크린샷 파일 하나만 받았다
    fn show(&self, picture: &Path, path: PathBuf, recording: bool) {
        let inner = &self.0;
        let file = gio::File::for_path(&path);
        let texture = match gdk::Texture::from_file(&gio::File::for_path(picture)) {
            Ok(texture) => texture,
            Err(err) => {
                eprintln!("bunt-shell: cannot read {}: {err}", picture.display());
                return;
            }
        };
        let (screenshot_label, recording_label) = &inner.labels;
        let label = if recording {
            recording_label
        } else {
            screenshot_label
        };
        inner
            .card
            .update_property(&[gtk::accessible::Property::Label(label)]);
        let (width, height) = thumbnail_size(texture.width(), texture.height());
        inner.picture.set_paintable(Some(&texture));
        inner.picture.set_size_request(width, height);
        inner.file.replace(Some(file));
        // 크기가 다른 스크린샷이면 창을 다시 띄워 그 크기로 만든다
        inner.window.set_visible(false);
        inner
            .window
            .set_default_size(width + THUMBNAIL_CHROME.0, height + THUMBNAIL_CHROME.1);
        inner.card.remove_css_class("shown");
        inner.window.present();
        let card = inner.card.clone();
        glib::timeout_add_local_once(Duration::from_millis(16), move || {
            card.add_css_class("shown")
        });
        inner.left.set(THUMBNAIL_SECONDS);
        inner.hovered.set(false);
        if inner.countdown.borrow().is_some() {
            return;
        }
        let weak = Rc::downgrade(inner);
        let countdown = glib::timeout_add_seconds_local(1, move || {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if !inner.window.is_visible() {
                inner.countdown.take();
                return glib::ControlFlow::Break;
            }
            if inner.hovered.get() {
                return glib::ControlFlow::Continue;
            }
            let left = inner.left.get().saturating_sub(1);
            inner.left.set(left);
            if left > 0 {
                return glib::ControlFlow::Continue;
            }
            inner.countdown.take();
            Thumbnail(inner).hide();
            glib::ControlFlow::Break
        });
        inner.countdown.replace(Some(countdown));
    }

    fn hide(&self) {
        let inner = &self.0;
        inner.card.remove_css_class("shown");
        inner.window.set_visible(false);
    }
}

/// 스크린샷 그림(`width`×`height`)을 긴 변 THUMBNAIL_SIZE에 비율을 지켜 맞춘 크기
fn thumbnail_size(width: i32, height: i32) -> (i32, i32) {
    let (width, height) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let scale = THUMBNAIL_SIZE / width.max(height);
    (
        (width * scale).round().max(1.0) as i32,
        (height * scale).round().max(1.0) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_keep_the_picture_shape() {
        // 1280×800 화면은 200×125, 세로로 긴 창은 높이가 200이다
        assert_eq!(thumbnail_size(1280, 800), (200, 125));
        assert_eq!(thumbnail_size(300, 600), (100, 200));
        assert_eq!(thumbnail_size(0, 0), (200, 200));
    }

    #[test]
    fn menus_and_modes_match_the_settings_schema() {
        let schema = include_str!("../data/org.buntos.shell.gschema.xml");
        for (value, _, _) in MODES {
            assert!(
                schema.contains(&format!("<choice value=\"{value}\"/>")),
                "{value}"
            );
        }
        // 선택 영역을 쓰는 방식과 화면 기록 방식은 막대에 있는 방식이다
        for mode in AREA_MODES {
            assert!(MODES.iter().any(|(value, _, _)| *value == mode), "{mode}");
        }
        assert_eq!(recording_mode("record-area"), Some("area"));
        assert_eq!(recording_mode("record-screen"), Some("screen"));
        assert_eq!(recording_mode("area"), None);
        // 캡처 방식이 앞에, 화면 기록 방식이 뒤에 모여 있다 (그 사이에 구분선을 둔다)
        let records: Vec<bool> = MODES
            .iter()
            .map(|(value, _, _)| recording_mode(value).is_some())
            .collect();
        assert_eq!(records, [false, false, false, true, true]);
        assert!(records[..CAPTURE_MODES].iter().all(|record| !record));
        for (value, _) in LOCATIONS {
            assert!(
                schema.contains(&format!("<choice value=\"{value}\"/>")),
                "{value}"
            );
        }
        for key in [MODE_KEY, LOCATION_KEY, TIMER_KEY, THUMBNAIL_KEY] {
            assert!(schema.contains(&format!("<key name=\"{key}\"")), "{key}");
        }
    }
}
