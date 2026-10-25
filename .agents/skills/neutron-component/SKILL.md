---
name: neutron-component
description: >-
  Turn a reusable piece of Zed (workspace shell parts, pickers, command palette, selectors, title
  bar, settings UI, keymap editor, search bar, dev tools…) into a generic riso component, or write a
  new one. Covers declaring the upstream origin in upstream.toml, removing IDE coupling with the
  P1–P11 patterns, the component conventions, and verifying with cargo neutron. Use whenever code is
  taken from Zed into riso/ or a riso_* component is created or changed.
---

# Building a riso component

A riso component is a **base plus traits** that knows nothing about any product: the base owns
layout, focus, keyboard, theme and persistence; an app or a crate in `crate/` supplies the instance
(items, delegates, text, actions). Read `riso/AGENTS.md` and `docs/adr/0008-riso-component-conventions.md`
first.

## Before writing anything

- Check `core/ui` first. Buttons, context and popover menus, modals, tabs, lists, tree items, data
  tables, notifications, tooltips, keybinding hints and `ui_input::InputField` already exist there;
  do not rebuild them in riso.
- Find the Zed code to start from in `apps/ide/crates/` (the mirror of upstream). Measure its
  coupling: `grep -c "project::\|editor::\|workspace::"` and its `Cargo.toml` dependencies. Files with
  little coupling are taken in `merge` mode; files that need large removals in `watch` mode.
- Pick the riso crate from ADR 0008 (for example `riso_shell` for workspace parts). Do not create a
  new crate per upstream crate.

## Taking a file from Zed

1. Declare the file in `upstream.toml`:
   ```toml
   [[riso.files]]
   path = "riso/shell/src/status_bar.rs"
   origin = "crates/workspace/src/status_bar.rs"
   mode = "merge"   # or "watch"
   base = ""
   ```
2. Run `cargo neutron sync --upstream <zed checkout>`. It copies the origin with a
   `// Modified from Zed (...) for Neutron, <date>.` header and records `base`. Never remove the
   header (GPLv3 §5(a); `verify` V7).
3. Remove the IDE coupling with the patterns below. **Delete or replace only what is necessary.**
   Keep Zed's type and module names (`Workspace`, `Pane`, `Item`, `StatusItemView`, `crate::pane`…),
   and do not rename, reformat or reorder upstream code: every extra line of diff is a future merge
   conflict.
4. Keep the `#[cfg(test)]` module that came with the file; adapt tests that need `Project`, and say
   in the commit message which ones were removed and why.

| Pattern | Zed coupling | Replace with |
|---|---|---|
| P1 | `Entity<Project>` in `Workspace` and its APIs | no project; the app attaches its own model with `workspace.set_context(entity)` / `workspace.context::<T>()` |
| P2 | `ProjectItem`, opening by `ProjectPath` | an opener registry over `OpenTarget` (path, URI or app key); dedupe with `Item::item_key()` |
| P3 | `project` parameter of `Item::save/save_as/reload` | drop the parameter; the item saves itself |
| P4 | `Editor` as a single-line input | `ui_input::InputField` |
| P5 | `zed_actions::…` | `riso_actions` (Zed's `zed` namespace is `app` there) |
| P6 | `client`, zed.dev URLs | drop `client`; keep `telemetry::event!`; URLs and names come from `riso_identity` or the instance |
| P7 | `markdown` for prompt or notification details | plain text (`Label`) |
| P8 | settings | reuse a key that core's `settings_content` already has (`tabs`, `tab_bar`, `status_bar`, `toolbar`, `title_bar`…); new keys go through `riso_settings` |
| P9 | syntax highlighting through `language` | `ui::HighlightedLabel` and `syntax_theme`; drop what needs a language |
| P10 | hard-coded UI strings, "Zed", docs links | parameters supplied by the instance, neutral defaults (`verify` V10) |
| P11 | collab, follow, AI, remote code paths | delete them; do not leave stubs |

## Component conventions

1. Base plus slot or delegate traits (`StatusBar` + `StatusItemView`, `Picker<D: PickerDelegate>`), with
   default methods good enough that an instance writes only what differs.
2. No UI strings, branding or domain logic in the base.
3. Declare the component's actions (in `riso_actions` when they are shared) and its default keymap;
   the app decides whether to load the keymap.
4. Implement `component::Component` so it shows up in the gallery.
5. Add at least one instance to an app in `apps/examples/`.
6. Depend only on core, shims and lower riso crates; never on `apps/ide`.

## Keeping up with upstream

- `cargo neutron riso status --upstream <zed checkout>` lists every taken file, its mode, its base and
  the upstream commits waiting since then.
- `merge` files are merged by every sync. On a conflict, resolve the markers, then run
  `cargo neutron ack <path> --upstream <zed checkout>`.
- `watch` files show up in the sync report under "Riso files to port". Port the changes that matter,
  then run `cargo neutron ack <path> --upstream <zed checkout>`.

## Verify

```sh
cargo neutron verify --upstream <zed checkout>   # V2 deps, V4 markers, V7 header, V10 branding
cargo neutron doctor --upstream <zed checkout>   # verify + cargo check of core, shims, riso, IDE
```

Check the result in the gallery app on macOS, and on Linux and Windows VMs for window chrome.
Commit with a message that names the origin, the mode and what was removed.
