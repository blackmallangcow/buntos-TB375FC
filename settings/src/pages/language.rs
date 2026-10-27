// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 언어: 그래픽 세션의 표시 언어. 기본 이미지에 들어 있는 로캘만 고를 수 있다

use std::rc::Rc;

use bunt_i18n::FluentArgs;
use gtk4::{self as gtk, prelude::*};

use super::Context;
use crate::{
    system,
    widgets::{group, page, row},
};

/// (로캘, 그 언어로 쓴 언어 이름). 이미지의 mkosi.postinst.chroot가 만드는 로캘과 맞춘다
const LANGUAGES: [(&str, &str); 2] = [("ko_KR.UTF-8", "한국어"), ("en_US.UTF-8", "English")];

pub fn build(context: &Rc<Context>) -> gtk::Widget {
    let tr = &context.tr;
    let (scroller, column) = page();
    let list = group(&column, None, Some(&tr.get("language-description")));

    let dropdown = gtk::DropDown::from_strings(&LANGUAGES.map(|(_, name)| name));
    // 고른 적이 없으면 지금 세션의 언어를 보여 준다
    let current = system::session_language().unwrap_or_else(bunt_i18n::current_language);
    if let Some(index) = LANGUAGES
        .iter()
        .position(|(locale, _)| same_language(locale, &current))
    {
        dropdown.set_selected(index as u32);
    }

    let context = context.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        let Some((locale, name)) = LANGUAGES.get(dropdown.selected() as usize) else {
            return;
        };
        match system::set_session_language(locale) {
            Ok(()) => {
                let mut args = FluentArgs::new();
                args.set("language", *name);
                context.notify(&context.tr.get_with("language-saved", &args));
            }
            Err(err) => context.notify_error("language-error", err),
        }
    });
    list.append(&row(&tr.get("language-display"), &dropdown));
    scroller.upcast()
}

/// 로캘 이름(ko_KR.UTF-8)이나 언어 코드(ko)가 같은 언어를 가리키는지
fn same_language(locale: &str, other: &str) -> bool {
    let language = |text: &str| {
        text.split(['_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    language(locale) == language(other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_match_by_code() {
        assert!(same_language("ko_KR.UTF-8", "ko"));
        assert!(same_language("en_US.UTF-8", "en_GB.UTF-8"));
        assert!(!same_language("ko_KR.UTF-8", "en_US.UTF-8"));
    }
}
