@echo off
setlocal
set "NODE_EXE=%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe"
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%NODE_EXE%" goto node_missing
if not exist "%VSWHERE%" goto vswhere_missing
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSROOT=%%i"
if not defined VSROOT goto vs_missing
call "%VSROOT%\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b %errorlevel%
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0.."
"%NODE_EXE%" "node_modules\@tauri-apps\cli\tauri.js" build --bundles nsis
exit /b %errorlevel%

:node_missing
echo Node.js runtime not found: %NODE_EXE%
exit /b 1

:vswhere_missing
echo Visual Studio locator not found: %VSWHERE%
exit /b 1

:vs_missing
echo Visual Studio C++ build tools were not found.
exit /b 1
