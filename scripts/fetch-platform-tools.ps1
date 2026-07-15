#requires -Version 5.1
<#
.SYNOPSIS
  Fetch the upstream Google Android Platform-Tools archive and stage
  adb.exe (+ fastboot.exe) as Tauri sidecars under
  src-tauri/binaries/<target-triple>/.

.DESCRIPTION
  Tauri's bundle.externalBin requires per-target-triple naming:

    binaries/adb-x86_64-pc-windows-msvc.exe
    binaries/fastboot-x86_64-pc-windows-msvc.exe

  This script handles ONLY the Windows targets (x86_64 + aarch64).
  The POSIX side lives in fetch-platform-tools.sh.

  The platform-tools archive Google publishes already bundles both
  binaries for every desktop platform under one URL per OS. We download
  the OS-appropriate ZIP, verify the SHA-256, extract, copy.

.PARAMETER Force
  Re-download even if the staged binaries already exist.

.PARAMETER Channel
  "stable" (default) or "preview". The preview channel pulls the same
  rolling upstream URL without a pinned checksum for local experiments.

.NOTES
  Stable metadata comes from platform-tools-policy.json. Bumping the policy
  version and checksums is a deliberate PR; never silent.
#>
[CmdletBinding()]
param(
    [switch] $Force,
    [ValidateSet("stable", "preview")] [string] $Channel = "stable"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$binDir = Join-Path $repoRoot "src-tauri\binaries"
$policyPath = Join-Path $repoRoot "platform-tools-policy.json"
$policy = Get-Content -LiteralPath $policyPath -Raw | ConvertFrom-Json
$stableDownload = $policy.downloads.windows

# Pinned upstream metadata. Update only platform-tools-policy.json after
# reviewing the official release notes and hashing each official archive.
$PinnedStable = @{
    Url    = $stableDownload.url
    Sha256 = $stableDownload.sha256
    Version = $policy.recommendedVersion
}
$PinnedPreview = $PinnedStable.Clone()
$PinnedPreview.Sha256 = "SKIP"  # rolling channel; checksum can't be pinned
$PinnedPreview.Version = "rolling"

$Pinned = if ($Channel -eq "preview") { $PinnedPreview } else { $PinnedStable }

if ($Channel -eq "stable" -and $Pinned.Sha256 -notmatch "^[0-9a-fA-F]{64}$") {
    throw "[fetch-platform-tools] stable checksum in platform-tools-policy.json is invalid; refusing to download."
}

$triples = @("x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc")

function Test-StagedComplete {
    foreach ($triple in $triples) {
        $adb = Join-Path $binDir "adb-$triple.exe"
        $fb  = Join-Path $binDir "fastboot-$triple.exe"
        if (-not ((Test-Path $adb) -and (Test-Path $fb))) {
            return $false
        }
    }
    return $true
}

if ((Test-StagedComplete) -and (-not $Force)) {
    Write-Host "[fetch-platform-tools] All Windows sidecars already staged. Use -Force to re-fetch." -ForegroundColor Green
    exit 0
}

New-Item -ItemType Directory -Force -Path $binDir | Out-Null

$tmp = New-Item -ItemType Directory -Force -Path (Join-Path ([IO.Path]::GetTempPath()) "droidsmith-pt-$([Guid]::NewGuid().ToString('N'))")
$zip = Join-Path $tmp "platform-tools.zip"

try {
    Write-Host "[fetch-platform-tools] Downloading $($Pinned.Url) ..." -ForegroundColor Cyan
    Invoke-WebRequest -Uri $Pinned.Url -OutFile $zip -UseBasicParsing

    if ($Pinned.Sha256 -ne "SKIP") {
        $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        $expected = $Pinned.Sha256.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw "[fetch-platform-tools] SHA-256 mismatch! expected $expected got $actual. Refusing to use the archive."
        }
        Write-Host "[fetch-platform-tools] SHA-256 verified." -ForegroundColor Green
    } else {
        Write-Warning "[fetch-platform-tools] $Channel channel: SHA-256 verification skipped (rolling channel)."
    }

    $extractRoot = Join-Path $tmp "extracted"
    Expand-Archive -LiteralPath $zip -DestinationPath $extractRoot -Force

    # The zip extracts to ./platform-tools/{adb.exe,fastboot.exe,...}
    $adbSrc = Join-Path $extractRoot "platform-tools\adb.exe"
    $fbSrc  = Join-Path $extractRoot "platform-tools\fastboot.exe"
    $sourceProperties = Join-Path $extractRoot "platform-tools\source.properties"
    if (-not (Test-Path $adbSrc)) { throw "adb.exe not found in archive" }
    if (-not (Test-Path $fbSrc))  { throw "fastboot.exe not found in archive" }
    if ($Channel -eq "stable") {
        $revision = (Get-Content -LiteralPath $sourceProperties | Where-Object { $_ -match '^Pkg\.Revision=' } | Select-Object -First 1) -replace '^Pkg\.Revision=', ''
        if ($revision -ne $Pinned.Version) {
            throw "[fetch-platform-tools] archive version $revision does not match policy version $($Pinned.Version)."
        }
    }

    # Google ships a single x86_64 binary that runs under emulation on
    # ARM Windows. Stage the same artefact for both triples until Google
    # publishes a native aarch64 build.
    foreach ($triple in $triples) {
        Copy-Item -Force $adbSrc (Join-Path $binDir "adb-$triple.exe")
        Copy-Item -Force $fbSrc  (Join-Path $binDir "fastboot-$triple.exe")
        Write-Host "[fetch-platform-tools] Staged adb+fastboot for $triple"
    }

    # Also copy the supporting DLLs adb.exe needs (AdbWinApi.dll, AdbWinUsbApi.dll).
    foreach ($dll in @("AdbWinApi.dll", "AdbWinUsbApi.dll")) {
        $src = Join-Path $extractRoot "platform-tools\$dll"
        if (Test-Path $src) {
            Copy-Item -Force $src (Join-Path $binDir $dll)
        }
    }

    Write-Host "[fetch-platform-tools] Done. Channel=$Channel Version=$($Pinned.Version)" -ForegroundColor Green
    exit 0
}
finally {
    if (Test-Path $tmp) {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}
