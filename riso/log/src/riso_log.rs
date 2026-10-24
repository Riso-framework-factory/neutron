//! Installs Zed's logger (`zlog`) for a Neutron app.
//!
//! `gpui_util`'s `log_err()` derives the log target from the caller's file by splitting at
//! `"crates/"`, because every Zed crate lives in a `crates` directory. Neutron keeps crates in
//! `core/`, `riso/`, `crate/` and `apps/`, so those errors arrive with an empty target and per-crate
//! log filters cannot match them. This logger fills the target in from the file path and forwards
//! everything else to `zlog` unchanged.
//!
//! Call [`init`] instead of `zlog::init`.

use std::collections::HashMap;

static LOGGER: CrateTargetLogger = CrateTargetLogger {
    inner: &zlog::Zlog {},
};

/// Installs the logger and applies `ZED_LOG`/`RUST_LOG`, like `zlog::init`.
pub fn init() {
    if let Err(error) = try_init(None) {
        eprintln!("{error}");
    }
}

/// Like [`init`], with a filter used when neither `ZED_LOG` nor `RUST_LOG` is set.
pub fn try_init(filter: Option<String>) -> anyhow::Result<()> {
    log::set_logger(&LOGGER)?;
    log::set_max_level(log::LevelFilter::max());
    zlog::process_env(filter);
    zlog::filter::refresh_from_settings(&HashMap::default());
    Ok(())
}

struct CrateTargetLogger {
    inner: &'static dyn log::Log,
}

impl log::Log for CrateTargetLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if !record.target().is_empty() {
            self.inner.log(record);
            return;
        }
        let Some(target) = record.file().and_then(target_from_file) else {
            self.inner.log(record);
            return;
        };
        self.inner.log(
            &log::Record::builder()
                .target(&target)
                .module_path(record.module_path().or(record.file()))
                .args(*record.args())
                .file(record.file())
                .line(record.line())
                .level(record.level())
                .build(),
        );
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// `crate::module` for a source file, following the rule `gpui_util` applies to `crates/<crate>`:
/// the crate is the directory that contains `src/`, and a module named after the crate is the root.
fn target_from_file(file: &str) -> Option<String> {
    let file = file.replace('\\', "/");
    let (crate_directory, module) = file.split_once("/src/")?;
    let krate = crate_directory.rsplit('/').next()?.replace('-', "_");
    let module = module.trim_end_matches(".rs").replace('/', "::");
    if krate.is_empty() || module.is_empty() {
        return None;
    }
    Some(if module.starts_with(&krate) {
        module
    } else {
        format!("{krate}::{module}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_util::ResultExt as _;
    use std::sync::Mutex;

    #[test]
    fn derives_targets_for_neutron_directories() {
        assert_eq!(
            target_from_file("core/gpui/src/app/entity_map.rs").as_deref(),
            Some("gpui::app::entity_map")
        );
        assert_eq!(
            target_from_file("core/gpui_util/src/gpui_util.rs").as_deref(),
            Some("gpui_util")
        );
        assert_eq!(
            target_from_file("apps/notes/src/main.rs").as_deref(),
            Some("notes::main")
        );
        assert_eq!(
            target_from_file("core/refineable/derive_refineable/src/derive_refineable.rs")
                .as_deref(),
            Some("derive_refineable")
        );
        assert_eq!(target_from_file("build.rs"), None);
    }

    static CAPTURED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

    struct CaptureLogger;

    impl log::Log for CaptureLogger {
        fn enabled(&self, _: &log::Metadata) -> bool {
            true
        }

        fn log(&self, record: &log::Record) {
            if let Ok(mut captured) = CAPTURED.lock() {
                captured.push((record.target().to_string(), record.args().to_string()));
            }
        }

        fn flush(&self) {}
    }

    static TEST_LOGGER: CrateTargetLogger = CrateTargetLogger {
        inner: &CaptureLogger,
    };

    #[test]
    fn log_err_from_outside_crates_gets_a_target() {
        log::set_logger(&TEST_LOGGER).expect("only this test installs a logger");
        log::set_max_level(log::LevelFilter::max());

        Err::<(), _>(anyhow::anyhow!("disk full")).log_err();

        let captured = CAPTURED.lock().expect("logger mutex poisoned");
        assert_eq!(
            captured.as_slice(),
            [("log::riso_log".to_string(), "disk full".to_string())]
        );
    }
}
