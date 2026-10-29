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
notifications-expand = Show More
notifications-collapse = Show Less
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

## AI access request (agent_access.rs). Asks when an AI that is not allowed tries to control this computer, like the
## macOS prompt for accessibility access. $name is the AI program's name
agent-access-title = “{ $name }” would like to control this computer.
agent-access-message = If you allow it, this AI can read what apps show, press buttons and type text. You can allow it in System Settings > AI Agents.
agent-access-deny = Deny
agent-access-open = Open System Settings

# Screenshot toolbar (⇧⌘5) and the preview after taking one (macOS Screenshot)
screenshot-close = Close
screenshot-mode-screen = Capture Entire Screen
screenshot-mode-window = Capture Selected Window
screenshot-mode-area = Capture Selected Portion
screenshot-options = Options
screenshot-capture = Capture
screenshot-save-to = Save to
screenshot-location-desktop = Desktop
screenshot-location-documents = Documents
screenshot-location-pictures = Pictures
screenshot-location-clipboard = Clipboard
screenshot-timer = Timer
screenshot-timer-none = None
screenshot-timer-5 = 5 Seconds
screenshot-timer-10 = 10 Seconds
screenshot-show-thumbnail = Show Floating Thumbnail
screenshot-preview = Screenshot preview

## Session items in the logo menu (like the Apple menu on macOS)
menu-force-quit = Force Quit…
menu-sleep = Sleep
menu-lock-screen = Lock Screen
# $name: the user's full name (or the account name)
menu-log-out = Log Out { $name }…
## Logout confirmation (⇧⌘Q). Logs out by itself if nothing is chosen
logout-title = Are you sure you want to quit all applications and log out now?
# $seconds: seconds left before logging out by itself
logout-countdown = { $seconds ->
    [one] If you do nothing, the system will log out automatically in 1 second.
   *[other] If you do nothing, the system will log out automatically in { $seconds } seconds.
}
logout-confirm = Log Out
## Force Quit Applications window (⌥⌘⎋)
force-quit-title = Force Quit Applications
force-quit-message = If an app doesn’t respond for a while, select its name and click Force Quit.
force-quit-hint = You can open this window by pressing ⌥⌘⎋.
force-quit-button = Force Quit
force-quit-close = Close
force-quit-not-responding = (Not Responding)
force-quit-empty = No apps are running.
force-quit-unavailable = Couldn’t get the list of apps from the compositor.
# $app: the app's name
force-quit-confirm-title = Do you want to force “{ $app }” to quit?
force-quit-confirm-message = You will lose any unsaved changes.
# $error: the reason
force-quit-failed = Couldn’t force the app to quit: { $error }

## Wi-Fi password dialog (secret_agent.rs, NetworkManager secret agent), like the macOS Wi-Fi password dialog.
## $name is the network name (the SSID for Wi-Fi, otherwise the connection name), $security is wpa2, wpa3, wep or leap
secret-wifi-title = The Wi-Fi network “{ $name }” requires a { $security ->
        [wpa3] WPA3
        [wep] WEP
        [leap] LEAP
       *[wpa2] WPA2
    } password.
secret-wifi-enterprise-title = The Wi-Fi network “{ $name }” requires WPA2 enterprise credentials.
secret-wired-title = The network “{ $name }” requires a password.
secret-private-key-title = The network “{ $name }” requires the password of your certificate’s private key.
# When NetworkManager asks again after a wrong password (REQUEST_NEW)
secret-retry = The password is incorrect.
secret-user-name = User Name
secret-password = Password
secret-private-key-password = Private Key Password
secret-show-password = Show password
secret-cancel = Cancel
secret-join = Join
# Administrator authentication message (auth-action-…). NetworkManager asks when it saves a Wi-Fi password in a system connection
auth-action-org-freedesktop-NetworkManager-settings-modify-system = { $app } wants to change network settings for all users.

## Bluetooth pairing dialog (pairing_agent.rs, BlueZ pairing agent), like the macOS pairing request. $name is the device name
pairing-title = Bluetooth Pairing Request
pairing-confirm = Confirm that “{ $name }” shows the same code.
pairing-display = Type the following code on “{ $name }”, then press Return or Enter.
pairing-pin = Enter the PIN code for “{ $name }”. It is in the device’s manual (0000 or 1234 are common).
pairing-passkey = Enter the 6-digit code shown on “{ $name }”.
pairing-authorize = “{ $name }” wants to pair with this computer.
pairing-pin-placeholder = PIN Code
pairing-passkey-placeholder = Code
pairing-cancel = Cancel
pairing-connect = Connect
pairing-deny = Deny
pairing-allow = Allow
# When a device that is not trusted wants to use a service. $service is the service name (pairing-service-… below, or the UUID)
pairing-service-title = Bluetooth Connection Request
pairing-service = “{ $name }” wants to use the { $service } service of this computer.
pairing-service-audio = audio
pairing-service-handsfree = hands-free
pairing-service-input = keyboard and mouse
pairing-service-files = file transfer
pairing-service-network = network
pairing-service-serial = serial port
# Nearby devices in the menu bar Bluetooth menu (searched while the menu is open; click to pair)
bluetooth-nearby = Nearby Devices
