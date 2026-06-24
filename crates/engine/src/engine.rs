//! The `Engine` service — the pipeline as a controllable object, so the headless
//! runner AND the Tauri panel drive the SAME logic (the v4 discipline: behaviour in
//! the engine crate, thin shells on top). Owns config + seen-store + the drip queue
//! + session stats + a recent-activity log.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::analyze::{self, Category, Read};
use crate::config::Config;
use crate::store::Seen;
use crate::{discord, feeds, rss, skins};

pub struct Engine {
    pub cfg_path: String,
    pub cfg: Config,
    seen_file: String,
    seen: Seen,
    queue: Vec<(rss::Item, Read)>,
    client: reqwest::Client,
    posted: HashMap<String, usize>, // band key -> count this session
    last_poll_unix: Option<u64>,
    log: VecDeque<String>, // recent activity, newest at the back
}

/// A serialized snapshot the panel reads each tick.
#[derive(serde::Serialize, Clone)]
pub struct Status {
    pub seen: usize,
    pub queued: usize,
    pub posted_financial: usize,
    pub posted_political: usize,
    pub posted_technology: usize,
    pub posted_catastrophe: usize,
    pub last_poll_unix: Option<u64>,
    pub min_severity: u8,
    pub poll_minutes: u64,
    pub drip_seconds: u64,
    pub feeds: usize,
}

impl Engine {
    pub fn new(cfg_path: &str) -> anyhow::Result<Self> {
        let cfg = Config::load(cfg_path)?;
        let base = std::path::Path::new(cfg_path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let seen_file = base.join(&cfg.seen_path).to_string_lossy().to_string();
        let seen = Seen::load(&seen_file);
        Ok(Self {
            cfg_path: cfg_path.to_string(),
            cfg,
            seen_file,
            seen,
            queue: Vec::new(),
            client: crate::ua_client(),
            posted: HashMap::new(),
            last_poll_unix: None,
            log: VecDeque::new(),
        })
    }

    /// Re-read config.local.json (so the panel's edits go live without a restart).
    pub fn reload_config(&mut self) {
        if let Ok(c) = Config::load(&self.cfg_path) {
            self.cfg = c;
        }
    }

    pub fn seen_empty(&self) -> bool {
        self.seen.is_empty()
    }

    fn note(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > 200 {
            self.log.pop_front();
        }
    }

    /// First-run seed: mark every current headline seen, post nothing. Returns count.
    pub async fn seed(&mut self) -> usize {
        let items = self.gather().await;
        for it in &items {
            self.seen.insert(norm(&it.title));
        }
        let _ = self.seen.save(&self.seen_file);
        let n = items.len();
        self.note(format!("seeded {n} headlines (posted nothing)"));
        n
    }

    /// Fetch every feed, deduped by title within the batch.
    async fn gather(&self) -> Vec<rss::Item> {
        let mut items = Vec::new();
        let mut batch = HashSet::new();
        for feed in feeds::feeds() {
            if let Ok(list) = rss::fetch(&self.client, &feed).await {
                for it in list {
                    if it.title.trim().is_empty() || !batch.insert(norm(&it.title)) {
                        continue;
                    }
                    items.push(it);
                }
            }
        }
        items
    }

    /// Poll: classify only UNSEEN items, enqueue the cleared ones. Returns new count.
    pub async fn poll(&mut self) -> usize {
        let items = self.gather().await;
        let mut new = 0;
        for it in items {
            let key = norm(&it.title);
            if self.seen.contains(&key) {
                continue;
            }
            if let Ok(read) = analyze::analyze(&self.client, &it.title, &it.source).await {
                self.seen.insert(key);
                if read.category != Category::Drop && read.severity >= self.cfg.min_severity {
                    self.queue.push((it, read));
                    new += 1;
                }
            }
        }
        self.queue.sort_by(|a, b| b.1.severity.cmp(&a.1.severity)); // strongest first
        let _ = self.seen.save(&self.seen_file);
        self.last_poll_unix = now_unix();
        self.note(format!(
            "poll: {new} new · {} queued · {} seen",
            self.queue.len(),
            self.seen.len()
        ));
        new
    }

    /// Drip: post up to `max_per_drop` strongest cards. Returns the lines it logged.
    pub async fn drip(&mut self, dry: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut posted = 0;
        while posted < self.cfg.max_per_drop && !self.queue.is_empty() {
            let (it, read) = self.queue.remove(0);
            let s = skins::skin(read.category);
            let webhook = self.cfg.webhook_for(read.category.key()).cloned();

            let line = if dry {
                format!("[dry] {} {} — {}", s.emoji, s.label, it.title)
            } else if let Some(url) = webhook {
                let payload = discord::embed(read.category, &read, &it.title, &it.link, &it.source);
                match discord::post(&self.client, &url, &payload).await {
                    Ok(()) => {
                        *self.posted.entry(read.category.key().to_string()).or_insert(0) += 1;
                        format!("posted {} {} — {}", s.emoji, s.label, it.title)
                    }
                    Err(e) => format!("post FAILED {} — {e}", s.label),
                }
            } else {
                format!("no webhook for {} — held", s.label)
            };
            out.push(line.clone());
            self.note(line);
            posted += 1;
        }
        out
    }

    pub fn status(&self) -> Status {
        Status {
            seen: self.seen.len(),
            queued: self.queue.len(),
            posted_financial: *self.posted.get("financial").unwrap_or(&0),
            posted_political: *self.posted.get("political").unwrap_or(&0),
            posted_technology: *self.posted.get("technology").unwrap_or(&0),
            posted_catastrophe: *self.posted.get("catastrophe").unwrap_or(&0),
            last_poll_unix: self.last_poll_unix,
            min_severity: self.cfg.min_severity,
            poll_minutes: self.cfg.poll_minutes,
            drip_seconds: self.cfg.drip_seconds,
            feeds: feeds::feeds().len(),
        }
    }

    /// The most recent `n` activity lines, newest first.
    pub fn recent_log(&self, n: usize) -> Vec<String> {
        self.log.iter().rev().take(n).cloned().collect()
    }
}

/// Fingerprint a headline for dedupe. Drops a trailing " - Publisher" (Google News
/// appends it, so the same story from different sources collapses to one key), then
/// lowercases and strips punctuation. Cross-source near-dupes with genuinely DIFFERENT
/// wording still slip through — that needs semantic dedupe (future).
fn norm(t: &str) -> String {
    let t = t.trim();
    let core = match t.rfind(" - ") {
        Some(i) if i > 0 => &t[..i],
        _ => t,
    };
    core.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn now_unix() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}
