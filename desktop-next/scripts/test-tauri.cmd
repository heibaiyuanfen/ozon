@echo off
call "D:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b %errorlevel%
"%USERPROFILE%\.cargo\bin\cargo.exe" test --manifest-path src-tauri\Cargo.toml --lib
