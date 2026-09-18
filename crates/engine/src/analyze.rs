//! The analyzer — one Ollama pass per item returns `{ category, read, severity }`.
//! This is the single LLM read that replaces the three engines' separate analyzers.
//! Neutral, no fabrication, no severity inflation (the macroscope/richter lessons).
//!
//! [`coerce`] degrades a sloppy response rather than crashing the cycle, and that
//! degrading used to be silent: a category word no arm recognised became `Drop`, and a
//! severity that was not a number became 1, which is under the default floor. Either way
//! the headline is marked seen and can never be offered again, so a model whose output
//! format has drifted looked exactly like a quiet news day. The behaviour is unchanged,
//! but what could not be mapped now comes back beside the [`Read`] as [`Unmapped`], and
//! the caller writes one record per distinct word. [CR-1 chunk 0 · observability law]

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

/// What the model said that the mapping did not recognise. Empty on a clean response.
/// A value here means the item was degraded, not understood — and a degraded item is
/// dropped and marked seen, so it never gets a second reading. [CR-1 chunk 0]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unmapped {
    /// the raw `category` word, when no arm matched it. The item becomes `Drop`.
    pub category: Option<String>,
    /// the raw `severity` value, when it was not a number. The item becomes severity 1.
    pub severity: Option<String>,
}

impl Unmapped {
    pub fn any(&self) -> bool {
        self.category.is_some() || self.severity.is_some()
    }
    /// The words themselves, as one key, so repeats of the same drift roll up together.
    pub fn key(&self) -> String {
        format!(
            "category={} severity={}",
            self.category.as_deref().unwrap_or("-"),
            self.severity.as_deref().unwrap_or("-")
        )
    }
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

/// One read of one headline. The second half of the pair is what the mapping could not
/// recognise in the model's answer — empty when the answer was clean.
pub async fn analyze(
    client: &reqwest::Client,
    title: &str,
    source: &str,
) -> anyhow::Result<(Read, Unmapped)> {
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
/// sloppy response degrades to a safe `Drop` rather than crashing the cycle. The second
/// return value names every field the mapping could not read, so the degrading is not
/// silent.
fn coerce(raw: RawRead) -> (Read, Unmapped) {
    let mut unmapped = Unmapped::default();

    let word = raw.category.to_lowercase().replace('_', "-");
    let category = match word.as_str() {
        "financial" | "finance" | "money" | "markets" | "economy" | "business" => Category::Financial,
        "political" | "politics" | "policy" | "government" | "election" | "geopolitics" => {
            Category::Political
        }
        "technology" | "tech" | "science" => Category::Technology,
        "catastrophe" | "crisis" | "war" | "disaster" | "conflict" | "humanitarian" => {
            Category::Catastrophe
        }
        // `drop` is the model doing as it was told; anything else is drift, and the two
        // must not look the same in the log.
        "drop" => Category::Drop,
        _ => {
            unmapped.category = Some(raw.category.clone());
            Category::Drop
        }
    };

    let severity = (match &raw.severity {
        serde_json::Value::Number(n) => match n.as_u64() {
            Some(v) => v as u8,
            // a float or a negative number: the scale is 1..=4 and this is not on it
            None => {
                unmapped.severity = Some(n.to_string());
                1
            }
        },
        serde_json::Value::String(s) => s.trim().parse().unwrap_or_else(|_| {
            unmapped.severity = Some(s.clone());
            1
        }),
        serde_json::Value::Null => 1,
        other => {
            unmapped.severity = Some(other.to_string());
            1
        }
    })
    .clamp(1, 4);

    let confidence = if raw.confidence.is_empty() {
        "low".into()
    } else {
        raw.confidence.to_lowercase()
    };
    (
        Read {
            category,
            read: raw.read,
            severity,
            confidence,
        },
        unmapped,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(category: &str, severity: serde_json::Value) -> RawRead {
        RawRead {
            category: category.into(),
            read: "a terse read".into(),
            severity,
            confidence: "high".into(),
        }
    }

    #[test]
    fn a_clean_answer_maps_with_nothing_left_over() {
        let (read, unmapped) = coerce(raw("financial", serde_json::json!(3)));
        assert_eq!(read.category, Category::Financial);
        assert_eq!(read.severity, 3);
        assert!(!unmapped.any(), "a clean answer must report no drift: {unmapped:?}");
    }

    #[test]
    fn the_model_obeying_the_drop_instruction_is_not_drift() {
        let (read, unmapped) = coerce(raw("drop", serde_json::json!(1)));
        assert_eq!(read.category, Category::Drop);
        assert!(!unmapped.any(), "an explicit drop is the prompt working, not a failure");
    }

    #[test]
    fn a_category_word_no_arm_knows_is_reported_and_still_dropped() {
        let (read, unmapped) = coerce(raw("sports-and-culture", serde_json::json!(2)));
        assert_eq!(read.category, Category::Drop, "the behaviour is unchanged");
        assert_eq!(unmapped.category.as_deref(), Some("sports-and-culture"));
        assert!(unmapped.key().contains("sports-and-culture"), "the log line names the word");
    }

    #[test]
    fn a_severity_that_is_not_a_number_is_reported_and_still_becomes_one() {
        let (read, unmapped) = coerce(raw("financial", serde_json::json!("very high")));
        assert_eq!(read.severity, 1, "the behaviour is unchanged: under the default floor");
        assert_eq!(unmapped.severity.as_deref(), Some("very high"));
    }

    #[test]
    fn a_severity_written_as_a_numeric_string_still_maps_cleanly() {
        let (read, unmapped) = coerce(raw("financial", serde_json::json!("3")));
        assert_eq!(read.severity, 3);
        assert!(!unmapped.any(), "a number in quotes is not drift");
    }

    #[test]
    fn a_missing_severity_field_is_the_recorded_default_and_not_drift() {
        // `#[serde(default)]` gives `Value::Null` when the model omits the field.
        let (read, unmapped) = coerce(raw("political", serde_json::Value::Null));
        assert_eq!(read.severity, 1);
        assert!(!unmapped.severity.is_some(), "an absent field is the known default path");
    }
}
