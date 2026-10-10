@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0Start-Bowser.ps1"
set "BOWSER_EXIT_CODE=%ERRORLEVEL%"
if not "%BOWSER_EXIT_CODE%"=="0" pause
exit /b %BOWSER_EXIT_CODE%
