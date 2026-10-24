# 0006. Riso contains reusable base components, not features

Status: accepted (2026-10-24)

## Decision
`riso/` provides reusable components as a base plus traits (for example `StatusBar` and
`trait StatusItem`, `Picker<D: PickerDelegate>`). `crate/` provides concrete items for a product and
`apps/<app>` creates and wires the instances. Riso does not build specific features such as an
editor, terminal or agent.

## Consequences
- Zed's `workspace` is not forked as a whole. Its small, project-free files (`status_bar.rs`,
  `toolbar.rs`, `modal_layer.rs`, `toast_layer.rs`, `dock.rs`) are taken into `riso_shell` with a
  per-file origin so they keep receiving upstream changes; `pane.rs`/`item.rs` come last.
- Engine crates in `core/` (`language`, `markdown`, `terminal`, AI SDKs) stay available as libraries.
- Components contain no hard-coded UI strings, branding or domain logic.
