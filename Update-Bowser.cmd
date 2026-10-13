@echo off
setlocal
set "UPDATER=%~dp0Update-Bowser.ps1"
if not exist "%UPDATER%" (
  echo Bowser updater is missing: "%UPDATER%"
  pause
  exit /b 1
)
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%UPDATER%" %*
set "RESULT=%ERRORLEVEL%"
if not "%RESULT%"=="0" (
  echo.
  echo Bowser was not updated. See the log path printed above.
  pause
)
exit /b %RESULT%
