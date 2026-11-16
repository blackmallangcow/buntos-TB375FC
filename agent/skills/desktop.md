<!--
SPDX-FileCopyrightText: 2026 buntOS contributors
SPDX-License-Identifier: GPL-3.0-or-later
-->
# Working on the buntOS desktop

buntOS is a macOS-like desktop on Debian. An AI that the user allowed can read and control its
apps through `buntd-agent`, either with the `bunt agent` command or with JavaScript in the REPL
(`bunt agent repl`, or the MCP tool `repl`; see the `repl` skill).

## What the user decides

- Which AIs may control this computer (System Settings > AI Agents). The first time an AI that
  is not on the list connects, buntOS asks the user. When a call is refused with
  `NotAllowedError`, tell the user what the message says; only they can allow you.
- Which apps AIs may not touch. A blocked app is missing from `desktop.apps()` and every call on
  it fails with `BlockedAppError`. Do not try to reach it another way.
- Windows only a person answers: the administrator password dialog, Wi-Fi passwords, Bluetooth
  pairing and the lock screen. You cannot read them or type into them. Ask the user.
- While the screen is locked everything fails with `ScreenLockedError` until the user unlocks it.
- The user can pause AI control from the menu bar. Until they resume it, every action fails with
  `PausedError`; reading still works. Only they can resume it.

## Sharing the computer with the user

The user keeps working while you do. buntOS puts them first and never lets you take their
keyboard focus.

- Accessible actions need no keyboard focus and work while the user types elsewhere: `click`
  on elements with an accessible action, `fill`, `set`, `select`, `check`, and `type` or `press`
  on a text field outside web pages and browsers (`type` adds the text at the end of the field,
  `press "ctrl+a BackSpace"` clears it, `press Return` runs the field's activate action). Prefer
  them.
- Keys, text and the wheel without a ref (`desktop.keyboard`, `type`, `press` and `scroll`
  without `--app`) go only to the window you worked on last, and only while it still has the
  keyboard focus. When the user has moved the focus they fail with `FocusMovedError`, which names
  the window that has it. Use a ref, or name the app.
- Keys to an app (`app.keyboard`, `--app`), text on web pages and browser address bars, pointer
  clicks and the wheel go through the compositor. It checks that the window still has the
  keyboard focus (for the pointer: that it is on top at that point) and refuses while the user
  is typing or using the pointer: `UserActiveError`, with `retryAfterMs`. Nothing was sent.
- Never take the focus back: do not activate the window again or click again to get it. Read the
  window again and continue with refs, or try later.
- `snapshot()` without an app reads the window you worked on last, not the one the user is
  using. When there is none it reads the active window and says so in its first line.
- Opening an app while the user works in another window leaves it in the background.
- The first line of every action names the app and window that received it and the way:
  `accessibility` or `target-checked input`. `unchecked input` with a warning means the
  compositor could not check the target.

## Reading

1. `snapshot({ interactive: true })` of the app you work on: what can be clicked or typed into.
2. `snapshot()` when you need the text around it, or `locator(ref).snapshot()` for one part.
3. After an action, read the diff it returns instead of a new snapshot.
4. `screenshot()` only when the tree does not show it (a canvas, a game, a drawn chart); then
   `mouse.click(x, y)` with the pixels of that screenshot.

Trees show the window you worked on last unless you name an app. Name apps by desktop
ID (`org.gnome.Calculator`) to stay independent of the user's language; `desktop.installed()`
lists them. A window without a tree is listed with `tree: false`: Chromium, Electron, Firefox
and Qt apps started before an AI was first allowed expose it only after a restart, and some
terminals (foot) never do.

## Acting

- Prefer refs and roles over pixels. Actions wait until the window stops changing and report
  what changed, new windows, and whether the app quit.
- `fill` replaces a field's text and checks it; when the app drops the text (a browser address
  bar) it clears the field and types it, and says so. `type` with a ref adds text to that field
  in any language; without a ref it types into the focused field of your window (it fails
  rather than pressing keys elsewhere when no field has the focus; pass `keys: true` for
  terminals and games).
- Elements scrolled out of view in a web page are marked `[offscreen]`; acting on them scrolls
  them into view first. GTK lists only build the rows on screen: scroll the list and read again.
- Use `wait` for things that take time (a page loading, a dialog opening, a result appearing)
  instead of sleeping. `waitForLoadState()` (CLI `wait --loaded`) waits until the visible page has
  finished loading; waiting for a window does this too. A wait ends with `TargetGoneError` when
  the app quits or restarts.
- A diff of a replaced document is summed up when it is longer than 400 lines (`maxLines`, CLI
  `--max-lines`); take a snapshot to read the new page.

## Apps

- Calculator `org.gnome.Calculator`, Text Editor `org.gnome.TextEditor`, Files
  `org.gnome.Nautilus`, System Settings `org.buntos.Settings`, Firefox `firefox-esr`, Image
  Viewer `org.gnome.Loupe`, Document Viewer `org.gnome.Papers`; `desktop.installed()` lists the
  rest. The menu bar, Dock and Control Center belong to the app `bunt-shell`.
- Opening an app that is already open brings its window forward instead of opening a second one
  (it stays in the background while the user is using another window).
