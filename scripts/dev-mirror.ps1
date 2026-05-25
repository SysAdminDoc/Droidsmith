#requires -Version 5.1
<#
Mirrors the Droidsmith source tree from this repo location to C:\tmp\Droidsmith
so Vite and Cargo can run against a non-HGFS path.

WHY: when this repo lives on a VMware Shared Folders mount (\\vmware-host\Shared
Folders\repos\Droidsmith) the path contains a space, which breaks Vite's module
resolver. Cargo also pays a heavy fsync tax on HGFS. We work around both by
mirroring source-only files to a fast local SSD path before invoking dev/build.

USAGE
  ./scripts/dev-mirror.ps1            # one-shot mirror
  ./scripts/dev-mirror.ps1 -Watch     # mirror, then keep watching for changes
  ./scripts/dev-mirror.ps1 -Reverse   # copy build artefacts back to the repo

The mirror is a strict mirror (/MIR) — files deleted in the source are removed
from the destination. node_modules/, target/, dist/ are excluded.
#>
[CmdletBinding()]
param(
    [switch] $Watch,
    [switch] $Reverse,
    [string] $Destination = "C:\tmp\Droidsmith"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$src = if ($Reverse) { $Destination } else { $repoRoot }
$dst = if ($Reverse) { $repoRoot }   else { $Destination }

if (-not (Test-Path $dst)) {
    New-Item -ItemType Directory -Force -Path $dst | Out-Null
}

$exclude = @(
    "node_modules",
    "target",
    ".git",
    ".dev-mirror",
    "src-tauri\target",
    "src-tauri\gen"
)
# Note: dist/ is NOT excluded — we ship a placeholder index.html so
# `tauri::generate_context!()` can validate the frontendDist path before the
# first `npm run build`. Real Vite output is overwritten on every build.
$excludeArgs = @()
foreach ($d in $exclude) { $excludeArgs += @("/XD", $d) }

function Invoke-Mirror {
    Write-Host "[dev-mirror] $src -> $dst" -ForegroundColor Cyan
    & robocopy $src $dst /MIR /NFL /NDL /NJH /NJS /NP @excludeArgs | Out-Null
    $code = $LASTEXITCODE
    # robocopy success codes: 0 (no copies) .. 7 (mismatched + extras); 8+ is failure.
    if ($code -ge 8) { throw "robocopy failed with code $code" }
    Write-Host "[dev-mirror] done (robocopy code $code)" -ForegroundColor Green
    # Robocopy uses non-zero exit codes for normal success (1 = files copied,
    # 3 = copied + extras, etc). Reset so callers can `if (-not $?) { fail }`
    # without false positives.
    $global:LASTEXITCODE = 0
}

Invoke-Mirror

if (-not $Watch) { exit 0 }

$fsw = New-Object System.IO.FileSystemWatcher
$fsw.Path = $repoRoot
$fsw.IncludeSubdirectories = $true
$fsw.EnableRaisingEvents = $true

$debounce = $null
$action = {
    if ($script:debounce) { return }
    $script:debounce = $true
    Start-Sleep -Milliseconds 200
    try { Invoke-Mirror } catch { Write-Warning $_ }
    $script:debounce = $false
}

Register-ObjectEvent $fsw Changed -Action $action | Out-Null
Register-ObjectEvent $fsw Created -Action $action | Out-Null
Register-ObjectEvent $fsw Deleted -Action $action | Out-Null
Register-ObjectEvent $fsw Renamed -Action $action | Out-Null

Write-Host "[dev-mirror] watching $repoRoot — Ctrl+C to stop" -ForegroundColor Yellow
while ($true) { Start-Sleep -Seconds 30 }
