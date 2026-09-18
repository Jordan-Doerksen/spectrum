//! The log file itself: the process-wide sink, the size cap, the rotation and the
//! pruning of old runs. Nothing here formats an event — that is `log.rs`.
//!
//! Every knob arrives from `LogConfig` through [`configure`]. The defaults below only
//! cover the window before `log::init` runs, so an early failure still reaches stderr
//! and is buffered for the file.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{fmt_utc, Level};

struct Sink {
    file: Option<File>,
    path: PathBuf,
    written: u64,
    parts: Vec<PathBuf>, // rotated parts of THIS run, oldest first
    max_bytes: u64,
    max_files: usize,
    file_level: Option<Level>,
    console_level: Option<Level>,
    buffered: Vec<String>, // events emitted before a file existed
    rotations: u64,
    write_errors: u64,
    last_error: Option<String>,
}

impl Default for Sink {
    fn default() -> Self {
        Self {
            file: None, path: PathBuf::new(), written: 0, parts: Vec::new(),
            max_bytes: 5_000_000, max_files: 3,
            file_level: Some(Level::Info), console_level: Some(Level::Info),
            buffered: Vec::new(), rotations: 0, write_errors: 0, last_error: None,
        }
    }
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

/// A poisoned lock must never stop the log: recover the guard instead of panicking.
fn sink() -> MutexGuard<'static, Sink> {
    SINK.get_or_init(|| Mutex::new(Sink::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ------------------------------------------------------------------ the API log.rs uses

pub(super) fn configure(
    file_level: Option<Level>,
    console_level: Option<Level>,
    max_bytes: u64,
    max_files: usize,
) {
    let mut s = sink();
    s.file_level = file_level;
    s.console_level = console_level;
    s.max_bytes = max_bytes;
    s.max_files = max_files.max(1);
}

/// Logging is off in config: drop the file and the buffer, keep the console levels.
pub(super) fn disable(path: &Path) {
    let mut s = sink();
    s.file = None;
    s.path = path.to_path_buf();
    s.buffered.clear();
}

pub(super) fn is_open_at(path: &Path) -> bool {
    let s = sink();
    s.file.is_some() && s.path == path
}

/// Open the run file and flush everything emitted before it existed.
pub(super) fn open(path: &Path) -> std::io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut s = sink();
    s.file = Some(file);
    s.path = path.to_path_buf();
    s.written = size;
    s.parts.clear();
    for line in std::mem::take(&mut s.buffered) {
        s.write_line(&line);
    }
    Ok(())
}

/// Write the JSON line if it passes the file level. Returns true when the caller
/// should also print the console line — stdio stays outside this lock.
pub(super) fn dispatch(level: Level, json: &str) -> bool {
    let mut s = sink();
    if level.passes(s.file_level) {
        s.write_line(json);
    }
    level.passes(s.console_level)
}

/// What the log is doing right now — for a status tile or a shutdown line.
#[derive(Clone, Debug, serde::Serialize)]
pub struct LogHealth {
    pub path: String,
    pub file_open: bool,
    pub bytes: u64,
    pub rotations: u64,
    pub buffered: usize,
    pub write_errors: u64,
    pub last_error: Option<String>,
}

pub fn health() -> LogHealth {
    let s = sink();
    LogHealth {
        path: s.path.to_string_lossy().to_string(),
        file_open: s.file.is_some(),
        bytes: s.written,
        rotations: s.rotations,
        buffered: s.buffered.len(),
        write_errors: s.write_errors,
        last_error: s.last_error.clone(),
    }
}

// ------------------------------------------------------------------ writing

impl Sink {
    fn write_line(&mut self, line: &str) {
        if self.file.is_none() {
            if self.buffered.len() < 400 {
                self.buffered.push(line.to_string());
            }
            return;
        }
        let need = line.len() as u64 + 1;
        if self.max_bytes > 0 && self.written + need > self.max_bytes {
            self.rotate();
        }
        let res = self.file.as_mut().map(|f| {
            f.write_all(line.as_bytes()).and_then(|_| f.write_all(b"\n")).and_then(|_| f.flush())
        });
        match res {
            Some(Ok(())) => self.written += need,
            Some(Err(e)) => self.note_failure(e),
            None => {}
        }
    }

    /// Roll the run file aside, open a fresh one, keep `max_files` files for this run.
    fn rotate(&mut self) {
        self.file = None; // close before the rename
        let part = part_path(&self.path, self.rotations + 1);
        if fs::rename(&self.path, &part).is_ok() {
            self.parts.push(part);
        }
        while self.parts.len() > self.max_files.saturating_sub(1) {
            let _ = fs::remove_file(self.parts.remove(0));
        }
        match OpenOptions::new().create(true).append(true).open(&self.path) {
            Ok(mut f) => {
                self.rotations += 1;
                let line = format!(
                    "{{\"ts\":\"{}\",\"level\":\"info\",\"event\":\"log.rotated\",\"rotations\":{},\"parts_kept\":{}}}",
                    fmt_utc(SystemTime::now()), self.rotations, self.parts.len()
                );
                let _ = writeln!(f, "{line}");
                self.written = line.len() as u64 + 1;
                self.file = Some(f);
            }
            Err(e) => self.note_failure(e),
        }
    }

    /// The logger's own failure is never silent, and never a panic.
    fn note_failure(&mut self, e: std::io::Error) {
        self.write_errors += 1;
        let msg = e.to_string();
        if self.last_error.as_deref() != Some(msg.as_str()) {
            eprintln!("[spectrum] LOG WRITE FAILED ({}): {msg}", self.path.display());
        }
        self.last_error = Some(msg);
    }
}

// ------------------------------------------------------------------ file naming

fn part_path(path: &Path, n: u64) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let ext = path.extension().unwrap_or_default().to_string_lossy().to_string();
    path.with_file_name(if ext.is_empty() {
        format!("{stem}.{n}")
    } else {
        format!("{stem}.{n}.{ext}")
    })
}

/// The date and time of THIS run, fixed the first time they are asked for. The stamp
/// must not move: the panel and the engine both call `log::init`, and a second call
/// has to resolve to the same file name, not open a second file for one run.
fn run_stamp() -> &'static (String, String) {
    static STAMP: OnceLock<(String, String)> = OnceLock::new();
    STAMP.get_or_init(|| {
        let ts = fmt_utc(SystemTime::now());
        (ts[..10].replace('-', ""), ts[11..19].replace(':', ""))
    })
}

/// `{date}` `{time}` `{pid}` `{run}` in the configured file name. Anything that is
/// not a letter, a digit, `.`, `-` or `_` becomes `-`, so a config typo cannot walk
/// out of the log directory.
pub(super) fn render_pattern(pattern: &str) -> String {
    let (date, time) = run_stamp().clone();
    let name: String = pattern
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{run}", &format!("{date}-{time}"))
        .replace("{pid}", &std::process::id().to_string())
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' })
        .collect();
    if name.trim_matches('-').is_empty() { format!("spectrum-{date}-{time}.jsonl") } else { name }
}

/// The literal head of the pattern, so pruning only ever deletes this engine's files.
pub(super) fn pattern_prefix(pattern: &str) -> String {
    pattern.split('{').next().unwrap_or("").to_string()
}

/// The extension the run files carry. Empty when the pattern names none.
pub(super) fn pattern_ext(pattern: &str) -> String {
    ext_of(pattern)
}

fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// May the pruner delete this file? Only this engine's own run files.
///
/// The prefix alone is not enough. The default pattern yields the prefix `spectrum-`, and
/// an operator who sets `logging.dir` to `data` — not an obviously dangerous value — puts
/// the live single-instance lock, `data\spectrum-headless.lock`, in the prune set. Delete
/// that and a second runner starts and posts every card twice. So the extension has to
/// match the pattern's own as well, and a `.lock` file is never a candidate whatever it
/// is called. [CR-1 chunk 0]
fn prunable(name: &str, prefix: &str, ext: &str) -> bool {
    let found = ext_of(name);
    found != "lock"
        && (prefix.is_empty() || name.starts_with(prefix))
        && found == ext.to_ascii_lowercase()
}

/// Keep the newest `keep` run files in `dir`; delete the rest. Bounds the disk across
/// many runs, the way `max_file_bytes` bounds one run.
pub(super) fn prune_runs(dir: &Path, prefix: &str, ext: &str, keep: usize) {
    if keep == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| prunable(&e.file_name().to_string_lossy(), prefix, ext))
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH), e.path()))
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0)); // newest first
    for (_, path) in files.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::{pattern_ext, pattern_prefix, prunable};

    const PATTERN: &str = "spectrum-{date}-{time}-{pid}.jsonl";

    #[test]
    fn the_prune_filter_takes_run_files_and_leaves_everything_else() {
        let (p, e) = (pattern_prefix(PATTERN), pattern_ext(PATTERN));
        assert_eq!((p.as_str(), e.as_str()), ("spectrum-", "jsonl"));

        assert!(prunable("spectrum-20260918-201403-8123.jsonl", &p, &e), "an old run file");
        assert!(prunable("spectrum-20260918-201403-8123.1.jsonl", &p, &e), "a rotated part");

        // The one that cost a lock: same prefix, same directory, different job.
        assert!(!prunable("spectrum-headless.lock", &p, &e), "the single-instance lock");
        assert!(!prunable("seen.json", &p, &e), "the dedupe store");
        assert!(!prunable("spectrum-notes.txt", &p, &e), "a file that only shares the prefix");
        assert!(!prunable("richter-20260918.jsonl", &p, &e), "another tool's log");
    }

    #[test]
    fn a_lock_file_is_never_prunable_even_when_the_pattern_asks_for_one() {
        let (p, e) = (pattern_prefix("spectrum-{run}.lock"), pattern_ext("spectrum-{run}.lock"));
        assert!(!prunable("spectrum-headless.lock", &p, &e));
    }
}
