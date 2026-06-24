# Spectrum

One news engine, three Discord channels.

Spectrum reads a wide spread of finance, politics, tech, and world news, has a local LLM
sort each headline into one band and weigh how much it matters, and posts the ones that
clear the bar as themed cards to the matching Discord channel. A small desktop control
panel rides on top — start it, stop it, watch it work.

It replaces three separate engines (Doomfeed, Macroscope, Richter) that each hit the same
local LLM on their own. Now it's one engine, one read per headline, three channels.

**Stack:** Rust (the engine) + Tauri (the panel), a local Ollama model for the reads,
RSS in, Discord webhooks out. No cloud model, no API keys, no dashboards.

## Run it

The control panel (Windows):
```
RUN PANEL.bat
```
First launch builds the Tauri app (a few minutes), then opens the panel. Press **Start**;
tick **Dry run** first if you want to watch it classify without posting. Needs
[Ollama](https://ollama.com) running with `llama3.1:8b`, and a `config.local.json` with
your Discord webhooks (copy `config.example.json`).

Headless (no window):
```
scripts/dev.ps1 run -p spectrum-headless
```

## How it works

Pooled RSS feeds (34 across four bands — dedicated desks + Google News topic sweeps) →
fetch once → drop anything already seen → one Ollama pass per new headline returns
`{ category, read, severity }` → route to the matching webhook with a band-themed card →
a drip pacer trickles them out instead of bursting. The first run seeds the backlog
silently, so day one doesn't dump history.

The four bands — **finance · politics · technology · catastrophe** — each get their own
colour, label, and severity meter. (Tech can have its own channel or fold into politics;
it's a one-line config change.)

## Honest by design

Show nothing rather than something false: off-topic items are dropped, the severity floor
keeps noise off Discord, and a quiet channel beats a wrong card. Spectrum reports the
news — it doesn't vouch for it.

## Status

Built and running: the engine (seed · dedupe · classify · route · drip), the live Discord
posting, and the control panel. See [CHANGELOG](CHANGELOG.md) for the build log and
[DECISIONS](DECISIONS.md) for the why.

## License

MIT — see [LICENSE](LICENSE).

---
*Doomfeed → Macroscope → Richter → Spectrum. The Observatory.*
