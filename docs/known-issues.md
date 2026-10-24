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

## `util` fs_embed test needs `debug-embed`

`util`'s test of `fs_embed!` in dev mode reads `crates/util/src` from the repository root.
Run util's tests with `--features util/debug-embed`. Allowed in `upstream.toml`
(`policy.allow_core_path_escapes`).

## Flaky under load

- `gpui` `app::bench_context::tests::foreground_work_reports_long_task_without_window_draw` is
  timing-based. It can fail when the whole core test suite runs in parallel and passes on its own.
