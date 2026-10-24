# crate/

Shared components of your product. A crate here implements traits from `riso/` (for example a
`StatusItem` or a `Panel`) or provides product logic used by several apps in `apps/`.

Register new crates in `neutron.workspace.toml` (`[manifest.workspace] members` and
`[manifest.workspace.dependencies]`), then run `cargo neutron sync`.
