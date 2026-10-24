# 0003. Upstream ref

Status: accepted (2026-10-24)

## Decision
`main` of `zed-industries/zed` is synced daily. Neutron releases are pinned to Zed stable tags
(`v0.xxx.y`) by running the sync with `--ref <tag>`.

## Consequences
Neutron `main` behaves like a nightly channel; API breaks from upstream show up within a day.
