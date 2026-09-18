//! Ingest — read every feed, and name the feed that did not answer.
//!
//! `gather` is the only place a feed error can be lost, so it is the only place that
//! decides what to do with one: count it, log it, and continue with the other feeds.
//! `seed` is the first-run pass that marks the backlog seen and posts nothing.
//! [CR-1 chunk 0 · observability law]

use std::collections::HashSet;

use super::cycle::GatherStats;
use super::keys::norm;
use super::Engine;
use crate::{feeds, log, rss};

impl Engine {
    /// First-run seed: mark every current headline seen, post nothing. Returns count.
    pub async fn seed(&mut self) -> usize {
        let (items, g) = self.gather().await;
        for it in &items {
            self.seen.insert(norm(&it.title));
        }
        self.save_seen("seed");
        let n = items.len();
        log::info("seed.done")
            .count("headlines", n)
            .count("feeds_ok", g.feeds_ok)
            .count("feeds_failed", g.feeds_failed)
            .not_doing(
                "post any card",
                "the first run marks the backlog seen on purpose, so it never dumps history",
            )
            .emit();
        self.note(format!("seeded {n} headlines (posted nothing)"));
        n
    }

    /// Fetch every feed, deduped by title within the batch. A feed that fails is
    /// skipped, and now says so.
    pub(super) async fn gather(&mut self) -> (Vec<rss::Item>, GatherStats) {
        let mut items = Vec::new();
        let mut batch = HashSet::new();
        let mut st = GatherStats::default();
        for feed in feeds::feeds() {
            st.feeds_total += 1;
            let fetched = rss::fetch(&self.client, &feed).await;
            match fetched {
                Ok(list) => {
                    st.feeds_ok += 1;
                    st.items_raw += list.len();
                    for it in list {
                        if it.title.trim().is_empty() {
                            st.empty_titles += 1;
                            continue;
                        }
                        if !batch.insert(norm(&it.title)) {
                            st.batch_dupes += 1;
                            continue;
                        }
                        items.push(it);
                    }
                }
                Err(e) => {
                    st.feeds_failed += 1;
                    let line = log::warn("feed.fetch.failed")
                        .field("feed", &feed.source)
                        .field("url", &feed.url)
                        .field("error", format!("{e:#}"))
                        .not_doing(
                            "read any item from this feed",
                            "the cycle continues with the other feeds, so this source is missing from this poll",
                        )
                        .emit();
                    self.note(line);
                }
            }
        }
        st.items_kept = items.len();
        if st.feeds_total > 0 && st.feeds_ok == 0 {
            let line = log::error("feeds.all.failed")
                .count("feeds", st.feeds_total)
                .not_doing(
                    "gather any headline",
                    "every feed fetch failed, so nothing reaches the analyzer — check the network",
                )
                .emit();
            self.note(line);
        }
        (items, st)
    }
}
