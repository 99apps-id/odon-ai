@echo off
title ODON Installer
echo ====================================================
echo    ODON - Open Digital Orchestration Network
echo ====================================================
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1"
pause
