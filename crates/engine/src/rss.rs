//! Fetch + parse a single feed. `feed-rs` handles RSS *and* Atom from one call,
//! so the pooled list can mix formats freely.

use crate::feeds::Feed;

#[derive(Debug, Clone)]
pub struct Item {
    pub title: String,
    pub link: String,
    pub source: String,
}

/// Fetch a feed over HTTPS and return its entries. Errors (404, TLS, parse) bubble
/// up so the caller can log-and-continue rather than abort the whole cycle.
pub async fn fetch(client: &reqwest::Client, feed: &Feed) -> anyhow::Result<Vec<Item>> {
    let body = client
        .get(feed.url.as_str())
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let parsed = feed_rs::parser::parse(&body[..])?;
    let items = parsed
        .entries
        .into_iter()
        .map(|e| Item {
            title: e.title.map(|t| t.content).unwrap_or_default(),
            link: e.links.first().map(|l| l.href.clone()).unwrap_or_default(),
            source: feed.source.to_string(),
        })
        .collect();
    Ok(items)
}
