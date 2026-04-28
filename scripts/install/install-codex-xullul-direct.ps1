param(
    [string]$Release = $(if ([string]::IsNullOrWhiteSpace($env:CODEX_XULLUL_RELEASE)) { "latest" } else { $env:CODEX_XULLUL_RELEASE }),
    [string]$Repo = $(if ([string]::IsNullOrWhiteSpace($env:CODEX_XULLUL_REPO)) { "xullul/codex" } else { $env:CODEX_XULLUL_REPO }),
    [string]$CommandName = $(if ([string]::IsNullOrWhiteSpace($env:CODEX_XULLUL_COMMAND)) { "codex-xullul" } else { $env:CODEX_XULLUL_COMMAND })
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message"
}

function Assert-ValidRepo {
    param([string]$Value)
    if ($Value -notmatch "^[^/\s]+/[^/\s]+$") {
        throw "Repo must be in OWNER/REPO form."
    }
}

function Normalize-ReleaseTag {
    param([string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -eq "latest") {
        return "latest"
    }

    if ($Value.StartsWith("xullul-v")) {
        return $Value
    }

    return "xullul-v$Value"
}

function Get-ReleaseMetadata {
    $tag = Normalize-ReleaseTag -Value $Release
    if ($tag -eq "latest") {
        return Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    }

    return Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/tags/$tag"
}

function Get-ReleaseAsset {
    param(
        [object]$ReleaseMetadata,
        [string]$Name
    )

    $asset = $ReleaseMetadata.assets | Where-Object { $_.name -eq $Name } | Select-Object -First 1
    if ($null -eq $asset) {
        throw "Could not find release asset $Name in $($ReleaseMetadata.tag_name)."
    }

    return $asset
}

function Path-Contains {
    param(
        [string]$PathValue,
        [string]$Entry
    )

    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        return $false
    }

    $needle = $Entry.TrimEnd("\")
    foreach ($segment in $PathValue.Split(";", [System.StringSplitOptions]::RemoveEmptyEntries)) {
        if ($segment.TrimEnd("\") -ieq $needle) {
            return $true
        }
    }

    return $false
}

function Write-CommandShim {
    param(
        [string]$ShimPath,
        [string]$TargetExe
    )

    $shim = @"
@echo off
"$TargetExe" %*
"@
    Set-Content -LiteralPath $ShimPath -Value $shim -Encoding ASCII
}

if ($env:OS -ne "Windows_NT") {
    Write-Error "install-codex-xullul.ps1 supports Windows only."
    exit 1
}

if (-not [Environment]::Is64BitOperatingSystem) {
    Write-Error "$CommandName requires a 64-bit version of Windows."
    exit 1
}

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
if ($architecture -ne [System.Runtime.InteropServices.Architecture]::X64) {
    Write-Error "This direct installer currently supports Windows x64 only. Detected: $architecture."
    exit 1
}

Assert-ValidRepo -Value $Repo

$installDir = if ([string]::IsNullOrWhiteSpace($env:CODEX_XULLUL_INSTALL_DIR)) {
    Join-Path $env:LOCALAPPDATA "Programs\Xullul\Codex\bin"
} else {
    $env:CODEX_XULLUL_INSTALL_DIR
}

$releaseMetadata = Get-ReleaseMetadata
$tagName = [string]$releaseMetadata.tag_name
$version = if ($tagName.StartsWith("xullul-v")) { $tagName.Substring(8) } else { $tagName }

$exeAsset = Get-ReleaseAsset -ReleaseMetadata $releaseMetadata -Name "codex-xullul-windows-x64.exe"
$shaAsset = Get-ReleaseAsset -ReleaseMetadata $releaseMetadata -Name "codex-xullul-windows-x64.exe.sha256"

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("$CommandName-direct-install-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

try {
    $downloadedExe = Join-Path $tempDir "codex-xullul-windows-x64.exe"
    $downloadedSha = Join-Path $tempDir "codex-xullul-windows-x64.exe.sha256"

    Write-Step "Downloading $CommandName $version from $Repo"
    Invoke-WebRequest -Uri $exeAsset.browser_download_url -OutFile $downloadedExe
    Invoke-WebRequest -Uri $shaAsset.browser_download_url -OutFile $downloadedSha

    $expectedHash = ((Get-Content -LiteralPath $downloadedSha -Raw) -split "\s+")[0].ToLowerInvariant()
    if ($expectedHash -notmatch "^[0-9a-f]{64}$") {
        throw "Release checksum file did not contain a valid SHA-256 hash."
    }

    $actualHash = (Get-FileHash -LiteralPath $downloadedExe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) {
        throw "Downloaded $CommandName checksum did not match. Expected $expectedHash but got $actualHash."
    }

    New-Item -ItemType Directory -Force -Path $installDir | Out-Null
    $targetExe = Join-Path $installDir "$CommandName.exe"
    $shimPath = Join-Path $installDir "$CommandName.cmd"

    Copy-Item -LiteralPath $downloadedExe -Destination $targetExe -Force
    Write-CommandShim -ShimPath $shimPath -TargetExe $targetExe

    & $shimPath --version *> $null
    if ($LASTEXITCODE -ne 0) {
        throw "Installed $CommandName command failed verification."
    }
} finally {
    Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue
}

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not (Path-Contains -PathValue $userPath -Entry $installDir)) {
    $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) { $installDir } else { "$installDir;$userPath" }
    [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
    Write-Step "PATH updated for future PowerShell sessions."
} else {
    Write-Step "PATH is already configured for future PowerShell sessions."
}

if (-not (Path-Contains -PathValue $env:Path -Entry $installDir)) {
    $env:Path = if ([string]::IsNullOrWhiteSpace($env:Path)) { $installDir } else { "$installDir;$env:Path" }
}

Write-Host "$CommandName $version installed successfully."
