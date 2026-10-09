@echo off
setlocal
set "BOWSER_EXE=%~dp0target\release\bowser.exe"
if not exist "%BOWSER_EXE%" (
  echo Bowser has not been built. Open a terminal in this project and run:
  echo cargo build --release --locked
  pause
  exit /b 1
)
start "" "%BOWSER_EXE%"
exit /b 0
