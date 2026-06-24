//! The analyzer — one Ollama pass per item returns `{ category, read, severity }`.
//! This is the single LLM read that replaces the three engines' separate analyzers.
//! Neutral, no fabrication, no severity inflation (the macroscope/richter lessons).

use serde::Deserialize;

/// The four Discord channels, plus `Drop` (don't post). Decided by the model, not
/// the feed the item came from. Finance and politics are SEPARATE channels. [D-0004]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Financial,
    Political,
    Technology,
    Catastrophe,
    Drop,
}

impl Category {
    /// The config/webhook key for this band.
    pub fn key(self) -> &'static str {
        match self {
            Category::Financial => "financial",
            Category::Political => "political",
            Category::Technology => "technology",
            Category::Catastrophe => "catastrophe",
            Category::Drop => "drop",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Read {
    pub category: Category,
    pub read: String,
    pub severity: u8, // 1 minor · 2 notable · 3 major · 4 seismic
    pub confidence: String,
}

#[derive(Deserialize)]
struct OllamaResp {
    response: String,
}

#[derive(Deserialize)]
struct RawRead {
    #[serde(default)]
    category: String,
    #[serde(default)]
    read: String,
    #[serde(default)]
    severity: serde_json::Value,
    #[serde(default)]
    confidence: String,
}

const MODEL: &str = "llama3.1:8b";
const OLLAMA: &str = "http://localhost:11434/api/generate";

pub async fn analyze(client: &reqwest::Client, title: &str, source: &str) -> anyhow::Result<Read> {
    let prompt = format!(
        "You are a neutral news-desk router. Classify the headline into exactly ONE band and give a terse read.\n\
         Bands:\n\
         - financial: markets, macro, economy, central banks, earnings, commodities, corporate finance\n\
         - political: elections, government, policy, legislation, officials, non-violent geopolitics\n\
         - technology: software, hardware, AI, chips, platforms, space, science\n\
         - catastrophe: war, armed conflict, disaster, mass-casualty, major civil unrest, humanitarian crisis\n\
         - drop: sport, celebrity, lifestyle, trivia, or anything not newsworthy for those bands\n\
         Rules: pick the DOMINANT frame; never fabricate; do NOT inflate severity.\n\
         If a story is both financial and political, choose the frame the headline leads with.\n\
         severity: 1 minor, 2 notable, 3 major, 4 seismic. confidence: low|medium|high.\n\
         Return ONLY JSON of the form {{\"category\":\"\",\"read\":\"\",\"severity\":1,\"confidence\":\"\"}} where read is <=12 words.\n\n\
         Source: {source}\nHeadline: {title}"
    );

    let body = serde_json::json!({
        "model": MODEL,
        "prompt": prompt,
        "stream": false,
        "format": "json",
        "options": { "temperature": 0.2 }
    });

    let resp: OllamaResp = client
        .post(OLLAMA)
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let raw: RawRead = serde_json::from_str(&resp.response)?;
    Ok(coerce(raw))
}

/// Map the model's free-text fields onto our typed packet, clamped + defaulted so a
/// sloppy response degrades to a safe `Drop` rather than crashing the cycle.
fn coerce(raw: RawRead) -> Read {
    let category = match raw.category.to_lowercase().replace('_', "-").as_str() {
        "financial" | "finance" | "money" | "markets" | "economy" | "business" => Category::Financial,
        "political" | "politics" | "policy" | "government" | "election" | "geopolitics" => {
            Category::Political
        }
        "technology" | "tech" | "science" => Category::Technology,
        "catastrophe" | "crisis" | "war" | "disaster" | "conflict" | "humanitarian" => {
            Category::Catastrophe
        }
        _ => Category::Drop,
    };
    let severity = (match &raw.severity {
        serde_json::Value::Number(n) => n.as_u64().unwrap_or(1) as u8,
        serde_json::Value::String(s) => s.trim().parse().unwrap_or(1),
        _ => 1,
    })
    .clamp(1, 4);
    let confidence = if raw.confidence.is_empty() {
        "low".into()
    } else {
        raw.confidence.to_lowercase()
    };
    Read {
        category,
        read: raw.read,
        severity,
        confidence,
    }
}
