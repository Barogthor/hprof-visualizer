@echo off
setlocal EnableExtensions EnableDelayedExpansion

if "%~1"=="" goto :help
if /I "%~1"=="-h" goto :help
if /I "%~1"=="--help" goto :help
if /I "%~1"=="help" goto :help
if "%~1"=="/?" goto :help

set "MODE=auto"
set "HOLD_SECONDS=120"
set "PROFILE_SET=standard"
set "TRUNCATE_BYTES=0"
set "SCENARIO=01"
set "SANITIZE=off"

set "FIRST_ARG=%~1"
if "%FIRST_ARG:~0,1%"=="-" goto :parse_options

set "MODE=%~1"
if "%~2"=="" goto :parsed_args
set "HOLD_SECONDS=%~2"
if "%~3"=="" goto :parsed_args
set "PROFILE_SET=%~3"
if "%~4"=="" goto :parsed_args
set "TRUNCATE_BYTES=%~4"
if "%~5"=="" goto :parsed_args
set "SCENARIO=%~5"
if "%~6"=="" goto :parsed_args
set "SANITIZE=%~6"
goto :parsed_args

:parse_options
if "%~1"=="" goto :parsed_args
if /I "%~1"=="-m" goto :set_mode
if /I "%~1"=="--mode" goto :set_mode
if /I "%~1"=="-H" goto :set_hold
if /I "%~1"=="--hold-seconds" goto :set_hold
if /I "%~1"=="-p" goto :set_profile
if /I "%~1"=="--profile-set" goto :set_profile
if /I "%~1"=="-t" goto :set_truncate
if /I "%~1"=="--truncate-bytes" goto :set_truncate
if /I "%~1"=="-s" goto :set_scenario
if /I "%~1"=="--scenario" goto :set_scenario
if /I "%~1"=="-S" goto :set_sanitize
if /I "%~1"=="--sanitize" goto :set_sanitize

echo [heap-fixture] unknown option "%~1"
goto :help_error

:set_mode
if "%~2"=="" goto :help_error
set "MODE=%~2"
shift
shift
goto :parse_options

:set_hold
if "%~2"=="" goto :help_error
set "HOLD_SECONDS=%~2"
shift
shift
goto :parse_options

:set_profile
if "%~2"=="" goto :help_error
set "PROFILE_SET=%~2"
shift
shift
goto :parse_options

:set_truncate
if "%~2"=="" goto :help_error
set "TRUNCATE_BYTES=%~2"
shift
shift
goto :parse_options

:set_scenario
if "%~2"=="" goto :help_error
set "SCENARIO=%~2"
shift
shift
goto :parse_options

:set_sanitize
if "%~2"=="" goto :help_error
set "SANITIZE=%~2"
shift
shift
goto :parse_options

:parsed_args

if /I not "%MODE%"=="auto" if /I not "%MODE%"=="manual" if /I not "%MODE%"=="both" (
  echo [heap-fixture] invalid mode "%MODE%" ^(expected: auto^|manual^|both^)
  goto :help_error
)

if /I not "%SANITIZE%"=="on" if /I not "%SANITIZE%"=="off" if /I not "%SANITIZE%"=="only" (
  echo [heap-fixture] invalid sanitize "%SANITIZE%" ^(expected: off^|on^|only^)
  goto :help_error
)

set "SCRIPT_DIR=%~dp0"
set "CLASS_DIR=%SCRIPT_DIR%out"
set "ASSETS_DIR=%SCRIPT_DIR%..\..\assets\generated"
set "REDACT_CMD=%SCRIPT_DIR%..\hprof-redact-custom\redact.cmd"

if not exist "%CLASS_DIR%" mkdir "%CLASS_DIR%"
if not exist "%ASSETS_DIR%" mkdir "%ASSETS_DIR%"

if /I not "%SANITIZE%"=="off" if not exist "%REDACT_CMD%" (
  echo [heap-fixture] sanitizer script not found: %REDACT_CMD%
  exit /b 1
)

set "SCENARIOS="
if /I "%SCENARIO%"=="all" set "SCENARIOS=01 02 03 04 05 06 07 08 09 10"
if /I "%SCENARIO%"=="01" set "SCENARIOS=01"
if /I "%SCENARIO%"=="1" set "SCENARIOS=01"
if /I "%SCENARIO%"=="02" set "SCENARIOS=02"
if /I "%SCENARIO%"=="2" set "SCENARIOS=02"
if /I "%SCENARIO%"=="03" set "SCENARIOS=03"
if /I "%SCENARIO%"=="3" set "SCENARIOS=03"
if /I "%SCENARIO%"=="04" set "SCENARIOS=04"
if /I "%SCENARIO%"=="4" set "SCENARIOS=04"
if /I "%SCENARIO%"=="05" set "SCENARIOS=05"
if /I "%SCENARIO%"=="5" set "SCENARIOS=05"
if /I "%SCENARIO%"=="06" set "SCENARIOS=06"
if /I "%SCENARIO%"=="6" set "SCENARIOS=06"
if /I "%SCENARIO%"=="07" set "SCENARIOS=07"
if /I "%SCENARIO%"=="7" set "SCENARIOS=07"
if /I "%SCENARIO%"=="08" set "SCENARIOS=08"
if /I "%SCENARIO%"=="8" set "SCENARIOS=08"
if /I "%SCENARIO%"=="09" set "SCENARIOS=09"
if /I "%SCENARIO%"=="9" set "SCENARIOS=09"
if /I "%SCENARIO%"=="10" set "SCENARIOS=10"

if "%SCENARIOS%"=="" (
  echo [heap-fixture] invalid scenario "%SCENARIO%" ^(expected: 01^|02^|03^|04^|05^|06^|07^|08^|09^|10^|all^)
  goto :help_error
)

set "SRC_LIST=%CLASS_DIR%\sources.txt"
if exist "%SRC_LIST%" del "%SRC_LIST%"
echo %SCRIPT_DIR%HeapDumpFixture.java> "%SRC_LIST%"
for %%F in ("%SCRIPT_DIR%support\*.java") do echo %%~fF>> "%SRC_LIST%"
for %%F in ("%SCRIPT_DIR%scenarios\*.java") do echo %%~fF>> "%SRC_LIST%"

if /I not "%SANITIZE%"=="only" (
  javac -d "%CLASS_DIR%" @"%SRC_LIST%"
  if errorlevel 1 exit /b 1
)

set "PROFILES="
if /I "%PROFILE_SET%"=="standard" set "PROFILES=tiny medium large xlarge"
if /I "%PROFILE_SET%"=="all" set "PROFILES=tiny medium large xlarge ultra"
if /I "%PROFILE_SET%"=="ultra" set "PROFILES=ultra"

if "%PROFILES%"=="" (
  echo [heap-fixture] invalid profile_set "%PROFILE_SET%" (expected: standard^|all^|ultra)
  goto :help_error
)

for %%P in (%PROFILES%) do (
  for %%S in (%SCENARIOS%) do (
    set "OUTPUT=%ASSETS_DIR%\fixture-s%%S-%%P.hprof"
    if /I not "%SANITIZE%"=="only" (
      echo [heap-fixture] scenario=%%S profile=%%P mode=%MODE% output=!OUTPUT! truncateBytes=%TRUNCATE_BYTES%
      java -cp "%CLASS_DIR%" HeapDumpFixture --scenario %%S --profile %%P --dump-mode %MODE% --hold-seconds %HOLD_SECONDS% --truncate-bytes %TRUNCATE_BYTES% --output "!OUTPUT!"
      if errorlevel 1 exit /b 1
    )

    if /I "%SANITIZE%"=="on" (
      call :sanitize_prefix "%ASSETS_DIR%\fixture-s%%S-%%P"
      if errorlevel 1 exit /b 1
    )
    if /I "%SANITIZE%"=="only" (
      call :sanitize_prefix "%ASSETS_DIR%\fixture-s%%S-%%P"
      if errorlevel 1 exit /b 1
    )
  )
)

echo [heap-fixture] done
exit /b 0

:help
echo Usage:
echo   tools\java-dump-fixtures\generate-dumps.cmd ^<mode^> [hold_seconds] [profile_set] [truncate_bytes] [scenario]
echo   tools\java-dump-fixtures\generate-dumps.cmd ^<mode^> [hold_seconds] [profile_set] [truncate_bytes] [scenario] [sanitize]
echo   tools\java-dump-fixtures\generate-dumps.cmd [options]
echo.
echo Arguments:
echo   mode           auto ^| manual ^| both
echo   hold_seconds   default: 120
echo   profile_set    standard ^| all ^| ultra   ^(default: standard^)
echo   truncate_bytes default: 0
echo   scenario       01 ^| 02 ^| 03 ^| 04 ^| 05 ^| 06 ^| 07 ^| 08 ^| 09 ^| 10 ^| all   ^(default: 01^)
echo   sanitize       off ^| on ^| only   ^(default: off^)
echo.
echo Options:
echo   -m, --mode ^<value^>
echo   -H, --hold-seconds ^<value^>
echo   -p, --profile-set ^<value^>
echo   -t, --truncate-bytes ^<value^>
echo   -s, --scenario ^<value^>
echo   -S, --sanitize ^<value^>
echo.
echo Examples:
echo   tools\java-dump-fixtures\generate-dumps.cmd auto
echo   tools\java-dump-fixtures\generate-dumps.cmd both 180 all 4194304
echo   tools\java-dump-fixtures\generate-dumps.cmd auto 120 ultra 2097152 01
echo   tools\java-dump-fixtures\generate-dumps.cmd auto 120 standard 0 all
echo   tools\java-dump-fixtures\generate-dumps.cmd --mode auto --profile-set ultra --scenario 01 --sanitize on
echo   tools\java-dump-fixtures\generate-dumps.cmd --profile-set all --scenario all --sanitize only
exit /b 0

:help_error
echo.
echo Usage:
echo   tools\java-dump-fixtures\generate-dumps.cmd ^<mode^> [hold_seconds] [profile_set] [truncate_bytes] [scenario]
echo   tools\java-dump-fixtures\generate-dumps.cmd [options]
echo.
echo Arguments:
echo   mode           auto ^| manual ^| both
echo   hold_seconds   default: 120
echo   profile_set    standard ^| all ^| ultra   ^(default: standard^)
echo   truncate_bytes default: 0
echo   scenario       01 ^| 02 ^| 03 ^| 04 ^| 05 ^| 06 ^| 07 ^| 08 ^| 09 ^| 10 ^| all   ^(default: 01^)
echo   sanitize       off ^| on ^| only   ^(default: off^)
exit /b 1

:sanitize_prefix
set "PREFIX=%~1"
for %%F in ("%PREFIX%*.hprof") do (
  set "DUMP=%%~fF"
  set "NAME=%%~nF"
  set "SKIP=0"
  echo !NAME! | findstr /I /R "-truncated$ -truncated-[0-9][0-9]*$" >nul
  if not errorlevel 1 (
    set "SKIP=1"
    echo [heap-fixture] sanitize skip truncated=!DUMP!
  )
  if "!SKIP!"=="0" (
    echo !NAME! | findstr /I /R "-sanitized$ -sanitized-[0-9][0-9]*$" >nul
    if errorlevel 1 (
      set "SAN_OUT=%%~dpnF-sanitized.hprof"
      echo [heap-fixture] sanitize input=!DUMP! output=!SAN_OUT!
      call "%REDACT_CMD%" "!DUMP!" "!SAN_OUT!"
      if errorlevel 1 exit /b 1
    )
  )
)
exit /b 0
