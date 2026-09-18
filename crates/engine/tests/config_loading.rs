//! Config loading — `Config` and `LogConfig` in `crates\engine\src\config.rs`.
//!
//! Every tuning knob the engine has arrives through this file, and CR-1 adds more of
//! them. These tests read fixtures written into a scratch directory. They never open
//! the repo's real `config.local.json`, which holds live Discord webhook URLs, and
//! every webhook value below is an obviously fake placeholder.

mod common;

use common::TempDir;
use spectrum_engine::config::{Config, LockConfig, LogConfig};

/// Not a real webhook. The id is not a secret and the token is the word FAKE.
const FAKE_WEBHOOK: &str = "https://discord.com/api/webhooks/100000000000000000/FAKE-TEST-ONLY";

fn load(body: &str, label: &str) -> (TempDir, anyhow::Result<Config>) {
    let dir = TempDir::new(label);
    let path = dir.write("config.json", body);
    let cfg = Config::load(&path);
    (dir, cfg)
}

// -------------------------------------------------------------------- defaults

#[test]
fn a_minimal_config_loads_and_every_tuning_key_falls_back_to_its_default() {
    let (_dir, cfg) = load(r#"{"webhooks":{}}"#, "cfg-min");
    let cfg = cfg.expect("a config with only a webhooks block must load");

    assert_eq!(cfg.model, "llama3.1:8b", "default model");
    assert_eq!(cfg.min_severity, 2, "default severity floor");
    assert_eq!(cfg.poll_minutes, 10, "default minutes between polls");
    assert_eq!(cfg.drip_seconds, 90, "default seconds between drip posts");
    assert_eq!(cfg.max_per_drop, 1, "default cards per drip tick");
    assert_eq!(cfg.seen_path, "data/seen.json", "default dedupe store path");
}

#[test]
fn explicit_values_override_every_default() {
    let body = r#"{
      "webhooks": {},
      "model": "llama3.2:3b",
      "min_severity": 4,
      "poll_minutes": 3,
      "drip_seconds": 15,
      "max_per_drop": 6,
      "seen_path": "state/keys.json"
    }"#;
    let (_dir, cfg) = load(body, "cfg-explicit");
    let cfg = cfg.expect("a fully specified config must load");

    assert_eq!(cfg.model, "llama3.2:3b");
    assert_eq!(cfg.min_severity, 4);
    assert_eq!(cfg.poll_minutes, 3);
    assert_eq!(cfg.drip_seconds, 15);
    assert_eq!(cfg.max_per_drop, 6);
    assert_eq!(cfg.seen_path, "state/keys.json");
}

#[test]
fn webhook_for_finds_a_configured_band_and_returns_none_for_one_that_is_missing() {
    let body = format!(r#"{{"webhooks":{{"financial":"{FAKE_WEBHOOK}"}}}}"#);
    let (_dir, cfg) = load(&body, "cfg-webhook");
    let cfg = cfg.expect("load the config");

    assert_eq!(cfg.webhook_for("financial").map(String::as_str), Some(FAKE_WEBHOOK));
    assert!(
        cfg.webhook_for("catastrophe").is_none(),
        "an unconfigured band must report None so the caller can log the held card"
    );
}

// ------------------------------------------------------------------ the logging block

#[test]
fn an_absent_logging_block_still_yields_the_full_logger_defaults() {
    // The promise that lets an existing config.local.json keep working and still get
    // a log file, without the operator editing anything (`config.rs:29-32`).
    let (_dir, cfg) = load(r#"{"webhooks":{}}"#, "cfg-nolog");
    let log = cfg.expect("load the config").logging;

    assert!(log.enabled, "logging must be on by default");
    assert_eq!(log.dir, "data/logs");
    assert_eq!(log.file_level, "info");
    assert_eq!(log.console_level, "info");
    assert_eq!(log.max_file_bytes, 5_000_000);
    assert_eq!(log.max_files_per_run, 3);
    assert_eq!(log.keep_run_files, 20);
    assert!(
        log.file_pattern.contains("{date}"),
        "the default run-file name must vary per run, or two runs share one file"
    );
}

#[test]
fn a_partial_logging_block_keeps_the_defaults_for_the_keys_it_leaves_out() {
    let body = r#"{"webhooks":{},"logging":{"dir":"logs","max_file_bytes":1024}}"#;
    let (_dir, cfg) = load(body, "cfg-partiallog");
    let log = cfg.expect("load the config").logging;

    assert_eq!(log.dir, "logs", "the stated key wins");
    assert_eq!(log.max_file_bytes, 1024, "the stated key wins");
    assert!(log.enabled, "an omitted key keeps its default");
    assert_eq!(log.keep_run_files, 20, "an omitted key keeps its default");
}

#[test]
fn logging_can_be_turned_off_in_config() {
    let body = r#"{"webhooks":{},"logging":{"enabled":false}}"#;
    let (_dir, cfg) = load(body, "cfg-logoff");
    assert!(!cfg.expect("load the config").logging.enabled);
}

#[test]
fn the_log_config_default_impl_matches_what_an_absent_block_produces() {
    // Two paths reach the same values: serde's `#[serde(default)]` and the manual
    // `Default` impl the engine uses when the config file itself fails to load.
    let (_dir, cfg) = load(r#"{"webhooks":{}}"#, "cfg-defaultparity");
    let from_file = cfg.expect("load the config").logging;
    let from_code = LogConfig::default();

    assert_eq!(from_file.enabled, from_code.enabled);
    assert_eq!(from_file.dir, from_code.dir);
    assert_eq!(from_file.file_pattern, from_code.file_pattern);
    assert_eq!(from_file.file_level, from_code.file_level);
    assert_eq!(from_file.console_level, from_code.console_level);
    assert_eq!(from_file.max_file_bytes, from_code.max_file_bytes);
    assert_eq!(from_file.max_files_per_run, from_code.max_files_per_run);
    assert_eq!(from_file.keep_run_files, from_code.keep_run_files);
}

// ------------------------------------------------------------------- the lock block

#[test]
fn an_absent_lock_block_yields_the_values_the_runner_used_to_hardcode() {
    // The promise that lets an existing config.local.json keep working: the defaults are
    // exactly the constants that lived in `crates\headless\src\lock.rs` before CR-1
    // chunk 0, so a config with no `lock` block behaves as it always did.
    let (_dir, cfg) = load(r#"{"webhooks":{}}"#, "cfg-nolock");
    let lock = cfg.expect("load the config").lock;

    assert_eq!(lock.file_name, "spectrum-headless.lock");
    assert_eq!(lock.beat_seconds, 15);
    assert_eq!(lock.missed_beats, 6);
    assert_eq!(lock.read_tries, 3);
    assert_eq!(lock.read_retry_ms, 250);
    assert_eq!(lock.stale_seconds(), 90, "the window the runner had as a constant");
}

#[test]
fn the_stale_window_follows_the_configured_beat_and_missed_beats() {
    // The operationally load-bearing one: this window decides when a second copy may take
    // the lock and start double-posting.
    let body = r#"{"webhooks":{},"lock":{"beat_seconds":5,"missed_beats":4}}"#;
    let (_dir, cfg) = load(body, "cfg-lockwindow");
    let lock = cfg.expect("load the config").lock;

    assert_eq!(lock.stale_seconds(), 20);
    assert_eq!(lock.file_name, "spectrum-headless.lock", "an omitted key keeps its default");
}

#[test]
fn a_beat_of_zero_is_read_as_one_second_so_the_heartbeat_can_never_stop() {
    let body = r#"{"webhooks":{},"lock":{"beat_seconds":0,"missed_beats":6}}"#;
    let (_dir, cfg) = load(body, "cfg-lockzero");
    let lock = cfg.expect("load the config").lock;

    assert_eq!(lock.beat_seconds, 0, "the file says what it says");
    assert_eq!(lock.stale_seconds(), 6, "but the window is computed from at least one second");
}

#[test]
fn the_lock_config_default_impl_matches_what_an_absent_block_produces() {
    let (_dir, cfg) = load(r#"{"webhooks":{}}"#, "cfg-lockparity");
    let from_file = cfg.expect("load the config").lock;
    let from_code = LockConfig::default();

    assert_eq!(from_file.file_name, from_code.file_name);
    assert_eq!(from_file.beat_seconds, from_code.beat_seconds);
    assert_eq!(from_file.missed_beats, from_code.missed_beats);
    assert_eq!(from_file.read_tries, from_code.read_tries);
    assert_eq!(from_file.read_retry_ms, from_code.read_retry_ms);
}

// --------------------------------------------------------------------- failures

#[test]
fn a_config_with_no_webhooks_block_fails_to_load() {
    // `webhooks` has no default, so an engine can never start believing it has zero
    // channels by accident.
    let (_dir, cfg) = load(r#"{"model":"llama3.1:8b"}"#, "cfg-nowebhooks");
    assert!(cfg.is_err(), "a missing webhooks block must be a load error");
}

#[test]
fn a_value_of_the_wrong_type_fails_to_load_instead_of_falling_back_to_the_default() {
    let (_dir, cfg) = load(r#"{"webhooks":{},"min_severity":"high"}"#, "cfg-wrongtype");
    assert!(cfg.is_err(), "a string where a number belongs must be an error, not a default");
}

#[test]
fn a_number_outside_the_field_range_fails_to_load() {
    // `min_severity` is a u8 and severity only runs 1..=4, so 900 is a typo the load
    // catches. Nothing checks 1..=4 itself — see the gap test below.
    let (_dir, cfg) = load(r#"{"webhooks":{},"min_severity":900}"#, "cfg-range");
    assert!(cfg.is_err(), "a value past the field's type range must be an error");
}

#[test]
fn malformed_json_fails_to_load() {
    let (_dir, cfg) = load(r#"{"webhooks":{},"#, "cfg-malformed");
    assert!(cfg.is_err(), "truncated JSON must be an error");
}

#[test]
fn a_config_file_that_does_not_exist_fails_to_load() {
    let dir = TempDir::new("cfg-missing");
    let cfg = Config::load(&dir.path_str("nowhere.json"));
    assert!(cfg.is_err(), "a missing config file must be an error the caller can log");
}

// ------------------------------------------------------------------------- gaps

#[test]
fn gap_an_unknown_key_is_ignored_so_a_typo_silently_keeps_the_default() {
    // There is no `#[serde(deny_unknown_fields)]`. An operator who types
    // "min_severty" gets the default floor of 2 and no warning anywhere.
    let (_dir, cfg) = load(r#"{"webhooks":{},"min_severty":4}"#, "cfg-typo");
    let cfg = cfg.expect("recorded behaviour: the typo loads cleanly");

    assert_eq!(
        cfg.min_severity, 2,
        "recorded gap: the misspelled key is discarded and the default is used silently"
    );
}

#[test]
fn gap_a_severity_floor_outside_the_one_to_four_range_loads_without_complaint() {
    // `min_severity: 9` is inside u8 but outside the severity scale, so nothing ever
    // clears the floor and the engine posts nothing while looking healthy.
    let (_dir, cfg) = load(r#"{"webhooks":{},"min_severity":9}"#, "cfg-floor");
    let cfg = cfg.expect("recorded behaviour: an impossible floor loads cleanly");

    assert_eq!(
        cfg.min_severity, 9,
        "recorded gap: no range check on the severity floor, so a typo silences the engine"
    );
}

#[test]
fn gap_an_unknown_log_level_word_loads_and_is_only_caught_later_at_log_init() {
    // The level is a String, so "loud" passes deserialization. `log::init` is where
    // it is caught, and it logs `log.level.unknown` and falls back to info.
    let (_dir, cfg) = load(r#"{"webhooks":{},"logging":{"file_level":"loud"}}"#, "cfg-level");
    let log = cfg.expect("recorded behaviour: a nonsense level loads cleanly").logging;

    assert_eq!(
        log.file_level, "loud",
        "recorded gap: the level word is validated at init, not at load"
    );
}

#[test]
fn gap_the_model_key_loads_but_nothing_in_the_engine_reads_it() {
    // Recorded because DECISIONS.md already carries this as a correction of record:
    // `analyze.rs:56` hardcodes the model, so this value is inert. CR-1 S7 fixes it.
    let (_dir, cfg) = load(r#"{"webhooks":{},"model":"mistral:7b"}"#, "cfg-model");
    assert_eq!(
        cfg.expect("load the config").model,
        "mistral:7b",
        "recorded gap: the key parses, but the analyzer ignores it and calls llama3.1:8b"
    );
}
