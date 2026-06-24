@echo off
title Spectrum STOP
rem Halt the engine: kill the launcher window (and its child) plus the exe by name.
taskkill /FI "WINDOWTITLE eq Spectrum Engine*" /T /F >nul 2>&1
taskkill /IM spectrum-headless.exe /F >nul 2>&1
echo Spectrum stopped.
timeout /t 2 >nul
