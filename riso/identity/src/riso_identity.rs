//! Identity of the running Neutron app: its name and the identifiers derived from it.
//!
//! Zed hard-codes its identity (`APP_NAME = "Zed"`, bundle ids `dev.zed.Zed*`). Neutron's shims of
//! `paths` and `release_channel` read it from here instead, at runtime, so that switching between
//! apps in one workspace does not rebuild every crate that depends on `paths`.
//!
//! Call [`init`] first thing in `main`, before anything resolves a path. Without it, the identity
//! comes from the `NEUTRON_APP_NAME` and `NEUTRON_BUNDLE_ID` environment variables, falling back to
//! "Neutron".

use std::fmt;
use std::sync::OnceLock;

const DEFAULT_NAME: &str = "Neutron";
const DEFAULT_BUNDLE_ID: &str = "dev.neutron.Neutron";

static IDENTITY: OnceLock<AppIdentity> = OnceLock::new();

/// Release channels, mirroring `release_channel::ReleaseChannel` (which depends on this crate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Dev,
    Nightly,
    Preview,
    Stable,
}

impl Channel {
    const ALL: [Channel; 4] = [Channel::Dev, Channel::Nightly, Channel::Preview, Channel::Stable];

    fn index(self) -> usize {
        match self {
            Channel::Dev => 0,
            Channel::Nightly => 1,
            Channel::Preview => 2,
            Channel::Stable => 3,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Channel::Dev => "Dev",
            Channel::Nightly => "Nightly",
            Channel::Preview => "Preview",
            Channel::Stable => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppIdentity {
    name: String,
    name_lowercase: String,
    bundle_id: String,
    docs_url: Option<String>,
    display_names: [String; 4],
    app_ids: [String; 4],
    windows_identifiers: [String; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidIdentity(String);

impl fmt::Display for InvalidIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for InvalidIdentity {}

impl AppIdentity {
    /// `name` names the app's directories (for example `~/Library/Application Support/<name>`) and
    /// windows. `bundle_id` is the reverse-DNS identifier of the stable channel, such as
    /// `com.example.Notes`; other channels append `-Dev`, `-Nightly` or `-Preview`.
    pub fn new(name: impl Into<String>, bundle_id: impl Into<String>) -> Result<Self, InvalidIdentity> {
        let name = name.into();
        let bundle_id = bundle_id.into();
        validate_name(&name)?;
        if bundle_id.is_empty() || bundle_id.chars().any(char::is_whitespace) {
            return Err(InvalidIdentity(format!(
                "bundle id {bundle_id:?} must be non-empty and contain no whitespace"
            )));
        }

        let display_names = Channel::ALL.map(|channel| match channel.suffix() {
            "" => name.clone(),
            suffix => format!("{name} {suffix}"),
        });
        let app_ids = Channel::ALL.map(|channel| match channel.suffix() {
            "" => bundle_id.clone(),
            suffix => format!("{bundle_id}-{suffix}"),
        });
        let compact_name: String = name.chars().filter(|character| !character.is_whitespace()).collect();
        let windows_identifiers = Channel::ALL.map(|channel| match channel.suffix() {
            "" => format!("{compact_name}-Stable"),
            suffix => format!("{compact_name}-{suffix}"),
        });

        Ok(Self {
            name_lowercase: name.to_ascii_lowercase(),
            name,
            bundle_id,
            docs_url: None,
            display_names,
            app_ids,
            windows_identifiers,
        })
    }

    /// Base URL of the app's documentation, used by `release_channel::docs_url`.
    pub fn with_docs_url(mut self, docs_url: impl Into<String>) -> Self {
        self.docs_url = Some(docs_url.into().trim_end_matches('/').to_string());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Lowercase name, used for XDG-style directories on Linux and `~/.config` on macOS.
    pub fn name_lowercase(&self) -> &str {
        &self.name_lowercase
    }

    pub fn bundle_id(&self) -> &str {
        &self.bundle_id
    }

    pub fn docs_url(&self) -> Option<&str> {
        self.docs_url.as_deref()
    }

    /// Human-readable name for a channel, such as "Notes Preview".
    pub fn display_name(&self, channel: Channel) -> &str {
        &self.display_names[channel.index()]
    }

    /// Wayland app id, X11 WM_CLASS and macOS bundle identifier for a channel.
    pub fn app_id(&self, channel: Channel) -> &str {
        &self.app_ids[channel.index()]
    }

    /// Identifier used for Windows app registration, such as "Notes-Preview".
    pub fn windows_identifier(&self, channel: Channel) -> &str {
        &self.windows_identifiers[channel.index()]
    }
}

/// Sets the identity of the running app. Call it before anything resolves a path.
///
/// Fails if an identity is already in use, because paths derived from the old one may already be
/// cached.
pub fn init(identity: AppIdentity) -> Result<(), InvalidIdentity> {
    let name = identity.name.clone();
    IDENTITY.set(identity).map_err(|_| {
        let current = get().name();
        InvalidIdentity(format!(
            "cannot set the app identity to {name:?}: {current:?} is already in use; call riso_identity::init first in main"
        ))
    })
}

/// The identity of the running app.
pub fn get() -> &'static AppIdentity {
    IDENTITY.get_or_init(identity_from_environment)
}

fn identity_from_environment() -> AppIdentity {
    let name = std::env::var("NEUTRON_APP_NAME").unwrap_or_else(|_| DEFAULT_NAME.to_string());
    let bundle_id = std::env::var("NEUTRON_BUNDLE_ID").unwrap_or_else(|_| DEFAULT_BUNDLE_ID.to_string());
    match AppIdentity::new(name, bundle_id) {
        Ok(identity) => identity,
        Err(error) => {
            eprintln!("riso_identity: ignoring invalid identity from the environment: {error}");
            AppIdentity::new(DEFAULT_NAME, DEFAULT_BUNDLE_ID).unwrap_or_else(|_| unreachable!("the default identity is valid"))
        }
    }
}

fn validate_name(name: &str) -> Result<(), InvalidIdentity> {
    if name.is_empty() {
        return Err(InvalidIdentity("app name must not be empty".to_string()));
    }
    if !name.is_ascii() {
        return Err(InvalidIdentity(format!("app name {name:?} must be ASCII")));
    }
    if name.contains(['/', '\\']) {
        return Err(InvalidIdentity(format!("app name {name:?} must not contain path separators")));
    }
    if name.chars().any(|character| character.is_ascii_control()) {
        return Err(InvalidIdentity(format!("app name {name:?} must not contain control characters")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_channel_identifiers() {
        let identity = AppIdentity::new("Note Pad", "com.example.NotePad").expect("valid identity");
        assert_eq!(identity.name_lowercase(), "note pad");
        assert_eq!(identity.display_name(Channel::Stable), "Note Pad");
        assert_eq!(identity.display_name(Channel::Preview), "Note Pad Preview");
        assert_eq!(identity.app_id(Channel::Stable), "com.example.NotePad");
        assert_eq!(identity.app_id(Channel::Dev), "com.example.NotePad-Dev");
        assert_eq!(identity.windows_identifier(Channel::Nightly), "NotePad-Nightly");
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(AppIdentity::new("", "a.b").is_err());
        assert!(AppIdentity::new("a/b", "a.b").is_err());
        assert!(AppIdentity::new("Notes", "has space").is_err());
    }
}
