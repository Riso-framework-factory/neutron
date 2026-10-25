// Modified from Zed (crates/zed_actions/src/lib.rs) for Neutron, 2026-10-25. `cargo neutron sync` tracks upstream changes.
// Neutron keeps the actions that are not about the IDE (editor, git, agent, project, remote…) and
// renames the `zed` namespace to `app`, because action names are shown to an app's users.
use gpui::{Action, actions};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// If the app binary doesn't use anything in this crate, it will be optimized away
// and the actions won't initialize. So we just provide an empty initialization function
// to be called from main.
//
// These may provide relevant context:
// https://github.com/rust-lang/rust/issues/47384
// https://github.com/mmastrac/rust-ctor/issues/280
pub fn init() {}

/// Opens a URL in the system's default web browser.
#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct OpenBrowser {
    pub url: Arc<str>,
}

/// Opens the keymap to either add a keybinding or change an existing one
#[derive(PartialEq, Clone, Default, Action, JsonSchema, Serialize, Deserialize)]
#[action(namespace = app, no_json, no_register)]
pub struct ChangeKeybinding {
    pub action: String,
}

actions!(
    app,
    [
        /// Opens the settings editor.
        OpenSettings,
        /// Opens the settings JSON file.
        OpenSettingsFile,
        /// Opens the default keymap file.
        OpenDefaultKeymap,
        /// Opens the user keymap file.
        OpenKeymapFile,
        /// Opens the keymap editor.
        OpenKeymap,
        /// Quits the application.
        Quit,
        /// Shows information about the application.
        About,
        /// Opens the documentation website.
        OpenDocs,
        /// Views open source licenses.
        OpenLicenses,
        /// Opens the performance profiler.
        OpenPerformanceProfiler,
        /// Opens the onboarding view.
        OpenOnboarding,
        /// Shows the auto-update notification for testing.
        ShowUpdateNotification,
    ]
);

/// Opens the settings editor at a specific path.
#[derive(PartialEq, Clone, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct OpenSettingsAt {
    /// A path to a specific setting (e.g. `theme.mode`)
    pub path: String,
    /// The settings file to select before opening `path`. When omitted, the
    /// existing settings file selection is preserved.
    #[serde(default)]
    pub target: Option<OpenSettingsAtTarget>,
}

#[derive(PartialEq, Clone, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct OpenSettingsPage {
    /// A settings page title (e.g. `Appearance`).
    pub page: String,
    /// The settings file to select before opening `page`. When omitted, the
    /// existing settings file selection is preserved.
    #[serde(default)]
    pub target: Option<OpenSettingsAtTarget>,
}

#[derive(PartialEq, Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpenSettingsAtTarget {
    User,
}

/// Decreases the font size of the user interface.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct DecreaseUiFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Increases the font size of the user interface.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct IncreaseUiFontSize {
    #[serde(default)]
    pub persist: bool,
}

/// Resets the UI font size to the default value.
#[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
#[action(namespace = app)]
#[serde(deny_unknown_fields)]
pub struct ResetUiFontSize {
    #[serde(default)]
    pub persist: bool,
}

pub mod dev {
    use gpui::actions;

    actions!(
        dev,
        [
            /// Toggles the developer inspector for debugging UI elements.
            ToggleInspector,
            /// Cycles the debug frame-time overlay between hidden, current
            /// frame-time, and detailed frame-time statistics.
            ToggleFpsOverlay,
            /// Resets the debug frame-time overlay's statistics, except for the
            /// total frame count.
            ResetFrameOverlayStats,
            /// Opens the key context view for debugging keybindings.
            OpenKeyContextView
        ]
    );
}

pub mod toast {
    use gpui::actions;

    actions!(
        toast,
        [
            /// Runs the action associated with a toast notification.
            RunAction
        ]
    );
}

pub mod command_palette {
    use gpui::actions;

    actions!(
        command_palette,
        [
            /// Toggles the command palette.
            Toggle,
        ]
    );
}

pub mod feedback {
    use gpui::actions;

    actions!(
        feedback,
        [
            /// Opens the bug report form.
            FileBugReport,
            /// Opens the feature request form.
            RequestFeature
        ]
    );
}

pub mod theme {
    use gpui::actions;

    actions!(theme, [ToggleMode]);
}

pub mod theme_selector {
    use gpui::Action;
    use schemars::JsonSchema;
    use serde::Deserialize;

    /// Toggles the theme selector interface.
    #[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
    #[action(namespace = theme_selector)]
    #[serde(deny_unknown_fields)]
    pub struct Toggle {
        /// A list of theme names to filter the theme selector down to.
        pub themes_filter: Option<Vec<String>>,
    }
}

pub mod icon_theme_selector {
    use gpui::Action;
    use schemars::JsonSchema;
    use serde::Deserialize;

    /// Toggles the icon theme selector interface.
    #[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
    #[action(namespace = icon_theme_selector)]
    #[serde(deny_unknown_fields)]
    pub struct Toggle {
        /// A list of icon theme names to filter the theme selector down to.
        pub themes_filter: Option<Vec<String>>,
    }
}

pub mod search {
    use gpui::actions;

    actions!(
        search,
        [
            /// Focuses on the search input field.
            FocusSearch,
            /// Selects the next search match.
            SelectNextMatch,
            /// Selects the previous search match.
            SelectPreviousMatch,
            /// Toggles case-sensitive search.
            ToggleCaseSensitive,
        ]
    );
}
pub mod buffer_search {
    use gpui::{Action, actions};
    use schemars::JsonSchema;
    use serde::Deserialize;

    /// Opens the buffer search interface with the specified configuration.
    #[derive(PartialEq, Clone, Deserialize, JsonSchema, Action)]
    #[action(namespace = buffer_search)]
    #[serde(deny_unknown_fields)]
    pub struct Deploy {
        #[serde(default = "util::serde::default_true")]
        pub focus: bool,
        #[serde(default)]
        pub replace_enabled: bool,
        #[serde(default)]
        pub selection_search_enabled: bool,
    }

    impl Deploy {
        pub fn find() -> Self {
            Self {
                focus: true,
                replace_enabled: false,
                selection_search_enabled: false,
            }
        }

        pub fn replace() -> Self {
            Self {
                focus: true,
                replace_enabled: true,
                selection_search_enabled: false,
            }
        }
    }

    actions!(
        buffer_search,
        [
            /// Deploys the search and replace interface.
            DeployReplace,
            /// Dismisses the search bar.
            Dismiss,
            /// Focuses back on the searched item.
            FocusEditor,
            /// Sets the search query from the selection or word under cursor.
            UseSelectionForFind,
        ]
    );
}
pub mod settings_profile_selector {
    use gpui::Action;
    use schemars::JsonSchema;
    use serde::Deserialize;

    #[derive(PartialEq, Clone, Default, Debug, Deserialize, JsonSchema, Action)]
    #[action(namespace = settings_profile_selector)]
    pub struct Toggle;
}
