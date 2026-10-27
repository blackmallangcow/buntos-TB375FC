// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 설정 페이지

mod about;
mod appearance;
mod datetime;
mod language;

use std::rc::Rc;

use bunt_i18n::{FluentArgs, Translations};
use gtk4::{self as gtk, gio};

use crate::widgets::Notice;

/// GNOME 공용 데스크톱 설정. bunt-shell과 GTK·libadwaita 앱이 같은 값을 읽는다
const INTERFACE_SCHEMA: &str = "org.gnome.desktop.interface";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    About,
    Appearance,
    DateTime,
    Language,
}

impl Page {
    /// 왼쪽 목록에 놓는 순서
    pub const ALL: [Page; 4] = [
        Page::About,
        Page::Appearance,
        Page::DateTime,
        Page::Language,
    ];

    /// 명령줄과 .desktop 파일의 동작에서 쓰는 이름
    pub fn name(self) -> &'static str {
        match self {
            Self::About => "about",
            Self::Appearance => "appearance",
            Self::DateTime => "datetime",
            Self::Language => "language",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|page| page.name() == name)
    }

    /// 제목의 Fluent 메시지 ID
    pub fn title_id(self) -> &'static str {
        match self {
            Self::About => "page-about",
            Self::Appearance => "page-appearance",
            Self::DateTime => "page-datetime",
            Self::Language => "page-language",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::About => "computer-symbolic",
            Self::Appearance => "preferences-desktop-appearance-symbolic",
            Self::DateTime => "preferences-system-time-symbolic",
            Self::Language => "preferences-desktop-locale-symbolic",
        }
    }

    pub fn build(self, context: &Rc<Context>) -> gtk::ScrolledWindow {
        match self {
            Self::About => about::build(context),
            Self::Appearance => appearance::build(context),
            Self::DateTime => datetime::build(context),
            Self::Language => language::build(context),
        }
    }
}

/// 페이지들이 함께 쓰는 것
pub struct Context {
    pub tr: Rc<Translations>,
    pub notice: Notice,
}

impl Context {
    /// 창 아래에 잠깐 알림을 띄운다.
    pub fn notify(&self, message: &str) {
        self.notice.show(message);
    }

    /// 실패 알림. 메시지의 $error 자리에 오류 내용을 넣는다
    pub fn notify_error(&self, message_id: &str, error: impl ToString) {
        let mut args = FluentArgs::new();
        args.set("error", error.to_string());
        self.notify(&self.tr.get_with(message_id, &args));
    }
}

/// 공용 데스크톱 설정. 스키마가 설치되지 않았으면 None이다.
pub fn interface_settings() -> Option<gio::Settings> {
    gio::SettingsSchemaSource::default()?.lookup(INTERFACE_SCHEMA, true)?;
    Some(gio::Settings::new(INTERFACE_SCHEMA))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_names_round_trip() {
        for page in Page::ALL {
            assert_eq!(Page::from_name(page.name()), Some(page));
        }
        assert_eq!(Page::from_name("no-such-page"), None);
    }
}
