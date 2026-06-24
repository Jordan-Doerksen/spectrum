//! Spectrum engine — the pipeline shared by the headless runner and the Tauri shell.
//!
//! Pipeline (mirrors the three proven Node engines, collapsed into one):
//!   feeds -> rss(fetch+parse) -> prefilter -> dedupe -> analyze(ollama)
//!         -> route by category -> themed Discord card -> drip pacer
//!
//! M0 slice: just `feeds` + `rss`, to prove HTTPS/TLS builds and fetches on this
//! GNU/no-C-compiler box. The rest lands in M1.

pub mod analyze;
pub mod config;
pub mod discord;
pub mod engine;
pub mod feeds;
pub mod rss;
pub mod skins;
pub mod store;

// Re-export so the runner can name the client type without its own reqwest dep.
pub use reqwest::Client;

/// A reqwest client pre-loaded with the User-Agent the Node engines learned they
/// need — Cloudflare 403s the default agent (the macroscope/richter lesson).
pub fn ua_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::USER_AGENT,
        reqwest::header::HeaderValue::from_static(
            "Mozilla/5.0 (Spectrum news watcher; +https://github.com/Jordan-Doerksen/spectrum)",
        ),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("build reqwest client")
}
