//! Persistent dedupe store — the fingerprints (normalized titles) Spectrum has already
//! seen, so a restart never re-posts and the analyzer never re-reads the same headline.
//! Insertion-ordered with a hard cap; oldest fingerprints fall off.
//!
//! Every failure here is loud, because every failure here costs money and noise: a
//! store that does not load makes the engine treat a live run as a first run and
//! re-seed, and a store that does not save makes the next start re-read the same
//! headlines. The behaviour is unchanged — only the record is new.
//! [CR-1 chunk 0 · observability law]

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::ErrorKind;

use crate::log;

const CAP: usize = 4000;

#[derive(Default, Serialize, Deserialize)]
pub struct Seen {
    order: Vec<String>, // insertion order, for pruning the oldest
    #[serde(skip)]
    set: HashSet<String>, // rebuilt from `order` on load
}

impl Seen {
    pub fn load(path: &str) -> Self {
        let txt = match std::fs::read_to_string(path) {
            Ok(txt) => txt,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                log::info("seen.absent")
                    .field("path", path)
                    .not_doing(
                        "load any fingerprint",
                        "no store file exists yet, so the first cycle seeds the backlog and posts nothing",
                    )
                    .emit();
                return Seen::default();
            }
            Err(e) => {
                log::error("seen.read.failed")
                    .field("path", path)
                    .field("error", e.to_string())
                    .not_doing(
                        "load the fingerprints",
                        "the store starts EMPTY, so the engine treats this run as a first run and re-seeds; every earlier headline can be read again",
                    )
                    .emit();
                return Seen::default();
            }
        };
        match serde_json::from_str::<Seen>(&txt) {
            Ok(mut s) => {
                s.set = s.order.iter().cloned().collect();
                log::info("seen.loaded")
                    .field("path", path)
                    .count("keys", s.order.len())
                    .emit();
                s
            }
            Err(e) => {
                log::error("seen.parse.failed")
                    .field("path", path)
                    .count("bytes", txt.len())
                    .field("error", e.to_string())
                    .not_doing(
                        "keep the fingerprints on disk",
                        "the file is corrupt, so the store starts EMPTY: the engine treats this run as a first run, seeds the whole backlog, and every earlier headline can be read again",
                    )
                    .emit();
                Seen::default()
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
    pub fn len(&self) -> usize {
        self.order.len()
    }
    pub fn contains(&self, key: &str) -> bool {
        self.set.contains(key)
    }

    pub fn insert(&mut self, key: String) {
        if self.set.insert(key.clone()) {
            self.order.push(key);
            if self.order.len() > CAP {
                let drop = self.order.len() - CAP;
                for k in self.order.drain(0..drop) {
                    self.set.remove(&k);
                }
            }
        }
    }

    /// Write the store. The error goes to the caller, which names the phase it failed
    /// in; a directory that cannot be created is reported here, because the write
    /// error that follows does not say the directory was the cause.
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    log::warn("seen.dir.failed")
                        .field("path", parent.to_string_lossy())
                        .field("error", e.to_string())
                        .not_doing(
                            "create the store directory",
                            "the write below fails as well, and the caller reports the loss",
                        )
                        .emit();
                }
            }
        }
        std::fs::write(path, serde_json::to_string(self)?)?;
        Ok(())
    }
}
