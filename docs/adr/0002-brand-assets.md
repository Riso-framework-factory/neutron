# 0002. Zed brand assets

Status: accepted (2026-10-24)

## Context
`assets/` contains Zed brand files (`images/zed_logo.svg`, `icons/zed_*.svg`, plan stamps). Core
crates embed `assets/` and expect these paths to exist.

## Decision
Sync `assets/` verbatim. Filter brand files out of app bundles with an allowlist in `riso_assets`
and `riso_bundle`.

## Consequences
Core keeps working unchanged; apps never ship Zed's logo by default.
