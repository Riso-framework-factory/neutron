# crate/

Shared components of your product. A crate here implements traits from `riso/` (for example a
`StatusItemView` or a `Panel`) or provides product logic used by several apps in `apps/`.

Log with the `log` macros and `.log_err()`; never install a logger here, the app does that with
`riso_log::init()`.

Register new crates in `neutron.workspace.toml` (`[manifest.workspace] members` and
`[manifest.workspace.dependencies]`), then run `cargo neutron sync`.
