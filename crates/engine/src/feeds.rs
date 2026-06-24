//! The pooled feed list — the union of the three engines' proven sources (markets,
//! world, politics) plus a new technology band. Each feed carries a *hint* at its
//! likely band, but the final category is decided by the analyzer, not the feed.
//!
//! Two source kinds: dedicated RSS desks (rich, already-newsworthy) and targeted
//! Google News topic searches (`when:1d` = last day) that keep us current on the
//! specific things each band cares about. Ported from macroscope + richter.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    Financial,
    Political,
    Technology,
    Catastrophe,
}

pub struct Feed {
    pub url: String,
    pub source: String,
    pub hint: Band,
}

fn rss(url: &str, source: &str, hint: Band) -> Feed {
    Feed { url: url.to_string(), source: source.to_string(), hint }
}

/// A Google News topic search as an RSS feed.
fn gnews(query: &str, source: &str, hint: Band) -> Feed {
    let url = format!(
        "https://news.google.com/rss/search?q={}&hl=en-US&gl=US&ceid=US:en",
        enc(query)
    );
    Feed { url, source: source.to_string(), hint }
}

/// Minimal percent-encoder for the search query (unreserved chars pass through).
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "%20".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The full balanced pool. Built fresh each poll (cheap); the analyzer routes each item.
pub fn feeds() -> Vec<Feed> {
    use Band::*;
    vec![
        // ── financial: dedicated markets desks (ported from macroscope) ──
        rss("https://feeds.content.dowjones.io/public/rss/RSSMarketsMain", "DJ Markets", Financial),
        rss("https://www.cnbc.com/id/100003114/device/rss/rss.html", "CNBC Top", Financial),
        rss("https://www.cnbc.com/id/20910258/device/rss/rss.html", "CNBC Markets", Financial),
        rss("https://feeds.marketwatch.com/marketwatch/topstories/", "MarketWatch", Financial),
        // financial: targeted Google News sweeps
        gnews("Federal Reserve OR FOMC OR Powell OR \"interest rate\" OR \"rate decision\" when:1d", "Fed/Policy", Financial),
        gnews("CPI OR inflation OR \"jobs report\" OR payrolls OR GDP OR PCE when:1d", "Inflation/Data", Financial),
        gnews("\"stock market\" OR \"S&P 500\" OR Nasdaq OR \"Dow Jones\" OR earnings when:1d", "Equities", Financial),
        gnews("\"Treasury yields\" OR \"bond market\" OR \"10-year yield\" OR \"yield curve\" when:1d", "Rates", Financial),
        gnews("\"US dollar\" OR \"dollar index\" OR DXY OR forex when:1d", "FX/Dollar", Financial),
        gnews("\"oil prices\" OR crude OR OPEC OR \"natural gas\" OR Brent OR WTI when:1d", "Oil/Energy", Financial),
        gnews("\"gold price\" OR bullion OR \"precious metals\" when:1d", "Gold", Financial),
        gnews("bitcoin OR ethereum OR crypto OR \"digital assets\" when:1d", "Crypto", Financial),

        // ── political: dedicated desks + sweeps (ported from richter) ──
        rss("https://feeds.bbci.co.uk/news/politics/rss.xml", "BBC Politics", Political),
        rss("https://www.theguardian.com/politics/rss", "Guardian Politics", Political),
        gnews("election OR president OR parliament OR government OR vote OR senate OR congress when:1d", "Politics", Political),
        gnews("policy OR legislation OR \"supreme court\" OR diplomacy OR summit OR tariffs OR sanctions when:1d", "Policy/Trade", Political),

        // ── catastrophe: world desks + crisis sweeps (ported from richter) ──
        rss("https://feeds.bbci.co.uk/news/world/rss.xml", "BBC World", Catastrophe),
        rss("https://www.aljazeera.com/xml/rss/all.xml", "Al Jazeera", Catastrophe),
        rss("https://www.theguardian.com/world/rss", "Guardian World", Catastrophe),
        rss("https://feeds.npr.org/1001/rss.xml", "NPR News", Catastrophe),
        rss("https://rss.nytimes.com/services/xml/rss/nyt/World.xml", "NYT World", Catastrophe),
        rss("https://feeds.skynews.com/feeds/rss/world.xml", "Sky World", Catastrophe),
        gnews("war OR airstrike OR ceasefire OR military OR offensive OR invasion OR troops when:1d", "Conflict", Catastrophe),
        gnews("earthquake OR flood OR wildfire OR hurricane OR explosion OR disaster OR evacuation when:1d", "Disaster", Catastrophe),
        gnews("protest OR unrest OR riot OR coup OR \"state of emergency\" OR crackdown when:1d", "Unrest", Catastrophe),

        // ── technology: dedicated desks + sweeps (the NEW band) ──
        rss("https://feeds.arstechnica.com/arstechnica/index", "Ars Technica", Technology),
        rss("https://www.theverge.com/rss/index.xml", "The Verge", Technology),
        rss("https://hnrss.org/frontpage", "Hacker News", Technology),
        rss("https://www.wired.com/feed/rss", "Wired", Technology),
        rss("https://www.engadget.com/rss.xml", "Engadget", Technology),
        rss("https://techcrunch.com/feed/", "TechCrunch", Technology),
        rss("https://www.theregister.com/headlines.atom", "The Register", Technology),
        gnews("\"artificial intelligence\" OR OpenAI OR \"large language model\" OR Nvidia OR semiconductors OR chips when:1d", "AI/Chips", Technology),
        gnews("Apple OR Google OR Microsoft OR Meta OR Amazon OR Tesla OR \"big tech\" when:1d", "Big Tech", Technology),
    ]
}
