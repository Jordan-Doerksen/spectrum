# Changelog

Phase log for Spectrum. Newest first. Absolute dates (YYYY-MM-DD).

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
