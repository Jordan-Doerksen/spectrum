@echo off
title Spectrum - Disable auto-start
set "LNK=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Spectrum.lnk"
if exist "%LNK%" del "%LNK%"
echo Auto-start disabled. Spectrum will no longer start with Windows.
echo (It keeps running until you Quit it from the tray or run STOP.bat.)
pause
