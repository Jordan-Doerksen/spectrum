//! Disk logger — one JSON object per event to a file per run, plus one readable
//! console line per event. [CR-1 chunk 0 · observability law]
//!
//! * An event says what the engine IS doing. A failure event also says what it is NOT
//!   doing, and why: `Event::not_doing(action, why)`.
//! * One key, one meaning. `error` holds what went wrong ([`Event::err`], or an explicit
//!   `.field("error", …)`); `not_doing` holds the action that did not happen and `why`
//!   holds its consequence. A failure record carries both, on one line.
//! * No secret reaches the disk. The event name and every string value pass through
//!   [`redact`], which cuts a Discord webhook down to its id and blanks a token, a
//!   secret, a password or an `Authorization` value anywhere in the text.
//! * The log cannot fill the disk: `max_file_bytes` rotates the run file,
//!   `max_files_per_run` caps this run, `keep_run_files` prunes old runs at startup.
//!   All three are config keys, never constants.
//! * The logger never panics and never swallows its own failure: a write error is
//!   counted in [`health`] and hits stderr once per distinct message; a poisoned mutex
//!   is recovered, not unwrapped.
//! * No new dependency (D-0003): the UTC stamp is std plus civil-from-days. Events
//!   emitted before [`init`] are buffered, then flushed into the file.
//!
//! Split by job (house law: review at 300 lines): this file is the public face —
//! levels, [`init`] and the [`Event`] builder. `log/sink.rs` owns the file, the
//! rotation and the pruning. `log/redact.rs` owns the secret-cutting.

mod redact;
mod sink;

pub use redact::{redact, webhook_id, webhook_label};
pub use sink::{health, LogHealth};

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::LogConfig;

// ------------------------------------------------------------------ levels

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        ["error", "warn", "info", "debug"][self as usize]
    }
    /// `"off"` -> `Ok(None)`: nothing passes. An unknown word -> `Err(())`.
    fn threshold(word: &str) -> Result<Option<Level>, ()> {
        match word.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "silent" => Ok(None),
            "error" => Ok(Some(Level::Error)),
            "warn" | "warning" => Ok(Some(Level::Warn)),
            "info" => Ok(Some(Level::Info)),
            "debug" | "trace" => Ok(Some(Level::Debug)),
            _ => Err(()),
        }
    }
    fn passes(self, threshold: Option<Level>) -> bool {
        matches!(threshold, Some(max) if self <= max)
    }
}

fn level_word(t: Option<Level>) -> &'static str {
    t.map(Level::as_str).unwrap_or("off")
}

// ------------------------------------------------------------------ init

/// Open this run's log file. `base` is the directory the config file sits in, so
/// `logging.dir` resolves exactly the way `seen_path` does. Safe to call twice: a
/// second call with the same resolved path only re-applies the levels and the caps,
/// so the panel and the engine can both call it.
pub fn init(cfg: &LogConfig, base: &Path) -> anyhow::Result<PathBuf> {
    let file_level = level_or_info(&cfg.file_level, "file_level");
    let console_level = level_or_info(&cfg.console_level, "console_level");
    let dir = base.join(&cfg.dir);
    let path = dir.join(sink::render_pattern(&cfg.file_pattern));
    sink::configure(file_level, console_level, cfg.max_file_bytes, cfg.max_files_per_run);
    if !cfg.enabled {
        sink::disable(&path);
        warn("log.file.disabled")
            .not_doing("write any log file", "logging.enabled is false in config")
            .emit();
        return Ok(path);
    }
    if sink::is_open_at(&path) {
        return Ok(path);
    }
    std::fs::create_dir_all(&dir)?;
    sink::prune_runs(
        &dir,
        &sink::pattern_prefix(&cfg.file_pattern),
        &sink::pattern_ext(&cfg.file_pattern),
        cfg.keep_run_files,
    );
    sink::open(&path)?;
    info("log.started")
        .field("file", path.to_string_lossy())
        .field("file_level", level_word(file_level))
        .field("console_level", level_word(console_level))
        .num("max_file_bytes", cfg.max_file_bytes as i64)
        .count("max_files_per_run", cfg.max_files_per_run)
        .count("keep_run_files", cfg.keep_run_files)
        .emit();
    Ok(path)
}

/// An unknown level word is a config error, not a silent fallback: it is logged.
fn level_or_info(word: &str, key: &str) -> Option<Level> {
    Level::threshold(word).unwrap_or_else(|()| {
        warn("log.level.unknown")
            .field("key", key)
            .field("value", word)
            .not_doing("apply the configured level", "expected off/error/warn/info/debug — using info")
            .emit();
        Some(Level::Info)
    })
}

// ------------------------------------------------------------------ events

pub fn error(event: &str) -> Event { Event::new(Level::Error, event) }
pub fn warn(event: &str) -> Event { Event::new(Level::Warn, event) }
pub fn info(event: &str) -> Event { Event::new(Level::Info, event) }
pub fn debug(event: &str) -> Event { Event::new(Level::Debug, event) }

/// One log event: build it, then `emit()`. Values are held JSON-encoded, so the same
/// field list writes both the JSON line and the console line.
pub struct Event {
    level: Level,
    name: String,
    fields: Vec<(String, String)>,
}

impl Event {
    fn new(level: Level, event: &str) -> Self {
        Self { level, name: redact(event), fields: Vec::new() }
    }

    fn put(mut self, key: &str, json_value: String) -> Self {
        match self.fields.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = json_value,
            None => self.fields.push((key.to_string(), json_value)),
        }
        self
    }

    /// A string field. The value is redacted before it is written.
    pub fn field(self, key: &str, value: impl AsRef<str>) -> Self {
        let v = json_str(&redact(value.as_ref()));
        self.put(key, v)
    }
    pub fn num(self, key: &str, value: i64) -> Self { self.put(key, value.to_string()) }
    pub fn count(self, key: &str, value: usize) -> Self { self.put(key, value.to_string()) }
    pub fn flag(self, key: &str, value: bool) -> Self { self.put(key, value.to_string()) }
    /// The error behind this event, as the `error` field.
    ///
    /// It was the `why` field until 2026-09-18, and `not_doing` writes `why` too, so
    /// `.err(e).not_doing(a, w)` overwrote the error text with the reason and the record
    /// said what did not happen but never why it failed. One key, one meaning:
    /// `error` is what went wrong, `why` is the consequence spelled out by `not_doing`.
    pub fn err(self, e: impl std::fmt::Display) -> Self {
        let v = json_str(&redact(&e.to_string()));
        self.put("error", v)
    }
    /// The observability law in one call: the action that did NOT happen, and why.
    pub fn not_doing(self, action: &str, why: &str) -> Self {
        self.field("not_doing", action).field("why", why)
    }
    /// A webhook named safely: the channel key plus the webhook id, never the token.
    pub fn webhook(self, channel: &str, url: &str) -> Self {
        let v = json_str(&webhook_label(channel, url));
        self.put("webhook", v)
    }

    /// Write the event. Returns the console line, so a caller can also push it into
    /// the panel's in-memory ring: `self.note(log::warn("x").emit())`.
    pub fn emit(self) -> String {
        let ts = fmt_utc(SystemTime::now());
        let mut json = String::with_capacity(96 + self.name.len());
        json.push_str("{\"ts\":");
        json.push_str(&json_str(&ts));
        json.push_str(",\"level\":\"");
        json.push_str(self.level.as_str());
        json.push_str("\",\"event\":");
        json.push_str(&json_str(&self.name));
        let mut console = format!(
            "{} {:<5} {}",
            ts[..19].replace('T', " "), self.level.as_str().to_uppercase(), self.name
        );
        for (k, v) in &self.fields {
            json.push(',');
            json.push_str(&json_str(k));
            json.push(':');
            json.push_str(v);
            console.push(' ');
            console.push_str(k);
            console.push('=');
            console.push_str(v);
        }
        json.push('}');
        if sink::dispatch(self.level, &json) {
            // stdio happens outside the sink lock
            if self.level <= Level::Warn { eprintln!("{console}") } else { println!("{console}") }
        }
        console
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// `2026-09-18T20:14:03.123Z`. UTC only — no zone guessing, no time crate (D-0003).
pub fn fmt_utc(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let (y, m, day) = civil_from_days((secs / 86_400) as i64);
    let sod = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        sod / 3600, (sod % 3600) / 60, sod % 60, d.subsec_millis()
    )
}

/// Days since 1970-01-01 -> (year, month, day). Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, day)
}
