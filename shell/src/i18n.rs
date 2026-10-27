// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! 사용자에게 보이는 문자열은 Fluent 리소스(i18n/)에서 가져온다 (ARCHITECTURE 7장).

use fluent_bundle::{FluentBundle, FluentResource};
use unic_langid::LanguageIdentifier;

const KO: &str = include_str!("../../i18n/ko/bunt-shell.ftl");
const EN: &str = include_str!("../../i18n/en/bunt-shell.ftl");

pub struct Translations {
    bundle: FluentBundle<FluentResource>,
}

impl Translations {
    /// LC_ALL, LC_MESSAGES, LANG 순으로 언어를 고르고, 한국어가 아니면 영어를 쓴다.
    pub fn load() -> Self {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        let (language, source) = if locale.starts_with("ko") {
            ("ko", KO)
        } else {
            ("en", EN)
        };
        Self::from_source(language, source)
    }

    fn from_source(language: &str, source: &str) -> Self {
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
        let Some(pattern) = self
            .bundle
            .get_message(id)
            .and_then(|message| message.value())
        else {
            return id.to_owned();
        };
        let mut errors = Vec::new();
        self.bundle
            .format_pattern(pattern, None, &mut errors)
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use fluent_bundle::FluentResource;
    use fluent_syntax::ast::Entry;

    use super::*;

    fn message_ids(source: &str) -> BTreeSet<String> {
        let resource = FluentResource::try_new(source.to_owned()).unwrap();
        resource
            .entries()
            .filter_map(|entry| match entry {
                Entry::Message(message) => Some(message.id.name.to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_language_has_the_same_messages() {
        assert_eq!(message_ids(KO), message_ids(EN));
    }

    #[test]
    fn missing_message_falls_back_to_id() {
        let tr = Translations::from_source("en", EN);
        assert_eq!(tr.get("no-such-message"), "no-such-message");
        assert_eq!(tr.get("dock-terminal"), "Terminal");
    }
}
