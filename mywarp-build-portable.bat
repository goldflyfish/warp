@echo off
setlocal
title MyWarp - Portable Package Builder
where py >nul 2>nul
if errorlevel 1 goto use_python
py -3 "%~dp0build-portable.py" --pause %*
exit /b %errorlevel%
:use_python
where python >nul 2>nul
if errorlevel 1 goto missing_python
python "%~dp0build-portable.py" --pause %*
exit /b %errorlevel%
:missing_python
echo.
echo   MYWARP - BUILD NEEDS ATTENTION
echo   Python 3.11 or newer is required. Install Python and run this script again.
if not defined CI pause
exit /b 1
