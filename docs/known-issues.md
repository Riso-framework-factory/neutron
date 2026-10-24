# Known issues

Problems that come from running Zed's crates in Neutron's layout, and test results that differ
from upstream. Each entry says how to recognize it and what fixes it.

## `log_err()` loses the crate name outside `crates/`

`gpui_util::log_error_with_caller` derives the log target from the caller's file path by splitting
at `"crates/"` (upstream assumes every crate lives in a `crates` directory). Errors logged with
`.log_err()` from `core/`, `riso/`, `crate/` or `apps/<app>/` get an empty target, so log lines lose
their crate name and cannot be filtered by module. Logging itself still works. `apps/ide` is not
affected because it keeps the `crates/` directory.

- Symptom: `gpui_util` test `logging_tests::logging_preserves_diagnostics_and_callers` fails.
- Fix: upstream PR deriving the crate from the path component before `/src/`. Until then, the
  options are to accept it or to shim `gpui_util`.

## Debug builds embed assets instead of reading the checkout

Upstream's `util::fs_embed!` makes debug builds read files at runtime from the repository root
plus a `crates/...` path (`grammars` uses `crates/grammars/src`, `util`'s own test uses
`crates/util/src`). Those paths do not exist in Neutron's layout, and core cannot be rewritten, so
`neutron.workspace.toml` enables `util/debug-embed` for the whole workspace. Debug builds then
embed these files at compile time.

- Effect: editing a file in `assets/` or a tree-sitter query needs a rebuild to show up, unlike in
  Zed's own checkout.
- Without it: debug builds panic at startup with "missing cpp/semantic_token_rules.json".
- Fix: upstream PR so `root_relative` does not assume `crates/`.

## Flaky under load

- `gpui` `app::bench_context::tests::foreground_work_reports_long_task_without_window_draw` is
  timing-based. It can fail when the whole core test suite runs in parallel and passes on its own.

## `clock` tests need `test-support`

`clock`'s `#[cfg(test)]` code uses `parking_lot`, which is only enabled by its `test-support`
feature. In Zed's full workspace another crate enables that feature, so the bug never shows; when
checking or testing core on its own, pass `--features clock/test-support`.
