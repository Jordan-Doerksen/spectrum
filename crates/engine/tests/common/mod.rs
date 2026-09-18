//! Shared helpers for the engine's integration tests.
//!
//! **No new crate.** The dependency tree is C-free and every new crate is a Change
//! Request (D-0003), so there is no `tempfile` here — this file is the std-only
//! replacement for the one thing the tests need: a scratch directory that cleans
//! itself up.
//!
//! **What these tests never touch.** Nothing here reads or writes the repo's real
//! `data\seen.json`, `config.local.json`, or `.env`. Every path a test hands to the
//! engine points inside a directory under the OS temp dir, and that directory is
//! deleted when the [`TempDir`] value drops. No test opens a socket, calls Ollama,
//! or calls Discord.
//!
//! `tests/common/mod.rs` is a module, not a test target: cargo compiles it only
//! into the test binaries that declare `mod common;`.

#![allow(dead_code)] // each test binary uses a different subset of these helpers

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Distinguishes two directories created inside the same test binary in the same
/// nanosecond. Tests run in parallel threads, so the name has to be unique.
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

/// A unique scratch directory under the OS temp dir, removed on drop.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// `label` names the test that owns the directory, so a leftover directory after
    /// a hard crash says which test made it.
    pub fn new(label: &str) -> TempDir {
        let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "spectrum-test-{label}-{}-{seq}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("create the test scratch directory");
        TempDir { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// The engine's own APIs (`Config::load`, `Seen::load`, `Seen::save`) take `&str`
    /// paths, so a test needs the joined path as an owned `String`.
    pub fn path_str(&self, name: &str) -> String {
        self.join(name).to_string_lossy().to_string()
    }

    /// Write a fixture file and return its path as a `String`.
    pub fn write(&self, name: &str, body: &str) -> String {
        let file = self.join(name);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("create the fixture's parent directory");
        }
        std::fs::write(&file, body).expect("write the fixture file");
        file.to_string_lossy().to_string()
    }

    /// Every file directly inside the directory whose name starts with `prefix`.
    pub fn files_starting_with(&self, prefix: &str) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.path) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .map(|e| e.path())
            .collect();
        found.sort();
        found
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // A failed clean-up must not mask the assertion that already failed.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
