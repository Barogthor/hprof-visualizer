param(
    [Parameter(Position = 0)]
    [Alias("Input", "In")]
    [string]$InputPath,

    [Parameter(Position = 1)]
    [Alias("Output", "Out")]
    [string]$OutputPath,

    [switch]$Help
)

$ErrorActionPreference = "Stop"

function Show-Usage {
    Write-Host "Usage:"
    Write-Host "  ./tools/hprof-redact-custom/redact.ps1 <input.hprof> <output.hprof>"
    Write-Host "  ./tools/hprof-redact-custom/redact.ps1 -InputPath <input.hprof> -OutputPath <output.hprof>"
    Write-Host ""
    Write-Host "Example:"
    Write-Host "  ./tools/hprof-redact-custom/redact.ps1 assets/generated/fixture-s01-ultra-auto.hprof assets/generated/fixture-s01-ultra-auto-redacted.hprof"
}

if ($Help.IsPresent -or $PSBoundParameters.Count -eq 0) {
    Show-Usage
    exit 0
}

if ([string]::IsNullOrWhiteSpace($InputPath) -or [string]::IsNullOrWhiteSpace($OutputPath)) {
    Write-Error "InputPath and OutputPath are required."
    Show-Usage
    exit 1
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$Pom = Join-Path $ScriptDir "pom.xml"
$Jar = Join-Path $ScriptDir "target\hprof-path-redact.jar"

mvn -q -f $Pom -DskipTests package
java -jar $Jar $InputPath $OutputPath
