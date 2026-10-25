# 0006. Riso contains reusable base components, not features

Status: accepted (2026-10-24)

## Decision
`riso/` provides reusable components as a base plus traits (for example `StatusBar` and
`trait StatusItemView`, `Picker<D: PickerDelegate>`; type names follow Zed, see ADR 0008). `crate/` provides concrete items for a product and
`apps/<app>` creates and wires the instances. Riso does not build specific features such as an
editor, terminal or agent.

## Consequences
- Zed's `workspace` is not forked as a whole. Its small, project-free files (`status_bar.rs`,
  `toolbar.rs`, `modal_layer.rs`, `toast_layer.rs`, `dock.rs`) are taken into `riso_shell` with a
  per-file origin so they keep receiving upstream changes; `pane.rs`/`item.rs` come last.
- Core holds only what the framework builds on (ADR 0007); engines such as `language`, `markdown`,
  `terminal` and the AI SDKs stay in `apps/ide`.
- Components contain no hard-coded UI strings, branding or domain logic.
