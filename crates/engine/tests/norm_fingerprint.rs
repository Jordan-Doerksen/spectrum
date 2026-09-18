//! The title normaliser — `norm()` in `crates\engine\src\engine\keys.rs`.
//!
//! `norm()` is the dedupe fingerprint. Every headline becomes a key, the key decides
//! whether the story is new, and the key is what `data\seen.json` stores. It is the
//! single highest-leverage function in the engine, and CR-1 chunk 2 replaces it.
//! These tests record what it does **today**, gaps included, so chunk 2 changes a
//! measured behaviour instead of a remembered one.
//!
//! The tests below call the real function — no copy, no mirror. Every `gap_` test
//! asserts behaviour that is WRONG on purpose. A red `gap_` test after chunk 2 is the
//! signal that the fix landed, not a regression: re-record it, do not delete it.
//!
//! Pure in-memory. No network, no Ollama, no Discord, no file I/O.

use spectrum_engine::engine::norm;

// --------------------------------------------------------------- recorded behaviour

#[test]
fn a_google_news_publisher_suffix_is_cut_so_the_same_story_shares_one_key() {
    // This is the D-0013 fix working: Google News appends " - Publisher", the direct
    // feed does not, and both have to fingerprint to the same key or the story posts
    // twice.
    assert_eq!(
        norm("Fed holds rates steady - Reuters"),
        norm("Fed holds rates steady"),
        "a \" - Publisher\" suffix must not create a second key for one story"
    );
    assert_eq!(norm("Fed holds rates steady - Reuters"), "fed holds rates steady");
}

#[test]
fn case_and_punctuation_are_stripped_so_wording_variants_share_one_key() {
    assert_eq!(norm("S&P 500 Hits 6,000!"), "s p 500 hits 6 000");
    assert_eq!(norm("Fed's chair speaks"), "fed s chair speaks");
}

#[test]
fn surrounding_and_repeated_whitespace_collapses_to_single_spaces() {
    assert_eq!(norm("   Oil   slips  again   "), "oil slips again");
    assert_eq!(norm("Oil\tslips\nagain"), "oil slips again");
}

#[test]
fn only_the_last_ascii_dash_separator_is_treated_as_the_publisher_cut() {
    // "Oil slips - again - Bloomberg": the cut happens at the LAST " - ", so the
    // earlier dash survives into the key as a space.
    assert_eq!(norm("Oil slips - again - Bloomberg"), "oil slips again");
}

#[test]
fn unicode_letters_and_digits_survive_the_strip() {
    // `is_alphanumeric` is Unicode-aware, so a non-ASCII headline keeps its words
    // instead of collapsing to an empty key.
    assert_eq!(norm("Café über 5 Jahre"), "café über 5 jahre");
    assert_eq!(norm("日経平均 が 上昇"), "日経平均 が 上昇");
}

#[test]
fn a_title_that_is_only_punctuation_or_blank_normalises_to_an_empty_key() {
    // The engine filters an empty title before it fingerprints, so this is defence in
    // depth, not a live path. Recorded because an empty key would merge every such
    // title into one story if that filter ever moved.
    assert_eq!(norm(""), "");
    assert_eq!(norm("   "), "");
    assert_eq!(norm("!!! ---"), "");
}

#[test]
fn the_same_title_always_produces_the_same_key() {
    let title = "ECB signals a pause - Financial Times";
    assert_eq!(norm(title), norm(title));
}

// ------------------------------------------------------------------------- the gaps
//
// Each test below records behaviour that is WRONG and that CR-1 chunk 2 is expected
// to change. They assert today's output on purpose.

#[test]
fn gap_a_real_dash_inside_a_headline_eats_everything_after_it() {
    // "Ukraine - Russia talks stall in Geneva" has no publisher suffix. The cut still
    // fires on the last " - ", and the key becomes one word.
    //
    // CHUNK 2 MUST CHANGE THIS: the cut should only fire on a trailing segment that
    // actually looks like an outlet name.
    assert_eq!(
        norm("Ukraine - Russia talks stall in Geneva"),
        "ukraine",
        "recorded gap: the whole headline after the dash is discarded"
    );
}

#[test]
fn gap_two_different_stories_collapse_into_one_key_when_both_start_with_a_dashed_phrase() {
    // The consequence of the gap above, and the expensive one: the second story is
    // already "seen", so it is never classified and never posts.
    //
    // CHUNK 2 MUST CHANGE THIS.
    let stall = norm("Ukraine - Russia talks stall in Geneva");
    let resume = norm("Ukraine - Russia talks resume in Istanbul");
    assert_eq!(
        stall, resume,
        "recorded gap: two unrelated stories share the key {stall:?}, so the second \
         one is silently dropped as a duplicate"
    );
}

#[test]
fn gap_an_em_dash_publisher_suffix_is_not_cut() {
    // Several feeds separate the outlet with an em dash, not " - ". The cut misses
    // it, the outlet name stays in the key, and the same story posts twice.
    //
    // CHUNK 2 MUST CHANGE THIS.
    assert_eq!(norm("Fed holds rates steady — Reuters"), "fed holds rates steady reuters");
    assert_ne!(
        norm("Fed holds rates steady — Reuters"),
        norm("Fed holds rates steady"),
        "recorded gap: an em-dash suffix makes a second key for one story"
    );
}

#[test]
fn gap_an_en_dash_publisher_suffix_is_not_cut() {
    // CHUNK 2 MUST CHANGE THIS. Same failure as the em dash, different code point.
    assert_eq!(norm("Fed holds rates steady – Reuters"), "fed holds rates steady reuters");
    assert_ne!(
        norm("Fed holds rates steady – Reuters"),
        norm("Fed holds rates steady"),
        "recorded gap: an en-dash suffix makes a second key for one story"
    );
}

#[test]
fn gap_a_colon_publisher_prefix_is_not_cut_either() {
    // "Reuters: Fed holds rates steady" is a third outlet-attribution shape the cut
    // does not know about. Recorded so chunk 2 decides about it deliberately.
    assert_eq!(norm("Reuters: Fed holds rates steady"), "reuters fed holds rates steady");
    assert_ne!(norm("Reuters: Fed holds rates steady"), norm("Fed holds rates steady"));
}

#[test]
fn gap_the_key_is_built_from_the_title_and_nothing_else() {
    // No publish date, no source, no link goes into the key, and the store holds no
    // timestamps either (see `seen_store.rs`). A headline that recurs — "Jobs report
    // beats expectations" — is classified exactly once and then never again.
    //
    // CHUNK 2 MUST CHANGE THIS: item model v2 adds pubDate and source, and the store
    // gains a 48 h window.
    let january = norm("Jobs report beats expectations");
    let december = norm("Jobs report beats expectations");
    assert_eq!(
        january, december,
        "recorded gap: two runs of the same headline eleven months apart are one key"
    );

    // The same wording from two different outlets is also one key, which is the part
    // that is correct today and must stay correct after chunk 2.
    assert_eq!(
        norm("Jobs report beats expectations - Reuters"),
        norm("Jobs report beats expectations - Bloomberg"),
        "one story from two outlets must stay one key"
    );
}
