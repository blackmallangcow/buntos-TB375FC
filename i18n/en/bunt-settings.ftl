# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

app-title = System Settings
page-about = About
page-appearance = Appearance
page-datetime = Date & Time
page-language = Language
unknown = Unknown

about-device-group = This Device
about-device-name = Device Name
about-device-name-apply = Change
about-device-name-saved = Device name changed to { $name }
about-device-name-error = Could not change the device name: { $error }
about-model = Model
about-software-group = Software
about-os = Operating System
about-os-based-on = buntOS (based on { $base })
about-kernel = Kernel
about-architecture = Architecture
about-hardware-group = Hardware
about-processor = Processor
about-processor-cores = { $count ->
    [one] 1 core
   *[other] { $count } cores
}
about-processor-model = { $model }, { $count ->
    [one] 1 core
   *[other] { $count } cores
}
about-memory = Memory
about-disk = Storage
about-disk-free = { $free } available of { $total }
about-display = Display
about-display-mode = { $width } × { $height }, { $refresh } Hz

appearance-style = Style
appearance-style-light = Light
appearance-style-dark = Dark
appearance-unavailable = There is nowhere to store appearance settings (gsettings-desktop-schemas is not installed)

datetime-timezone = Time Zone
datetime-timezone-error = Could not change the time zone: { $error }
datetime-automatic = Set Time Automatically
datetime-automatic-error = Could not change automatic time: { $error }
datetime-24h = 24-Hour Time

language-description = A new language takes effect after a restart
language-display = Display Language
language-saved = { $language } will be used after a restart
language-error = Could not save the language setting: { $error }
