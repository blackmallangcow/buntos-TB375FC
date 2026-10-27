# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

menubar-logo = buntOS
# GLib DateTime format strings. 12-hour or 24-hour time is chosen in System Settings
menubar-clock-format-12h = %a %b %-d  %-I:%M %p
menubar-clock-format-24h = %a %b %-d  %H:%M
menu-about = About This System
menu-settings = System Settings…
menu-restart = Restart…
menu-power-off = Shut Down…
restart-confirm = Restart now?
power-off-confirm = Shut down now?
dialog-cancel = Cancel
dialog-restart = Restart
dialog-power-off = Shut Down

# Menus of the frontmost app, shown in the menu bar. $app is the app name
app-menu-about = About { $app }
app-menu-preferences = Settings…
app-menu-quit = Quit { $app }
app-menu-file = File
app-menu-new-window = New Window
app-menu-close = Close
app-menu-window = Window
app-menu-zoom = Zoom
app-menu-fullscreen-enter = Enter Full Screen
app-menu-fullscreen-leave = Exit Full Screen
app-menu-help = Help
app-menu-help-app = { $app } Help

network-wired = Connected to a wired network
network-wireless = Connected to Wi-Fi '{ $ssid }'
network-other = Connected to a network
network-limited = Connected, but no internet access
network-connecting = Connecting…
network-disconnected = Not connected to a network

notifications-title = Notification Center
notifications-clear = Clear All
notifications-empty = No new notifications
notifications-now = now
notifications-minutes-ago = { $minutes }m ago
notifications-hours-ago = { $hours }h ago
# Date of notifications older than a day. GLib DateTime format string
notifications-date-format = %b %-d

dock-terminal = Terminal
dock-apps = Apps
launcher-search = Search
