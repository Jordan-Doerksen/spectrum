//! The dedupe store — `Seen` in `crates\engine\src\store.rs`.
//!
//! `Seen` decides whether a headline is new. If it forgets a key the story posts
//! twice; if it keeps a key wrongly the story never posts at all. It is also the file
//! CR-1 chunk 2 rewrites, so today's eviction rule is recorded here before it moves.
//!
//! Every test writes inside a scratch directory under the OS temp dir. None of them
//! reads or writes the repo's real `data\seen.json`.

mod common;

use common::TempDir;
use spectrum_engine::store::Seen;

/// `store.rs:8` — `const CAP: usize = 4000`. The constant is private, so the tests
/// pin it behaviourally. If the cap moves, the two cap tests below fail and say so.
const EXPECTED_CAP: usize = 4000;

fn filled_to_cap() -> Seen {
    let mut seen = Seen::default();
    for i in 0..EXPECTED_CAP {
        seen.insert(format!("key-{i:05}"));
    }
    seen
}

// ------------------------------------------------------------------- in memory

#[test]
fn an_inserted_key_is_contained_and_counted() {
    let mut seen = Seen::default();
    assert!(seen.is_empty(), "a fresh store must start empty");
    seen.insert("fed holds rates steady".to_string());
    assert!(seen.contains("fed holds rates steady"));
    assert_eq!(seen.len(), 1);
    assert!(!seen.is_empty());
}

#[test]
fn a_key_that_was_never_inserted_is_not_contained() {
    let mut seen = Seen::default();
    seen.insert("one story".to_string());
    assert!(!seen.contains("another story"));
}

#[test]
fn inserting_the_same_key_twice_does_not_grow_the_store() {
    let mut seen = Seen::default();
    seen.insert("same story".to_string());
    seen.insert("same story".to_string());
    assert_eq!(seen.len(), 1, "a duplicate insert must not add a second entry");
}

#[test]
fn the_oldest_key_is_evicted_when_the_store_passes_its_cap_of_4000() {
    let mut seen = filled_to_cap();
    assert_eq!(seen.len(), EXPECTED_CAP, "the store must hold exactly the cap before eviction");
    assert!(seen.contains("key-00000"), "nothing is evicted while the store is only at the cap");

    seen.insert("one over the cap".to_string());

    assert_eq!(seen.len(), EXPECTED_CAP, "the store must never grow past its cap");
    assert!(!seen.contains("key-00000"), "the FIRST key inserted is the one evicted");
    assert!(seen.contains("key-00001"), "the second-oldest key must survive one eviction");
    assert!(seen.contains("one over the cap"), "the newest key must be kept");
}

#[test]
fn seeing_a_key_again_does_not_refresh_it_so_it_still_falls_off_first() {
    // The eviction order is insertion order, not last-seen order. A headline that
    // keeps reappearing in the feeds does NOT hold its place in the store: it ages
    // out on schedule and can then be treated as new and posted a second time.
    let mut seen = filled_to_cap();

    seen.insert("key-00000".to_string()); // seen again, right now
    assert_eq!(seen.len(), EXPECTED_CAP, "re-inserting a known key must change nothing");

    seen.insert("a brand new story".to_string());

    assert!(
        !seen.contains("key-00000"),
        "recorded behaviour: re-seeing a key does not move it to the back of the queue, \
         so it is still the first one evicted"
    );
    assert!(seen.contains("key-00001"));
}

// ------------------------------------------------------------------- on disk

#[test]
fn save_then_load_round_trips_every_key() {
    let dir = TempDir::new("store-roundtrip");
    let path = dir.path_str("seen.json");

    let mut seen = Seen::default();
    seen.insert("first story".to_string());
    seen.insert("second story".to_string());
    seen.save(&path).expect("saving to a writable path must succeed");

    let loaded = Seen::load(&path);
    assert_eq!(loaded.len(), 2, "both keys must come back");
    assert!(loaded.contains("first story"));
    assert!(loaded.contains("second story"));
}

#[test]
fn save_creates_the_parent_directory_that_does_not_exist_yet() {
    let dir = TempDir::new("store-mkdir");
    let path = dir.path_str("nested/deeper/seen.json");

    let mut seen = Seen::default();
    seen.insert("a story".to_string());
    seen.save(&path).expect("save must create data/ on a first run");

    assert!(std::path::Path::new(&path).is_file(), "the store file must exist after save");
}

#[test]
fn a_save_error_is_returned_to_the_caller_and_not_swallowed() {
    // The path is an existing DIRECTORY, so the write cannot succeed. `save` must
    // hand the error back. (Whether the caller then logs it is the engine's job:
    // `engine.rs:86,127` currently discards it with `let _ =`, which is the silent
    // failure CR-1 chunk 0 wires to the logger.)
    let dir = TempDir::new("store-saveerr");
    let blocked = dir.join("occupied");
    std::fs::create_dir_all(&blocked).expect("create the blocking directory");

    let mut seen = Seen::default();
    seen.insert("a story".to_string());
    let result = seen.save(&blocked.to_string_lossy());

    assert!(
        result.is_err(),
        "save must return Err when the file cannot be written, so the caller can log it"
    );
}

#[test]
fn loading_a_file_that_does_not_exist_gives_an_empty_store() {
    let dir = TempDir::new("store-missing");
    let seen = Seen::load(&dir.path_str("not-here.json"));
    assert!(seen.is_empty(), "a first run has no store file and must start empty");
    assert_eq!(seen.len(), 0);
}

#[test]
fn the_set_field_in_the_file_is_ignored_and_rebuilt_from_the_order_list() {
    // `set` carries `#[serde(skip)]` (`store.rs:13-14`), so only `order` is durable.
    // A hand-edited file that lists keys under `set` alone loses them.
    let dir = TempDir::new("store-setfield");
    let path = dir.write(
        "seen.json",
        r#"{"order":["real story"],"set":["ghost story"]}"#,
    );

    let seen = Seen::load(&path);
    assert!(seen.contains("real story"), "keys in `order` are the ones that load");
    assert!(!seen.contains("ghost story"), "keys in `set` are not read back");
    assert_eq!(seen.len(), 1);
}

#[test]
fn gap_a_corrupt_store_file_loads_as_empty_with_no_error_which_silently_triggers_a_reseed() {
    // `store.rs:21` uses `unwrap_or_default()`. A truncated or corrupt `seen.json` is
    // therefore indistinguishable from a first run: the engine re-seeds every current
    // headline and posts nothing, and the only clue is a count in the activity line.
    //
    // CHUNK 0 MUST LOG THIS (Definition of Done, item 2). The fallback itself is not
    // being changed — losing the store is better than refusing to start — but it has
    // to write a record that says a re-seed follows.
    let dir = TempDir::new("store-corrupt");
    let path = dir.write("seen.json", "{\"order\":[\"half a st");

    let seen = Seen::load(&path);

    assert!(
        seen.is_empty(),
        "recorded behaviour: corrupt JSON falls back to an empty store, with no error \
         returned and no way for the caller to tell this from a first run"
    );
}

#[test]
fn gap_a_store_file_holding_something_other_than_an_object_also_loads_as_empty() {
    // Same silent fallback, reached a different way: a file someone overwrote with a
    // list, or an empty file.
    let dir = TempDir::new("store-wrongshape");
    assert!(Seen::load(&dir.write("a.json", "[]")).is_empty());
    assert!(Seen::load(&dir.write("b.json", "")).is_empty());
    assert!(Seen::load(&dir.write("c.json", "null")).is_empty());
}

#[test]
fn gap_the_store_holds_no_timestamps_so_a_key_can_only_leave_by_eviction() {
    // There is no time window: a key stays until 4000 newer keys push it out. The
    // saved file is a bare `order` list, which is what makes the 48 h window in
    // chunk 2 a format change and not just a code change.
    let dir = TempDir::new("store-notime");
    let path = dir.path_str("seen.json");

    let mut seen = Seen::default();
    seen.insert("a story".to_string());
    seen.save(&path).expect("save the store");

    let on_disk = std::fs::read_to_string(&path).expect("read the saved store");
    assert_eq!(
        on_disk, r#"{"order":["a story"]}"#,
        "recorded behaviour: the store format carries keys only — no first-seen time, \
         no last-seen time, no source"
    );
}
