@echo off
rem Launch Spectrum. The engine auto-runs and the app lives in the system tray.
cd /d "%~dp0"
set "SPECTRUM_CONFIG=%~dp0config.local.json"
set "SPECTRUM_AUTOSTART=1"
start "" "%~dp0spectrum-pro.exe"
