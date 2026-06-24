//! Persistent dedupe store — the fingerprints (normalized titles) Spectrum has already
//! seen, so a restart never re-posts and the analyzer never re-reads the same headline.
//! Insertion-ordered with a hard cap; oldest fingerprints fall off.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const CAP: usize = 4000;

#[derive(Default, Serialize, Deserialize)]
pub struct Seen {
    order: Vec<String>, // insertion order, for pruning the oldest
    #[serde(skip)]
    set: HashSet<String>, // rebuilt from `order` on load
}

impl Seen {
    pub fn load(path: &str) -> Self {
        match std::fs::read_to_string(path) {
            Ok(txt) => {
                let mut s: Seen = serde_json::from_str(&txt).unwrap_or_default();
                s.set = s.order.iter().cloned().collect();
                s
            }
            Err(_) => Seen::default(),
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

    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(path, serde_json::to_string(self)?)?;
        Ok(())
    }
}
