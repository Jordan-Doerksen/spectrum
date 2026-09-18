//! What the lock file holds, and how it reaches the disk safely.
//!
//! Plain `key=value` lines, not JSON: the runner crate carries no serde dependency and
//! a new one is a Change Request (D-0003). The operator can read the file, and so can a
//! triage agent:
//!
//! ```text
//! pid=12345
//! exe=spectrum-headless.exe
//! started=2026-09-18T20:14:03.123Z
//! heartbeat=1758226443
//! ```

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub pid: u32,
    pub exe: String,
    pub started: String,
    pub heartbeat: u64,
}

impl Record {
    /// The record this process would write.
    pub fn of_this_process(heartbeat: u64) -> Self {
        Self {
            pid: std::process::id(),
            exe: exe_name(),
            started: spectrum_engine::log::fmt_utc(SystemTime::now()),
            heartbeat,
        }
    }
}

pub fn render(r: &Record) -> String {
    format!(
        "pid={}\nexe={}\nstarted={}\nheartbeat={}\n",
        r.pid, r.exe, r.started, r.heartbeat
    )
}

/// `None` when the text is not a record: empty, truncated, or missing `pid`/`heartbeat`.
fn parse(text: &str) -> Option<Record> {
    let (mut pid, mut heartbeat) = (None, None);
    let (mut exe, mut started) = (String::new(), String::new());
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "pid" => pid = value.trim().parse::<u32>().ok(),
            "heartbeat" => heartbeat = value.trim().parse::<u64>().ok(),
            "exe" => exe = value.trim().to_string(),
            "started" => started = value.trim().to_string(),
            _ => {}
        }
    }
    Some(Record { pid: pid?, exe, started, heartbeat: heartbeat? })
}

/// Read the record, retrying a write that is in flight (the file is rewritten by
/// rename, so a torn read is short and rare).
///
/// `tries` and `retry_ms` are the `lock.read_tries` and `lock.read_retry_ms` keys of
/// `config.local.json` — they were constants here until CR-1 chunk 0. `tries` is read as
/// at least 1, so a config of 0 still reads the file once instead of never.
pub fn read(path: &Path, tries: u32, retry_ms: u64) -> Option<Record> {
    let tries = tries.max(1);
    for attempt in 0..tries {
        if let Some(rec) = fs::read_to_string(path).ok().as_deref().and_then(parse) {
            return Some(rec);
        }
        if attempt + 1 < tries {
            std::thread::sleep(Duration::from_millis(retry_ms));
        }
    }
    None
}

/// Write through a temporary file and rename over the lock: a reader sees the old
/// record or the new one, never half of one.
pub fn write(path: &Path, rec: &Record) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, render(rec))?;
    fs::rename(&tmp, path)
}

fn exe_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default()
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Record {
        Record {
            pid: 4321,
            exe: "spectrum-headless.exe".into(),
            started: "2026-09-18T20:14:03.123Z".into(),
            heartbeat: 1_758_226_443,
        }
    }

    #[test]
    fn a_record_survives_a_round_trip() {
        let rec = sample();
        assert_eq!(parse(&render(&rec)), Some(rec));
    }

    #[test]
    fn half_a_record_is_no_record() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("pid=12\n"), None); // no heartbeat
        assert_eq!(parse("heartbeat=9\n"), None); // no pid
        assert_eq!(parse("pid=not-a-number\nheartbeat=9\n"), None);
    }

    #[test]
    fn a_write_can_be_read_back() {
        let dir = std::env::temp_dir().join(format!("spectrum-record-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("probe.lock");
        write(&path, &sample()).unwrap();
        assert_eq!(read(&path, 3, 250), Some(sample()));
        let _ = fs::remove_dir_all(&dir);
    }
}
