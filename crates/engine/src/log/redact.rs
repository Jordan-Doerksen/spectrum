//! Secret-cutting. Every event name and every string value goes through [`redact`]
//! before it reaches the disk or the console, so a webhook URL or a token can never be
//! logged — not by a careless call site, and not inside an error message from reqwest.
//!
//! Two passes:
//! 1. A Discord webhook URL keeps its id and loses its token:
//!    `https://discord.com/api/webhooks/123/abcdef` -> `.../api/webhooks/123/REDACTED`.
//! 2. A value that follows `token`, `secret`, `password`, `api_key`, `apikey`,
//!    `authorization` or `bearer` is blanked.
//!
//! Over-redaction is the accepted trade: a headline that contains the word "token"
//! loses its next word, and a leak loses a channel. These are pure functions with no
//! state and no I/O, so they are directly testable.

const SECRET_KEYS: [&str; 7] =
    ["token", "secret", "password", "api_key", "apikey", "authorization", "bearer"];

const MARK: &str = "/api/webhooks/";

/// Cut every secret out of a line.
pub fn redact(text: &str) -> String {
    redact_secret_values(&redact_webhooks(text))
}

/// The webhook id inside a Discord webhook URL, if it has one. The id is not a secret
/// — the token that follows it is.
pub fn webhook_id(url: &str) -> Option<&str> {
    let rest = &url[url.find(MARK)? + MARK.len()..];
    let end = rest.find(|c: char| c == '/' || is_url_end(c)).unwrap_or(rest.len());
    if end == 0 { None } else { Some(&rest[..end]) }
}

/// How a webhook is named in a log line: the channel key plus the id, never the token.
/// A URL with no id reads `news:unknown`, which still says which channel was meant.
pub fn webhook_label(channel: &str, url: &str) -> String {
    format!("{channel}:{}", webhook_id(url).unwrap_or("unknown"))
}

fn is_url_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '?' | '&' | ',' | ')' | '<' | '>' | '\\' | ';')
}

/// A word that introduces a secret instead of being one.
fn is_scheme_word(word: &str) -> bool {
    matches!(word, "bearer" | "basic" | "digest" | "token")
}

fn is_value_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | ',' | '&' | '}' | ')' | '<' | '>' | ';')
}

fn redact_webhooks(s: &str) -> String {
    let (mut out, mut rest) = (String::with_capacity(s.len()), s);
    while let Some(i) = rest.find(MARK) {
        let (head, tail) = rest.split_at(i + MARK.len());
        out.push_str(head);
        let id_end = tail.find(|c: char| c == '/' || is_url_end(c)).unwrap_or(tail.len());
        let (id, after) = tail.split_at(id_end);
        out.push_str(if id.is_empty() { "unknown" } else { id });
        match after.strip_prefix('/') {
            Some(token) => {
                let tok_end = token.find(is_url_end).unwrap_or(token.len());
                out.push_str(if tok_end > 0 { "/REDACTED" } else { "/" });
                rest = &token[tok_end..];
            }
            None => rest = after,
        }
    }
    out.push_str(rest);
    out
}

fn redact_secret_values(s: &str) -> String {
    let lower = s.to_ascii_lowercase(); // an ASCII fold keeps every byte offset
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    'scan: while i < s.len() {
        for key in SECRET_KEYS {
            if lower[i..].starts_with(key) {
                out.push_str(&s[i..i + key.len()]);
                let mut j = i + key.len();
                // the separator run between the key and its value
                while j < s.len() && matches!(bytes[j], b'=' | b':' | b'"' | b'\'' | b' ') {
                    out.push(bytes[j] as char);
                    j += 1;
                }
                let start = j;
                // every terminator is ASCII, so j always lands on a char boundary
                while j < s.len() && !is_value_end(bytes[j] as char) {
                    j += 1;
                }
                // `Authorization: Bearer abc` — the value IS the scheme word, and the
                // secret is the word after it. Hand that word back to the scanner.
                if is_scheme_word(&lower[start..j]) {
                    i = start;
                    continue 'scan;
                }
                if j > start {
                    out.push_str("REDACTED");
                }
                i = j;
                continue 'scan;
            }
        }
        let ch = s[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
