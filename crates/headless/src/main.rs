//! Spectrum runner — the always-on engine, a thin loop over the `Engine` service
//! (the same service the Tauri panel drives).
//!
//! Flags: `--once` (one cycle then exit) · `--dry` (never post) · `--feedcheck`
//! (fetch every feed, report ok/err, exit).
//!
//! Two things land here in CR-1 chunk 0.
//! * **The disk log opens here**, as early as the config allows, so a failure before
//!   the first poll still leaves a record. A config that will not load still gets a
//!   log, written with the default logging settings.
//! * **A single-instance lock** (`lock.rs`) stops a second runner on this host from
//!   racing `data\seen.json` and posting every card twice. `--feedcheck` takes no lock:
//!   it reads feeds, touches no store and posts nothing. `--dry` DOES take it, because
//!   a dry run still marks headlines seen.
//!
//! No `println!` carries anything the log file does not: every line below is a log
//! event, which prints its own console line. The one exception is the block the lock
//! prints when it refuses to start, which restates logged facts for a person.

mod lock;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use spectrum_engine::config::Config;
use spectrum_engine::{engine::Engine, feeds, log, rss, ua_client};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dry = std::env::args().any(|a| a == "--dry");
    let once = std::env::args().any(|a| a == "--once");
    let feedcheck = std::env::args().any(|a| a == "--feedcheck");

    let cfg_path = std::env::var("SPECTRUM_CONFIG").unwrap_or_else(|_| "config.local.json".into());
    let base = Path::new(&cfg_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    // The config says where the log goes, so load it first — but a config error is
    // exactly the kind of failure the log exists to record, so a failed load still
    // opens a log with the defaults.
    let cfg = Config::load(&cfg_path);
    open_log(cfg.as_ref().ok(), &base);

    if feedcheck {
        return feedcheck_run().await;
    }

    let cfg = match cfg {
        Ok(c) => c,
        Err(e) => {
            log::error("config.load.failed")
                .field("path", &cfg_path)
                .err(&e)
                .not_doing(
                    "start the engine",
                    "the runner has no webhooks and no tuning without this file",
                )
                .emit();
            return Err(anyhow::anyhow!("config '{cfg_path}': {e}"));
        }
    };

    // One runner per host. The lock lives beside the store it protects, and every one of
    // its knobs (the file name, the beat, the stale window) is the `lock` block of the
    // config, not a constant. [CR-1 chunk 0]
    let lock = lock::acquire(&data_dir(&base, &cfg.seen_path), &cfg.lock)?;
    if lock::panel_running() {
        log::warn("runner.panel.running")
            .field("process", lock::PANEL_EXE)
            .not_doing(
                "block this start",
                "this lock covers headless copies only — if the panel's engine is running, both post",
            )
            .emit();
    }
    {
        let beat = lock.clone();
        let every = beat.beat_seconds();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(every)).await;
                beat.beat();
            }
        });
    }

    let outcome = run(&cfg_path, dry, once).await;
    lock.release();
    if let Err(e) = &outcome {
        log::error("runner.stopped.failed")
            .err(e)
            .not_doing("keep polling", "the runner exited on an error")
            .emit();
    } else {
        let h = log::health();
        log::info("runner.stopped")
            .field("log", h.path)
            .num("log_bytes", h.bytes as i64)
            .num("log_write_errors", h.write_errors as i64)
            .emit();
    }
    outcome
}

/// The poll/drip loop. Separated from `main` so the lock is released on every exit.
async fn run(cfg_path: &str, dry: bool, once: bool) -> anyhow::Result<()> {
    let mut engine = match Engine::new(cfg_path) {
        Ok(e) => e,
        Err(e) => {
            log::error("engine.init.failed")
                .field("config", cfg_path)
                .err(&e)
                .not_doing("start the poll loop", "the engine could not be built")
                .emit();
            return Err(anyhow::anyhow!("config '{cfg_path}': {e}"));
        }
    };

    let st = engine.status();
    log::info("runner.started")
        .flag("dry", dry)
        .flag("once", once)
        .num("poll_minutes", st.poll_minutes as i64)
        .num("drip_seconds", st.drip_seconds as i64)
        .num("min_severity", st.min_severity as i64)
        .count("feeds", st.feeds)
        .count("seen", st.seen)
        .emit();

    if engine.seen_empty() {
        let n = engine.seed().await;
        log::info("runner.seeded")
            .count("headlines", n)
            .not_doing(
                "post any card",
                "the first run seeds the backlog and stays silent",
            )
            .emit();
        if once {
            return Ok(());
        }
    }

    let mut last_poll = Instant::now();
    let mut first = true;
    loop {
        if first || last_poll.elapsed().as_secs() >= engine.cfg.poll_minutes * 60 {
            engine.reload_config();
            let new = engine.poll().await;
            last_poll = Instant::now();
            first = false;
            let st = engine.status();
            log::info("runner.poll.done")
                .count("new", new)
                .count("queued", st.queued)
                .count("seen", st.seen)
                .emit();
        }

        for line in engine.drip(dry).await {
            log::info("runner.drip").flag("dry", dry).field("card", &line).emit();
        }

        if once {
            log::info("runner.once.done")
                .count("queued", engine.status().queued)
                .emit();
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(engine.cfg.drip_seconds)).await;
    }
}

/// `--feedcheck`: fetch every feed once, record ok/err + counts, exit. No config, no
/// lock, no store, no posting.
async fn feedcheck_run() -> anyhow::Result<()> {
    let client = ua_client();
    let (mut ok, mut failed, mut items) = (0usize, 0usize, 0usize);
    for feed in feeds::feeds() {
        match rss::fetch(&client, &feed).await {
            Ok(list) => {
                ok += 1;
                items += list.len();
                log::info("feedcheck.ok")
                    .field("feed", &feed.source)
                    .count("items", list.len())
                    .field("hint", format!("{:?}", feed.hint))
                    .emit();
            }
            Err(e) => {
                failed += 1;
                log::error("feedcheck.failed")
                    .field("feed", &feed.source)
                    .err(e)
                    .not_doing(
                        "read any item from this feed",
                        "the fetch or the parse failed — a live poll would skip it in silence",
                    )
                    .emit();
            }
        }
    }
    log::info("feedcheck.done")
        .count("ok", ok)
        .count("failed", failed)
        .count("items", items)
        .emit();
    Ok(())
}

/// Open the run's log file. The logger buffers what it cannot write yet, so events
/// emitted before this call still reach the file.
fn open_log(cfg: Option<&Config>, base: &Path) {
    let logging = cfg.map(|c| c.logging.clone()).unwrap_or_default();
    if let Err(e) = log::init(&logging, base) {
        log::error("log.init.failed")
            .field("dir", &logging.dir)
            .err(e)
            .not_doing(
                "write a log file",
                "the console lines are the only record for this run",
            )
            .emit();
    }
}

/// The directory `seen_path` resolves into — the lock belongs beside the store it
/// protects, and that keeps both under the one gitignored `data\` path.
fn data_dir(base: &Path, seen_path: &str) -> PathBuf {
    let seen = base.join(seen_path);
    seen.parent().map(Path::to_path_buf).unwrap_or_else(|| base.join("data"))
}
