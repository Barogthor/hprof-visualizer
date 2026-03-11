@echo off
setlocal EnableExtensions

if "%~1"=="" goto :help
if /I "%~1"=="-h" goto :help
if /I "%~1"=="--help" goto :help
if /I "%~1"=="help" goto :help
if "%~1"=="/?" goto :help

if "%~2"=="" goto :help_error

set "SCRIPT_DIR=%~dp0"
set "POM=%SCRIPT_DIR%pom.xml"
set "JAR=%SCRIPT_DIR%target\hprof-path-redact.jar"

mvn -q -f "%POM%" -DskipTests package
if errorlevel 1 exit /b 1

java -jar "%JAR%" "%~1" "%~2"
exit /b %errorlevel%

:help
echo Usage:
echo   tools\hprof-redact-custom\redact.cmd ^<input.hprof^> ^<output.hprof^>
echo.
echo Example:
echo   tools\hprof-redact-custom\redact.cmd assets\generated\fixture-s01-ultra-auto.hprof assets\generated\fixture-s01-ultra-auto-redacted.hprof
exit /b 0

:help_error
echo Invalid arguments.
echo.
echo Usage:
echo   tools\hprof-redact-custom\redact.cmd ^<input.hprof^> ^<output.hprof^>
exit /b 1
