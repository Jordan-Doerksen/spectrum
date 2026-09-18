//! The log file itself — `log::init` and the sink in `crates\engine\src\log\sink.rs`.
//!
//! Two claims from the Definition of Done for CR-1 chunk 0: the logger **writes a
//! line**, and it **respects its cap** so it can never fill the disk. A third is
//! checked here because it is the one that matters on a public repo: **no secret
//! reaches the file.**
//!
//! The whole file is ONE test on purpose. The sink is process-global (`sink.rs:42`,
//! a `OnceLock<Mutex<Sink>>`) and cargo runs the tests inside one binary on parallel
//! threads, so two tests calling `log::init` would fight over the same file handle.
//! Cargo gives each `tests/*.rs` file its own process, so the isolation lives at the
//! file level instead. Every assertion carries its own message, which is what a
//! failure has to read like.
//!
//! The log directory is a scratch directory under the OS temp dir. Nothing here
//! touches `data\logs`, `data\seen.json` or `config.local.json`.

mod common;

use common::TempDir;
use spectrum_engine::config::LogConfig;
use spectrum_engine::log;

const FAKE_ID: &str = "100000000000000000";
const FAKE_TOKEN: &str = "FAKEtokenVALUE-Nf9x_ZZ-notReal";
const PREFIX: &str = "spectrum-sink-test-";

/// A cap small enough that a few dozen events force several rotations, and a run
/// limit small enough that the pruning is visible.
const CAP_BYTES: u64 = 700;
const FILES_PER_RUN: usize = 2;

#[test]
fn the_log_file_is_written_stays_under_its_cap_and_carries_no_secret() {
    let base = TempDir::new("logsink");
    let logs = base.join("logs");

    let cfg = LogConfig {
        dir: "logs".to_string(),
        file_pattern: format!("{PREFIX}{{pid}}.jsonl"),
        max_file_bytes: CAP_BYTES,
        max_files_per_run: FILES_PER_RUN,
        keep_run_files: 10,
        ..LogConfig::default()
    };

    // --- it opens where the config says, relative to the config file's directory ---
    let path = log::init(&cfg, base.path()).expect("log::init must open a file in a writable dir");
    assert_eq!(
        path.parent(),
        Some(logs.as_path()),
        "the log file must land in `base/<logging.dir>`, not the working directory"
    );
    assert!(path.is_file(), "log::init must create the run file, at {}", path.display());

    // --- calling init twice must not open a second file for one run ---
    let again = log::init(&cfg, base.path()).expect("a second init must be harmless");
    assert_eq!(again, path, "the panel and the engine both call init; one run means one file");

    // --- it writes a line ---
    log::info("test.sink.first").field("what", "the first event").emit();
    assert!(
        std::fs::read_to_string(&path).unwrap_or_default().contains("test.sink.first"),
        "the event must be on disk immediately — the sink flushes, it does not buffer"
    );

    // --- enough traffic to force the cap to act several times ---
    let padding = "x".repeat(80);
    for i in 0..40 {
        log::info("test.sink.traffic").count("n", i).field("padding", &padding).emit();
    }

    // --- a webhook goes in last, so the surviving file is the one to inspect ---
    let url = format!("https://discord.com/api/webhooks/{FAKE_ID}/{FAKE_TOKEN}");
    log::warn("test.sink.webhook")
        .webhook("news", &url)
        .field("url", &url)
        .not_doing("post this card", "this is a test, nothing was sent")
        .emit();

    // --- the cap held ---
    let live_bytes = std::fs::metadata(&path).expect("stat the live log file").len();
    assert!(
        live_bytes <= CAP_BYTES,
        "the live log file is {live_bytes} bytes, past the {CAP_BYTES}-byte cap"
    );

    let health = log::health();
    assert!(health.file_open, "the sink must still hold an open file after rotating");
    assert_eq!(health.write_errors, 0, "no write may fail: {:?}", health.last_error);
    assert!(
        health.rotations >= 2,
        "40 padded events past a {CAP_BYTES}-byte cap must rotate more than once, got {}",
        health.rotations
    );
    assert_eq!(
        health.bytes, live_bytes,
        "health() must report the live file's real size, or a status tile lies"
    );
    assert_eq!(health.buffered, 0, "nothing may stay buffered once a file is open");

    // --- the rotation prunes, so one run cannot grow without a limit ---
    let files = base_files(&base);
    assert_eq!(
        files.len(),
        FILES_PER_RUN,
        "one run may keep {FILES_PER_RUN} files; found {}: {:?}",
        files.len(),
        files
    );
    let total: u64 = files
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();
    assert!(
        total <= CAP_BYTES * FILES_PER_RUN as u64,
        "one run wrote {total} bytes across {FILES_PER_RUN} files, past the bound"
    );

    // --- every line is a parseable JSON object with the required keys ---
    let live = std::fs::read_to_string(&path).expect("read the live log file");
    let mut lines = 0;
    for line in live.lines().filter(|l| !l.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("a log line is not valid JSON ({e}): {line}"));
        assert!(value.get("ts").and_then(|v| v.as_str()).is_some(), "no `ts` on: {line}");
        assert!(value.get("level").and_then(|v| v.as_str()).is_some(), "no `level` on: {line}");
        assert!(value.get("event").and_then(|v| v.as_str()).is_some(), "no `event` on: {line}");
        lines += 1;
    }
    assert!(lines > 0, "the live file must hold at least one event");

    // --- no secret reached the disk, in any surviving file ---
    for file in &files {
        let body = std::fs::read_to_string(file).unwrap_or_default();
        assert!(
            !body.contains(FAKE_TOKEN),
            "a webhook token reached the log file {}",
            file.display()
        );
    }
    assert!(
        live.contains("REDACTED") && live.contains(FAKE_ID),
        "the webhook line must still name the channel it meant, with the token cut"
    );
    assert!(
        live.contains("post this card"),
        "the record must say what did NOT happen — the observability law's whole point"
    );
}

/// The run's files: the live one plus whatever rotated parts survive.
fn base_files(base: &TempDir) -> Vec<std::path::PathBuf> {
    let logs = base.join("logs");
    let Ok(entries) = std::fs::read_dir(&logs) else {
        panic!("the log directory {} does not exist", logs.display());
    };
    let mut found: Vec<std::path::PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| e.file_name().to_string_lossy().starts_with(PREFIX))
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}
