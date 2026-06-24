@echo off
title Spectrum Engine
cd /d "%~dp0"
rem Spectrum local launcher — prepends the GNU toolchain dirs (DECISIONS D-0002),
rem points at the gitignored webhook config, and runs the always-on engine.
set "PATH=%USERPROFILE%\.cargo\bin;%USERPROFILE%\winlibs\mingw64\bin;%PATH%"
set "SPECTRUM_CONFIG=%~dp0config.local.json"
echo Starting Spectrum news engine...
echo First run seeds the backlog silently, then cards drip to Discord as news breaks.
echo Close this window or run STOP.bat to halt.
echo.
cargo run --release -p spectrum-headless
echo.
echo Spectrum stopped.
pause
