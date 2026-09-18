//! The `Engine` service — the pipeline as a controllable object, so the headless
//! runner AND the Tauri panel drive the SAME logic (the v4 discipline: behaviour in
//! the engine crate, thin shells on top). Owns config + seen-store + the drip queue
//! + session stats + a recent-activity log.
//!
//! Every failure path writes a record through `crate::log`: it names the action that
//! did NOT happen, and why. A silent failure is a defect (observability law, CR-1
//! chunk 0). The 200-line in-memory ring stays, because the panel reads it, so a
//! failure line goes into BOTH the ring and the disk log. The human lines the engine
//! returns are unchanged, with one exception: a card with no webhook now reads
//! "card DROPPED" instead of "held", because "held" was false — the card is gone.
//!
//! Split by domain (house law: review at 300 lines). This file is the service object:
//! the config lifecycle, the store, the ring, and the snapshot the panel reads.
//!   * `engine/gather.rs` — ingest: the feeds and the first-run seed.
//!   * `engine/poll.rs`   — classify: the analyzer read and the queue.
//!   * `engine/drip.rs`   — deliver: the Discord post, and every lost card.
//!   * `engine/cycle.rs`  — the counters and the two run-summary records.
//!   * `engine/keys.rs`   — the dedupe fingerprint and the clock.

mod cycle;
mod drip;
mod gather;
mod keys;
mod poll;

pub use keys::norm;

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use crate::analyze::{Category, Read};
use crate::config::{Config, LogConfig};
use crate::log;
use crate::store::Seen;
use crate::{feeds, rss};
use cycle::Drops;

/// The bands a card can carry. `Drop` never posts, so it needs no webhook.
const BANDS: [Category; 4] = [
    Category::Financial,
    Category::Political,
    Category::Technology,
    Category::Catastrophe,
];

pub struct Engine {
    pub cfg_path: String,
    pub cfg: Config,
    seen_file: String,
    seen: Seen,
    queue: Vec<(rss::Item, Read)>,
    client: reqwest::Client,
    posted: HashMap<String, usize>, // band key -> count this session
    drops: Drops,                   // cards that left the queue and never posted
    last_poll_unix: Option<u64>,
    log: VecDeque<String>, // recent activity, newest at the back
}

/// A serialized snapshot the panel reads each tick.
#[derive(serde::Serialize, Clone)]
pub struct Status {
    pub seen: usize,
    pub queued: usize,
    pub posted_financial: usize,
    pub posted_political: usize,
    pub posted_technology: usize,
    pub posted_catastrophe: usize,
    /// Cards lost this session, by reason. A lost card never comes back: it is out of
    /// the queue and its headline is already marked seen. [CR-1 chunk 0]
    pub dropped_total: usize,
    pub dropped_dry: usize,
    pub dropped_no_webhook: usize,
    pub dropped_post_failed: usize,
    pub last_poll_unix: Option<u64>,
    pub min_severity: u8,
    pub poll_minutes: u64,
    pub drip_seconds: u64,
    pub feeds: usize,
}

impl Engine {
    pub fn new(cfg_path: &str) -> anyhow::Result<Self> {
        let base = Path::new(cfg_path).parent().unwrap_or_else(|| Path::new("."));
        let cfg = match Config::load(cfg_path) {
            Ok(c) => {
                start_log(&c.logging, base);
                c
            }
            Err(e) => {
                start_log(&LogConfig::default(), base);
                log::error("config.load.failed")
                    .field("path", cfg_path)
                    .field("error", format!("{e:#}"))
                    .not_doing(
                        "start the engine",
                        "no config means no webhooks and no tuning; the caller reports the error and exits",
                    )
                    .emit();
                return Err(e);
            }
        };
        let seen_file = base.join(&cfg.seen_path).to_string_lossy().to_string();
        let seen = Seen::load(&seen_file);
        log::info("engine.started")
            .field("config", cfg_path)
            .field("seen_file", &seen_file)
            .count("seen", seen.len())
            .count("feeds", feeds::feeds().len())
            .num("min_severity", cfg.min_severity as i64)
            .num("poll_minutes", cfg.poll_minutes as i64)
            .num("drip_seconds", cfg.drip_seconds as i64)
            .count("max_per_drop", cfg.max_per_drop)
            .flag("seed_pending", seen.is_empty())
            .emit();
        let engine = Self {
            cfg_path: cfg_path.to_string(),
            cfg,
            seen_file,
            seen,
            queue: Vec::new(),
            client: crate::ua_client(),
            posted: HashMap::new(),
            drops: Drops::default(),
            last_poll_unix: None,
            log: VecDeque::new(),
        };
        engine.report_missing_webhooks();
        Ok(engine)
    }

    /// A band with no webhook loses every card it clears. Say so at the start, not at
    /// the moment a card dies.
    fn report_missing_webhooks(&self) {
        for band in BANDS {
            if self.cfg.webhook_for(band.key()).is_none() {
                log::warn("webhook.missing")
                    .field("band", band.key())
                    .not_doing(
                        "post any card in this band",
                        "config.local.json has no webhook for this key, so a cleared card leaves the queue and is lost",
                    )
                    .emit();
            }
        }
    }

    /// Re-read config.local.json (so the panel's edits go live without a restart).
    pub fn reload_config(&mut self) {
        match Config::load(&self.cfg_path) {
            Ok(c) => {
                self.cfg = c;
                log::debug("config.reloaded").field("path", &self.cfg_path).emit();
            }
            Err(e) => {
                let line = log::warn("config.reload.failed")
                    .field("path", &self.cfg_path)
                    .field("error", format!("{e:#}"))
                    .not_doing(
                        "apply the edited config",
                        "the config already in memory stays live, so an edit made now has no effect",
                    )
                    .emit();
                self.note(line);
            }
        }
    }

    pub fn seen_empty(&self) -> bool {
        self.seen.is_empty()
    }

    /// Put one human line in the ring the panel reads. Every line is redacted on the way
    /// in — see [`ring_line`].
    fn note(&mut self, line: String) {
        self.log.push_back(ring_line(line));
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }

    /// Save the dedupe store. A failed save is loud: this run's keys then live in
    /// memory only, so a restart re-reads the same headlines and can post them.
    fn save_seen(&mut self, phase: &str) {
        if let Err(e) = self.seen.save(&self.seen_file) {
            let line = log::error("seen.save.failed")
                .field("phase", phase)
                .field("path", &self.seen_file)
                .field("error", format!("{e:#}"))
                .not_doing(
                    "persist the dedupe store",
                    "this run's keys stay in memory only, so a restart reads the same headlines again and can post them",
                )
                .emit();
            self.note(line);
        }
    }

    pub fn status(&self) -> Status {
        Status {
            seen: self.seen.len(),
            queued: self.queue.len(),
            posted_financial: *self.posted.get("financial").unwrap_or(&0),
            posted_political: *self.posted.get("political").unwrap_or(&0),
            posted_technology: *self.posted.get("technology").unwrap_or(&0),
            posted_catastrophe: *self.posted.get("catastrophe").unwrap_or(&0),
            dropped_total: self.drops.total(),
            dropped_dry: self.drops.dry,
            dropped_no_webhook: self.drops.no_webhook,
            dropped_post_failed: self.drops.post_failed,
            last_poll_unix: self.last_poll_unix,
            min_severity: self.cfg.min_severity,
            poll_minutes: self.cfg.poll_minutes,
            drip_seconds: self.cfg.drip_seconds,
            feeds: feeds::feeds().len(),
        }
    }

    /// The most recent `n` activity lines, newest first.
    pub fn recent_log(&self, n: usize) -> Vec<String> {
        self.log.iter().rev().take(n).cloned().collect()
    }
}

/// The one gate every line into the ring passes: cut the secrets out of it.
///
/// The ring is not the disk log. `Engine::recent_log` hands it to the Tauri panel, which
/// renders it in the window (`ui\app.js:38-40`), so a line built with `format!` around an
/// error reaches a person unredacted unless it is cut here. reqwest's transport errors
/// carry the full request URL, and on the drip path that url is the Discord webhook with
/// its token. One cut here covers every call site, present and future. [CR-1 chunk 0]
fn ring_line(line: String) -> String {
    log::redact(&line)
}

/// Open this run's log file. Safe when the runner already opened it: `log::init` pins
/// one file per process. A logger that cannot open its file says so on the console and
/// the engine continues — a missing log must never stop the news.
fn start_log(cfg: &LogConfig, base: &Path) {
    if let Err(e) = log::init(cfg, base) {
        log::error("log.init.failed")
            .field("dir", &cfg.dir)
            .field("error", format!("{e:#}"))
            .not_doing(
                "write a log file for this run",
                "the console lines are the only record until that path is writable",
            )
            .emit();
    }
}

#[cfg(test)]
mod tests {
    use super::ring_line;

    const FAKE_ID: &str = "100000000000000000";
    const FAKE_TOKEN: &str = "FAKEtokenVALUE-Nf9x_ZZ-notReal";

    /// No string that reaches `note()` can carry a webhook token into the panel window.
    #[test]
    fn a_line_carrying_a_webhook_url_loses_its_token_before_it_enters_the_ring() {
        let line = ring_line(format!(
            "post FAILED CRISIS — error sending request for url (https://discord.com/api/webhooks/{FAKE_ID}/{FAKE_TOKEN})"
        ));

        assert!(!line.contains(FAKE_TOKEN), "the ring must not carry a token: {line}");
        assert!(line.contains(FAKE_ID), "the id names the channel and is not a secret");
    }

    #[test]
    fn an_ordinary_activity_line_reaches_the_ring_unchanged() {
        let line = "poll: 3 new · 12 queued · 2281 seen";
        assert_eq!(ring_line(line.to_string()), line);
    }
}
