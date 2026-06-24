@echo off
title Spectrum - Enable auto-start
set "LNK=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\Spectrum.lnk"

powershell -NoProfile -ExecutionPolicy Bypass -Command "$w=New-Object -ComObject WScript.Shell; $s=$w.CreateShortcut('%LNK%'); $s.TargetPath='%~dp0START.bat'; $s.WorkingDirectory='%~dp0'; $s.WindowStyle=7; $s.Save()"

rem Start it now too, so there's no wait until the next reboot.
call "%~dp0START.bat"

echo.
echo ============================================================
echo  Done. Spectrum will now start automatically when this PC
echo  starts, and live in the system tray (near the clock).
echo  Click the tray icon for the window; right-click it for
echo  Start / Stop / Quit.
echo.
echo  To turn this off later, double-click UNINSTALL-AUTOSTART.bat.
echo ============================================================
echo.
pause
