@echo off
setlocal
cd /d "%~dp0"

set "LOCAL_PYTHON=%LOCALAPPDATA%\Programs\Python\Python312\pythonw.exe"
if exist "%LOCAL_PYTHON%" (
  start "" "%LOCAL_PYTHON%" "%~dp0main.py"
  exit /b 0
)

set "BUNDLED_PYTHON=%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\python\pythonw.exe"
if exist "%BUNDLED_PYTHON%" (
  start "" "%BUNDLED_PYTHON%" "%~dp0main.py"
  exit /b 0
)

where pyw >nul 2>nul
if %errorlevel%==0 (
  start "" pyw -3 "%~dp0main.py"
  exit /b 0
)

where pythonw >nul 2>nul
if %errorlevel%==0 (
  start "" pythonw "%~dp0main.py"
  exit /b 0
)

echo [ERROR] Python 3.11 or newer was not found.
echo Install Python from https://www.python.org/downloads/windows/
pause
