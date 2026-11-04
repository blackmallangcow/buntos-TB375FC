# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

## Package Manager (bunt-packages)
app-name = Package Manager

## Formats and where things are installed
kind-deb = Debian package
kind-appimage = AppImage
kind-flatpak = Flatpak app
kind-flatpak-repo = Flatpak repository
location-system = System-wide (requires an administrator password)
location-user = This user's Applications folder (~/Applications)
location-flatpak = Flatpak (each app runs in a sandbox)
location-repo = Software repository list

## Install window: the steps on the left match the macOS Installer
installer-step-introduction = Introduction
installer-step-install = Installation
installer-step-summary = Summary
installer-title-reading = Reading the installer file
installer-title-install = Install { $name }
installer-title-installed = { $name } is already installed
installer-title-cannot = Can't Install
installer-title-installing = Installing
installer-title-removing = Removing
installer-title-summary = Summary
installer-version = Version { $version }
installer-version-newer = Version { $version } (newer than this file)
installer-fact-size = Installed size
installer-fact-maintainer = Made by
installer-fact-homepage = Website
installer-fact-source = Source
installer-fact-file = File
installer-fact-location = Location
installer-fact-installed = Installed version
installer-fact-also-install = Also installs
installer-fact-also-update = Also updates
installer-fact-also-remove = Also removes
installer-count-names = { $count }: { $names }
installer-trust-deb = This file did not come from the buntOS repositories. Only install files from a source you trust. The package's scripts run with administrator rights during installation.
installer-trust-appimage = Only install files from a source you trust. This app runs without a sandbox, with this user's rights.
installer-trust-flatpak = Only install files from a source you trust. The app runs in a sandbox.
installer-trust-repo = Apps from this repository will appear in Software. Only add repositories you trust.
installer-cancel = Cancel
installer-install = Install
installer-reinstall = Reinstall
installer-update = Update
installer-downgrade = Use This Version
installer-add-repo = Add Repository
installer-remove = Remove…
installer-close = Close
installer-open = Open
installer-status-reading = Reading the installer file…
installer-status-refresh = Downloading package lists…
installer-status-refresh-percent = Downloading package lists… { $percent }%
installer-status-dependencies = Checking required packages…
installer-status-preparing = Preparing to install…
installer-status-auth = Waiting for administrator authentication…
installer-status-lock = Waiting for another installation to finish…
installer-status-download = Downloading required packages…
installer-status-check = Checking packages…
installer-status-installing = Installing…
installer-status-removing = Removing…
installer-status-cleanup = Cleaning up…
installer-success = The installation was successful.
installer-success-details = The software was installed.
installer-repo-added = The repository was added.
installer-repo-added-details = Apps from this repository can be found in Software.
installer-repo-exists = This repository has already been added.
installer-removed = Removed.
installer-failed = The installation failed.
installer-failed-details = Error: { $error }
installer-move-to-trash = Move Installer File to Trash
installer-moved-to-trash = Moved to Trash
installer-not-authorized = Nothing was installed because administrator authentication didn't finish. Click the button to try again.
installer-not-local = Only files on this computer can be installed. Download the file and open it again.
installer-appimage-format = This AppImage uses a format buntOS can't unpack (type 1 or DwarFS). Look for a newer version, or a .deb or Flatpak version, from the developer.
installer-rpm = RPM packages are the Fedora family's format and can't be installed on buntOS. Look for a .deb, Flatpak or AppImage version from the developer.
installer-snap = Snap packages can't be installed on buntOS. Look for a .deb, Flatpak or AppImage version from the developer.
installer-unknown = This isn't an installer file that Package Manager knows. It can open .deb, AppImage, .flatpak, .flatpakref and .flatpakrepo files.
installer-read-failed = The installer file couldn't be read. It may be damaged. ({ $error })
installer-wrong-arch = This file is for { $arch } and this computer is { $host }. Get the { $host } file from the developer.

## Why it failed
failure-network = Required packages couldn't be downloaded. Check your internet connection.
failure-dependencies = Other packages this one needs weren't found or don't fit together, so it can't be installed.
failure-lock = Another program is installing packages. Try again when it finishes.
failure-space = There isn't enough disk space.
failure-conflict = This conflicts with a package that is already installed.
failure-architecture = This package doesn't fit this computer's architecture.
failure-corrupt = The package file is damaged. Download it again.
failure-system-package = This package is essential to the system and can't be removed.
failure-service = The package service (PackageKit) wasn't found.

## Removing
remove-question = Remove “{ $name }”?
remove-details-deb = This removes the app and the packages installed for it. Your documents and settings stay.
remove-details-flatpak = This removes the app and runtimes nothing uses any more. The app's data (~/.var/app) stays.
remove-details-appimage = This removes the app from the Applications folder. Your documents and settings stay.
remove-also = Also removes: { $names }
remove-blocked = This can't be removed because it would also remove packages essential to the system: { $names }
remove-cancel = Cancel
remove-confirm = Remove

## List window
manager-open-file = Open Installer File…
manager-store = Software
manager-search = Search
manager-file-filter = Installer files
manager-footnote = Only apps and packages you installed are listed. Apps included in buntOS can't be removed.
manager-empty-title = No Apps Installed
manager-empty-details = Double-click a downloaded installer file (.deb, AppImage, Flatpak) or drag it into this window to install it.
manager-no-match = No matching apps
manager-row-open = Open
manager-row-remove = Remove…
manager-row-removing = Removing…
manager-removed = “{ $name }” was removed.
