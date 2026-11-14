@echo off
setlocal

rem Keep the build and configuration rules in the reviewed PowerShell script.
rem This wrapper only forwards arguments and preserves the resulting exit code.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-windows.ps1" %*
set "kvnc_exit_code=%ERRORLEVEL%"
endlocal & exit /b %kvnc_exit_code%
