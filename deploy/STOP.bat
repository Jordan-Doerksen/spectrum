@echo off
title Spectrum - Stop
taskkill /IM spectrum-pro.exe /F >nul 2>&1
echo Spectrum stopped.
timeout /t 2 /nobreak >nul
