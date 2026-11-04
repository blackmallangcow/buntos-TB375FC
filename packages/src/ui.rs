// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 두 창이 함께 쓰는 것: 제목 막대, 밝게·어둡게, 입력 모드, 확인 시트, 앱 실행

use std::{cell::Cell, path::Path, rc::Rc};

use gtk4::{self as gtk, gdk, gio, glib, prelude::*};

const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";
/// 터치 모드에서 창에 다는 CSS 클래스 (디자인 스펙 11장)
const TOUCH_CLASS: &str = "touch";

/// 제목 막대: 왼쪽에 창 단추(신호등, theme/gtk4-apps.css), 가운데에 제목, 오른쪽에 도구
pub fn title_bar(window: &impl IsA<gtk::Window>, title: &str, end: Option<&gtk::Box>) {
    let bar = gtk::CenterBox::builder()
        .css_classes(["pm-titlebar"])
        .build();
    bar.set_start_widget(Some(&gtk::WindowControls::new(gtk::PackType::Start)));
    bar.set_center_widget(Some(
        &gtk::Label::builder()
            .label(title)
            .css_classes(["pm-window-title"])
            .build(),
    ));
    // 창 단추의 자리는 양쪽에 둔다. 로그인 직후 포털이 준비되기 전에는 GTK가 기본 배치(오른쪽 닫기)를 쓴다
    let end_side = gtk::Box::builder()
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();
    if let Some(end) = end {
        end_side.append(end);
    }
    end_side.append(&gtk::WindowControls::new(gtk::PackType::End));
    bar.set_end_widget(Some(&end_side));
    let handle = gtk::WindowHandle::builder().child(&bar).build();
    window.as_ref().set_titlebar(Some(&handle));
}

/// 밝게·어둡게 설정을 따른다. GTK 기본 테마는 prefer-dark로, 자체 CSS는 dark 클래스로 바꾼다 (시스템 설정과 같다)
pub fn follow_color_scheme(window: &impl IsA<gtk::Window>) {
    let schema_exists = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(INTERFACE_SCHEMA, true))
        .is_some();
    if !schema_exists {
        return;
    }
    let settings = gio::Settings::new(INTERFACE_SCHEMA);
    let window = window.as_ref().clone();
    let apply = |settings: &gio::Settings, window: &gtk::Window| {
        let dark = settings.string("color-scheme") == "prefer-dark";
        if let Some(gtk_settings) = gtk::Settings::default() {
            gtk_settings.set_gtk_application_prefer_dark_theme(dark);
        }
        if dark {
            window.add_css_class("dark");
        } else {
            window.remove_css_class("dark");
        }
    };
    apply(&settings, &window);
    let keep_alive = settings.clone();
    settings.connect_changed(
        Some("color-scheme"),
        glib::clone!(
            #[weak]
            window,
            move |settings, _| {
                // 변경 알림은 설정 객체가 살아 있을 때만 온다. 처리기가 객체를 붙든다
                let _ = &keep_alive;
                apply(settings, &window);
            }
        ),
    );
}

/// 입력 모드를 따른다. 컴포지터가 터치 모드라고 하면 모든 창에 touch 클래스를 달아 누를 영역을 넓힌다
/// (settings/src/input_mode.rs와 같은 신호). 앱이 뜰 때 한 번 부른다
pub fn follow_input_mode() {
    const NAME: &str = "org.buntos.Compositor";
    const PATH: &str = "/org/buntos/Compositor";
    const INTERFACE: &str = "org.buntos.Compositor1";
    let touch = Rc::new(Cell::new(false));
    let style = |window: &gtk::Widget, touch: bool| {
        if touch {
            window.add_css_class(TOUCH_CLASS);
        } else {
            window.remove_css_class(TOUCH_CLASS);
        }
    };
    // 새 창은 뜰 때 지금 모드를 받는다
    let current = touch.clone();
    gtk::Window::toplevels().connect_items_changed(move |list, position, _, added| {
        for index in position..position.saturating_add(added) {
            if let Some(window) = list.item(index).and_downcast::<gtk::Widget>() {
                let current = current.clone();
                window.connect_realize(move |window| style(window, current.get()));
            }
        }
    });
    gio::bus_get(
        gio::BusType::Session,
        gio::Cancellable::NONE,
        move |result| {
            let Ok(connection) = result else {
                return;
            };
            let apply = Rc::new(move |name: &str| {
                let is_touch = name == "touch";
                touch.set(is_touch);
                for window in gtk::Window::list_toplevels() {
                    style(&window, is_touch);
                }
            });
            let on_signal = apply.clone();
            let subscription = connection.subscribe_to_signal(
                Some(NAME),
                Some(INTERFACE),
                Some("InputModeChanged"),
                Some(PATH),
                None,
                gio::DBusSignalFlags::NONE,
                move |signal| {
                    if let Some((name,)) = signal.parameters.get::<(String,)>() {
                        on_signal(&name);
                    }
                },
            );
            // 앱이 끝날 때까지 받는다
            std::mem::forget(subscription);
            connection.call(
                Some(NAME),
                PATH,
                INTERFACE,
                "GetInputMode",
                None,
                Some(glib::VariantTy::new("(s)").expect("valid type")),
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                move |reply| {
                    if let Some((name,)) = reply.ok().and_then(|reply| reply.get::<(String,)>()) {
                        apply(&name);
                    }
                },
            );
        },
    );
}

/// 작은 확인 창(시트): 제목, 설명, 내용, 취소·확인. Esc로 닫고 Return은 확인이다
pub struct Sheet {
    pub window: gtk::Window,
    pub confirm: gtk::Button,
}

impl Sheet {
    pub fn new(
        parent: &impl IsA<gtk::Window>,
        title: &str,
        description: Option<&str>,
        cancel: &str,
        confirm: &str,
    ) -> Self {
        let parent = parent.as_ref();
        let window = gtk::Window::builder()
            .modal(true)
            .resizable(false)
            .default_width(380)
            .transient_for(parent)
            .css_classes(["pm-sheet"])
            .build();
        // 앱에 묶어야 창 목록에 앱 ID로 보인다
        window.set_application(parent.application().as_ref());
        if parent.has_css_class("dark") {
            window.add_css_class("dark");
        }
        // 제목 표시줄 없이 내용만 보인다. 숨긴 위젯을 제목 표시줄로 두면 GTK가 기본 막대를 만들지 않는다
        let titlebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        titlebar.set_visible(false);
        window.set_titlebar(Some(&titlebar));

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .css_classes(["pm-sheet-body"])
            .build();
        body.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::WordChar)
                .max_width_chars(44)
                .css_classes(["pm-sheet-title"])
                .build(),
        );
        if let Some(description) = description {
            body.append(
                &gtk::Label::builder()
                    .label(description)
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .max_width_chars(52)
                    .css_classes(["pm-secondary"])
                    .build(),
            );
        }
        let confirm = gtk::Button::builder()
            .label(confirm)
            .css_classes(["suggested-action"])
            .build();
        let cancel = gtk::Button::with_label(cancel);
        cancel.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| window.close()
        ));
        let buttons = gtk::Box::builder()
            .spacing(8)
            .halign(gtk::Align::End)
            .margin_top(6)
            .build();
        buttons.append(&cancel);
        buttons.append(&confirm);
        body.append(&buttons);
        window.set_child(Some(&body));
        window.set_default_widget(Some(&confirm));
        close_on_escape(&window);
        Self { window, confirm }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

/// Esc를 누르면 창을 닫는다
pub fn close_on_escape(window: &impl IsA<gtk::Window>) {
    let escape = gtk::ShortcutController::new();
    escape.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(|widget, _| {
            if let Some(window) = widget.downcast_ref::<gtk::Window>() {
                window.close();
            }
            glib::Propagation::Stop
        })),
    ));
    window.as_ref().add_controller(escape);
}

/// .desktop 파일의 앱을 실행한다
pub fn launch(
    desktop: &Path,
    widget: &impl IsA<gtk::Widget>,
    on_error: impl FnOnce(glib::Error) + 'static,
) {
    match gio_unix::DesktopAppInfo::from_filename(desktop) {
        Some(info) => launch_app(&info, widget, on_error),
        None => on_error(glib::Error::new(
            gio::IOErrorEnum::NotFound,
            &desktop.display().to_string(),
        )),
    }
}

/// 앱을 실행한다. D-Bus로 뜨는 앱(DBusActivatable)은 뜨는 사이에 부른 쪽이 세션 버스를 떠나면 dbus-daemon이 요청을 버려 창
/// 없이 떴다가 사라진다. 그래서 창을 닫아도 앱이 답할 때까지 이 프로세스를 남긴다
pub fn launch_app(
    info: &gio_unix::DesktopAppInfo,
    widget: &impl IsA<gtk::Widget>,
    on_error: impl FnOnce(glib::Error) + 'static,
) {
    let context = WidgetExt::display(widget.as_ref()).app_launch_context();
    let launching = info.launch_uris_future(&[], Some(&context));
    let hold = gio::Application::default().map(|app| app.hold());
    glib::spawn_future_local(async move {
        let result = launching.await;
        drop(hold);
        // 앱이 늦게 답할 뿐이면 실패가 아니다. 요청은 앱에 갔다
        if let Err(err) = result
            && !err.matches(gio::IOErrorEnum::TimedOut)
        {
            on_error(err);
        }
    });
}

/// .desktop 파일의 아이콘이 있는 그림. 없으면 패키지 그림이다
pub fn app_image(desktop: Option<&Path>, size: i32) -> gtk::Image {
    let icon = desktop
        .and_then(gio_unix::DesktopAppInfo::from_filename)
        .and_then(|info| info.icon());
    let image = match icon {
        Some(icon) => gtk::Image::from_gicon(&icon),
        None => gtk::Image::from_icon_name("org.buntos.PackageManager"),
    };
    image.set_pixel_size(size);
    image
}

/// 파일에서 읽은 아이콘의 내용으로 그림을 만든다. 그리지 못하면 None
pub fn texture_from_bytes(data: &[u8]) -> Option<gdk::Texture> {
    gdk::Texture::from_bytes(&glib::Bytes::from(data)).ok()
}
