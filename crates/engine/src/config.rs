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

    /// disk-logger settings. Absent in an older config.local.json — the defaults below
    /// then apply, so an old config keeps working and still gets a log. [CR-1 chunk 0]
    #[serde(default)]
    pub logging: LogConfig,

    /// single-instance-lock settings for the headless runner. Same promise as `logging`:
    /// leave the block out and the defaults apply. [CR-1 chunk 0]
    #[serde(default)]
    pub lock: LockConfig,
}

/// Every knob of the disk logger. No level, path or cap is a constant in the code.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// false writes no file at all; the console lines stay.
    pub enabled: bool,
    /// log directory, resolved relative to the config file (the way `seen_path` is).
    pub dir: String,
    /// the run file name. `{date}` `{time}` `{pid}` `{run}` are substituted.
    pub file_pattern: String,
    /// off | error | warn | info | debug — what reaches the file.
    pub file_level: String,
    /// off | error | warn | info | debug — what reaches the console.
    pub console_level: String,
    /// rotate the run file when it would pass this size. 0 = never rotate.
    pub max_file_bytes: u64,
    /// how many files ONE run may keep, the live file included. The oldest is deleted.
    pub max_files_per_run: usize,
    /// how many earlier run files to keep in the directory. 0 = keep them all.
    pub keep_run_files: usize,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: "data/logs".into(),
            file_pattern: "spectrum-{date}-{time}-{pid}.jsonl".into(),
            file_level: "info".into(),
            console_level: "info".into(),
            max_file_bytes: 5_000_000,
            max_files_per_run: 3,
            keep_run_files: 20,
        }
    }
}

/// Every knob of the headless runner's single-instance lock. The stale window in
/// particular is operationally load-bearing: it decides when a second copy may take the
/// lock and start double-posting, so it belongs in `.json` where the operator can see it,
/// not in a constant. [CR-1 chunk 0 · DECISIONS "Every threshold, interval and cap in
/// this CR lives in .json"]
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LockConfig {
    /// the lock file, inside the directory `seen_path` resolves into.
    pub file_name: String,
    /// how often the holder rewrites its heartbeat. 0 is read as 1.
    pub beat_seconds: u64,
    /// missed beats before a lock whose holder cannot be probed counts as dead.
    pub missed_beats: u64,
    /// reads of a record that is being rewritten right now, before it is called unreadable.
    pub read_tries: u32,
    /// the pause between those reads.
    pub read_retry_ms: u64,
}

impl Default for LockConfig {
    fn default() -> Self {
        Self {
            file_name: "spectrum-headless.lock".into(),
            beat_seconds: 15,
            missed_beats: 6,
            read_tries: 3,
            read_retry_ms: 250,
        }
    }
}

impl LockConfig {
    /// The heartbeat fallback window: long enough for a stalled disk, short enough that a
    /// crashed run does not block a restart for longer than it must.
    pub fn stale_seconds(&self) -> u64 {
        self.beat_seconds.max(1).saturating_mul(self.missed_beats)
    }
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
