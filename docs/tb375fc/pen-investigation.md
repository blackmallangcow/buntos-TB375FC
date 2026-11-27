<!--
SPDX-FileCopyrightText: 2026 Antonio Lamb <antoniolamb@gmail.com>
SPDX-License-Identifier: CC-BY-SA-4.0
-->

# TB375FC active pen: findings from a separate Linux port

These notes collect evidence gathered on the same hardware while investigating
the pen for a different Linux port. Device: Lenovo Xiaoxin Pad Pro 12.7
(TB375FC, MT6897), Novatek NT36532 controller, Lenovo Tab Pen Plus. Collection
ran from 2026-11-20 to 2026-11-25 on Fedora 45 and LineageOS 23.2 (Android 16,
build `BP4A.251205.006`, vendor kernel `6.1.173-android14`).

This is offered as evidence for issue #1 and for the open speculation in
`DESIGN.md` v1.95. It does not propose kernel or compositor changes. Raw
captures, hashes and analysis tools are in the public repository
[antomicblitz/xiaoxin-linux](https://github.com/antomicblitz/xiaoxin-linux);
no firmware or binary artifacts are included here.

## 1. The 120 Hz requirement is in the controller, and Android has it too

LineageOS exposes 144, 120, 60 and 30 Hz. Its default peak is already 120 Hz
(`mActiveModeId=2`, `peak_refresh_rate` unset). Forcing 144 Hz with
`settings put system peak_refresh_rate 144` and `min_refresh_rate 144` gives:

| Condition | Display | Stylus result |
|---|---|---|
| Forced 144 Hz | `mActiveModeId=1`, stayed at 144 Hz while the pen was used | Zero events in a 75 s capture; `/dev/input/event3` (`touch-pen`) silent |
| Default 120 Hz | `mActiveModeId=2` | 7,075 events for the same gesture, pressure up to 2,438; finger touch works in both cases |

Finger touch keeps working at 144 Hz, so the failure is specific to the pen
path. The same gate appears on Linux: at 144 Hz the controller emits only the
three byte detect record `f0 05 01` and leaves the stylus record at `buf[65]`
as `ff`, while at 120 Hz the driver receives the full 14 byte record with
position, pressure, tilt and distance.

Two conclusions follow:

- LineageOS does **not** drop the panel to 120 Hz when the pen appears. A
  Developer options refresh-rate overlay would show 144 Hz with a dead pen.
- The 120 Hz requirement lives in the touch/display controller or its
  firmware, identically under Android and Linux. It is not a Linux driver
  defect.

About stock ZUI: the LineageOS TB375FC maintainer is reported to document
stock Lenovo software calling a MediaTek SurfaceFlinger function,
`set_pen_timer`, to step the display down to 120 Hz when the pen becomes
active. That would match the controller behaviour measured here, and it is
also consistent with the note in `DESIGN.md` v1.95 that the stock touch driver
has a pen-state notification path and the stock panel driver has a 120 Hz only
mode name. This was **not verified on this unit**; the unit carried LineageOS,
not ZUI. Confirming it needs a stock ZUI device with Developer options, Show
refresh rate, on while hovering or writing.

Source: `mainline-debian/findings/android-pen-controller-gate-2026-11-21.md`,
`mainline-debian/findings/fedora-kde-pen-120hz-2026-11-20.md`.

## 2. Pressure is reported correctly; contact detection is the defect

A useful correction for anyone picking this up: the pressure sensor is not
unresponsive. Under LineageOS a 38 s writing session in Saber produced 7,451
sync frames with pressure 0 to 3,821 and axes X 0..18400, Y 0..29440, tilt
-60..60, distance 0..1. A contributor measured 0 to 4,088 on the buntOS
driver at 120 Hz, and the Linux driver reports the same axis ranges. The
stream is structurally the same on both systems.

The difference is where a stroke starts:

- Android starts a stroke from the driver's `BTN_TOUCH` bit, which is 0 in
  every hover report.
- libinput ignores `BTN_TOUCH` for a device with an `ABS_PRESSURE` axis and
  derives tip state from a pressure threshold. In one Linux capture, 7 kernel
  `BTN_TOUCH` transitions became about 60 libinput tip down/up pairs. That is
  a userspace thresholding effect, not missing pressure data.

The controller state matters as well. Android leaves the vendor variable
`/proc/support_pen` at 0 while the pen works. The mainline driver writes
`0x7b 0x01` then `0x7b 0x03` at boot; its own comment calls state 1 pen-link
mode, where the force field does not track pressure and can be nonzero in the
air. That gives libinput a nonzero pressure outside contact. Note that
`/proc/support_pen` is a driver variable, not a live firmware readback, so
"Android uses 0" is a lead to verify at the wire level rather than proof.

Source: `mainline-debian/findings/android-pen-controller-gate-2026-11-21.md`.

## 3. Phantom contact during hover remains the hard problem

On the working Linux baseline at 120 Hz, a 120 s capture produced 2,286 pen
frames, of which 342 contact frames arrived in 12 bursts during an instructed
hover. Bursts lasted 12 to 557 ms, median about 130 ms, with peak contact
force 3,120 while writing reached 4,093. Earlier runs on the same
configuration reproduce the pattern.

Checks that did **not** find a cause:

- Both checksums pass: the driver checks the touch checksum over bytes 0..64,
  and an offline audit checked the separate 14 byte pen checksum over bytes
  65..78. All 3,779 pen records in the hover trace and all 7,429 in the
  hover-and-writing trace pass, including every phantom record.
- A byte level comparison of all 108 payload positions found no contact or
  quality flag beyond the understood fields. Phantom coordinates agree with
  the hover records around each burst, so they are not stale copies.
- The firmware identity was resolved after an earlier misreading: both
  systems select the Tianma image `novatek_ts_fw_tm.bin` (panel ID strap 1).
  This is not a BOE versus Tianma firmware mismatch.

A contact record cannot be distinguished from an accidental real touch by
the data alone. Two operator confirmed accidental touches had exactly the
phantom signature. Android near-hover captures also show low pressure contact
claims, with peak 306 against the Fedora peak of 3,120, but the near-hover
label cannot exclude a light real touch. Every phantom claim therefore needs
operator confirmation that the nib was off the glass.

No driver side rule was found that rejects phantom records without also
dropping real input.

Sources: `mainline-debian/findings/stylus-phantom-android-vs-linux-2026-11-24.md`,
`mainline-debian/findings/stylus-raw-2026-11-24/README.md`,
`.opencode/handoffs/2026-11-22-stylus-pressure.md`. Raw captures include
`page-hover-trace.bin.gz`, `hovertouch-trace.bin.gz`, `base120-long.jsonl.gz`
and the `pen-trace-read.py` decoder, each with SHA-256 hashes in the
repository.

## 4. What was tried

- The `0223` to `0225` stylus parity patches on a separate branch, plus
  experimental driver patches `0001` to `0007` (diagnostic frame trace,
  contact-rule experiments, debounce and a page cache). `0005` produced
  mid-air ink; `0007` had one quiet run that was not reproducible. These are
  parked, not recommended.
- A vendor NT36532 driver and firmware port (`0720` to `0722`, `nt36532.ko`).
  Its one valid bind produced no pen events.
- Pressure premise tests that overturned the initial theory: the clean
  driver already delivers pressure, so pressure parser changes were not
  needed.

None of these resolved phantom contact.

## 5. Open questions

- Compositor tablet protocol support, already tracked in issue #1:
  `comp/src/input.rs` has no `TabletToolTip` or `TabletToolAxis` arms.
- The 144 Hz timing margin hypothesis in `DESIGN.md` v1.95: 144 Hz uses a
  different panel timing table and a shorter horizontal margin (146 px versus
  376 px at 120 Hz). Unconfirmed.
- First hand confirmation of `set_pen_timer` on stock ZUI.
- A rule that separates phantom contact from real touch without losing real
  strokes. This is the blocking item for reliable pen input.

## AI disclosure and validation

AI tools were used to analyse the captured logs, draft the source findings
notes, and write this document. No code or firmware is changed here.

Quality control applied to these claims:

- Every number was traced back to the linked source note in
  `antomicblitz/xiaoxin-linux`. Several earlier interpretations were corrected
  there after re-reading the raw captures: an invalid hover extrapolation, a
  BOE versus Tianma firmware label misreading, and the claim that Android had
  no near-hover contact records.
- Both checksums were audited offline across 3,779 records in the hover trace
  and 7,429 in the hover-and-writing trace, instead of relying on the driver's
  single checksum check.
- The 144 Hz result was measured on the device under LineageOS with the active
  display mode recorded, and reproduced separately on Linux from driver frame
  logs. The 120 Hz baseline was used for both systems where possible.
- Phantom claims required operator confirmation that the nib was off the
  glass. Two apparent positives turned out to be accidental real touches, and
  the analysis was corrected before it was written up here.
- Before submission, this text was checked against the source notes and the
  patch numbers were corrected. gitleaks and semgrep passed on the staged
  change. The `reuse` CLI was unavailable locally, so the SPDX header and the
  `REUSE.toml` documentation annotation were checked by hand; CI runs
  `reuse lint`.

Limits: stock ZUI was not available on this unit, so the `set_pen_timer`
step-down is reported evidence, not a verified measurement. This document
proposes no fix and was not used to change firmware, kernel or compositor code.

## Evidence index

All paths below are in `antomicblitz/xiaoxin-linux`:

| Path | Content |
|---|---|
| `mainline-debian/findings/android-pen-controller-gate-2026-11-21.md` | Android refresh-rate experiment, writing capture, ZUI lead |
| `mainline-debian/findings/fedora-kde-pen-120hz-2026-11-20.md` | 144 Hz detect record versus 120 Hz full record on Linux |
| `mainline-debian/findings/stylus-phantom-android-vs-linux-2026-11-24.md` | Corrected phantom analysis, checksum audit, Android comparison |
| `mainline-debian/findings/stylus-raw-2026-11-24/` | Raw traces, decoders, hashes |
| `.opencode/handoffs/2026-11-22-stylus-pressure.md` | Chronological investigation handoff |
| `.opencode/work/stylus-fedora-pressure/evidence-manifest.md` | Android and Fedora capture hashes, module hashes, firmware identity |
