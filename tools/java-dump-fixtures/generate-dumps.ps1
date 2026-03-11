param(
    [ValidateSet("auto", "manual", "both")]
    [Alias("m")]
    [string]$Mode = "auto",

    [Alias("H")]
    [int]$HoldSeconds = 120,

    [ValidateSet("standard", "all", "ultra")]
    [Alias("p")]
    [string]$ProfileSet = "standard",

    [Alias("t")]
    [long]$TruncateBytes = 0,

    [ValidateSet("01", "02", "03", "04", "05", "all")]
    [Alias("s")]
    [string]$Scenario = "01",

    [ValidateSet("off", "on", "only")]
    [Alias("S")]
    [string]$Sanitize = "off",

    [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Usage {
    Write-Host "Usage:"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode <mode> [-HoldSeconds <n>] [-ProfileSet <set>] [-TruncateBytes <n>] [-Scenario <id>] [-Sanitize <off|on|only>]"
    Write-Host ""
    Write-Host "Arguments:"
    Write-Host "  Mode          auto | manual | both"
    Write-Host "  HoldSeconds   default: 120"
    Write-Host "  ProfileSet    standard | all | ultra   (default: standard)"
    Write-Host "  TruncateBytes default: 0"
    Write-Host "  Scenario      01 | 02 | 03 | 04 | 05 | all   (default: 01)"
    Write-Host "  Sanitize      off | on | only   (default: off)"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode both -HoldSeconds 180 -ProfileSet all -TruncateBytes 4194304"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto -ProfileSet standard -Scenario all"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -m auto -p ultra -s 01 -S on"
}

if ($Help.IsPresent) {
    Show-Usage
    exit 0
}

if ($PSBoundParameters.Count -eq 0) {
    Show-Usage
    exit 0
}

if ($HoldSeconds -lt 1) {
    throw "HoldSeconds must be >= 1"
}

if ($TruncateBytes -lt 0) {
    throw "TruncateBytes must be >= 0"
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ClassDir = Join-Path $ScriptDir "out"
$AssetsDir = Join-Path $ScriptDir "..\..\assets\generated"
$RedactScript = Join-Path $ScriptDir "..\hprof-redact-custom\redact.ps1"

New-Item -ItemType Directory -Force -Path $ClassDir | Out-Null
New-Item -ItemType Directory -Force -Path $AssetsDir | Out-Null

if ($Sanitize -ne "off" -and -not (Test-Path $RedactScript)) {
    throw "Sanitizer script not found: $RedactScript"
}

function Invoke-SanitizeForPrefix {
    param([string]$Prefix, [string]$RedactScriptPath)

    $dumps = Get-ChildItem -Path ($Prefix + "*.hprof") -File -ErrorAction SilentlyContinue
    foreach ($dump in $dumps) {
        if ($dump.Name -like "*-sanitized.hprof" -or $dump.Name -like "*-sanitized-*.hprof") {
            continue
        }

        $out = [System.IO.Path]::Combine($dump.DirectoryName, ([System.IO.Path]::GetFileNameWithoutExtension($dump.Name) + "-sanitized.hprof"))
        Write-Host "[heap-fixture] sanitize input=$($dump.FullName) output=$out"
        & $RedactScriptPath $dump.FullName $out
    }
}

switch ($ProfileSet) {
    "standard" { $profiles = @("tiny", "medium", "large", "xlarge") }
    "all" { $profiles = @("tiny", "medium", "large", "xlarge", "ultra") }
    "ultra" { $profiles = @("ultra") }
    default { throw "Unexpected ProfileSet: $ProfileSet" }
}

switch ($Scenario) {
    "all" { $scenarios = @("01", "02", "03", "04", "05") }
    default { $scenarios = @($Scenario) }
}

$sources = @(
    (Join-Path $ScriptDir "HeapDumpFixture.java")
)
$sources += (Get-ChildItem -Path (Join-Path $ScriptDir "support") -Filter "*.java" | ForEach-Object { $_.FullName })
$sources += (Get-ChildItem -Path (Join-Path $ScriptDir "scenarios") -Filter "*.java" | ForEach-Object { $_.FullName })

if ($Sanitize -ne "only") {
    javac -d $ClassDir $sources
}

foreach ($profile in $profiles) {
    foreach ($scenarioId in $scenarios) {
        $output = Join-Path $AssetsDir ("fixture-s{0}-{1}.hprof" -f $scenarioId, $profile)
        if ($Sanitize -ne "only") {
            Write-Host "[heap-fixture] scenario=$scenarioId profile=$profile mode=$Mode output=$output truncateBytes=$TruncateBytes"

            java -cp $ClassDir HeapDumpFixture `
                --scenario $scenarioId `
                --profile $profile `
                --dump-mode $Mode `
                --hold-seconds $HoldSeconds `
                --truncate-bytes $TruncateBytes `
                --output $output
        }

        if ($Sanitize -eq "on" -or $Sanitize -eq "only") {
            $prefix = [System.IO.Path]::Combine($AssetsDir, ("fixture-s{0}-{1}" -f $scenarioId, $profile))
            Invoke-SanitizeForPrefix -Prefix $prefix -RedactScriptPath $RedactScript
        }
    }
}

Write-Host "[heap-fixture] done"
