//! Secret-cutting in the logger — `redact`, `webhook_id` and `webhook_label` in
//! `crates\engine\src\log\redact.rs`, and the same cut applied through an `Event`.
//!
//! Spectrum is a PUBLIC repo (D-0012) and its only secrets are the Discord webhook
//! URLs. The log file is the one new place in CR-1 chunk 0 where a secret could land,
//! so this file's job is one claim: **a webhook token never reaches the output.**
//!
//! No test here calls `log::init`, so no log file is ever opened and nothing touches
//! `data\`. `emit()` still returns the console line, which is what gets inspected.
//! Every URL below is a fabricated shape, not a live webhook.

use spectrum_engine::log;

/// The shape of a Discord webhook URL. The id is public; the token is the secret.
const FAKE_ID: &str = "100000000000000000";
const FAKE_TOKEN: &str = "FAKEtokenVALUE-Nf9x_ZZ-notReal";

fn fake_webhook() -> String {
    format!("https://discord.com/api/webhooks/{FAKE_ID}/{FAKE_TOKEN}")
}

// ------------------------------------------------------------------ pure redaction

#[test]
fn a_discord_webhook_url_keeps_its_id_and_loses_its_token() {
    let out = log::redact(&fake_webhook());

    assert!(!out.contains(FAKE_TOKEN), "the token must not survive redaction: {out}");
    assert!(out.contains(FAKE_ID), "the id is not a secret and is kept so a log names the channel");
    assert_eq!(out, format!("https://discord.com/api/webhooks/{FAKE_ID}/REDACTED"));
}

#[test]
fn a_webhook_url_buried_in_a_longer_sentence_is_still_cut() {
    let line = format!("POST to {} failed after 3 tries", fake_webhook());
    let out = log::redact(&line);

    assert!(!out.contains(FAKE_TOKEN), "a token inside an error message must not survive: {out}");
    assert!(out.contains("failed after 3 tries"), "the rest of the message must survive");
}

#[test]
fn two_webhook_urls_on_one_line_are_both_cut() {
    let line = format!("{} and {}", fake_webhook(), fake_webhook());
    let out = log::redact(&line);
    assert!(!out.contains(FAKE_TOKEN), "both tokens must go, not just the first: {out}");
    assert_eq!(out.matches("REDACTED").count(), 2);
}

#[test]
fn a_bearer_authorization_value_loses_the_token_and_not_just_the_scheme_word() {
    // The bug that was found and fixed while the logger was written: the scanner used
    // to blank the word "Bearer" and leave the key that followed it in the clear.
    let out = log::redact("Authorization: Bearer sk-live-9988");

    assert!(!out.contains("sk-live-9988"), "the key after the scheme word must go: {out}");
    assert_eq!(out, "Authorization: Bearer REDACTED");
}

#[test]
fn token_secret_password_and_api_key_values_are_all_blanked() {
    for (line, leaked) in [
        ("token=abc123", "abc123"),
        ("secret: hunter2", "hunter2"),
        ("password=\"p@ssw0rd\"", "p@ssw0rd"),
        ("api_key=sk-9f3a", "sk-9f3a"),
        ("apikey: 9f3a-bbbb", "9f3a-bbbb"),
    ] {
        let out = log::redact(line);
        assert!(!out.contains(leaked), "`{line}` leaked `{leaked}` as `{out}`");
        assert!(out.contains("REDACTED"), "`{line}` must show that something was cut");
    }
}

#[test]
fn an_ordinary_headline_passes_through_unchanged() {
    let line = "Fed holds rates steady - Reuters";
    assert_eq!(log::redact(line), line, "redaction must not damage normal text");
}

// ------------------------------------------------------------------ webhook naming

#[test]
fn webhook_id_reads_the_id_out_of_a_url() {
    assert_eq!(log::webhook_id(&fake_webhook()), Some(FAKE_ID));
}

#[test]
fn webhook_id_is_none_for_a_url_that_is_not_a_webhook() {
    assert_eq!(log::webhook_id("https://discord.com/channels/1/2"), None);
    assert_eq!(log::webhook_id("not a url at all"), None);
    assert_eq!(log::webhook_id("https://discord.com/api/webhooks/"), None);
}

#[test]
fn webhook_label_names_the_channel_and_the_id_but_never_the_token() {
    let label = log::webhook_label("news", &fake_webhook());
    assert_eq!(label, format!("news:{FAKE_ID}"));
    assert!(!label.contains(FAKE_TOKEN));
}

#[test]
fn webhook_label_still_names_the_channel_when_the_url_has_no_id() {
    // A misconfigured webhook must still produce a log line that says which channel
    // was meant, or the operator cannot tell which one is broken.
    assert_eq!(log::webhook_label("news", "https://example.test/hook"), "news:unknown");
}

// ------------------------------------------------- redaction through a live event

#[test]
fn an_event_field_holding_a_webhook_url_never_carries_the_token() {
    let line = log::info("test.field.redaction").field("url", fake_webhook()).emit();

    assert!(!line.contains(FAKE_TOKEN), "a careless `.field` call must still be safe: {line}");
    assert!(line.contains("REDACTED"));
}

#[test]
fn the_webhook_helper_puts_only_the_channel_and_the_id_on_the_line() {
    let line = log::warn("test.webhook.helper").webhook("news", &fake_webhook()).emit();

    assert!(!line.contains(FAKE_TOKEN), "the intended call shape must be safe: {line}");
    assert!(line.contains(&format!("news:{FAKE_ID}")), "the line must still name the channel");
}

#[test]
fn an_error_message_carrying_a_webhook_url_is_cut_before_it_is_logged() {
    // The realistic leak: reqwest puts the full request URL into its error text, and
    // the call site logs the error verbatim.
    let pretend_error = format!("error sending request for url ({})", fake_webhook());
    let line = log::error("test.post.failed").err(pretend_error).emit();

    assert!(!line.contains(FAKE_TOKEN), "an error's own text must be cut too: {line}");
    assert!(line.contains("error sending request"), "the useful part of the error must survive");
}

#[test]
fn an_event_name_is_redacted_as_well_as_its_fields() {
    let line = log::info(&format!("post.to.{}", fake_webhook())).emit();
    assert!(!line.contains(FAKE_TOKEN), "the event name is not a safe place either: {line}");
}

#[test]
fn an_error_and_a_not_doing_reason_both_survive_on_one_line() {
    // The bug this test exists for: `err()` wrote the error into `why`, and `not_doing`
    // writes `why` as well, so `.err(e).not_doing(a, w)` dropped the error text. Seven
    // live call sites chain them, and every one of those records said what did not
    // happen but never why it failed — the observability law's whole point.
    let line = log::error("test.config.load.failed")
        .field("path", "config.local.json")
        .err("No such file or directory (os error 2)")
        .not_doing("start the engine", "the runner has no webhooks without this file")
        .emit();

    assert!(line.contains("os error 2"), "the error text must survive `not_doing`: {line}");
    assert!(line.contains("start the engine"), "the skipped action must survive `err`: {line}");
    assert!(
        line.contains("the runner has no webhooks without this file"),
        "the reason must survive too: {line}"
    );
    assert_eq!(line.matches("error=").count(), 1, "one key, one meaning: {line}");
    assert_eq!(line.matches("why=").count(), 1, "one key, one meaning: {line}");
}

#[test]
fn a_not_doing_record_states_the_action_and_the_reason() {
    // The observability law in one line: the record has to say what did NOT happen.
    let line = log::warn("test.feed.failed")
        .field("feed", "Reuters Business")
        .not_doing("items from this feed", "the poll continues without them")
        .emit();

    assert!(line.contains("test.feed.failed"));
    assert!(line.contains("items from this feed"), "the skipped action must be named: {line}");
    assert!(line.contains("the poll continues without them"), "the reason must be named: {line}");
}

// ------------------------------------------------------------------------- the cost

#[test]
fn over_redaction_is_the_accepted_trade_and_a_normal_word_can_lose_its_neighbour() {
    // Recorded, not celebrated. A headline about crypto loses the word after "token",
    // and a word that merely starts with "token" loses its tail. The trade was chosen
    // deliberately: a mangled headline in a log costs nothing, a leaked webhook costs
    // a channel.
    assert_eq!(log::redact("Ethereum token holders vote"), "Ethereum token REDACTED vote");
    assert_eq!(log::redact("tokenized assets"), "tokenREDACTED assets");

    // The value is what the redaction is worth: no headline text can smuggle a secret
    // past it, because the scan is on the text and not on the call site.
    assert!(!log::redact(&format!("headline mentioning {}", fake_webhook())).contains(FAKE_TOKEN));
}
