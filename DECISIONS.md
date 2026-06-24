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
