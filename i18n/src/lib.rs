// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 사용자에게 보이는 문자열은 Fluent 리소스(i18n/<언어>/<컴포넌트>.ftl)에서 가져온다 (ARCHITECTURE 7장).
//! 각 컴포넌트는 자기 리소스를 `include_str!`로 넣어 [`Translations::load`]에 넘긴다.

use std::collections::BTreeSet;

pub use fluent_bundle::FluentArgs;
use fluent_bundle::{FluentBundle, FluentResource};
use fluent_syntax::ast::Entry;
use unic_langid::LanguageIdentifier;

/// 대체 언어. 모든 컴포넌트가 이 언어의 리소스를 가진다
const FALLBACK: &str = "en";

/// (언어 코드, Fluent 원문) 목록
pub type Sources = [(&'static str, &'static str)];

pub struct Translations {
    bundle: FluentBundle<FluentResource>,
}

impl Translations {
    /// 현재 언어([`current_language`])의 리소스를 쓰고, 없으면 영어를 쓴다.
    pub fn load(sources: &Sources) -> Self {
        let language = current_language();
        let (language, source) = sources
            .iter()
            .find(|(code, _)| *code == language)
            .or_else(|| sources.iter().find(|(code, _)| *code == FALLBACK))
            .copied()
            .expect("an English Fluent resource");
        Self::from_source(language, source)
    }

    pub fn from_source(language: &str, source: &str) -> Self {
        let language: LanguageIdentifier = language.parse().expect("valid language identifier");
        let resource = FluentResource::try_new(source.to_owned()).expect("valid Fluent resource");
        let mut bundle = FluentBundle::new(vec![language]);
        // GTK 레이블에 유니코드 방향 격리 문자가 끼지 않게 한다
        bundle.set_use_isolating(false);
        bundle
            .add_resource(resource)
            .expect("no duplicate Fluent messages");
        Self { bundle }
    }

    /// 메시지가 없으면 ID를 그대로 돌려준다.
    pub fn get(&self, id: &str) -> String {
        self.format(id, None)
    }

    pub fn get_with(&self, id: &str, args: &FluentArgs) -> String {
        self.format(id, Some(args))
    }

    fn format(&self, id: &str, args: Option<&FluentArgs>) -> String {
        let Some(pattern) = self
            .bundle
            .get_message(id)
            .and_then(|message| message.value())
        else {
            return id.to_owned();
        };
        let mut errors = Vec::new();
        self.bundle
            .format_pattern(pattern, args, &mut errors)
            .into_owned()
    }
}

/// LC_ALL, LC_MESSAGES, LANG 순으로 찾은 로캘의 언어 코드 (예: "ko_KR.UTF-8" → "ko")
pub fn current_language() -> String {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|value| !value.is_empty())
        .unwrap_or_default();
    language_of(&locale).to_owned()
}

fn language_of(locale: &str) -> &str {
    locale.split(['_', '.', '@']).next().unwrap_or_default()
}

/// 모든 언어의 리소스가 같은 메시지를 가지는지 확인한다. 컴포넌트의 테스트에서 쓴다.
pub fn check_same_messages(sources: &Sources) -> Result<(), String> {
    let mut languages = sources
        .iter()
        .map(|(code, source)| (code, message_ids(source)));
    let Some((first_code, first_ids)) = languages.next() else {
        return Ok(());
    };
    for (code, ids) in languages {
        if ids != first_ids {
            let only_first: Vec<_> = first_ids.difference(&ids).collect();
            let only_other: Vec<_> = ids.difference(&first_ids).collect();
            return Err(format!(
                "only in {first_code}: {only_first:?}, only in {code}: {only_other:?}"
            ));
        }
    }
    Ok(())
}

fn message_ids(source: &str) -> BTreeSet<String> {
    let resource = FluentResource::try_new(source.to_owned()).expect("valid Fluent resource");
    resource
        .entries()
        .filter_map(|entry| match entry {
            Entry::Message(message) => Some(message.id.name.to_owned()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "hello = Hello\ngreet = Hello, { $name }\n";
    const KO: &str = "hello = 안녕하세요\ngreet = { $name }님, 안녕하세요\n";

    #[test]
    fn language_comes_from_the_locale_name() {
        assert_eq!(language_of("ko_KR.UTF-8"), "ko");
        assert_eq!(language_of("en_US"), "en");
        assert_eq!(language_of("C.UTF-8"), "C");
        assert_eq!(language_of(""), "");
    }

    #[test]
    fn missing_message_falls_back_to_id() {
        let tr = Translations::from_source("en", EN);
        assert_eq!(tr.get("hello"), "Hello");
        assert_eq!(tr.get("no-such-message"), "no-such-message");
    }

    #[test]
    fn arguments_are_substituted() {
        let tr = Translations::from_source("ko", KO);
        let mut args = FluentArgs::new();
        args.set("name", "buntOS");
        assert_eq!(tr.get_with("greet", &args), "buntOS님, 안녕하세요");
    }

    #[test]
    fn differing_messages_are_reported() {
        assert!(check_same_messages(&[("ko", KO), ("en", EN)]).is_ok());
        assert!(check_same_messages(&[("ko", "hello = 안녕\n"), ("en", EN)]).is_err());
    }
}
