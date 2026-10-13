@echo off
setlocal
rem Builds Dev Bowser and rebuilds it when the source changes. It does not touch the installed Bowser.
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0Dev-Bowser.ps1" %*
set "BOWSER_EXIT_CODE=%ERRORLEVEL%"
if not "%BOWSER_EXIT_CODE%"=="0" (
  echo.
  echo Dev Bowser did not start.
  pause
)
exit /b %BOWSER_EXIT_CODE%
