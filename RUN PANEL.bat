@echo off
title Spectrum Panel
cd /d "%~dp0"
rem Build + launch the Spectrum control panel (Tauri). First build compiles the
rem webview stack and takes a few minutes; later launches are fast.
set "PATH=%USERPROFILE%\.cargo\bin;%USERPROFILE%\winlibs\mingw64\bin;%PATH%"
set "SPECTRUM_CONFIG=%~dp0config.local.json"
echo Building and launching the Spectrum control panel...
echo (first build takes a few minutes — the webview stack compiles once)
cargo run --release -p spectrum-pro
echo.
echo Panel closed.
pause
