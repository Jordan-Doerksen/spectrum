# Spectrum — Decisions

Append-only. Each entry: the context, the call, the consequence. (Mirrors the
sentinel-pro-v4 discipline; several decisions deliberately reuse v4's proven setup.)

### D-0001 — Rust + a Cargo workspace on the GNU toolchain
**Context:** This box runs the Rust **GNU** toolchain (`stable-x86_64-pc-windows-gnu`,
cargo 1.96) — the same one sentinel-pro-v4 ships on. MSVC would need Visual Studio,
which isn't installed; we do not switch toolchains. Cargo is NOT on the default PATH.
**Call:** GNU toolchain. Workspace = `crates/engine` (the pipeline lib) + `crates/headless`
(pure-server runner); `src-tauri` (the control panel) joins at M3. The engine is a
library so it's unit-testable and runs headless *or* inside the Tauri window.
**Consequence:** Reproducible only via the build incantation (D-0002). Mirrors v4 D-0001.

### D-0002 — The build incantation: `.cargo\bin` + `winlibs\mingw64\bin` on PATH
**Context:** On the GNU target, `windows-sys` / `getrandom` (and later the Tauri
webview stack) link Windows import libraries that mingw-w64's `dlltool` + `windres`
must generate. Those tools live at `%USERPROFILE%\winlibs\mingw64\bin` and are NOT on
PATH by default — without them the build dies with
`error calling dlltool 'dlltool.exe': program not found`.
**Call:** Every build prepends BOTH dirs first:
`$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\winlibs\mingw64\bin;$env:PATH"`
`scripts/dev.ps1` wraps it. Mirrors v4 D-0023.
**Consequence:** Clean builds on this host. **M0 verified 2026-06-24:** 8/8 feeds
fetched, 237 items, cold build 1m37s.

### D-0003 — A C-free dependency tree; reqwest on native-tls → SChannel
**Context:** reqwest 0.13's default TLS is rustls + **aws-lc-rs**, which compiles
AWS-LC (needs a C compiler + NASM). We keep the tree C-free so a build never depends
on a C toolchain being found on PATH.
**Call:** `reqwest = { default-features = false, features = ["native-tls", "gzip"] }`
→ on Windows that's the pure-Rust `schannel` crate (the OS TLS, no OpenSSL, no `cc`).
feed-rs, tokio, serde, scraper are all pure Rust.
**Consequence:** No C build step; the whole engine is HTTP through one reqwest client
(RSS now; Ollama + Discord next), so this one choice de-risks all networking. Mirrors v4 D-0016.

### D-0004 — One pipeline, route by category (not three engines)
**Context:** Replacing doomfeed (crisis) + macroscope (markets) + richter (world/political),
which ran as three separate processes with three dedupe stores. The three Discord
channels wanted — **financial+political**, **technology**, **catastrophe/crisis/war** —
do not map 1:1 to those engines (Technology is a brand-new band none of them covered).
**Call:** Pool all feeds, fetch once, one cheap prefilter + one cross-feed dedupe, then
**one** Ollama pass per item returns `{ category, read, severity }`. `category` picks the
webhook; one primary category per item (no cross-posting). Dashboards are dropped.
**Consequence:** One fetch, one LLM read per unique story, one dedupe store, three
webhooks. Kills the 3× polling/LLM waste and the cross-engine duplicate-post risk.

### D-0005 — Four bands, not three: finance and politics are separate channels
**Context:** At webhook time the operator provided three DISTINCT channels — money,
politics, catastrophe — splitting the originally-combined "financial+political" band,
and (so far) no technology channel.
**Call:** Bands are now **Financial · Political · Technology · Catastrophe** (+ Drop).
Webhooks map by band key in `config.local.json`. The analyzer prompt distinguishes
finance vs politics by the frame the headline leads with. Technology is built + routed
but **held** (no webhook) pending the operator's call: add a 4th channel, drop tech, or
fold it into finance/politics.
**Consequence:** `Category` has four live variants. **M1 verified LIVE 2026-06-24:**
themed cards POSTed to all three configured channels (Finance/Politics/Crisis), Tech held.

### D-0006 — Technology folded into the Political channel
**Context:** Operator's call — no separate tech channel; fold tech into political.
**Call:** The analyzer still *detects* `Technology` (keeps the distinction + the 🔬 TECH
skin), but the `technology` webhook key points at the POLITICS channel in
`config.local.json` — tech cards post into the politics channel wearing the TECH skin.
**Consequence:** Three live Discord channels: money · politics (+tech) · catastrophe.
Tech stays a first-class category in the engine, so splitting it back out later is a
one-line config change (give `technology` its own webhook). No recompile — config only.

### D-0007 — The always-on loop: seed the backlog, classify-once, drip from a queue
**Context:** Turning the one-shot into a continuous engine without (a) dumping history
on day one, (b) re-reading the same headline every poll, or (c) bursting cards.
**Call:** A persistent `data/seen.json` (insertion-ordered, capped 4000) holds
fingerprints (normalized titles). **First run** seeds every current headline as seen and
posts nothing. Each poll classifies only UNSEEN items and marks them seen once read (any
outcome) — so each headline hits the LLM exactly once. Cleared cards (category≠Drop,
severity≥floor) enter an in-memory queue, strongest first; a drip tick posts
≤`max_per_drop` every `drip_seconds`. `--once` = one cycle (testing/cron); `--dry` = never post.
**Consequence:** No backlog dump, no re-classification, no bursts. Trade-off: a restart
loses the in-memory queue (those cards are already marked seen, so they won't post) —
acceptable for V1; persist the queue if it ever bites. **Verified 2026-06-24:** seed
(226, 0 posts) · dedup (0 new) · classify+drip (new items → queued → dry-posted), zero spam.

### D-0008 — The feed pool: 34 sources across 4 bands (dedicated desks + Google News sweeps)
**Context:** The starter list was 8 feeds; the real engine needs comprehensive, balanced coverage.
**Call:** `feeds::feeds()` returns 34 sources — the union of macroscope's markets desks +
richter's world/politics desks, plus targeted Google News `when:1d` topic searches per
band (Fed/CPI/oil/gold/crypto · elections/policy · conflict/disaster/unrest · AI/big-tech)
and a real tech desk set (Ars · Verge · HN · Wired · Engadget · TechCrunch · The Register).
A function, not a const, because gnews URLs are composed at runtime. `--feedcheck` validates it.
**Consequence:** Verified 2026-06-24 — **34/34 feeds fetch, 2019 items.** Per-poll LLM
load stays bounded by classify-once + the seen-store (first seed marks ~2000 items seen
WITHOUT classifying). A lexicon prefilter is the lever if gnews churn makes polls too heavy.

### D-0010 — The control panel: Tauri 2 shell, vanilla UI + withGlobalTauri (React deferred)
**Context:** M3 — the panel the Rust+Tauri stack was chosen for. v4 proved that on this
GNU box the friction-free path is a vanilla static UI + `withGlobalTauri` (its lib.rs
notes the webview stack is finicky here — test binaries can't even init it), NOT Vite/React.
**Call:** Mirror v4 exactly. `src-tauri/` is a Tauri 2 shell; the `Engine` runs in a
background thread with its own Tokio runtime, publishing a status+log snapshot into
`Arc<Mutex<>>`; commands (`status`/`start`/`stop`/`recent_log`/`set_dry`/`get_tuning`/
`set_tuning`) only READ the snapshot + push control flags, so a long poll never freezes
the UI. UI is vanilla `ui/index.html`+`styles.css`+`app.js` (dark cockpit) via
withGlobalTauri invoke. React-via-Vite is a drop-in `frontendDist` swap if the panel grows.
**Consequence:** Links clean on the GNU toolchain (Tauri 2.11.3 / wry 0.55 / webview2-com),
cold build 4m57s. Engine logic stays in the engine crate (testable); the shell is thin
glue. Visual/runtime confirm is operator-side — a native window can't be seen from the
build agent (the visual-verify-loop rule).

### D-0009 — Local launcher: START.bat / STOP.bat (run without the terminal)
**Context:** Hard rule — anything the operator runs himself ships with a double-click
launcher; making him type the cargo incantation is a failed handoff.
**Call:** `START.bat` prepends the toolchain dirs (D-0002), sets `SPECTRUM_CONFIG`, and
runs `cargo run --release -p spectrum-headless`. `STOP.bat` kills it by window title + exe.
This is the LOCAL/dev launcher; M4 produces the standalone exe + Ollama SETUP for Riley.

### D-0011 — Standalone package for Riley: self-contained exe + Ollama SETUP + autostart
**Context:** M4 — get Spectrum onto Riley's always-on box without the dev toolchain. Hard
rules: ship double-click launchers, bundle every dep, keep the webhook in a private deploy.
**Call:** `deploy/package.ps1` assembles `Spectrum-for-Riley.zip` (→ Downloads): the release
exe (22.6 MB) + `WebView2Loader.dll` — its ONLY non-system dependency (objdump confirmed the
GNU build static-links libgcc/winpthread, no C++, TLS via system SChannel) — + `config.local.json`
(webhooks kept) + the macroscope-pattern launchers + `RILEY.md`. `SPECTRUM_AUTOSTART=1` (set by
START.bat) makes the panel auto-run and boot hidden into the tray. `SETUP.bat` auto-installs
Ollama + pulls llama3.1:8b; `INSTALL-AUTOSTART.bat` adds the Startup shortcut + starts it now.
**Consequence:** 7.13 MB zip, unzip→run, no manual dep installs. WebView2 runtime assumed present
(Win10/11). Single-host like macroscope: only Riley runs it live, or copies double-post the shared
channels. SETUP + autostart reuse macroscope's verified pattern; the clean-box end-to-end is Riley-side.

### D-0012 — M5: published public, on the Observatory, the predecessors retired
**Context:** M5 — the north-star wrap: a standalone repo + the portfolio + retiring the three engines this one replaces.
**Call:** Full doc set (README/SPEC/HANDOFF/CHANGELOG/DECISIONS/LICENSE/config.example.json),
`git init` + first commit (secrets gitignored — config.local.json + data/ untracked), pushed
**PUBLIC** to github.com/Jordan-Doerksen/spectrum (operator-confirmed visibility). Added the Spectrum
card to the live Observatory as the head of the news-engine lineage. Retired doomfeed/macroscope/richter:
kept their portfolio cards (the lineage tells the story) but archived the GitHub repos + set
"Superseded by Spectrum" descriptions.
**Consequence:** Lineage Doomfeed → Macroscope → Richter → Spectrum, one live engine. **Project complete (M0–M5).**

### D-0013 — Fix duplicate posts: single-instance + cross-source dedupe (+ the one-host rule)
**Context:** Live dupes after handoff. Two causes: (1) two instances on one box (e.g. the
panel plus the package's hidden-tray `START.bat` instance) both polling and racing the same
`seen.json` — and Stop on one couldn't stop the other; (2) overlapping feeds posting the same
story with slightly different titles (Google News appends " - Publisher"), which the
exact-title fingerprint missed.
**Call:** (1) `tauri-plugin-single-instance` (registered first) — a 2nd launch focuses the
existing window, never starts a 2nd engine. (2) `norm()` drops a trailing " - Publisher" and
strips punctuation before fingerprinting. (3) Operational: ONE host runs live — shared webhooks
mean two machines double-post regardless; the other runs Dry, or not at all.
**Consequence:** No double-instance dupes; near-dupe sweeps collapse. Genuinely different wording
for the same event still slips (semantic dedupe = future). Changing the fingerprint makes old
`seen.json` keys stale → delete it for a fresh seed on the fixed build.

---

## How to change this file (added 2026-09-18 with CR-1)

Entries D-0001 to D-0013 above are unchanged. They were written before this block existed, so
they carry no Change Rule of their own. Rule 2 below is their Change Rule.

1. **Every decision from CR-1 forward carries a Change Rule.** The Change Rule says who can
   change the decision, and what evidence a change needs. An owner decision changes only on the
   owner's word. An agent default changes on evidence, and the change goes in the Change Log.
2. **A change to behaviour, config, risk, execution, data flow, or dependencies is a Change
   Request. It is not a silent edit.** This includes a new crate, a new feed, a new field on a
   Discord card, a new threshold, and any change to what posts or does not post.
3. **A CR records four things:** the context, the call, the consequence, and what it reverses.
   "What it reverses" names every earlier decision ID that the CR cancels, and separately every
   ID that it only touches.
4. **The file stays append-only.** A CR does not edit an earlier entry. When an earlier entry
   stops matching the code, the CR records the correction and the original text stays.
5. **Every CR adds a row to the Change Log.**

## Change Log

| Date | Entry | What changed | Reverses / touches | Filed by |
|---|---|---|---|---|
| 2026-09-18 | Change Rule + Change Log | This file gained a change procedure and this log. No decision text changed. | none | agent |
| 2026-09-18 | CR-1 | The News rework. Decisions S1 to S12, as the owner locked them between 2026-09-13 and 2026-09-18. | Reverses D-0004, D-0005, D-0006, D-0013. Touches D-0007, D-0008, D-0010, D-0011, D-0012. Bound by D-0003. | agent, from the owner's rulings |
| 2026-09-18 | CR-1 chunk 0 — groundwork | **Config.** Two new optional blocks in `config.local.json`, each with the current values as defaults, so an existing config keeps working: `logging` (enabled, dir, file_pattern, file_level, console_level, max_file_bytes, max_files_per_run, keep_run_files) and `lock` (file_name, beat_seconds, missed_beats, read_tries, read_retry_ms) — the lock knobs were constants in `crates\headless\src\lock.rs` and `lock\record.rs`, and the stale window decides when a second copy may take the lock and double-post, so it belongs in `.json`. **New files on disk.** A run log at `data\logs\spectrum-{date}-{time}-{pid}.jsonl`, and the single-instance lock at `data\spectrum-headless.lock`. Both are inside the gitignored `data\`. **New API surface.** `spectrum_engine::log` (the whole module), `spectrum_engine::config::LockConfig`, `spectrum_engine::engine::norm` (the fingerprint, made public for its test), and `analyze::analyze` now returns `(Read, Unmapped)` so an answer the map cannot read is reported instead of silently degraded. **Status.** Four fields added for the panel: `dropped_total`, `dropped_dry`, `dropped_no_webhook`, `dropped_post_failed`. **One human string changed:** a card with no webhook reads "card DROPPED", not "held" — "held" was false, the card is gone. **Module split** (house law, review at 300 lines): `engine.rs` into `engine/{gather,poll,drip,cycle,keys}.rs`, `log.rs` into `log/{sink,redact}.rs`, `lock.rs` into `lock/{liveness,record}.rs`. **Tools and docs.** `scripts\dev.ps1` puts back the `--` separator PowerShell strips, and exits with cargo's code; the root `STOP.bat` probes and kills all three targets; `docs\OBSERVABILITY.md` is the bug-identification-and-fix plan. **Known gap recorded, not fixed:** the headless lock covers headless copies only — `spectrum-pro.exe` keeps its own Tauri lock and never touches the file, so a panel and a runner on one host both poll and both post. The runner logs `runner.panel.running` when it sees the panel. The fix is one lock in the engine crate, taken where the poll loop starts, which is `src-tauri`. It is proposed for chunk 5 beside the `deploy\STOP.bat` gap; the owner can move it. | Touches D-0007 (the store and the queue are now instrumented, not changed), D-0009 (STOP.bat), D-0010 (the panel gains a log and recovers a poisoned mutex), D-0013 (the headless half of the single-instance gap). No posting behaviour changed. No new dependency (D-0003). | agent |

---

# CR-1 — Spectrum is the only News engine

**Filed:** 2026-09-18. **Status:** approved by the owner. Built in chunks. Chunk 0 in progress.
**Source of the decisions:** `C:\projects\bots\discord-servers\docs\news-signals-plan\SPECTRUM-CR-DRAFT.md`
(S1 to S12). **Evidence:** `BRIEF.md` sections 6, 7, 8 and 11 in the same folder, and
`map-spectrum-pipeline.md`, a read-only survey of this repo dated 2026-09-12.

## Context

The owner ruled on 2026-09-12 and 2026-09-13 that every automated Discord post belongs to one of
two groups. **Signals** is trading-desk alerts only. **News** is every other tool post. Spectrum
becomes the only News engine. News has no split by domain: its goal is what is trending and what
just dropped. Dedupe must be tighter. Cards must be cleaner and as large as the API permits.
Spectrum also takes over nightdesk's 15-minute economic calendar warnings as an organized domain.

The current engine does not do this. It routes by an LLM band into one webhook per band
(D-0004 to D-0006), it judges each headline alone with no count of outlets, and it keys dedupe on
a normalized title in a FIFO store with no time window (D-0007, D-0013).

## The call — decisions S1 to S12

| # | Decision | The call | Locked | What it reverses or touches |
|---|---|---|---|---|
| S1 | News engine | Spectrum only (discord-servers Decision 3.4). | 2026-09-13 | Sets the scope of this CR. Touches **D-0012**: the repo is public, so the feed list and the card JSON become public. |
| S2 | nightdesk jobs that move | Calendar warnings move to Spectrum. The briefing and the tape board stay on nightdesk's desk page. nightdesk's headline, online and watchdog posts end. | 2026-09-13 | Adds a domain that no decision in this file covers. The other half needs a nightdesk CR. |
| S3 | Calendar actuals | Deferred until the owner approves a licensed source. Warnings come from the ForexFactory weekly JSON (unofficial, no KRW). | 2026-09-13 | Nothing earlier. It bounds the calendar domain to warnings only. |
| S4 | Trending method | Count distinct outlets per story cluster (`BRIEF.md` §6, Option A, V1 slice). No LLM ranks stories. One week of shadow mode, logs only, before the first post. | 2026-09-13 | **Reverses D-0004, D-0005 and D-0006**: an LLM band no longer selects a webhook, and per-headline severity no longer selects a card. **Touches D-0008**: the feed pool becomes the input to a count, so the band hint has no reader. **Touches D-0007**: queue order comes from a score, not from severity. |
| S5 | News channel | One `#news` channel in Desk (discord-servers Decision 3.2). | 2026-09-13 | **Reverses D-0005 and D-0006**: the webhook map keyed by band ends, and with it the three-channel layout. **Touches D-0010**: the panel counts posts per band. |
| S6 | V1 source set | Trimmed. Cut the 15 Google News keyword searches and 5 noisy feeds (MarketWatch, Guardian Politics, Wired, Engadget, Hacker News). Add Google News top stories, USGS earthquakes, and nightdesk's investingLive news, investingLive central banks and Bloomberg Markets feeds. Social sources (GDELT, Bluesky, HN API, NWS, Mastodon) stay off behind a flag. | 2026-09-13 | **Reverses D-0004, D-0005 and D-0006** with S4. **Touches D-0008**: the pool is no longer 34 feeds in four bands, and the lexicon prefilter named there is no longer the load lever. |
| S7 | The LLM's role | The LLM reads card candidates only. After the scorer selects a story to post, one call writes a one-line read and checks that the story is real news, not an advertisement or an explainer. About one call per card, with a short `keep_alive` so the model unloads. The per-item read ends. | 2026-09-14 | **Reverses the D-0004 model** of one LLM read per unique item. **Touches D-0007**: "classify once per headline" becomes "read once per card". Touches the hardcoded model at `crates\engine\src\analyze.rs:56`. |
| S8 | A story that keeps growing | The card updates in place: the outlet count, and the tier label from BREAKING to TRENDING. It never posts again. This needs `?wait=true` to store the message id, PATCH with the same webhook token, at most 1 edit per story per 5 min, a cap of 10 edits, and a check that the returned body matches what was sent. Test in a private channel whether an edited card holds its position. | 2026-09-14 | **Touches D-0007**: a story and message ledger joins the dedupe store. **Touches D-0011**: only the same token can edit a message, so one poster per stream is now a design constraint, not only an operating rule. |
| S9 | News notifications | BREAKING cards and calendar warnings post with normal notifications. TRENDING cards post with `SUPPRESS_NOTIFICATIONS` (flag 4096). Card updates never notify. The flag is fixed at send time, so a card that posts silent stays silent. Jordan and Riley set `#news` to All Messages in their own clients. | 2026-09-14 | New behaviour. No earlier decision covers notification flags. |
| S10 | Time labels on cards | Discord timestamp markup (`<t:unix:t>`), which shows each reader a local time. No hardcoded zone label. | 2026-09-14 | New behaviour. The current card carries no timestamp (`crates\engine\src\discord.rs:9-27`). |
| S11 | Calendar warning filter | Every ForexFactory currency: USD, EUR, GBP, JPY, CNY, AUD, NZD, CAD, CHF, at medium and high impact. nightdesk's low-impact exception for JPY and CNY is not carried. KRW is not available from ForexFactory. The filter lives in `.json`. | 2026-09-14 | New domain. The `.json` location follows the house law on config. |
| S12 | News card format | Components V2 container: accent bar, kicker line, one full-width image, large headline, one-line read, link, separator, outlet line. No image means no gallery item, and never a placeholder. Signals keeps the classic embed, because a role ping needs a `content` field, which Components V2 forbids. | mockup approved 2026-09-18 | **Touches D-0010**: the panel's per-band counters and tiles report a band split that ends with S5. The four skins, the band label and the severity meter go with the band split already reversed under S4 and S6. |

## Agent defaults inside CR-1

These were stated to the owner on 2026-09-13 and 2026-09-14. Each is an execution decision, and
each can change on his word or on evidence.

- **Dedupe depth.** Fix the story key, add a URL key, match by token overlap, use a 48 h window,
  and keep one card per story (`BRIEF.md` §7, F10 a). Embedding matching stays off until the
  shadow logs show a real gray-zone rate.
  **Reverses D-0013.** The `norm()` fingerprint at `crates\engine\src\engine.rs:193-206` is
  replaced, so every key in `data\seen.json` becomes stale. D-0013 already states the
  consequence: delete the store and seed again. The re-seed posts nothing, and it happens on the
  new build on the live host, not on this machine. **Touches D-0007** (the FIFO cap of 4000
  gives way to a time window) and **D-0003** (the candidate crates `unicode-normalization` and
  `url` must be pure Rust, and each one must be named in a CR with its reason before it is added).
- **Volume.** A score threshold plus a hard cap in `.json`, starting at 6 News cards per hour,
  tuned from the shadow week (F14 a). The value 6 is a guess with no measurement behind it.
  **Touches D-0007**, the drip pacer.
- **Order of work.** A disk logger and the logging of every silent failure (`BRIEF.md` §11) come
  before any new feature. That order is chunk 0 below.
- **Dirty trees.** The owner commits nightdesk CR-22 and pushes Spectrum `b90ac5f` before CR work
  starts. An agent does not commit and does not push (F15 a).

## Change Rule for CR-1

- **S1 to S12 are owner decisions.** An agent does not change one. A change needs a new CR and
  the owner's word.
- **The agent defaults above are execution decisions.** An agent can change one on evidence from
  the shadow logs. The change goes in the Change Log with its evidence.
- **Every threshold, weight, interval, cap and filter in this CR lives in `.json`.** Changing a
  value is not a CR. Changing what a value controls is a CR.
- **A new dependency is a CR** (D-0003). It must be pure Rust, and the CR names it and its reason.

## What CR-1 reverses and touches, in one view

| ID | Lines | Status under CR-1 |
|---|---|---|
| D-0003 | `:27-35` | **Binds.** The tree stays C-free. Every new crate is named in a CR. |
| D-0004 | `:37-46` | **Reversed** by S4, S6 and S7: routing by LLM category, and one read per item. |
| D-0005 | `:48-58` | **Reversed** by S4, S5 and S6: four bands become one News stream. |
| D-0006 | `:60-67` | **Reversed** by S5: the `technology` to politics mapping ends with the band map. |
| D-0007 | `:69-81` | **Touched** by S4, S7, S8 and the volume default: a time window, a cluster and message ledger, and a score order. |
| D-0008 | `:83-92` | **Touched** by S4 and S6: the 34-feed pool is trimmed and added to, and the band hint has no reader. |
| D-0010 | `:94-107` | **Touched** by S5 and S12: the per-band counters (`crates\engine\src\engine.rs:30-33`) and the panel tiles (`ui\index.html:28-31`) report a split that ends. |
| D-0011 | `:116-127` | **Kept, and now binding.** One host posts. S8 makes this a design constraint, because only the same token can edit a card. |
| D-0012 | `:129-137` | **Touched** by S1: the repo is public, so the feed list, the card JSON and any mockup in the repo are public. |
| D-0013 | `:139-151` | **Reversed** by the dedupe-depth default: a new story key. `data\seen.json` becomes stale and needs a delete and a fresh seed on the live host. |

## Corrections of record

D-0001 to D-0013 stay as written (rule 4). These statements in them do not match the code. Each
was verified on 2026-09-18 by reading the source in this repo.

| Where | What the entry says | What the code does |
|---|---|---|
| D-0004 `:42` | "one cheap prefilter" | No prefilter exists. The only filters are an empty title, a duplicate inside one batch, and an already-seen key (`engine.rs:95-99,115`). `crates\engine\src\lib.rs:4` repeats the claim. |
| D-0007 `:74-75` | An item is marked seen "once read (any outcome)" | An item is marked only when `analyze` returns `Ok` (`engine.rs:118-119`). With Ollama down, every poll retries every unseen item. |
| D-0007 `:78-80` | A restart is the only way a cleared card is lost | A dry run, a failed POST and a missing webhook also lose the card. The card is removed from the queue first (`engine.rs:142`), so the log line "held" at `:158` means dropped. |
| D-0013 `:145-147` | The single-instance lock ends double-instance duplicates | The lock is in the Tauri exe only. `crates\headless\src\main.rs` has no lock, and the root `STOP.bat` does not kill `spectrum-pro.exe`. **Note, 2026-09-18 (chunk 0):** both halves of this line changed inside this chunk. The headless runner now takes a lock (`crates\headless\src\lock.rs`), and the root `STOP.bat` probes all three targets — the launcher window, `spectrum-headless.exe` and `spectrum-pro.exe` — names each one, and exits non-zero when a kill fails (`STOP.bat:18-20,51-63,74`). What remains true: the panel and a runner on one host still both post, because the panel's Tauri lock and the runner's file lock do not see each other. The surviving STOP gap is `deploy\STOP.bat:3`, Riley's packaged copy, which is chunk 5. |
| D-0013 `:150-151` | Delete `seen.json` after the fingerprint change | The store is still on disk in the pre-fix format: 2281 keys, of which 2119 hold punctuation and 1702 hold `" - "`. A run on this machine would send about 2000 LLM reads and then post. |
| D-0003 `:31-33` | reqwest features `["native-tls","gzip"]`; `scraper` is a dependency | `crates\engine\Cargo.toml:16` uses `["native-tls","gzip","json"]`. `scraper` is not a dependency. |
| SPEC §4, `config.rs:11-12` | The `model` key selects the model | Nothing reads it. `analyze.rs:56` hardcodes `llama3.1:8b`. |

> **Note, 2026-09-18 (chunk 0).** Every claim in the table above was read again at the end of
> chunk 0 and each one still holds. Three of the *references* moved, because chunk 0 split the
> files and changed one string. The original text stays under rule 4.
>
> * **The line numbers point at the pre-split files.** `engine.rs` is now the service object
>   plus `engine\{gather,poll,drip,cycle,keys}.rs`; `norm()` is `engine\keys.rs`; the analyzer
>   read and the seen-marking are `engine\poll.rs`; the three lost-card paths are
>   `engine\drip.rs`; the panel mutexes are still `src-tauri\src\lib.rs`. `analyze.rs:56` is
>   now `analyze.rs:89`, and the model is still hardcoded there.
> * **The log line that read `"held"` now reads `"card DROPPED"`** (`engine\drip.rs:85`). The
>   D-0007 row says "held" means dropped; the string was corrected in chunk 0 so that it says
>   what it means. The behaviour behind it is unchanged — the card is still lost.
> * **What did NOT change:** there is still no prefilter, an item is still marked seen only
>   when `analyze` returns `Ok`, a dry run and a failed POST and a missing webhook still lose
>   the card, `data\seen.json` is still the stale 2281-key file, and the reqwest features are
>   still `["native-tls","gzip","json"]` with no `scraper`.

## The chunks

CR-1 is built in chunks, and each chunk is a separate build with its own cross-check. The shape
below comes from the draft. Only chunk 0 is started.

| Chunk | Contents | Status |
|---|---|---|
| **0 Groundwork** | Disk logger; failure logging; Change Rules, a Change Log and this CR section in DECISIONS.md; the first tests; a single-instance lock for headless. | In progress (2026-09-18) |
| 1 Calendar | ForexFactory fetch with a JSON body check and a last-good copy; the filter in `.json`; the 15-minute warning; a warned ledger. | Not started |
| 2 Story store | Item model v2 (pubDate, source); the key fix; the URL key; token-overlap matching; the 48 h window. | Not started |
| 3 Trending | The cluster scorer, the tier rules, publisher weights; shadow mode, logs only. | Not started |
| 4 Cards and delivery | The card formatter; the story and message ledger; edit in place; rate-limit handling; a mockup for the owner first. | Not started |
| 5 Ship | The package for Riley; the store re-seed; the new `#news` webhook; the nightdesk CR that turns its posts off. **Carried here from chunk 0:** `deploy\STOP.bat`, Riley's packaged copy, which still kills the headless process only; and — proposed, the owner can move it — one single-instance lock in the engine crate, because the chunk-0 lock covers headless copies only and a panel plus a runner on one host still double-post (`runner.panel.running` is the detection). | Not started |

**Chunk 0 changes no posting behaviour.** The feeds, the bands, the routing, the dedupe key and
the card stay exactly as D-0004 to D-0013 describe them. Chunk 0 only makes the engine able to
report its own failures, and gives the later chunks a test harness to land on. Nothing in S1 to
S12 is built in chunk 0.

## Definition of Done — chunk 0

Chunk 0 is done when every line below is true. The evidence is a test run or a named file, not a
claim.

**1. The disk logger exists and both runners use it.**
- Structured lines with a UTC timestamp, a level and an event name, written to a file under
  `data\logs\`. The path, the level and the size cap are keys in `.json`, not constants.
- The headless runner and the panel both write to it. The in-memory ring of 200 lines
  (`engine.rs:73-78`) stays, because the panel reads it.
- No `println!` or `eprintln!` carries information that the log file does not carry. The release
  panel hides the console (`src-tauri\src\main.rs:4`), so a console line there is invisible.
- The log file rotates or truncates at the cap. It never grows without a limit.

**2. Every silent failure writes a record.** Each of these paths writes a line that names the
action that did not happen, and why:
- a feed fetch error (`engine.rs:97`), with the feed name;
- an LLM error (`engine.rs:118`), with a note that the item stays unseen and will be retried;
- a seen-store save error (`engine.rs:86,127`, `store.rs:53`);
- a config reload error (`engine.rs:63-66`), with a note that the old config stays live;
- a corrupt `seen.json` (`store.rs:21`), which today falls back to an empty store and triggers a
  silent re-seed. The record must say that a re-seed follows;
- a poisoned mutex in the panel (`src-tauri\src\lib.rs:43,49`);
- every card that leaves the queue without posting: dry run, failed POST, and no webhook
  (`engine.rs:142-158`). The line says which of the three it was.
- A poll writes a summary line: feeds fetched, feeds failed, items gathered, items skipped as
  seen, items that failed classification, items queued.

**3. The headless runner has a single-instance lock.** A second headless process on the same host
exits with a log line and does not poll. It does not race `data\seen.json`.
*Known gap, carried and not fixed in chunk 0:* the root `STOP.bat` kills the headless process
only, so it does not stop `spectrum-pro.exe`. Chunk 0 records this. A fix belongs to chunk 5.

> **Note, 2026-09-18 (chunk 0).** The gap above stopped being true inside this chunk, and the
> original text stays under rule 4. The root `STOP.bat` was fixed here: it probes the launcher
> window, `spectrum-headless.exe` and `spectrum-pro.exe` before any kill, reports each one by
> name, and exits non-zero when a kill fails (`STOP.bat:18-20,51-63,74`). **The gap that is
> still open** is `deploy\STOP.bat:3` — Riley's packaged copy, which is rebuilt in chunk 5.
>
> **A second gap belongs beside it, and it is the original D-0013 symptom.** The lock in this
> chunk covers headless copies only. `spectrum-pro.exe` keeps its own Tauri single-instance
> lock and never touches the lock file, so a panel and a runner on one host both poll and both
> post. The runner detects the case and logs `runner.panel.running`
> (`crates\headless\src\main.rs:69-77`); it does not refuse, because an open panel is not a
> running engine. The fix is one lock in the engine crate, taken where the poll loop starts,
> which is `src-tauri`. It is an execution decision with a design edge — it can stop a panel
> from starting — so it is proposed for chunk 5 with the packaged `STOP.bat`, and the owner
> can move it to a chunk of its own.

**4. Tests exist and pass offline.**
- `.\scripts\dev.ps1 test --workspace` passes. The repo had no test before this chunk: a search
  for `#[test]` and `cfg(test)` in `crates\` and `src-tauri\src\` returned nothing on 2026-09-18.
- No test opens a network socket, and no test calls Ollama or Discord.
- No test reads or writes the real `data\` store or `config.local.json`. Each test that needs a
  file uses a temporary directory.
- The first tests cover: `norm()`, including the `" - "` cut and the punctuation strip; the store
  (insert, contains, the 4000 cap and FIFO eviction, and a corrupt file); the config defaults;
  and the logger (it writes a line, and it respects the cap).

**5. The build is clean through the wrapper.** `.\scripts\dev.ps1 check --workspace` and
`.\scripts\dev.ps1 build --workspace` both pass, with the D-0002 PATH prepend. No new dependency
was added. If one was needed, it is pure Rust and it is named in this CR with its reason (D-0003).

**6. The bug-identification-and-fix plan is written.** The second half of the observability law:
`docs\OBSERVABILITY.md` in this repo gives the log location, the triage order, how to read a log
line, and the areas that are still silent.

**7. The documents match the code.** The Change Log has a row for chunk 0. `CHANGELOG.md` has an
entry. `HANDOFF.md` points a new agent at this CR. The corrections of record above stay true.

**8. The engine was not run.** Chunk 0 is verified by tests and by reading, never by a live poll.
No `cargo run`, no panel, no headless run. `data\seen.json` is not deleted in chunk 0: the delete
and the re-seed belong to the new build on the live host (chunk 5). A run on this machine would
send about 2000 LLM reads and then post to the live channels.

## Open questions

- The manifest still has no **Core Goal** and no **Non-Negotiable Constraints** block, which the
  design-first law requires. Adding them states owner-level intent, so it is a separate CR and
  the owner's call, not an agent edit.
- Riley's host is unchecked: the GPU and VRAM for `llama3.1:8b`, whether Spectrum runs there now,
  the state of its `seen.json`, and whether an old macroscope or richter autostart still exists
  (`BRIEF.md` §11). S7 depends on the GPU answer.
- Whether an edited card holds its position in the channel is unverified. S8 needs a private
  channel test before chunk 4 ships.
- The trending track joins a cluster at Jaccard 0.5 and the dedupe track matches at 0.6, or 0.4
  with two anchor tokens. No value is calibrated for headlines. One value is picked in shadow mode.
