// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 입력기(fcitx5)의 지금 입력 소스 (로그인 화면의 화면 키보드). 셸은 메뉴 막대의 입력 소스 항목
//! (shell/src/input_source.rs)을 같이 쓴다. 지금 입력기를 짧은 간격으로 읽어 따라가고(단축키로도 바뀐다), 지구본 키로
//! 지금 그룹의 다음 입력기로 바꾼다(컴포지터의 입력 소스 전환과 같다). 암호 칸에서는 fcitx5가 스스로 영문 배열을 쓰므로
//! 키보드도 QWERTY가 된다. fcitx5가 없으면 QWERTY에 지구본 키 없이 둔다.

use std::{cell::RefCell, rc::Rc, time::Duration};

use gtk4::{gio, glib, prelude::*};

use crate::layout::Script;

const FCITX: &str = "org.fcitx.Fcitx5";
const CONTROLLER_PATH: &str = "/controller";
const CONTROLLER: &str = "org.fcitx.Fcitx.Controller1";
/// 단축키로 바꾼 입력 소스를 따라가는 간격 (셸의 메뉴 막대와 같다)
const POLL_INTERVAL: Duration = Duration::from_millis(400);
const CALL_TIMEOUT: i32 = 1000;

/// 입력기 이름의 글자 배열: 한글 입력기면 2벌식, 그 밖은 QWERTY
pub fn script_of(input_method: &str) -> Script {
    match input_method {
        "hangul" => Script::Hangul,
        _ => Script::Latin,
    }
}

/// 그룹에서 지금 입력기 다음의 입력기 (끝에서는 처음으로)
pub fn next_in_group(group: &[String], current: &str) -> Option<String> {
    let index = group.iter().position(|item| item == current)?;
    group.get((index + 1) % group.len()).cloned()
}

#[derive(Clone)]
pub struct Fcitx(Rc<Inner>);

struct Inner {
    proxy: RefCell<Option<gio::DBusProxy>>,
    current: RefCell<String>,
    group: RefCell<Vec<String>>,
    /// (글자 배열, 지구본 키를 둘지)
    changed: Box<dyn Fn(Script, bool)>,
}

impl Fcitx {
    /// fcitx5를 따라가기 시작한다. 바뀔 때마다 (글자 배열, 입력 소스가 둘 이상인지)를 알린다
    pub fn start(changed: impl Fn(Script, bool) + 'static) -> Self {
        let fcitx = Self(Rc::new(Inner {
            proxy: RefCell::default(),
            current: RefCell::default(),
            group: RefCell::default(),
            changed: Box::new(changed),
        }));
        let weak = Rc::downgrade(&fcitx.0);
        glib::spawn_future_local(async move {
            let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else {
                return;
            };
            let Ok(proxy) = gio::DBusProxy::new_future(
                &bus,
                gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES
                    | gio::DBusProxyFlags::DO_NOT_CONNECT_SIGNALS,
                None,
                Some(FCITX),
                CONTROLLER_PATH,
                CONTROLLER,
            )
            .await
            else {
                return;
            };
            if let Some(inner) = weak.upgrade() {
                inner.proxy.replace(Some(proxy));
            }
        });
        let weak = Rc::downgrade(&fcitx.0);
        glib::timeout_add_local(POLL_INTERVAL, move || match weak.upgrade() {
            Some(inner) => {
                Fcitx(inner).poll();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        fcitx
    }

    /// 지금 그룹의 다음 입력기로 바꾼다 (지구본 키)
    pub fn next(&self) {
        let Some(proxy) = self.0.proxy.borrow().clone() else {
            return;
        };
        let Some(next) = next_in_group(&self.0.group.borrow(), &self.0.current.borrow()) else {
            return;
        };
        proxy.call(
            "SetCurrentIM",
            Some(&(next.as_str(),).to_variant()),
            gio::DBusCallFlags::NONE,
            CALL_TIMEOUT,
            gio::Cancellable::NONE,
            |result| {
                if let Err(err) = result {
                    eprintln!("bunt-keyboard: {CONTROLLER}.SetCurrentIM: {err}");
                }
            },
        );
        self.0.current.replace(next);
        self.notify();
    }

    fn notify(&self) {
        let script = script_of(&self.0.current.borrow());
        (self.0.changed)(script, self.0.group.borrow().len() > 1);
    }

    /// 지금 입력기를 읽는다. 그룹에 없는 입력기면 그룹을 다시 읽는다
    fn poll(&self) {
        let Some(proxy) = self.0.proxy.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        glib::spawn_future_local(async move {
            let Ok(reply) = proxy
                .call_future(
                    "CurrentInputMethod",
                    None,
                    gio::DBusCallFlags::NONE,
                    CALL_TIMEOUT,
                )
                .await
            else {
                return;
            };
            let current: String = reply.child_value(0).get().unwrap_or_default();
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if *inner.current.borrow() == current {
                return;
            }
            let known = inner.group.borrow().contains(&current);
            inner.current.replace(current);
            if !known {
                let group = read_group(&proxy).await;
                inner.group.replace(group);
            }
            Fcitx(inner).notify();
        });
    }
}

/// 지금 그룹의 입력기 이름들
async fn read_group(proxy: &gio::DBusProxy) -> Vec<String> {
    let call = |method: &'static str, args: Option<glib::Variant>| {
        proxy.call_future(
            method,
            args.as_ref(),
            gio::DBusCallFlags::NONE,
            CALL_TIMEOUT,
        )
    };
    let Ok(group) = call("CurrentInputMethodGroup", None).await else {
        return Vec::new();
    };
    let group: String = group.child_value(0).get().unwrap_or_default();
    let Ok(info) = call("InputMethodGroupInfo", Some((group,).to_variant())).await else {
        return Vec::new();
    };
    // (레이아웃, [(입력기, 레이아웃)])
    let items: Vec<(String, String)> = info.child_value(1).get().unwrap_or_default();
    items.into_iter().map(|(name, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hangul_uses_dubeolsik_and_everything_else_qwerty() {
        assert_eq!(script_of("hangul"), Script::Hangul);
        assert_eq!(script_of("keyboard-us"), Script::Latin);
        assert_eq!(script_of(""), Script::Latin);
    }

    #[test]
    fn the_globe_goes_round_the_group() {
        let group: Vec<String> = ["keyboard-us", "hangul"].map(String::from).to_vec();
        assert_eq!(
            next_in_group(&group, "keyboard-us").as_deref(),
            Some("hangul")
        );
        assert_eq!(
            next_in_group(&group, "hangul").as_deref(),
            Some("keyboard-us")
        );
        assert_eq!(next_in_group(&group, "mozc"), None);
        assert_eq!(next_in_group(&[], "hangul"), None);
    }
}
