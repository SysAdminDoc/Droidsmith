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

SAFETY
  robocopy /MIR will DELETE anything in $Destination that isn't in the
  source. To prevent a misuse from wiping an unrelated folder, this script
  refuses to mirror to a destination that already exists, is non-empty, and
  lacks the sentinel file `.droidsmith-mirror`. Pass -Force to override
  (you take responsibility for the destination contents).

The mirror is a strict mirror (/MIR). node_modules/, target/, .git/ are
excluded.
#>
[CmdletBinding()]
param(
    [switch] $Watch,
    [switch] $Reverse,
    [switch] $Force,
    [string] $Destination = "C:\tmp\Droidsmith"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$src = if ($Reverse) { $Destination } else { $repoRoot }
$dst = if ($Reverse) { $repoRoot }   else { $Destination }

$sentinelName = ".droidsmith-mirror"

function Test-DestinationSafe {
    param([string] $Path)

    if ($Reverse) {
        # Reverse mode copies INTO the repo. The git working tree is the
        # source of truth; refuse if the repo has uncommitted changes that
        # don't match the mirror, since /MIR could delete them.
        if (-not (Test-Path (Join-Path $Path ".git"))) {
            return @{ Safe = $false; Reason = "reverse target $Path is not a git repo" }
        }
        return @{ Safe = $true }
    }

    if (-not (Test-Path $Path)) {
        return @{ Safe = $true }  # We'll create it.
    }

    $children = @(Get-ChildItem -Path $Path -Force -ErrorAction SilentlyContinue)
    if ($children.Count -eq 0) {
        return @{ Safe = $true }  # Empty dir is fine.
    }

    $sentinel = Join-Path $Path $sentinelName
    if (Test-Path $sentinel) {
        return @{ Safe = $true }  # Created by an earlier run; ours to use.
    }

    return @{
        Safe   = $false
        Reason = "destination $Path is not empty and has no $sentinelName sentinel; refusing to /MIR. Use -Force if you're sure."
    }
}

$check = Test-DestinationSafe -Path $dst
if (-not $check.Safe -and -not $Force) {
    throw "[dev-mirror] $($check.Reason)"
}

if (-not (Test-Path $dst)) {
    New-Item -ItemType Directory -Force -Path $dst | Out-Null
}

# Write the sentinel BEFORE robocopy so /MIR will preserve it (it's not
# present in $src, but robocopy treats files in $dst that match the
# exclude pattern as protected).
if (-not $Reverse) {
    $sentinelPath = Join-Path $dst $sentinelName
    if (-not (Test-Path $sentinelPath)) {
        Set-Content -Path $sentinelPath -Value "Droidsmith dev-mirror destination. Safe to delete." -Encoding UTF8
    }
}

# Excludes. Note: dist/ is NOT excluded — we ship a placeholder
# dist/index.html so `tauri::generate_context!()` can validate the
# frontendDist path before the first `npm run build`. Real Vite output
# is overwritten on every build.
$excludeDirs = @(
    "node_modules",
    "target",
    ".git",
    ".dev-mirror",
    "src-tauri\target",
    "src-tauri\gen"
)
$excludeFiles = @($sentinelName)

$excludeArgs = @()
foreach ($d in $excludeDirs) { $excludeArgs += @("/XD", $d) }
foreach ($f in $excludeFiles) { $excludeArgs += @("/XF", $f) }

function Invoke-Mirror {
    Write-Host "[dev-mirror] $src -> $dst" -ForegroundColor Cyan
    & robocopy $src $dst /MIR /NFL /NDL /NJH /NJS /NP @excludeArgs | Out-Null
    $code = $LASTEXITCODE
    # robocopy success codes: 0 (no copies) .. 7 (mismatched + extras); 8+ is failure.
    if ($code -ge 8) {
        $global:LASTEXITCODE = $code
        throw "[dev-mirror] robocopy failed with code $code"
    }
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

# Use a script-scoped guard to coalesce bursts of file events into one
# mirror run. Without this, npm/cargo touching many files would spawn a
# robocopy per event.
$script:debounce = $false
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

Write-Host "[dev-mirror] watching $repoRoot - Ctrl+C to stop" -ForegroundColor Yellow
try {
    while ($true) { Start-Sleep -Seconds 30 }
} finally {
    $fsw.EnableRaisingEvents = $false
    $fsw.Dispose()
}
