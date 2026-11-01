//! Saved addresses and settings, as JSON in the user's config folder
//! (%APPDATA%\RackIP on Windows, ~/.config/RackIP on Linux).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: u64,
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub mask: String,
    #[serde(default)]
    pub gateway: String,
    /// Port it was last applied to, so picking the profile also picks the port.
    #[serde(default)]
    pub adapter: Option<String>,
    #[serde(default)]
    pub last_used: u64,
}

impl Profile {
    /// Unnamed addresses are named after the IP, so show it only once.
    pub fn is_named_after_address(&self) -> bool {
        self.name.is_empty() || self.name == self.address
    }

    pub fn title(&self) -> String {
        if self.is_named_after_address() {
            self.address.clone()
        } else {
            format!("{} — {}", self.name, self.address)
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub selected_adapter: Option<String>,
    #[serde(default)]
    pub show_all: bool,
}

impl Store {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("RackIP").join("rackip.json"))
    }

    pub fn load() -> Store {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Most recently used first.
    pub fn sorted_profiles(&self) -> Vec<Profile> {
        let mut v = self.profiles.clone();
        v.sort_by(|a, b| b.last_used.cmp(&a.last_used).then_with(|| a.title().cmp(&b.title())));
        v
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn new_id() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1)
}
