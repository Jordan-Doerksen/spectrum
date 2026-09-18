//! Deliver — post the strongest card, or say which card was lost instead.
//!
//! A card leaves the queue BEFORE it is posted, and its headline is already marked
//! seen, so a card that does not post is gone for good. Three paths lose one: a dry
//! run, a failed POST, and a band with no webhook. Each now writes a `card.dropped`
//! record with the reason, and each is counted into the session total the panel reads.
//! The behaviour is unchanged — no requeue and no retry (CR-1 chunk 2 owns that fix).
//! [CR-1 chunk 0 · observability law]

use super::cycle::{self, DripStats};
use super::Engine;
use crate::{discord, log, skins};

impl Engine {
    /// Drip: post up to `max_per_drop` strongest cards. Returns the lines it logged.
    pub async fn drip(&mut self, dry: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut st = DripStats::default();
        let mut posted = 0;
        while posted < self.cfg.max_per_drop && !self.queue.is_empty() {
            let (it, read) = self.queue.remove(0);
            let s = skins::skin(read.category);
            let band = read.category.key();
            let webhook = self.cfg.webhook_for(band).cloned();

            let line = if dry {
                st.dry += 1;
                log::warn("card.dropped")
                    .field("reason", "dry-run")
                    .field("band", band)
                    .field("title", &it.title)
                    .field("source", &it.source)
                    .num("severity", read.severity as i64)
                    .not_doing(
                        "post this card",
                        "--dry is set; the card left the queue and its headline is already seen, so it never posts",
                    )
                    .emit();
                format!("[dry] {} {} — {}", s.emoji, s.label, it.title)
            } else if let Some(url) = webhook {
                let payload = discord::embed(read.category, &read, &it.title, &it.link, &it.source);
                let sent = discord::post(&self.client, &url, &payload).await;
                match sent {
                    Ok(()) => {
                        *self.posted.entry(band.to_string()).or_insert(0) += 1;
                        st.posted += 1;
                        log::info("card.posted")
                            .field("band", band)
                            .webhook(band, &url)
                            .field("title", &it.title)
                            .field("source", &it.source)
                            .num("severity", read.severity as i64)
                            .field("confidence", &read.confidence)
                            .emit();
                        format!("posted {} {} — {}", s.emoji, s.label, it.title)
                    }
                    Err(e) => {
                        st.post_failed += 1;
                        let why = format!("{e:#}");
                        log::error("card.dropped")
                            .field("reason", "post-failed")
                            .field("band", band)
                            .webhook(band, &url)
                            .field("title", &it.title)
                            .field("error", &why)
                            .not_doing(
                                "post this card",
                                "the POST failed; there is no retry and no requeue, and the headline is already seen, so the card is lost",
                            )
                            .emit();
                        post_failed_line(s.label, &why)
                    }
                }
            } else {
                st.no_webhook += 1;
                log::error("card.dropped")
                    .field("reason", "no-webhook")
                    .field("band", band)
                    .field("title", &it.title)
                    .not_doing(
                        "post this card",
                        "config.local.json has no webhook for this band; the card is out of the queue and is lost",
                    )
                    .emit();
                format!("no webhook for {} — card DROPPED", s.label)
            };
            out.push(line.clone());
            self.note(line);
            posted += 1;
        }
        if st.touched() {
            self.drops.add(&st);
            cycle::emit_drip_summary(&st, &self.drops, self.posted_total(), self.queue.len());
        }
        out
    }

    /// Cards posted this session, every band counted.
    fn posted_total(&self) -> usize {
        self.posted.values().sum()
    }
}

/// The ring line for a failed POST, with the webhook cut out of it.
///
/// `reqwest` appends " for url (<the url>)" to a transport error, and on this path that
/// url IS the Discord webhook, token and all. The disk log redacts every value it writes,
/// but this line goes somewhere else as well: `Engine::note` -> the 200-line ring ->
/// `Engine::recent_log` -> the panel window, which is the build that ships. So it is cut
/// here, where the leak was. `note` redacts again as a backstop, and `redact` is
/// idempotent. [CR-1 chunk 0]
fn post_failed_line(label: &str, why: &str) -> String {
    log::redact(&format!("post FAILED {label} — {why}"))
}

#[cfg(test)]
mod tests {
    use super::post_failed_line;

    const FAKE_ID: &str = "100000000000000000";
    const FAKE_TOKEN: &str = "FAKEtokenVALUE-Nf9x_ZZ-notReal";

    /// The exact shape reqwest produces when a POST to a webhook cannot be sent.
    #[test]
    fn a_failed_post_never_puts_a_webhook_token_in_the_panel_ring() {
        let why = format!(
            "error sending request for url (https://discord.com/api/webhooks/{FAKE_ID}/{FAKE_TOKEN})"
        );
        let line = post_failed_line("CRISIS", &why);

        assert!(!line.contains(FAKE_TOKEN), "the token reached the panel's ring: {line}");
        assert!(line.contains(FAKE_ID), "the id is not a secret; it names the broken channel");
        assert!(line.contains("post FAILED CRISIS"), "the line must still say what failed: {line}");
    }

    #[test]
    fn an_ordinary_failure_line_is_unchanged() {
        let line = post_failed_line("MONEY", "connection timed out");
        assert_eq!(line, "post FAILED MONEY — connection timed out");
    }
}
