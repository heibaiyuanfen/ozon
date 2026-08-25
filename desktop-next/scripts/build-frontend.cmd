@echo off
setlocal
set "NODE_EXE=%USERPROFILE%\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe"
if not exist "%NODE_EXE%" (
  echo Node.js runtime not found: %NODE_EXE%
  exit /b 1
)
"%NODE_EXE%" "node_modules\typescript\bin\tsc"
if errorlevel 1 exit /b %errorlevel%
"%NODE_EXE%" "node_modules\vite\bin\vite.js" build
exit /b %errorlevel%
