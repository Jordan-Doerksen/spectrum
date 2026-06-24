@echo off
title Spectrum - Setup
cd /d "%~dp0"
set "OLLAMA=%LOCALAPPDATA%\Programs\Ollama\ollama.exe"

echo ============================================================
echo  Spectrum - one-time setup
echo ============================================================
echo.
echo This installs the local AI and downloads its model. It only
echo needs to happen once and does everything for you. There are
echo two big downloads, so it'll take a while - leave it running.
echo.
pause

if exist "%OLLAMA%" goto have_ollama
where ollama >nul 2>nul && (set "OLLAMA=ollama" & goto have_ollama)

echo.
echo [1/2] Downloading Ollama (the local AI, about 1.3 GB)...
curl.exe -L --fail -o "%TEMP%\OllamaSetup.exe" "https://ollama.com/download/OllamaSetup.exe"
if errorlevel 1 (
  echo Download failed - check your internet connection and run SETUP.bat again.
  pause
  exit /b 1
)
echo Installing Ollama (a setup window may appear briefly)...
start /wait "" "%TEMP%\OllamaSetup.exe"
echo Giving Ollama a moment to start...
timeout /t 12 /nobreak >nul

:have_ollama
echo.
echo [2/2] Downloading the AI model (about 4.7 GB, one time)...
"%OLLAMA%" pull llama3.1:8b
if errorlevel 1 (
  echo.
  echo The model download didn't finish. Make sure the Ollama llama icon
  echo is showing near the clock, then run SETUP.bat again.
  pause
  exit /b 1
)

echo.
echo ============================================================
echo  Setup complete! Next:
echo   - Double-click INSTALL-AUTOSTART.bat (so it runs on its own), or
echo   - START.bat to run it right now.
echo ============================================================
echo.
pause
