//! Single-instance lock for the headless runner. [CR-1 chunk 0 · BRIEF §11]
//!
//! The panel has a lock (`src-tauri\src\lib.rs:183-189`), the runner had none, so two
//! headless copies on one host both polled, both raced `data\seen.json`, and both
//! posted. That is the D-0013 duplicate, still open on this side.
//!
//! The lock is a file under the data directory (the directory `seen_path` resolves
//! into), so it sits beside the store it protects and it is already gitignored.
//! `record.rs` owns its contents; `liveness.rs` owns the question below.
//!
//! **Taking it.** `create_new` is atomic, so the first copy wins the file. A copy that
//! loses reads the record and answers one question: is the holder alive?
//!
//! **Alive or stale.** Two independent checks, in this order.
//! 1. `tasklist /FI "PID eq <pid>"` — no row for the pid means the holder is dead and
//!    the lock is stale. A row whose image name matches means it runs. A row whose name
//!    does NOT match (a recycled pid, or a name past the 25-character column) answers
//!    nothing, and check 2 decides. No guess ever frees a lock.
//! 2. The heartbeat: the holder rewrites the file every `lock.beat_seconds`, so a stamp
//!    older than `beat_seconds × missed_beats` is a dead process.
//!
//! **Its knobs are config, not constants** (CR-1 chunk 0): the file name, the beat, the
//! missed-beat count and the two read-retry values are the `lock` block of
//! `config.local.json` ([`LockConfig`]). The defaults are the values this file used to
//! hardcode, so a config without a `lock` block behaves exactly as before.
//!
//! A stale lock is taken over, and the takeover is logged with the dead pid, the age and
//! the check that decided it. A live lock stops the start: nothing polls, nothing posts.
//!
//! **What this lock does NOT cover** (carried, not fixed in chunk 0): the panel
//! (`spectrum-pro.exe`) keeps its own Tauri lock and never touches this file, so a panel
//! and a runner on one host still double-post. The runner logs a warning when it sees
//! the panel process; it does not refuse, because an open panel is not a running engine.
//! The fix is one lock in the engine crate, taken where the poll loop starts.
//!
//! **Residual race:** two copies started in the same millisecond can both read the file
//! before the winner has written its record. The reader retries before it calls a record
//! unreadable, which closes the window in practice but not in theory.

mod liveness;
mod record;

pub use liveness::{panel_running, PANEL_EXE};

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use liveness::{probe, Liveness};
use record::{now_unix, render, Record};
use spectrum_engine::config::LockConfig;
use spectrum_engine::log;

// ------------------------------------------------------------------ taking the lock

/// Take the lock, or refuse to start. The refusal is an `Err`; the reason is on the
/// console and in the log before it returns.
pub fn acquire(dir: &Path, cfg: &LockConfig) -> anyhow::Result<Lock> {
    acquire_with(dir, cfg, now_unix(), probe)
}

/// The same, with the clock and the liveness check injected — the seam the tests use,
/// so no test spawns a process or waits out a stale window.
fn acquire_with(
    dir: &Path,
    cfg: &LockConfig,
    now: u64,
    probe: impl Fn(&Record) -> Liveness,
) -> anyhow::Result<Lock> {
    fs::create_dir_all(dir)?;
    let path = dir.join(&cfg.file_name);
    let me = Record::of_this_process(now);

    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(render(&me).as_bytes())?;
            return Ok(held(path, me, cfg, "fresh"));
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
        Err(e) => return Err(anyhow::anyhow!("lock file '{}': {e}", path.display())),
    }

    // The file was there. Either another copy runs, or the last one died holding it.
    let reason = match record::read(&path, cfg.read_tries, cfg.read_retry_ms) {
        None => {
            log::warn("lock.unreadable")
                .field("lock", path.to_string_lossy())
                .not_doing("trust the lock file", "it holds no readable pid — treating it as stale")
                .emit();
            "unreadable"
        }
        Some(r) if r.pid == me.pid => {
            log::warn("lock.self")
                .field("lock", path.to_string_lossy())
                .num("pid", r.pid as i64)
                .not_doing("refuse this start", "the lock names this very process — a leftover from a killed run")
                .emit();
            "same-pid"
        }
        Some(r) => {
            let age = now.saturating_sub(r.heartbeat);
            let live = probe(&r);
            let running = match live {
                Liveness::Alive => true,
                Liveness::Dead => false,
                Liveness::Unknown => age <= cfg.stale_seconds(),
            };
            if running {
                return Err(busy(&path, &r, age, live));
            }
            log::warn("lock.stale.recovered")
                .field("lock", path.to_string_lossy())
                .num("dead_pid", r.pid as i64)
                .field("dead_exe", &r.exe)
                .num("heartbeat_age_secs", age as i64)
                .field("decided_by", live.as_str())
                .not_doing("refuse this start", "the process that held the lock is gone")
                .emit();
            "stale"
        }
    };
    record::write(&path, &me)?;
    Ok(held(path, me, cfg, reason))
}

fn held(path: PathBuf, rec: Record, cfg: &LockConfig, how: &str) -> Lock {
    log::info("lock.taken")
        .field("lock", path.to_string_lossy())
        .num("pid", rec.pid as i64)
        .field("how", how)
        .num("beat_secs", cfg.beat_seconds.max(1) as i64)
        .num("stale_secs", cfg.stale_seconds() as i64)
        .emit();
    Lock {
        path,
        rec,
        cfg: cfg.clone(),
        released: Arc::new(AtomicBool::new(false)),
        beat_failures: Arc::new(AtomicU64::new(0)),
    }
}

/// The refusal: one log event with the facts, then the same facts as a block a person
/// can act on. Nothing starts, nothing polls, nothing posts.
fn busy(path: &Path, r: &Record, age: u64, live: Liveness) -> anyhow::Error {
    log::error("lock.busy")
        .field("lock", path.to_string_lossy())
        .num("running_pid", r.pid as i64)
        .field("running_exe", &r.exe)
        .field("running_since", &r.started)
        .num("heartbeat_age_secs", age as i64)
        .field("decided_by", live.as_str())
        .not_doing("start the engine", "another copy on this host holds the lock — two copies double-post")
        .emit();
    eprintln!(
        "\nSpectrum is already running on this host.\n  \
         pid        {} ({})\n  \
         started    {}\n  \
         heartbeat  {age} s ago\n  \
         lock file  {}\n\
         This copy will not start: two copies post every card twice.\n\
         Run STOP.bat to halt the running copy. If you are sure none runs, delete the lock file.\n",
        r.pid,
        if r.exe.is_empty() { "unknown image" } else { &r.exe },
        if r.started.is_empty() { "unknown" } else { &r.started },
        path.display()
    );
    anyhow::anyhow!("another Spectrum runner (pid {}) holds {}", r.pid, path.display())
}

// ------------------------------------------------------------------ holding it

/// The held lock. Clone it for the heartbeat task: every clone shares one released flag,
/// so a beat that fires after [`Lock::release`] cannot resurrect the file.
#[derive(Clone, Debug)]
pub struct Lock {
    path: PathBuf,
    rec: Record,
    cfg: LockConfig,
    released: Arc<AtomicBool>,
    beat_failures: Arc<AtomicU64>,
}

impl Lock {
    /// How often the holder must call [`Lock::beat`] — the runner's timer reads it here
    /// rather than from a constant of its own.
    pub fn beat_seconds(&self) -> u64 {
        self.cfg.beat_seconds.max(1)
    }

    /// Rewrite the heartbeat. Called every [`Lock::beat_seconds`] while the runner lives.
    pub fn beat(&self) {
        if self.released.load(Ordering::SeqCst) {
            return;
        }
        let mut rec = self.rec.clone();
        rec.heartbeat = now_unix();
        match record::write(&self.path, &rec) {
            Ok(()) => {
                self.beat_failures.store(0, Ordering::SeqCst);
            }
            Err(e) => {
                // Loud once, then every 20th: a broken disk must not flood the log.
                let n = self.beat_failures.fetch_add(1, Ordering::SeqCst) + 1;
                if n == 1 || n.is_multiple_of(20) {
                    log::warn("lock.heartbeat.failed")
                        .field("lock", self.path.to_string_lossy())
                        .num("consecutive", n as i64)
                        .num("stale_secs", self.cfg.stale_seconds() as i64)
                        .err(e)
                        .not_doing(
                            "refresh the lock",
                            "once the heartbeat is older than the stale window another copy may take the lock and double-post",
                        )
                        .emit();
                }
            }
        }
    }

    /// Give the lock up on a clean exit. A kill or a closed window skips this, and the
    /// next start recovers the file as stale.
    pub fn release(&self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        match fs::remove_file(&self.path) {
            Ok(()) => log::info("lock.released").field("lock", self.path.to_string_lossy()).emit(),
            Err(e) => log::warn("lock.release.failed")
                .field("lock", self.path.to_string_lossy())
                .err(e)
                .not_doing(
                    "delete the lock file",
                    "the next start will find it stale and recover it",
                )
                .emit(),
        };
    }
}

// ------------------------------------------------------------------ tests
// Offline by design: a temporary directory, an injected clock, an injected liveness
// check. No network, no `tasklist`, no real `data\` store. [DoD chunk 0, item 4]

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("spectrum-lock-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// The shipped defaults, which are the values this module used to hardcode.
    fn cfg() -> LockConfig {
        LockConfig::default()
    }

    /// The lock file's name now comes from the config, so the tests ask for it there.
    fn lock_file(dir: &Path) -> PathBuf {
        dir.join(cfg().file_name)
    }

    fn read_at(path: &Path) -> Option<Record> {
        let c = cfg();
        record::read(path, c.read_tries, c.read_retry_ms)
    }

    fn other(pid: u32, heartbeat: u64) -> Record {
        Record {
            pid,
            exe: "spectrum-headless.exe".into(),
            started: "2026-09-18T20:14:03.123Z".into(),
            heartbeat,
        }
    }

    #[test]
    fn a_clean_directory_gives_us_the_lock() {
        let dir = temp("clean");
        let lock = acquire_with(&dir, &cfg(), 1000, |_| Liveness::Alive).expect("first start holds");
        let rec = read_at(&lock.path).expect("the record is on disk");
        assert_eq!(rec.pid, std::process::id());
        lock.release();
        assert!(!lock.path.exists(), "release deletes the file");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_live_holder_refuses_the_second_start() {
        let dir = temp("busy");
        fs::create_dir_all(&dir).unwrap();
        let mine = std::process::id() + 1;
        record::write(&lock_file(&dir), &other(mine, 1000)).unwrap();

        let err = acquire_with(&dir, &cfg(), 1005, |_| Liveness::Alive).unwrap_err().to_string();
        assert!(err.contains(&mine.to_string()), "the message names the running pid: {err}");
        assert!(err.contains(&cfg().file_name), "the message names the lock file: {err}");
        // the holder's record is untouched
        assert_eq!(read_at(&lock_file(&dir)).unwrap().pid, mine);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_holder_is_recovered_however_fresh_the_heartbeat() {
        let dir = temp("dead");
        fs::create_dir_all(&dir).unwrap();
        record::write(&lock_file(&dir), &other(std::process::id() + 2, 1000)).unwrap();

        let lock =
            acquire_with(&dir, &cfg(), 1001, |_| Liveness::Dead).expect("a dead pid frees the lock");
        assert_eq!(read_at(&lock.path).unwrap().pid, std::process::id());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_liveness_answer_the_heartbeat_decides() {
        let dir = temp("unknown");
        fs::create_dir_all(&dir).unwrap();
        let ghost = std::process::id() + 3;
        record::write(&lock_file(&dir), &other(ghost, 1000)).unwrap();
        let stale = cfg().stale_seconds();

        // inside the window: assume it lives
        assert!(acquire_with(&dir, &cfg(), 1000 + stale, |_| Liveness::Unknown).is_err());
        // past it: the beat stopped, so the process did
        let lock = acquire_with(&dir, &cfg(), 1001 + stale, |_| Liveness::Unknown)
            .expect("a stale heartbeat frees the lock");
        assert_eq!(read_at(&lock.path).unwrap().pid, std::process::id());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_beat_after_release_never_resurrects_the_file() {
        let dir = temp("released");
        let lock = acquire_with(&dir, &cfg(), 1000, |_| Liveness::Dead).unwrap();
        let ghost = lock.clone();
        lock.release();
        ghost.beat();
        assert!(!lock.path.exists(), "a late beat must not recreate the lock");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The config block is not decoration: a changed name and a changed window have to
    /// reach the file on disk and the stale decision, or the `.json` is a lie.
    #[test]
    fn the_config_block_decides_the_file_name_and_the_stale_window() {
        let dir = temp("configured");
        let tuned = LockConfig {
            file_name: "runner.lock".into(),
            beat_seconds: 2,
            missed_beats: 3,
            ..LockConfig::default()
        };
        assert_eq!(tuned.stale_seconds(), 6, "the window is beat × missed beats");

        fs::create_dir_all(&dir).unwrap();
        record::write(&dir.join(&tuned.file_name), &other(std::process::id() + 4, 1000)).unwrap();

        // 6 s is still inside the tuned window, where the default 90 s window would be too
        assert!(acquire_with(&dir, &tuned, 1006, |_| Liveness::Unknown).is_err());
        let lock = acquire_with(&dir, &tuned, 1007, |_| Liveness::Unknown)
            .expect("one second past the tuned window frees the lock");
        assert_eq!(lock.path.file_name().unwrap(), "runner.lock");
        assert_eq!(lock.beat_seconds(), 2, "the runner's timer reads the configured beat");
        let _ = fs::remove_dir_all(&dir);
    }
}
