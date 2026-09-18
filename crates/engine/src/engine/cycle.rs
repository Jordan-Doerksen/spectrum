//! The counters of one cycle, and the summary records they write.
//!
//! A cycle is one `poll()` plus the `drip()` ticks that follow it, so the cycle record
//! is two lines: `poll.summary` (what came in, and where every item went) and
//! `drip.summary` (what went out, and every card that was lost instead). Together they
//! answer "what did this run do, and what did it NOT do" without reading the code.
//!
//! `Drops` is the session total of cards that left the queue and never posted. A
//! dropped card is gone: its headline is already marked seen, so the engine will not
//! offer it again. Chunk 0 counts and reports that loss; it does not change it.
//! [CR-1 chunk 0 · observability law]

use std::collections::BTreeMap;

use crate::analyze::Unmapped;
use crate::log;

/// What one `gather()` pass saw. Every field after `items_raw` is a reason an entry did
/// not become an item.
#[derive(Default)]
pub struct GatherStats {
    pub feeds_total: usize,
    pub feeds_ok: usize,
    pub feeds_failed: usize,
    pub items_raw: usize,
    pub empty_titles: usize,
    pub batch_dupes: usize,
    pub items_kept: usize,
}

/// What one `poll()` did. `failures` groups analyzer errors by message, so Ollama being
/// down writes two lines instead of two thousand — and still loses no count.
#[derive(Default)]
pub struct PollStats {
    pub gather: GatherStats,
    pub already_seen: usize,
    pub read_ok: usize,
    pub read_failed: usize,
    pub category_drop: usize,
    pub below_floor: usize,
    pub queued: usize,
    /// answers whose `category` word the analyzer's map does not know
    pub unmapped_category: usize,
    /// answers whose `severity` value was not a number on the 1..=4 scale
    pub unmapped_severity: usize,
    failures: BTreeMap<String, usize>,
    unmapped: BTreeMap<String, usize>,
}

impl PollStats {
    pub fn new(gather: GatherStats) -> Self {
        Self { gather, ..Default::default() }
    }

    /// Record one analyzer failure. `true` means this exact error is new in this poll,
    /// so the caller logs that item in full. Every later item with the same error is
    /// counted here and reported by [`PollStats::repeated`] — counted, never swallowed.
    pub fn note_analyze_failure(&mut self, why: &str) -> bool {
        self.read_failed += 1;
        let n = self.failures.entry(why.to_string()).or_insert(0);
        *n += 1;
        *n == 1
    }

    /// Record one answer the analyzer's map could not read. `true` means this exact drift
    /// is new in this poll, so the caller logs that item in full; every later item with
    /// the same drift is counted here and reported by [`PollStats::repeated`]. A model
    /// whose output format has changed then writes two lines, not two thousand.
    pub fn note_unmapped(&mut self, u: &Unmapped) -> bool {
        if u.category.is_some() {
            self.unmapped_category += 1;
        }
        if u.severity.is_some() {
            self.unmapped_severity += 1;
        }
        let n = self.unmapped.entry(u.key()).or_insert(0);
        *n += 1;
        *n == 1
    }

    /// One record for each analyzer error, and each unreadable answer, that hit more than
    /// one item. Returns the console lines, so the caller can also put them in the panel's
    /// ring.
    pub fn repeated(&self) -> Vec<String> {
        let errors = self.failures.iter().filter(|(_, n)| **n > 1).map(|(why, n)| {
            log::warn("analyze.failed.repeated")
                .count("items", *n)
                .field("error", why)
                .not_doing(
                    "classify these headlines",
                    "they stay unseen, so every later poll reads them again",
                )
                .emit()
        });
        let drifted = self.unmapped.iter().filter(|(_, n)| **n > 1).map(|(what, n)| {
            log::warn("analyze.unmapped.repeated")
                .count("items", *n)
                .field("raw", what)
                .not_doing(
                    "read these headlines as the model meant them",
                    "the same unknown words came back again, so the model's output format has drifted; every one of these items is dropped and already marked seen",
                )
                .emit()
        });
        errors.chain(drifted).collect()
    }

    /// The run summary for one poll: every item the poll saw, and where it went.
    pub fn emit(&self, queue_len: usize, seen_len: usize) {
        let g = &self.gather;
        log::info("poll.summary")
            .count("feeds", g.feeds_total)
            .count("feeds_ok", g.feeds_ok)
            .count("feeds_failed", g.feeds_failed)
            .count("items_fetched", g.items_raw)
            .count("skipped_empty_title", g.empty_titles)
            .count("deduped_in_batch", g.batch_dupes)
            .count("items_gathered", g.items_kept)
            .count("deduped_already_seen", self.already_seen)
            .count("read_ok", self.read_ok)
            .count("read_failed", self.read_failed)
            .count("dropped_category", self.category_drop)
            .count("dropped_below_floor", self.below_floor)
            .count("unmapped_category", self.unmapped_category)
            .count("unmapped_severity", self.unmapped_severity)
            .count("queued", self.queued)
            .count("queue_len", queue_len)
            .count("seen_len", seen_len)
            .emit();
    }
}

/// One drip tick: what posted, and what was lost instead.
#[derive(Default)]
pub struct DripStats {
    pub posted: usize,
    pub dry: usize,
    pub no_webhook: usize,
    pub post_failed: usize,
}

impl DripStats {
    pub fn dropped(&self) -> usize {
        self.dry + self.no_webhook + self.post_failed
    }
    /// A tick that moved no card writes no summary — a quiet drip is the normal case.
    pub fn touched(&self) -> bool {
        self.posted + self.dropped() > 0
    }
}

/// The session total of cards that left the queue without posting, by reason.
#[derive(Default)]
pub struct Drops {
    pub dry: usize,
    pub no_webhook: usize,
    pub post_failed: usize,
}

impl Drops {
    pub fn add(&mut self, t: &DripStats) {
        self.dry += t.dry;
        self.no_webhook += t.no_webhook;
        self.post_failed += t.post_failed;
    }
    pub fn total(&self) -> usize {
        self.dry + self.no_webhook + self.post_failed
    }
}

/// The run summary for one drip tick, with the session totals beside it: one line
/// answers "how many cards has this run posted, and how many has it lost".
pub fn emit_drip_summary(t: &DripStats, session: &Drops, session_posted: usize, queue_len: usize) {
    log::info("drip.summary")
        .count("posted", t.posted)
        .count("dropped", t.dropped())
        .count("dropped_dry", t.dry)
        .count("dropped_no_webhook", t.no_webhook)
        .count("dropped_post_failed", t.post_failed)
        .count("queue_len", queue_len)
        .count("session_posted", session_posted)
        .count("session_dropped", session.total())
        .count("session_dropped_dry", session.dry)
        .count("session_dropped_no_webhook", session.no_webhook)
        .count("session_dropped_post_failed", session.post_failed)
        .emit();
}
