# Spectrum — Build Handoff

> Read SPEC.md for the full design and DECISIONS.md for the why. This is the kickoff:
> how to stand up and build it. Home repo: `spectrum` (`C:\projects\tools\spectrum`).

> **Progress (as built, 2026-06-24):** Complete. Engine + live Discord posting + control
> panel + a self-contained Riley package, all verified. M0–M4 done; M5 (this doc set +
> the Observatory entry) in progress.

---

## ⚑ READ THIS FIRST — CR-1 is in progress (2026-09-18)

**The current work is CR-1, not M5.** Spectrum becomes the only News engine: one `#news`
channel, ranking by the count of distinct outlets, one card per story edited in place, the
15-minute economic calendar warnings taken over from nightdesk, and the Google News keyword
sweeps dropped. **Read `DECISIONS.md` before you touch anything** — the CR-1 section holds
decisions S1 to S12, the agent defaults, the corrections of record (statements in D-0001 to
D-0013 that the code does not match), the chunk list, and the Definition of Done for the
chunk in progress.

**The current chunk is chunk 0, groundwork.** A disk logger, a record for every silent
failure, a single-instance lock for the headless runner, the first tests, and
`docs\OBSERVABILITY.md`. Chunk 0 changes no posting behaviour. Nothing in S1 to S12 is
built yet.

**⛔ DO NOT RUN THE ENGINE ON THIS MACHINE.** `data\seen.json` is stale: it holds 2281 keys
in the pre-fix fingerprint format. One poll would send about 2000 headlines to Ollama and
then post to the **live** Discord channels. Use `.\scripts\dev.ps1 check`, `build` and
`test` only. The delete and the fresh re-seed belong to the new build on the live host,
which is chunk 5 — not here, and not now.

**Where things are.** The logs: `data\logs\`, and `docs\OBSERVABILITY.md` says how to read
them. The tests: `.\scripts\dev.ps1 test --workspace`, all offline. The config: copy
`config.example.json`; the `logging` and `lock` blocks are optional and documented inside it.

**Do not commit, do not push, do not branch.** The owner does that (CR-1, agent default
"Dirty trees").

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
  Split by domain in CR-1 chunk 0: `engine/{gather,poll,drip,cycle,keys}.rs` for the
  cycle, and `log.rs` + `log/{sink,redact}.rs` for the disk logger.
- `crates/headless` — a thin runner over `Engine` (`--once`, `--dry`, `--feedcheck`), plus
  the single-instance lock: `lock.rs` + `lock/{liveness,record}.rs`.
- `src-tauri` — the Tauri 2 shell: the engine in a background thread → a snapshot →
  withGlobalTauri commands (start/stop/status/dry/tuning/log). Vanilla `ui/`.
- `crates/engine/tests` — the offline test suite. No socket, no Ollama, no Discord, and no
  test reads the real `data\` store or `config.local.json`.
- `docs/OBSERVABILITY.md` — where the logs are, how to read one, the triage order, and the
  areas that are still silent.

## Build / run

Safe here, and the only three you need while CR-1 is in progress:
- `.\scripts\dev.ps1 check --workspace` · `build --workspace` · `test --workspace`.

⛔ **The three lines below RUN THE LIVE ENGINE. Do not use them on this machine** while
`data\seen.json` is stale — see the CR-1 block at the top of this file. They are recorded
for the live host.
- Panel: `RUN PANEL.bat` (or `.\scripts\dev.ps1 run -p spectrum-pro`). **Posts to live Discord.**
- Headless: `.\scripts\dev.ps1 run -p spectrum-headless` (`--dry` to not post). **Posts to
  live Discord without `--dry`, and `--dry` still marks every headline seen, which loses
  them for good.**
- Validate feeds: `.\scripts\dev.ps1 run -p spectrum-headless -- --feedcheck`. This one
  fetches feeds only — no store, no lock, no Ollama, no posting — but it is still network
  traffic from this machine.
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
