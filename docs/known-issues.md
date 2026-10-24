# Known issues

Problems that come from running Zed's crates in Neutron's layout, and test results that differ
from upstream. Each entry says how to recognize it and what fixes it.

## `log_err()` cannot be filtered by crate outside `crates/`

`gpui_util::log_error_with_caller` derives the log target and module path from the caller's file by
splitting at `"crates/"` (upstream assumes every crate lives in a `crates` directory). For callers in
`core/`, `riso/`, `crate/` or `apps/<app>/` both come out empty. `zlog` then falls back to the file
path, so the line still names the crate (`ERROR [core/gpui/src/app.rs:123] ...`), and errors are
enabled by default, so nothing is lost. `apps/ide` keeps `crates/` and is not affected.

- Effect: per-crate log filters (`RUST_LOG=gpui=off`, the `log` setting) do not apply to errors
  logged with `.log_err()` from those directories.
- Symptom: `gpui_util` test `logging_tests::logging_preserves_diagnostics_and_callers` fails.
- Fixed for Neutron apps: call `riso_log::init()` instead of `zlog::init()`. It installs a logger
  that fills the empty target in from the file path (the directory that contains `src/`) and
  forwards to `zlog`. The crate name is the directory name, so `riso/identity` logs as `identity`.
- Still worth an upstream PR, which would also make the failing test pass.

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
