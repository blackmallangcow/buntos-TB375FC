<p align="right"><strong>English</strong> · <a href="CONTRIBUTING.ko.md">한국어</a></p>

# Contributing

Report security vulnerabilities privately by following [SECURITY.md](SECURITY.md), not in public issues or pull requests.

## DCO sign-off

buntOS requires the [Developer Certificate of Origin](https://developercertificate.org/) (DCO) for contributions, the same way the Linux kernel does. Add this line at the end of every commit message:

```
Signed-off-by: Your Name <your.email@example.com>
```

`git commit -s` adds it for you. The line states that you agree to the DCO and have the right to submit the contribution under this repository's license.

## License headers

Add an SPDX header to every new file. A file that cannot carry a header is annotated in `REUSE.toml`.

| Target | `SPDX-License-Identifier` |
|---|---|
| Core components (compositor, shell, agent) | `GPL-3.0-or-later` |
| SDK and protocol crates that external apps link against | `MIT OR Apache-2.0` |
| Documentation | `CC-BY-SA-4.0` |

Example for a Rust file:

```rust
// SPDX-FileCopyrightText: 2026 buntOS contributors
// SPDX-License-Identifier: GPL-3.0-or-later
```

- When you bring in code from another project, keep its original copyright notice and license in the header. Example: `comp/src/grabs.rs` comes from a Smithay example and is `GPL-3.0-or-later AND MIT`.
- The first time a license is used, add its text to `LICENSES/` (`reuse download <SPDX-ID>`).
- Code directories have a default `GPL-3.0-or-later` annotation in `REUSE.toml`, because reuse sometimes fails to read the header of a file that contains Korean text. Still add a header to new files.
- Run `reuse lint` before you submit. CI runs the same check.

## Kernel and DTS

Kernel patches and DTS changes are submitted upstream (linux-mediatek and others) by default and follow upstream rules: kernel code is GPL-2.0, and DTS files follow the dual-license convention. See section 4.2.2 of the [design document](docs/design/ARCHITECTURE.md) (in Korean).

## Firmware

Firmware that cannot be redistributed is not put in the repository or in images. Follow the rules in section 2.1 of the design document.

## Design documents

Design documents are currently written in Korean. When you change a design, update the design document and the decision notes in `docs/design/` together.
