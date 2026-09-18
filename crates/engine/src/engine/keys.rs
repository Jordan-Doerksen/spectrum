//! The dedupe fingerprint and the clock. Pulled out of `engine.rs` so the service file
//! holds the pipeline and nothing else (house law: review at 300 lines).
//!
//! `norm` is public so a test can call it without a `#[cfg(test)]` block inside the
//! service: `spectrum_engine::engine::norm`. The behaviour is unchanged. [CR-1 chunk 0]

/// Fingerprint a headline for dedupe. Drops a trailing " - Publisher" (Google News
/// appends it, so the same story from different sources collapses to one key), then
/// lowercases and strips punctuation. Cross-source near-dupes with genuinely DIFFERENT
/// wording still slip through — that needs semantic dedupe (CR-1 chunk 2).
pub fn norm(t: &str) -> String {
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

/// Seconds since the epoch, or `None` if the clock is before 1970. The panel shows it
/// as "last poll N ago", so a `None` reads "no poll yet" rather than a wrong time.
pub fn now_unix() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}
