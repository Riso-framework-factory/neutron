# core/ is read-only

Every crate here is a verbatim copy of a crate from `zed-industries/zed` at the commit recorded in
`upstream.toml`. `cargo neutron sync` overwrites this directory and `cargo neutron verify` fails if a
file differs from upstream.

To change behaviour, send a PR to Zed. If the change cannot wait, add a shim in `riso/` (same crate
name, compatible API) or take the file into a `riso_*` crate with its origin recorded in `upstream.toml`.
