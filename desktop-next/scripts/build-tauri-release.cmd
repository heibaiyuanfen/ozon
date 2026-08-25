@echo off
setlocal
set "NODE_BIN=%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin"
set "PNPM_BIN=%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\bin\fallback"
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%NODE_BIN%\node.exe" goto node_missing
if not exist "%VSWHERE%" goto vswhere_missing
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSROOT=%%i"
if not defined VSROOT goto vs_missing
call "%VSROOT%\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b %errorlevel%
set "PATH=%NODE_BIN%;%PNPM_BIN%;%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0.."
"%NODE_BIN%\node.exe" "node_modules\@tauri-apps\cli\tauri.js" build --no-bundle
exit /b %errorlevel%

:node_missing
echo Node.js runtime not found: %NODE_BIN%\node.exe
exit /b 1

:vswhere_missing
echo Visual Studio locator not found: %VSWHERE%
exit /b 1

:vs_missing
echo Visual Studio C++ build tools were not found.
exit /b 1
