# SPDX-FileCopyrightText: 2026 buntOS contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Screenshot (⇧⌘3, ⇧⌘4) file name, like macOS: "Screenshot 2026-10-29 at 9.52.13 AM"
screenshot-file = Screenshot { $date } at { $time } { $period }
screenshot-am = AM
screenshot-pm = PM
# Screenshot notifications
screenshot-title = Screenshot
screenshot-saved = Saved to Pictures: { $name }
screenshot-copied = Copied to the clipboard.
screenshot-failed = Couldn’t take a screenshot.
# Screen recording (⇧⌘5 Record Entire Screen, Record Selected Portion) file name, like macOS:
# "Screen Recording 2026-10-30 at 3.12.45 PM"
recording-file = Screen Recording { $date } at { $time } { $period }
# Screen recording notifications. $name: the saved file's name
recording-title = Screen Recording
recording-saved = Saved the screen recording: { $name }
recording-failed = Couldn’t save the screen recording.
recording-unavailable = Can’t record the screen. The screen recording tool (GStreamer) isn’t installed.
# Shown when a window did not close in time while logging out. $app: the window's title
logout-cancelled-title = Logout Cancelled
logout-cancelled-body = “{ $app }” did not close, so logout was cancelled. Quit the app and log out again.
