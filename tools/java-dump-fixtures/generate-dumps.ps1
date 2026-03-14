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

    [ValidateSet("01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "all")]
    [Alias("s")]
    [string]$Scenario = "01",

    [ValidateSet("off", "on", "only")]
    [Alias("S")]
    [string]$Sanitize = "off",

    [ValidateSet("raw", "sanitized", "both")]
    [Alias("T")]
    [string]$TruncateTarget = "raw",

    [ValidateSet("off", "on")]
    [Alias("R")]
    [string]$RemoveRaw = "off",

    [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Usage {
    Write-Host "Usage:"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode <mode> [-HoldSeconds <n>] [-ProfileSet <set>] [-TruncateBytes <n>] [-Scenario <id>] [-Sanitize <off|on|only>] [-TruncateTarget <raw|sanitized|both>] [-RemoveRaw <off|on>]"
    Write-Host ""
    Write-Host "Arguments:"
    Write-Host "  Mode          auto | manual | both"
    Write-Host "  HoldSeconds   default: 120"
    Write-Host "  ProfileSet    standard | all | ultra   (default: standard)"
    Write-Host "  TruncateBytes default: 0"
    Write-Host "  Scenario      01 | 02 | 03 | 04 | 05 | 06 | 07 | 08 | 09 | 10 | all   (default: 01)"
    Write-Host "  Sanitize      off | on | only   (default: off)"
    Write-Host "  TruncateTarget raw | sanitized | both   (default: raw)"
    Write-Host "  RemoveRaw     off | on   (default: off; requires Sanitize=on)"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode both -HoldSeconds 180 -ProfileSet all -TruncateBytes 4194304"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -Mode auto -ProfileSet standard -Scenario all"
    Write-Host "  ./tools/java-dump-fixtures/generate-dumps.ps1 -m auto -p ultra -s 01 -S on -T both -R on"
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

if ($TruncateTarget -ne "raw" -and $TruncateBytes -eq 0) {
    throw "TruncateTarget '$TruncateTarget' requires TruncateBytes > 0"
}

if ($Sanitize -eq "off" -and ($TruncateTarget -eq "sanitized" -or $TruncateTarget -eq "both")) {
    throw "TruncateTarget '$TruncateTarget' requires Sanitize on or only"
}

if ($RemoveRaw -eq "on" -and $Sanitize -ne "on") {
    throw "RemoveRaw 'on' requires Sanitize=on"
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
        if ($dump.Name -like "*-san.hprof" -or $dump.Name -like "*-san-*.hprof" -or $dump.Name -like "*-sanitized.hprof" -or $dump.Name -like "*-sanitized-*.hprof") {
            continue
        }
        if ($dump.Name -like "*-truncated.hprof" -or $dump.Name -like "*-truncated-*.hprof") {
            Write-Host "[heap-fixture] sanitize skip truncated=$($dump.FullName)"
            continue
        }

        $baseName = [System.IO.Path]::GetFileNameWithoutExtension($dump.Name)
        if ($baseName.EndsWith("-raw")) {
            $baseName = $baseName.Substring(0, $baseName.Length - 4)
        }

        $out = [System.IO.Path]::Combine($dump.DirectoryName, ($baseName + "-san.hprof"))
        Write-Host "[heap-fixture] sanitize input=$($dump.FullName) output=$out"
        & $RedactScriptPath $dump.FullName $out
    }
}

function Invoke-TruncateFile {
    param([string]$InputPath, [long]$BytesToRemove)

    if (-not (Test-Path $InputPath)) {
        return
    }

    $output = [System.IO.Path]::Combine(
        [System.IO.Path]::GetDirectoryName($InputPath),
        ([System.IO.Path]::GetFileNameWithoutExtension($InputPath) + "-truncated.hprof")
    )

    $inputInfo = Get-Item $InputPath
    $keep = $inputInfo.Length - $BytesToRemove
    if ($keep -lt 1) {
        $keep = 1
    }

    if (Test-Path $output) {
        Remove-Item -Force $output
    }

    $buffer = New-Object byte[] 8192
    $remaining = $keep
    $src = [System.IO.File]::OpenRead($InputPath)
    $dst = [System.IO.File]::Open($output, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write)
    try {
        while ($remaining -gt 0) {
            $toRead = [Math]::Min($buffer.Length, [int][Math]::Min($remaining, [long]2147483647))
            $read = $src.Read($buffer, 0, $toRead)
            if ($read -le 0) { break }
            $dst.Write($buffer, 0, $read)
            $remaining -= $read
        }
    } finally {
        $dst.Dispose()
        $src.Dispose()
    }

    $outInfo = Get-Item $output
    Write-Host "truncatedDumpPath=$output original=$($inputInfo.Length) truncated=$($outInfo.Length)"
}

function Invoke-TruncateSanitizedForPrefix {
    param([string]$Prefix, [long]$BytesToRemove)

    $basePrefix = $Prefix
    if ($basePrefix.EndsWith("-raw")) {
        $basePrefix = $basePrefix.Substring(0, $basePrefix.Length - 4)
    }

    $dumps = @()
    $dumps += Get-ChildItem -Path ($basePrefix + "*-san.hprof") -File -ErrorAction SilentlyContinue
    $dumps += Get-ChildItem -Path ($basePrefix + "*-sanitized.hprof") -File -ErrorAction SilentlyContinue
    foreach ($dump in $dumps) {
        Write-Host "[heap-fixture] truncate sanitized input=$($dump.FullName)"
        Invoke-TruncateFile -InputPath $dump.FullName -BytesToRemove $BytesToRemove
    }
}

function Remove-RawForPrefix {
    param([string]$Prefix)

    $dumps = Get-ChildItem -Path ($Prefix + "*.hprof") -File -ErrorAction SilentlyContinue
    foreach ($dump in $dumps) {
        if ($dump.Name -like "*-san.hprof" -or $dump.Name -like "*-san-*.hprof" -or $dump.Name -like "*-sanitized.hprof" -or $dump.Name -like "*-sanitized-*.hprof") {
            continue
        }
        Write-Host "[heap-fixture] remove raw=$($dump.FullName)"
        Remove-Item -Force $dump.FullName
    }
}

switch ($ProfileSet) {
    "standard" { $profiles = @("tiny", "medium", "large", "xlarge") }
    "all" { $profiles = @("tiny", "medium", "large", "xlarge", "ultra") }
    "ultra" { $profiles = @("ultra") }
    default { throw "Unexpected ProfileSet: $ProfileSet" }
}

switch ($Scenario) {
    "all" { $scenarios = @("01", "02", "03", "04", "05", "06", "07", "08", "09", "10") }
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
        $output = Join-Path $AssetsDir ("fixture-s{0}-{1}-raw.hprof" -f $scenarioId, $profile)
        if ($Sanitize -ne "only") {
            Write-Host "[heap-fixture] scenario=$scenarioId profile=$profile mode=$Mode output=$output truncateBytes=$TruncateBytes"

            $truncateForJava = $TruncateBytes
            if ($TruncateTarget -eq "sanitized") {
                $truncateForJava = 0
            }

            java -cp $ClassDir HeapDumpFixture `
                --scenario $scenarioId `
                --profile $profile `
                --dump-mode $Mode `
                --hold-seconds $HoldSeconds `
                --truncate-bytes $truncateForJava `
                --output $output
        }

        if ($Sanitize -eq "on" -or $Sanitize -eq "only") {
            $prefix = [System.IO.Path]::Combine($AssetsDir, ("fixture-s{0}-{1}-raw" -f $scenarioId, $profile))
            Invoke-SanitizeForPrefix -Prefix $prefix -RedactScriptPath $RedactScript

            if ($TruncateBytes -gt 0 -and ($TruncateTarget -eq "sanitized" -or $TruncateTarget -eq "both")) {
                Invoke-TruncateSanitizedForPrefix -Prefix $prefix -BytesToRemove $TruncateBytes
            }

            if ($RemoveRaw -eq "on") {
                Remove-RawForPrefix -Prefix $prefix
            }
        }
    }
}

Write-Host "[heap-fixture] done"
