//! Classify — one analyzer read per unseen headline, then the queue.
//!
//! Three silent failures lived here. An analyzer error was swallowed, and the item was
//! left unseen, so with Ollama down every poll retried every item and said nothing. An
//! answer the analyzer's map could not read was degraded to a drop and marked seen, which
//! looks exactly like a quiet news day. And the queue has no bound, so a queue the drip
//! cannot drain grows for ever without a word. All three now write a record. The
//! behaviour is unchanged: an item that fails to classify still stays unseen, a degraded
//! item is still dropped, and the queue is still unbounded (a fix belongs to CR-1
//! chunk 2). [CR-1 chunk 0 · observability law]

use super::cycle::PollStats;
use super::keys::{norm, now_unix};
use super::Engine;
use crate::analyze::{self, Category, Read, Unmapped};
use crate::log;
use crate::rss;

impl Engine {
    /// Poll: classify only UNSEEN items, enqueue the cleared ones. Returns new count.
    pub async fn poll(&mut self) -> usize {
        let (items, gathered) = self.gather().await;
        let mut st = PollStats::new(gathered);
        let mut new = 0;
        for it in items {
            let key = norm(&it.title);
            if self.seen.contains(&key) {
                st.already_seen += 1;
                continue;
            }
            let read = analyze::analyze(&self.client, &it.title, &it.source).await;
            match read {
                Ok((read, unmapped)) => {
                    st.read_ok += 1;
                    self.seen.insert(key);
                    if unmapped.any() {
                        self.report_unmapped(&mut st, &it, &read, &unmapped);
                    }
                    if read.category == Category::Drop {
                        st.category_drop += 1;
                    } else if read.severity < self.cfg.min_severity {
                        st.below_floor += 1;
                    } else {
                        self.queue.push((it, read));
                        st.queued += 1;
                        new += 1;
                    }
                }
                Err(e) => {
                    let why = format!("{e:#}");
                    // The first item with this exact error is logged in full; every
                    // later one is counted and rolled up, so Ollama being down writes
                    // two lines and not two thousand.
                    if st.note_analyze_failure(&why) {
                        let line = log::warn("analyze.failed")
                            .field("title", &it.title)
                            .field("source", &it.source)
                            .field("error", &why)
                            .not_doing(
                                "classify this headline",
                                "the item stays unseen, so every later poll reads it again — is Ollama running?",
                            )
                            .emit();
                        self.note(line);
                    }
                }
            }
        }
        self.queue.sort_by(|a, b| b.1.severity.cmp(&a.1.severity)); // strongest first
        self.save_seen("poll");
        self.last_poll_unix = now_unix();
        for line in st.repeated() {
            self.note(line);
        }
        let (queue_len, seen_len) = (self.queue.len(), self.seen.len());
        st.emit(queue_len, seen_len);
        self.report_backlog(queue_len);
        self.note(format!("poll: {new} new · {queue_len} queued · {seen_len} seen"));
        new
    }

    /// An answer the analyzer's map could not read. The item is already marked seen, so
    /// it can never be offered again — and the only trace used to be one more tick on the
    /// aggregate `dropped_category` / `dropped_below_floor` count, which reads exactly
    /// like a quiet news day. Repeats roll up, so a drifted model writes two lines and not
    /// two thousand. [CR-1 chunk 0 · observability law]
    fn report_unmapped(
        &mut self,
        st: &mut PollStats,
        it: &rss::Item,
        read: &Read,
        unmapped: &Unmapped,
    ) {
        if !st.note_unmapped(unmapped) {
            return;
        }
        let line = log::warn("analyze.unmapped")
            .field("title", &it.title)
            .field("source", &it.source)
            .field("raw_category", unmapped.category.as_deref().unwrap_or("-"))
            .field("raw_severity", unmapped.severity.as_deref().unwrap_or("-"))
            .field("mapped_category", read.category.key())
            .num("mapped_severity", read.severity as i64)
            .not_doing(
                "read this headline as the model meant it",
                "the answer is not in the analyzer's map, so the item was degraded to drop or to severity 1, marked seen, and can never be offered again — check whether the model's output format has drifted",
            )
            .emit();
        self.note(line);
    }

    /// The queue has no bound and no age limit. When it holds more cards than the drip
    /// can post before the next poll, the tail waits for ever — and a restart loses it.
    /// The capacity comes from the live config, so this needs no tunable of its own.
    fn report_backlog(&mut self, queue_len: usize) {
        let window = self.cfg.poll_minutes.saturating_mul(60);
        let capacity =
            ((window / self.cfg.drip_seconds.max(1)) as usize).saturating_mul(self.cfg.max_per_drop);
        if queue_len > capacity {
            let line = log::warn("queue.backlog")
                .count("queued", queue_len)
                .count("drains_before_next_poll", capacity)
                .not_doing(
                    "post the tail of the queue",
                    "the poll adds cards faster than the drip posts them; the queue has no bound, and a restart loses every card in it",
                )
                .emit();
            self.note(line);
        }
    }
}
