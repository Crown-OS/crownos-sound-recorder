use anyhow::Result;

use super::Capture;
use crate::recording::Library;
use crate::settings::{Persistence, Preferences};

/// Everything the window is started with.
pub struct Environment {
    pub library: Library,
    pub preferences: Preferences,
    pub capture: Capture,
    pub persistence: Persistence,
}

impl Environment {
    /// The user's recordings folder, saved preferences and default microphone.
    pub fn system() -> Result<Self> {
        Ok(Self {
            library: Library::open_default()?,
            preferences: Preferences::load(),
            capture: Capture::Microphone,
            persistence: Persistence::Config,
        })
    }
}
