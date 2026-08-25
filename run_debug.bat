@echo off
setlocal
cd /d "%~dp0"

set "LOCAL_PYTHON=%LOCALAPPDATA%\Programs\Python\Python312\python.exe"
if exist "%LOCAL_PYTHON%" (
  "%LOCAL_PYTHON%" "%~dp0main.py"
  pause
  exit /b %errorlevel%
)

set "BUNDLED_PYTHON=%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\python.exe"
if exist "%BUNDLED_PYTHON%" (
  "%BUNDLED_PYTHON%" "%~dp0main.py"
  pause
  exit /b %errorlevel%
)

py -3 "%~dp0main.py"
pause
