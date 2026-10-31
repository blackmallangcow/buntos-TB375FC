# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

## Sound page (settings/src/pages/sound.rs), like System Settings > Sound in macOS 26

page-sound = Sound
page-sound-keywords = sound volume alert effects output input speaker headphones microphone balance mute
sound-description = Choose the alert sound, the output and input devices, and their volume.
sound-effects = Sound Effects
sound-alert = Alert sound
sound-alert-volume = Alert volume
# Alert sound names (one per sound name). Like macOS, named after how they sound rather than what they are for.
# These are the sounds of the buntOS sound theme (tools/sounds/generate.py)
sound-alert-message-new-instant = Chime
sound-alert-message = Droplet
sound-alert-bell = Bell
sound-alert-complete = Rise
sound-alert-dialog-information = Twinkle
sound-alert-dialog-warning = Thump
sound-alert-window-attention = Ping
# When the alert sound is set to an empty value (gsettings)
sound-alert-none = None
sound-output-input = Output & Input
sound-output = Output
sound-input = Input
sound-no-output = No Output Devices
sound-no-input = No Input Devices
# Device type in the device list
sound-kind-built-in = Built-in
sound-kind-hdmi = HDMI
sound-kind-usb = USB
sound-kind-bluetooth = Bluetooth
sound-output-volume = Output volume
sound-mute = Mute
sound-balance = Balance
sound-balance-left = Left
sound-balance-right = Right
sound-input-volume = Input volume
sound-input-level = Input level
sound-input-description = The input level opens the microphone only while this page shows input.
