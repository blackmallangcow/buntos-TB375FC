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

## Reading

1. `snapshot({ interactive: true })` of the app you work on: what can be clicked or typed into.
2. `snapshot()` when you need the text around it, or `locator(ref).snapshot()` for one part.
3. After an action, read the diff it returns instead of a new snapshot.
4. `screenshot()` only when the tree does not show it (a canvas, a game, a drawn chart); then
   `mouse.click(x, y)` with the pixels of that screenshot.

Trees name the window that has the keyboard focus unless you name an app. Name apps by desktop
ID (`org.gnome.Calculator`) to stay independent of the user's language; `desktop.installed()`
lists them. A window without a tree is listed with `tree: false`: Chromium, Electron, Firefox
and Qt apps started before an AI was first allowed expose it only after a restart, and some
terminals (foot) never do.

## Acting

- Prefer refs and roles over pixels. Actions wait until the window stops changing and report
  what changed, new windows, and whether the app quit.
- `fill` replaces a field's text and checks it; `type` types into the focused field in any
  language (it fails rather than pressing keys elsewhere when no field has the focus; pass
  `keys: true` for terminals and games).
- Elements scrolled out of view in a web page are marked `[offscreen]`; acting on them scrolls
  them into view first. GTK lists only build the rows on screen: scroll the list and read again.
- Use `wait` for things that take time (a page loading, a dialog opening, a result appearing)
  instead of sleeping.

## Apps

- Calculator `org.gnome.Calculator`, Text Editor `org.gnome.TextEditor`, Files
  `org.gnome.Nautilus`, System Settings `org.buntos.Settings`, Firefox `firefox-esr`, Image
  Viewer `org.gnome.Loupe`, Document Viewer `org.gnome.Papers`; `desktop.installed()` lists the
  rest. The menu bar, Dock and Control Center belong to the app `bunt-shell`.
- Opening an app that is already open brings its window forward instead of opening a second one.
