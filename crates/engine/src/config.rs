//! Local runtime config — webhooks + tuning. Lives in `config.local.json`, which is
//! gitignored: the Discord webhooks are secrets and never get committed. [D-0003]

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Config {
    /// band key ("financial" | "political" | "technology" | "catastrophe") -> webhook URL
    pub webhooks: HashMap<String, String>,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_min_severity")]
    pub min_severity: u8,

    /// minutes between feed polls
    #[serde(default = "default_poll_minutes")]
    pub poll_minutes: u64,
    /// seconds between drip posts (cards trickle, never burst)
    #[serde(default = "default_drip_seconds")]
    pub drip_seconds: u64,
    /// max cards posted per drip tick
    #[serde(default = "default_max_per_drop")]
    pub max_per_drop: usize,
    /// dedupe store path, resolved relative to the config file
    #[serde(default = "default_seen_path")]
    pub seen_path: String,
}

fn default_model() -> String {
    "llama3.1:8b".into()
}
fn default_min_severity() -> u8 {
    2
}
fn default_poll_minutes() -> u64 {
    10
}
fn default_drip_seconds() -> u64 {
    90
}
fn default_max_per_drop() -> usize {
    1
}
fn default_seen_path() -> String {
    "data/seen.json".into()
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let txt = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&txt)?)
    }

    pub fn webhook_for(&self, key: &str) -> Option<&String> {
        self.webhooks.get(key)
    }
}
