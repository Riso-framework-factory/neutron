# riso/

Two kinds of crates live here:

- **Shims** keep the upstream crate name (`paths`, `release_channel`) and replace that crate for the
  whole workspace. A shim must stay a superset of the upstream API, because core and `apps/ide`
  compile against it.
- **`riso_*` crates** are reusable components and app infrastructure.

## Component conventions

1. A component is a base plus slot or delegate traits (`StatusBar` + `trait StatusItem`,
   `Picker<D: PickerDelegate>`). Default trait methods must be good enough that an instance only
   writes what differs.
2. No hard-coded UI strings, branding or domain logic: text, icons and actions come from the instance.
3. Component settings go through `riso_settings`, one namespace per component.
4. A component declares its actions and default keymap; the app decides whether to load the keymap.
5. Implement `component::Component` so the component shows up in the gallery.
6. Every component has at least one instance in `apps/examples/*`.

## Files taken from Zed

- Record the origin and base commit in `upstream.toml` so `cargo neutron sync` can 3-way merge
  upstream changes into the file.
- Keep the "Modified from zed@<sha>, <date>" header (GPLv3 §5(a)); `cargo neutron verify` checks it.
- Keep the diff from upstream small; never depend on crates in `apps/ide`.
