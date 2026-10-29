# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

app-title = Login

# Login window
login-password-placeholder = Enter Password
login-submit = Log In
login-no-accounts = There are no accounts that can log in.
login-password-for = Password for { $name }
login-wrong-password = Incorrect password.
login-failed = Couldn’t log in: { $error }
# Clock (GLib date formats). The large clock has no AM/PM, like the macOS lock screen
clock-date-format = %A, %B %-d
clock-time-format-12h = %-I:%M
clock-time-format-24h = %H:%M
power-restart = Restart
power-off = Shut Down

# First-boot account setup (macOS Setup Assistant)
setup-title = Create a Computer Account
setup-description =
    Choose the name and password you’ll use to log in to this computer and change its settings.
    This account will be an administrator of this computer.
setup-full-name = Nickname
setup-account-name = Account Name
setup-account-name-hint = This will be the name of your home folder and can’t be changed later.
setup-password = Password
setup-required = Required
setup-password-hint = At least { $min } characters. Used at the login window and for administrator authentication (sudo and others).
setup-verify = Verify
setup-verify-placeholder = Verify password
setup-continue = Continue
setup-creating = Creating account…
setup-signing-in = Logging in…
setup-preview = The preview doesn’t create accounts.
setup-error-full-name = Enter a nickname. Colons (:) and commas (,) aren’t allowed.
setup-error-account-name = The account name must start with a lowercase letter and contain only lowercase letters, digits, - and _ (up to 32 characters).
setup-error-account-taken = That name is already in use. Enter a different account name.
setup-error-password-short = The password must be at least { $min } characters.
setup-error-password-invalid = The password can’t contain line breaks or be too long.
setup-error-password-mismatch = The passwords don’t match. Enter them again.
setup-error-failed = Couldn’t create the account: { $error }
