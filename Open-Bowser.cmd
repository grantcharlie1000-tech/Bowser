@echo off
setlocal
rem Opens the pinned stable Bowser. It does not build this source tree.
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0Open-Bowser.ps1" %*
set "BOWSER_EXIT_CODE=%ERRORLEVEL%"
if not "%BOWSER_EXIT_CODE%"=="0" (
  echo.
  echo Bowser did not open.
  pause
)
exit /b %BOWSER_EXIT_CODE%
