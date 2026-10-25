# 0008. Riso component conventions for Phase 4

Status: accepted (2026-10-25)

## Context
Phase 4 converts every reusable component Zed has (about 45: the workspace shell, pickers, command
palette, selectors, title bar, onboarding, settings UI, keymap editor, search bar, image viewer,
dev tools) into base-plus-instance components in `riso/`. Most of them come from upstream files
that keep changing, so the conversion has to stay mergeable.

## Decision
1. **Keep Zed's type and module names** in code taken from Zed: `Workspace`, `Pane`, `PaneGroup`,
   `Item`, `ItemHandle`, `Panel`, `Dock`, `StatusItemView`, `ToolbarItemView`, `ModalView`, and
   so on. There is no `AppShell` or `StatusItem`.
2. **Group riso crates by function** (`riso_shell`, `riso_picker`, `riso_command_palette`,
   `riso_selectors`, `riso_title_bar`, `riso_pages`, `riso_settings_ui`, `riso_keymap_editor`,
   `riso_search`, `riso_image_viewer`, `riso_devtools`, plus `riso_actions`, `riso_app`,
   `riso_assets`, `riso_settings` and the `neutron` facade). One riso crate may hold files from
   several upstream crates; origins are recorded per file in `upstream.toml`.
3. **Plain text** for prompt and notification details. Core gets no `markdown` root in Phase 4.
4. The tree panel, table view and "open recent" picker move to Phase 7.

## Consequences
- Files taken from Zed keep `crate::pane`, `crate::item`… and only lose the IDE-specific parts, so
  `cargo neutron sync` can 3-way merge them per file; files that need large removals are tracked in
  watch mode.
- App authors can read Zed's code and documentation without translating names.
- The examples in ADR 0006 and the AGENTS.md files use `StatusItemView`.
