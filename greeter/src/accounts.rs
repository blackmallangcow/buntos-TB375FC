// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! AccountsService로 계정의 별칭(RealName)과 사진(IconFile)을 읽는다. 로그인 화면(login.rs)과 잠금 화면
//! (bunt-lock)이 함께 쓴다

use std::path::PathBuf;

use gtk4::{gio, glib, prelude::*};

const ACCOUNTS: &str = "org.freedesktop.Accounts";
const ACCOUNTS_PATH: &str = "/org/freedesktop/Accounts";
const ACCOUNTS_USER: &str = "org.freedesktop.Accounts.User";

/// AccountsService의 (RealName, IconFile)
pub async fn account_details(
    bus: &gio::DBusConnection,
    name: &str,
) -> Option<(String, Option<PathBuf>)> {
    let reply = bus
        .call_future(
            Some(ACCOUNTS),
            ACCOUNTS_PATH,
            ACCOUNTS,
            "FindUserByName",
            Some(&(name,).to_variant()),
            Some(glib::VariantTy::new("(o)").ok()?),
            gio::DBusCallFlags::NONE,
            5000,
        )
        .await
        .ok()?;
    let path = reply.child_value(0).str()?.to_owned();
    let property = async |property: &str| {
        let reply = bus
            .call_future(
                Some(ACCOUNTS),
                &path,
                "org.freedesktop.DBus.Properties",
                "Get",
                Some(&(ACCOUNTS_USER, property).to_variant()),
                Some(glib::VariantTy::new("(v)").ok()?),
                gio::DBusCallFlags::NONE,
                5000,
            )
            .await
            .ok()?;
        reply.child_value(0).as_variant()?.str().map(str::to_owned)
    };
    let real_name = property("RealName").await.unwrap_or_default();
    let icon = property("IconFile")
        .await
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    Some((real_name, icon))
}
