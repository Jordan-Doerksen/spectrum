//! Build the themed embed per band and POST it to that band's webhook. The client
//! already carries the User-Agent (ua_client); Discord returns 204 on success.

use crate::analyze::{Category, Read};
use crate::skins;

/// The rich embed Discord renders: emoji+headline title (links out), one-line read,
/// and Band / Severity / Confidence fields, accent-colored by band.
pub fn embed(cat: Category, read: &Read, title: &str, link: &str, source: &str) -> serde_json::Value {
    let s = skins::skin(cat);
    let meter = skins::meter(read.severity);
    serde_json::json!({
        "username": "Spectrum",
        "embeds": [{
            "title": format!("{} {}", s.emoji, title),
            "url": link,
            "color": s.color,
            "description": format!("**{}**", read.read),
            "fields": [
                { "name": "Band", "value": s.label, "inline": true },
                { "name": "Severity", "value": format!("{}  {}/4", meter, read.severity), "inline": true },
                { "name": "Confidence", "value": read.confidence, "inline": true }
            ],
            "footer": { "text": format!("Spectrum · {source}") }
        }]
    })
}

/// Pretty-print a payload for `--dry` preview.
pub fn preview(payload: &serde_json::Value) -> String {
    serde_json::to_string_pretty(payload).unwrap_or_default()
}

pub async fn post(
    client: &reqwest::Client,
    webhook: &str,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    let resp = client.post(webhook).json(payload).send().await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("discord {status} — {body}");
    }
    Ok(())
}
