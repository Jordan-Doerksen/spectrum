@echo off
title Spectrum STOP
rem Halt every Spectrum process on this host, and name the one it stopped.
rem
rem Before CR-1 chunk 0 this file killed the launcher window and spectrum-headless.exe
rem only. The packaged control panel (spectrum-pro.exe) runs the same engine, so Stop
rem left it polling and posting. [BRIEF 11 · D-0013 · CR-1 chunk 0]
rem
rem Every target is probed before any kill, because taskkill reports success for a
rem window filter that matched nothing. The probe reads ".exe" in the tasklist row,
rem which is the same word in every Windows language.
setlocal
set "WINDOW=0"
set "HEADLESS=0"
set "PANEL=0"
set "FAILED=0"

tasklist /NH /FI "WINDOWTITLE eq Spectrum Engine*" 2>nul | find /I ".exe" >nul && set "WINDOW=1"
tasklist /NH /FI "IMAGENAME eq spectrum-headless.exe" 2>nul | find /I "spectrum-headless.exe" >nul && set "HEADLESS=1"
tasklist /NH /FI "IMAGENAME eq spectrum-pro.exe" 2>nul | find /I "spectrum-pro.exe" >nul && set "PANEL=1"

echo Spectrum STOP
echo.

if "%WINDOW%"=="1" (
    taskkill /FI "WINDOWTITLE eq Spectrum Engine*" /T /F >nul 2>&1
    if errorlevel 1 (
        echo   launcher window   - COULD NOT STOP. Try Run as administrator.
        set "FAILED=1"
    ) else (
        echo   launcher window   - stopped
    )
) else (
    echo   launcher window   - not running
)

if "%HEADLESS%"=="1" (
    taskkill /IM spectrum-headless.exe /T /F >nul 2>&1
    if errorlevel 128 (
        echo   headless runner   - stopped with the launcher window
    ) else if errorlevel 1 (
        echo   headless runner   - COULD NOT STOP. Try Run as administrator.
        set "FAILED=1"
    ) else (
        echo   headless runner   - stopped
    )
) else (
    echo   headless runner   - not running
)

if "%PANEL%"=="1" (
    taskkill /IM spectrum-pro.exe /T /F >nul 2>&1
    if errorlevel 128 (
        echo   control panel     - stopped with the launcher window
    ) else if errorlevel 1 (
        echo   control panel     - COULD NOT STOP. Try Run as administrator.
        set "FAILED=1"
    ) else (
        echo   control panel     - stopped
    )
) else (
    echo   control panel     - not running
)

echo.
if "%FAILED%"=="1" (
    echo One target did not stop. It still posts. Stop it in Task Manager.
) else (
    echo Spectrum posts nothing now.
)
echo A killed runner leaves data\spectrum-headless.lock behind. The next start finds the
echo dead process, recovers the lock, and writes one line about it in the log.
timeout /t 5 >nul 2>&1
if "%FAILED%"=="1" (endlocal & exit /b 1)
endlocal & exit /b 0
