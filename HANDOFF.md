# Spectrum — Build Handoff

> Read SPEC.md for the full design and DECISIONS.md for the why. This is the kickoff:
> how to stand up and build it. Home repo: `spectrum` (`C:\projects\spectrum`).

> **Progress (as built, 2026-06-24):** Complete. Engine + live Discord posting + control
> panel + a self-contained Riley package, all verified. M0–M4 done; M5 (this doc set +
> the Observatory entry) in progress.

## What you're building
One Rust + Tauri news engine that pools RSS, has a local LLM route each headline into one
of four bands, and posts themed cards to the matching Discord channel. Replaces three engines.

## Stand up the repo (this machine)
The Rust GNU toolchain isn't on PATH, and the Tauri/Windows import libs need mingw-w64.
**Every build prepends both dirs** (or it dies at `dlltool.exe not found`):
```
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\winlibs\mingw64\bin;$env:PATH"
```
`scripts/dev.ps1` wraps this — e.g. `.\scripts\dev.ps1 build --workspace`. [DECISIONS D-0001/D-0002]

Keep the dependency tree C-free: `reqwest` on `native-tls` → SChannel (not rustls/aws-lc-rs,
which needs a C compiler). [D-0003]

## Architecture
- `crates/engine` — the pipeline as an `Engine` service (feeds · rss · dedupe-by-seen ·
  analyze/Ollama · skins · discord · the poll/drip loop). The reusable core; unit-testable.
- `crates/headless` — a thin runner over `Engine` (`--once`, `--dry`, `--feedcheck`).
- `src-tauri` — the Tauri 2 shell: the engine in a background thread → a snapshot →
  withGlobalTauri commands (start/stop/status/dry/tuning/log). Vanilla `ui/`.

## Build / run
- Panel: `RUN PANEL.bat` (or `.\scripts\dev.ps1 run -p spectrum-pro`).
- Headless: `.\scripts\dev.ps1 run -p spectrum-headless` (`--dry` to not post).
- Validate feeds: `.\scripts\dev.ps1 run -p spectrum-headless -- --feedcheck`.
- Needs Ollama running (`llama3.1:8b`) + `config.local.json` (copy `config.example.json`).

## Build order (done)
- **M0** — toolchain de-risk (HTTPS + RSS on the GNU box). ✓
- **M1** — engine: seed · dedupe · classify · route · themed cards · drip · live Discord. ✓
- **M2** — the 34-feed pool (dedicated desks + Google News sweeps). ✓
- **M3** — the Tauri control panel. ✓
- **M4** — the standalone Riley package (exe + Ollama SETUP + autostart). ✓
- **M5** — this doc set + the Observatory entry. ←

## Deploy (Riley)
`deploy/package.ps1` → `Spectrum-for-Riley.zip` (exe + WebView2Loader.dll + config +
launchers + RILEY.md). One host only — two live copies double-post the shared channels.

## Keep / Don't carry over
- **Keep:** the proven pipeline from the three engines (the Discord User-Agent header, the
  drip pacer, cross-feed dedupe, the local-LLM read).
- **Don't carry over:** the web dashboards (dropped); three separate processes (collapsed
  into one); React + Vite for the panel (vanilla + withGlobalTauri is the proven path here).
