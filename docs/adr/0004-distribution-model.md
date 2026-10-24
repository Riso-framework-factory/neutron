# 0004. Distribution model

Status: accepted (2026-10-24)

## Context
Zed has no semver: every crate is `publish = false` and uses `workspace = true` dependencies.

## Decision
Neutron is distributed as a template monorepo. End developers fork it, write code in `crate/` and
`apps/<app>/`, and pull Neutron updates with `cargo neutron upgrade`, which reuses the sync engine.
Nothing is published to crates.io.

## Consequences
Neutron owns `core/`, `riso/`, `assets/`, `tooling/` and the generated root manifest; end developers
own `crate/`, `apps/<app>/` and their entries in `neutron.workspace.toml`. The template ships without
`apps/ide`.
