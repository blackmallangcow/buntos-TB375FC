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
launcher-search = Search Applications
launcher-empty = No Matching Apps
launcher-category-utilities = Utilities
launcher-category-developer = Developer Tools
launcher-category-productivity = Productivity
launcher-category-creativity = Creativity
launcher-category-internet = Internet
launcher-category-entertainment = Entertainment
launcher-category-games = Games
launcher-category-education = Education & Reference
launcher-category-other = Other

## Menu bar extras (Wi-Fi, battery, sound, Bluetooth, input sources)

network-title = Network
network-settings = Network Settings…
wifi-title = Wi-Fi
wifi-known = Known Networks
wifi-other = Other Networks
wifi-none = No Networks Found
wifi-settings = Wi-Fi Settings…
power-title = Power
power-battery = Battery
power-battery-percent = Battery { $percent }%
power-adapter = Using Power Adapter
power-source-adapter = Power Source: Power Adapter
power-source-battery = Power Source: Battery
power-fully-charged = Fully Charged
power-until-full = { $time } until full
power-remaining = { $time } remaining
power-duration = { $hours ->
    [0] { $minutes } min
   *[other] { $hours } hr { $minutes } min
}
power-mode = Power Mode
power-mode-low = Low Power Mode
power-mode-automatic = Automatic
power-mode-high = High Power
power-settings = Battery Settings…
sound-title = Sound
sound-output = Output
sound-volume = Volume
sound-mute = Mute
bluetooth-title = Bluetooth
bluetooth-devices = Devices
bluetooth-no-devices = No Paired Devices
bluetooth-connected = Connected
bluetooth-settings = Bluetooth Settings…
input-source-title = Input Source
input-source-abc = ABC
input-source-hangul = 2-Set Korean
input-source-shortcut = Switch with Ctrl+Space or the Han/Eng key
input-source-settings = Keyboard Settings…

## Control Center

cc-title = Control Center
cc-on = On
cc-off = Off
cc-wired = Ethernet
cc-unavailable = Unavailable
cc-focus = Do Not Disturb
cc-dark-mode = Dark Mode
cc-low-power = Low Power Mode
cc-display = Display
cc-brightness = Brightness
cc-edit = Menu Bar Settings…

## Administrator authentication (polkit agent). The title is the requesting app's name, or auth-title
auth-title = Authentication Required
auth-hint = Enter your password to allow this.
auth-hint-admin = Enter an administrator’s name and password to allow this.
auth-user-name = User Name
auth-password = Password
auth-cancel = Cancel
auth-confirm = OK
# Per-action descriptions (auth-action-<polkit action ID with . replaced by ->), used instead of polkit's message.
# $app is the requesting app's name (auth-someone if unknown), $program is what pkexec will run
auth-someone = A program
auth-action-org-buntos-agent-access-change = { $app } wants to change which AI can control this computer.
auth-action-org-freedesktop-timedate1-set-timezone = { $app } wants to change the time zone.
auth-action-org-freedesktop-timedate1-set-time = { $app } wants to change the system time.
auth-action-org-freedesktop-timedate1-set-ntp = { $app } wants to change whether the date and time are set automatically.
auth-action-org-freedesktop-timedate1-set-local-rtc = { $app } wants to change the time zone of the hardware clock.
auth-action-org-freedesktop-hostname1-set-static-hostname = { $app } wants to change the computer name.
auth-action-org-freedesktop-hostname1-set-hostname = { $app } wants to change the computer name.
auth-action-org-freedesktop-hostname1-set-machine-info = { $app } wants to change the computer information.
auth-action-org-freedesktop-policykit-exec = { $app } wants to run { $program } as an administrator.
