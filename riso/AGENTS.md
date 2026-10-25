# riso/

Two kinds of crates live here:

- **Shims** keep the upstream crate name (`paths`, `release_channel`) and replace that crate for the
  whole workspace. A shim must stay a superset of the upstream API, because core and `apps/ide`
  compile against it.
- **`riso_*` crates** are reusable components and app infrastructure.

## Component conventions

1. A component is a base plus slot or delegate traits (`StatusBar` + `trait StatusItemView`,
   `Picker<D: PickerDelegate>`). Default trait methods must be good enough that an instance only
   writes what differs.
2. No hard-coded UI strings, branding or domain logic: text, icons and actions come from the instance.
3. Reuse a settings key from core's `settings_content` when it already fits (`tabs`, `tab_bar`,
   `status_bar`, `toolbar`, `title_bar`…); new settings go through `riso_settings`, one namespace per
   component.
4. A component declares its actions and default keymap; the app decides whether to load the keymap.
5. Implement `component::Component` so the component shows up in the gallery.
6. Every component has at least one instance in `apps/examples/*`.

## Files taken from Zed

- Keep Zed's type and module names (`Workspace`, `Pane`, `Item`, `StatusItemView`…) and remove only
  the IDE-specific parts: no renames, reformatting or reordering of upstream code. Small diffs are what
  lets `cargo neutron sync` keep merging upstream changes (ADR 0008).
- Declare every file taken from Zed in `[[riso.files]]` (`path`, `origin`, `mode = "merge"` or
  `"watch"`) and let `cargo neutron sync` copy it; do not copy files by hand. Use `merge` when the
  file only loses a few lines, `watch` when it changes a lot.
- After resolving a merge conflict or porting a watch-mode file's upstream changes, run
  `cargo neutron ack <path> --upstream <zed checkout>`; `cargo neutron riso status` shows what is
  waiting.
- Keep the "Modified from Zed (...) for Neutron, <date>" header the sync writes (GPLv3 §5(a));
  `cargo neutron verify` checks it (V7).
- No Zed branding in strings and no `zed_actions` (V10): text comes from the instance or
  `riso_identity`, shared actions from `riso_actions`.
- Never depend on crates in `apps/ide`.
- The `neutron-component` skill walks through the whole procedure.
