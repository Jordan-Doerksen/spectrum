# Spectrum — Design Spec

> The full design: what it is, how it works, the data model. HANDOFF.md is the build
> kickoff; DECISIONS.md is the ADR log; this is the source of truth for *what* and *why*.

## 0. One-paragraph pitch
Spectrum is one news engine that replaces three (Doomfeed, Macroscope, Richter). It pools
RSS feeds across finance, politics, technology, and world news, has a local LLM sort each
headline into one band and weigh it, and posts the ones that matter as themed cards to the
matching Discord channel — with a small Tauri control panel on top. Sentiment and
newsworthiness only; it reports the news, never vouches for it.

## 1. Goals / non-goals
- **Goals:** one engine, one LLM read per unique headline, three live channels; refined,
  themed, labelled cards; always-on with a control surface; a clean single-exe handoff.
- **Non-goals:** no web dashboards (dropped on purpose); no cloud model or API keys (local
  Ollama only); never fabricate a read; one primary category per item (no cross-posting).

## 2. The core model
Each surviving headline gets ONE Ollama pass returning `{ category, read, severity }`:
- **category** ∈ financial · political · technology · catastrophe · drop — picks the webhook.
- **read** — a terse one-line signal (≤ 12 words).
- **severity** 1–4 (minor · notable · major · seismic) — drives the meter and the floor.

`drop` and anything below `min_severity` never reach Discord.

## 3. Features
- **Pooled feeds** (34): dedicated desks (markets, world, politics, tech) + Google News
  `when:1d` topic sweeps per band. The feed is a hint; the LLM decides the band.
- **Seed + dedupe:** a persistent `data/seen.json` (capped, fingerprint by title). The
  first run seeds the backlog and posts nothing; each headline is classified exactly once.
- **Themed cards:** one embed system, four skins — colour + emoji + label + severity meter.
- **Drip pacer:** ≤ `max_per_drop` cards every `drip_seconds`, strongest first — no bursts.
- **Control panel:** start/stop, dry-run, live counts per channel, tuning, activity log;
  system tray; `SPECTRUM_AUTOSTART` boots it running into the tray.

## 4. Tech spec
- **Stack:** Rust (GNU toolchain, no MSVC) + Tauri 2 (vanilla UI + withGlobalTauri),
  Ollama (`llama3.1:8b`) over HTTP, `reqwest` (native-tls → SChannel, C-free), `feed-rs`.
- **Workspace:** `crates/engine` (the pipeline as a reusable `Engine` service) ·
  `crates/headless` (server runner) · `src-tauri` (the panel) · `ui/` (vanilla cockpit).
- **Config:** `config.local.json` (gitignored) — webhooks by band, model, `min_severity`,
  `poll_minutes`, `drip_seconds`. `config.example.json` shows the shape.
- **State:** `data/seen.json` (dedupe). No database.

## 5. Routing
`category` → the webhook keyed by band name in `config.local.json`. Technology folds into
politics by pointing the `technology` key at the politics webhook (or give it its own).

## 6. Open questions / future
- Golden-vector classifier tests + borderline-routing tuning.
- Optional lexicon pre-filter if Google-News churn makes per-poll classification heavy.
- The in-memory drip queue is lost on restart (cards already marked seen won't post) —
  persist it if it bites. A breaking-news priority lane (Crop's idea) if wanted.
- React-via-Vite UI swap if the panel grows (drop-in `frontendDist`).

## Status (2026-06-24)
Built + verified: the engine (seed/dedupe/classify/route/drip, live Discord), the 34-feed
pool, the control panel, and the standalone Riley package. Supersedes Doomfeed, Macroscope,
and Richter.
