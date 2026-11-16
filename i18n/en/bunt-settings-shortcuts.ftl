# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

## Keyboard Shortcuts sheet (settings/src/pages/keyboard/shortcuts.rs). Follows the macOS 26 Keyboard Shortcuts sheet

# Shortcuts group on the Keyboard page
keyboard-shortcuts-button = Keyboard Shortcuts…
# Value of the Switch Input Source row. $shortcut: the shortcut set in the compositor (e.g. ⌃Space)
keyboard-switch-keys-with = { $shortcut }, Hangul key
keyboard-switch-keys-hangul = Hangul key

# Sheet
shortcuts-title = Keyboard Shortcuts
shortcuts-restore = Restore Defaults
shortcuts-none = none
# Shown in the shortcut field while it waits for keys
shortcuts-recording = Type Shortcut
# $name: the other shortcut that uses these keys
shortcuts-in-use = “{ $name }” already uses this shortcut.
shortcuts-needs-modifier = Press the key together with ⌃, ⌥ or ⌘. Function keys (F1 and so on) can be used alone.
shortcuts-no-shift = Text editing shortcuts can’t use ⇧. Adding ⇧ extends the selection.
# Only modifier keys were pressed and released (only input sources accept that)
shortcuts-needs-key = Modifier keys alone can’t be this shortcut. Press another key too.
shortcuts-error = Couldn’t change the shortcut: { $error }
shortcuts-unavailable-title = Keyboard Shortcuts Unavailable
shortcuts-unavailable-description = The compositor (bunt-comp) does not provide the keyboard shortcuts API.
# Key names inside shortcut symbols
shortcuts-key-space = Space
shortcuts-key-hangul = Hangul
# Below the category list: which PC keyboard key each symbol stands for
shortcuts-legend-control = Ctrl key
shortcuts-legend-option = Alt key
shortcuts-legend-shift = Shift key
shortcuts-legend-command = Super (Windows) key

# Categories (category IDs of the compositor)
shortcut-category-launcher = Apps & Dock
shortcut-category-windows = Apps & Windows
shortcut-category-input = Input Sources
shortcut-category-screenshots = Screenshots
shortcut-category-text = Text Editing
shortcut-category-app = App Shortcuts
# Notes below the list
shortcuts-note-input = You can also use modifier keys alone, like ⌃⇧ (press and release), or ⇧Space. The Hangul (한/영) key also switches input sources and can’t be changed.
shortcuts-note-text = These work while you type in a text field. Adding ⇧ extends the selection.
shortcuts-note-app = Sent to the app in front as its Ctrl shortcut. Terminals get Copy, Paste and the like as their own shortcuts (Ctrl+Shift+C…).

# Shortcuts. Message IDs are the compositor’s shortcut IDs with . replaced by - (app.copy → shortcut-app-copy)
shortcut-launcher-show = Show Apps
shortcut-launcher-dock-autohide = Turn Dock hiding on/off
shortcut-windows-next-app = Switch to next app
shortcut-windows-previous-app = Switch to previous app
shortcut-windows-next-window = Move focus to next window
shortcut-windows-previous-window = Move focus to previous window
shortcut-windows-quit-app = Quit app
shortcut-windows-minimize = Minimize window
shortcut-windows-hide-app = Hide app
shortcut-windows-hide-others = Hide others
shortcut-windows-fullscreen = Toggle full screen
shortcut-windows-close = Close window
shortcut-windows-terminal = Open Terminal
shortcut-input-switch-source = Switch input source
shortcut-screenshot-screen = Save picture of screen as a file
shortcut-screenshot-screen-clipboard = Copy picture of screen to the clipboard
shortcut-screenshot-area = Save picture of selected area as a file
shortcut-screenshot-area-clipboard = Copy picture of selected area to the clipboard
shortcut-screenshot-toolbar = Screenshot and recording options
shortcut-text-line-start = Move to beginning of line
shortcut-text-line-end = Move to end of line
shortcut-text-document-start = Move to beginning of document
shortcut-text-document-end = Move to end of document
shortcut-text-word-left = Move to previous word
shortcut-text-word-right = Move to next word
shortcut-text-paragraph-up = Move to previous paragraph
shortcut-text-paragraph-down = Move to next paragraph
shortcut-text-delete-word = Delete word to the left
shortcut-text-delete-word-forward = Delete word to the right
shortcut-text-delete-line = Delete to beginning of line
shortcut-app-cut = Cut
shortcut-app-copy = Copy
shortcut-app-paste = Paste
shortcut-app-undo = Undo
shortcut-app-redo = Redo
shortcut-app-select-all = Select All
shortcut-app-find = Find
shortcut-app-find-next = Find Next
shortcut-app-find-previous = Find Previous
shortcut-app-new = New
shortcut-app-new-tab = New Tab
shortcut-app-open = Open
shortcut-app-save = Save
shortcut-app-save-as = Save As
shortcut-app-print = Print
shortcut-app-close = Close Tab or Window
shortcut-app-settings = App Settings
shortcut-app-back = Back
shortcut-app-forward = Forward
shortcut-app-reload = Reload
shortcut-app-zoom-in = Zoom In
shortcut-app-zoom-out = Zoom Out
shortcut-app-zoom-reset = Actual Size
shortcut-app-other-command-keys = Send other ⌘ shortcuts to apps as Ctrl shortcuts
# Lock screen, log out and force quit (the Apple menu shortcuts on macOS)
shortcut-category-session = Lock Screen & Log Out
shortcut-windows-force-quit = Force Quit Applications
shortcut-session-lock-screen = Lock Screen
shortcut-session-log-out = Log Out
shortcut-session-log-out-now = Log Out Without Confirming
# AI control (the compositor's agent.pause; same words as the menu bar AI control panel)
shortcut-category-agent = AI Agents
shortcut-agent-pause = Pause or Resume AI Control
