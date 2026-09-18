//! What the logger is allowed to delete — `prune_runs` in `crates\engine\src\log\sink.rs`.
//!
//! The pruner keeps the disk bounded across many runs by deleting old run files at
//! startup. It used to select them by filename prefix alone, and the default pattern's
//! prefix is `spectrum-`. With the shipped `logging.dir` of `data/logs` that is harmless.
//! Point `logging.dir` at `data` — not an obviously dangerous value — and the same prefix
//! also matches `data\spectrum-headless.lock`, the single-instance lock. Deleting that
//! lets a second runner start, and two runners post every card twice.
//!
//! So this file asserts what the pruner leaves alone. It is its own test target because
//! `log::init` opens the one process-global sink, and cargo gives each `tests\*.rs` file
//! its own process.
//!
//! Nothing here touches `data\`, `config.local.json` or the network: the directory is a
//! scratch directory under the OS temp dir, and the only call is `log::init`.

mod common;

use common::TempDir;
use spectrum_engine::config::LogConfig;
use spectrum_engine::log;

/// Two old runs survive, so the pruning is visible in the count.
const KEEP: usize = 2;
/// The directory an operator would reach for: the store's own directory, not `logs`.
const DIR: &str = "store";

#[test]
fn pruning_old_runs_never_deletes_the_single_instance_lock() {
    let base = TempDir::new("logprune");
    let store = base.join(DIR);
    std::fs::create_dir_all(&store).expect("create the scratch store directory");

    // What a live `data\` directory holds beside the logs.
    let lock = store.join("spectrum-headless.lock");
    let seen = store.join("seen.json");
    std::fs::write(&lock, "pid=4321\nheartbeat=1758226443\n").expect("write the decoy lock");
    std::fs::write(&seen, "[]").expect("write the decoy store");

    // Four earlier runs, more than `keep`, so the pruner has to act.
    let old: Vec<std::path::PathBuf> = (1..=4)
        .map(|n| {
            let f = store.join(format!("spectrum-2026091{n}-000000-{n}.jsonl"));
            std::fs::write(&f, "{\"event\":\"old.run\"}\n").expect("write an old run file");
            f
        })
        .collect();

    let cfg = LogConfig {
        dir: DIR.to_string(),
        file_pattern: "spectrum-{date}-{time}-{pid}.jsonl".to_string(),
        keep_run_files: KEEP,
        ..LogConfig::default()
    };
    let live = log::init(&cfg, base.path()).expect("log::init must open a file in a writable dir");
    log::info("test.prune.first").field("what", "the run file exists").emit();

    assert!(
        lock.is_file(),
        "the pruner deleted {} — a second runner can now start and double-post",
        lock.display()
    );
    assert!(seen.is_file(), "the pruner deleted the dedupe store at {}", seen.display());

    let jsonl: Vec<std::path::PathBuf> = std::fs::read_dir(&store)
        .expect("read the scratch store directory")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "jsonl").unwrap_or(false))
        .collect();
    assert_eq!(
        jsonl.len(),
        KEEP + 1,
        "expected {KEEP} kept runs plus this run's file, found: {jsonl:?}"
    );
    assert!(live.is_file(), "this run's own file must survive its own pruning");
    assert!(
        old.iter().filter(|p| p.is_file()).count() == KEEP,
        "exactly {KEEP} of the four old runs must survive: {old:?}"
    );
}
