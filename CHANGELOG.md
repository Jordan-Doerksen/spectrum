# Changelog

Phase log for Spectrum. Newest first. Absolute dates (YYYY-MM-DD).

## 2026-09-18 — CR-1 chunk 0: groundwork (observability, a lock, the first tests)

Chunk 0 of the News rework. **No posting behaviour changed.** The feeds, the bands, the
routing, the dedupe key and the card are exactly as D-0004 to D-0013 describe them. This
chunk only makes the engine able to report its own failures, and gives the later chunks a
test harness to land on. The engine was not run: every claim below is held by a test or by
a named file. [DECISIONS.md, CR-1 · observability law]

- **A disk logger** (`crates\engine\src\log.rs`, split into `log\sink.rs` and
  `log\redact.rs`). One JSON object per event to `data\logs\spectrum-{date}-{time}-{pid}.jsonl`,
  plus one readable console line. The path, the two levels, the size cap, the files per run
  and the run files to keep are the `logging` block of `config.local.json` — no constants.
  The run file rotates at the cap and old runs are pruned, so the log cannot fill the disk.
  Every event name and every string value is redacted: a Discord webhook keeps its id and
  loses its token, which matters because the repo is public. Both runners open it, and the
  panel opens it before its engine thread starts.
- **Every silent failure now writes a record** that names the action that did not happen
  and why: a feed fetch, every feed failing at once, an Ollama error (rolled up, so a dead
  Ollama writes two lines and not two thousand), a model answer the analyzer cannot map, a
  store read/parse/save failure, a config load or reload failure, a band with no webhook,
  a queue the drip cannot drain, a poisoned mutex in the panel, and **every card that
  leaves the queue without posting** — dry run, failed POST, or no webhook — with the
  reason on the line. Two summary lines close each cycle: `poll.summary` and `drip.summary`.
- **A single-instance lock for the headless runner** (`crates\headless\src\lock.rs` with
  `lock\liveness.rs` and `lock\record.rs`). A second runner on the host exits with
  `lock.busy` and polls nothing. A lock whose holder is gone is recovered, decided by
  `tasklist` first and by the heartbeat second. Its knobs — the file name, the beat, the
  missed beats, the read retries — are the `lock` block of `config.local.json`. *Gap:* the
  panel keeps its own Tauri lock, so a panel plus a runner on one host still double-post.
- **The first tests.** 88 across the workspace, all offline: no socket, no Ollama, no
  Discord, and no test touches the real `data\` store or `config.local.json`. They cover
  `norm()`, the seen store, every config default, the log file and its cap, the redaction,
  the prune filter, the lock, and the analyzer's coercion.
- **Fixes found by review of this chunk:** `err()` no longer overwrites the `not_doing`
  reason, so a failure record carries the error AND the consequence; the panel's ring is
  redacted, so a failed POST cannot show a live webhook token in the panel window; the log
  pruner matches the log extension and never deletes a `.lock`, so a `logging.dir` of
  `data` cannot delete the single-instance lock; the panel logs a runtime that will not
  build instead of printing to a hidden console.
- **Tools and docs.** `scripts\dev.ps1` puts back the `--` separator PowerShell strips from
  a script's arguments, and exits with cargo's code. The root `STOP.bat` probes all three
  targets before any kill and names each one. `docs\OBSERVABILITY.md` is the second half of
  the observability law: the log location, how to read a line, the triage order, and the
  areas that are still silent.
- No new dependency (D-0003). `.\scripts\dev.ps1 check --workspace`,
  `build --workspace` and `test --workspace` all pass.

## 2026-06-24 — Fix: duplicate posts (post-handoff)
- **Single-instance lock** (`tauri-plugin-single-instance`): a second launch focuses the
  existing window instead of starting a second engine — kills "two copies on one box" dupes
  and makes Stop authoritative.
- **Tighter dedupe:** `norm()` drops a trailing " - Publisher" (Google News) and strips
  punctuation, so the same story from a desk and a sweep collapse to one fingerprint.
- Operational rule: one host only — shared webhooks mean two machines double-post. [D-0013]

## 2026-06-24 — M5: published + portfolio
- Doc set + public repo (github.com/Jordan-Doerksen/spectrum) + Observatory card (head of the
  lineage); the three predecessors archived with "Superseded by Spectrum". [D-0012]

## 2026-06-24 — M4: standalone package for Riley
- Release exe (self-contained: only `WebView2Loader.dll` beyond system DLLs), system tray
  (minimize-to-tray + Start/Stop/Quit menu), `SPECTRUM_AUTOSTART` (boots running into the tray).
- Ollama `SETUP.bat`, `INSTALL`/`UNINSTALL-AUTOSTART.bat`, `START`/`STOP.bat`, `RILEY.md`;
  `deploy/package.ps1` → `Spectrum-for-Riley.zip` (7.13 MB). [D-0011]

## 2026-06-24 — M3: control panel
- Tauri 2 shell (vanilla UI + withGlobalTauri); the engine refactored into a reusable
  `Engine` service run in a background thread → a snapshot; start/stop/status/dry/tuning/log
  commands. Dark cockpit panel. Links clean on the GNU toolchain (cold 4m57s). [D-0010]

## 2026-06-24 — M2: the feed pool
- 34 feeds across four bands (dedicated desks + Google News `when:1d` sweeps), validated
  (34/34 fetch, 2019 items). Local `START.bat` / `STOP.bat`. [D-0008/D-0009]

## 2026-06-24 — M1: the always-on engine
- Seed + persistent dedupe (`data/seen.json`), classify-once, route by category, themed
  cards (four skins), severity floor, drip pacer. Live Discord posting verified.
- Four bands: finance · politics (+tech) · catastrophe. [D-0004 – D-0007]

## 2026-06-24 — M0: toolchain
- Rust GNU toolchain de-risked on this box (the `dlltool`/winlibs PATH incantation; a
  C-free tree via native-tls → SChannel). HTTPS fetch + RSS parse proven. [D-0001 – D-0003]
