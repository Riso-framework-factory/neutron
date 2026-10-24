# 0001. License: GPL-3.0-or-later

Status: accepted (2026-10-24)

## Context
Zed has 209 crates under GPL-3.0-or-later and 42 under Apache-2.0. Of the 123 crates planned for
`core/`, only the GPUI/foundation batch (29 crates) is Apache-2.0; `ui`, `theme`, `settings`, `db`,
`fs` and everything above them are GPL. Rust links statically, so an app using them is a GPL
derivative work.

## Decision
Neutron, including all Neutron-owned code (shims, `riso/`, tooling), is GPL-3.0-or-later.
The README opens with a warning about the obligations for distributed apps and ends with a
disclaimer.

## Consequences
- Files copied from Zed and modified in `riso/` carry a "Modified from zed@<sha>, <date>" header
  (GPLv3 §5(a)); `cargo neutron sync` writes it and `verify` checks it.
- App templates ship a GPL `LICENSE`; `cargo neutron bundle` warns about incompatible licenses.
- Closed-source apps and the Apple App Store are out of scope for end developers.
