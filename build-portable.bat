@echo off
setlocal
where py >nul 2>nul
if errorlevel 1 goto use_python
py -3 "%~dp0build-portable.py" %*
exit /b %errorlevel%
:use_python
where python >nul 2>nul
if errorlevel 1 goto missing_python
python "%~dp0build-portable.py" %*
exit /b %errorlevel%
:missing_python
echo Python 3.11 or newer is required. Install Python and run this script again.
exit /b 1
