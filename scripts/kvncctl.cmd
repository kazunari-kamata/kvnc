@echo off
setlocal

rem Keep Windows process validation in the PowerShell implementation.  This
rem wrapper only forwards the requested action and preserves its exit status.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0kvncctl.ps1" %*
set "kvnc_exit_code=%ERRORLEVEL%"
endlocal & exit /b %kvnc_exit_code%
