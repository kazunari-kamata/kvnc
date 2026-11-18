[CmdletBinding()]
param(
    # Use the normal per-user path unless a separate deployment configuration is wanted.
    [string]$Config = (Join-Path $env:APPDATA "kvnc\config.toml"),
    # Explicit opt-in: affects only a configuration this script creates.
    [switch]$AllowInput,
    # Build and deploy the standalone binary without creating a configuration file.
    [switch]$SkipConfig
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$manifestPath = Join-Path $repositoryRoot "Cargo.toml"
$binaryDirectory = Join-Path $repositoryRoot "bin"
$builtBinary = Join-Path $repositoryRoot "target\release\kvnc.exe"
$installedBinary = Join-Path $binaryDirectory "kvnc.exe"

function Require-Command {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [string]$InstallHint
    )

    if ($null -eq (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "$Name was not found. $InstallHint"
    }
}

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Cargo.toml was not found beside this installer: $manifestPath"
}

Require-Command -Name "cargo" -InstallHint "Install the stable Rust MSVC toolchain from https://rustup.rs/."
Require-Command -Name "rustc" -InstallHint "Install the stable Rust MSVC toolchain from https://rustup.rs/."

$rustcInfo = @(& rustc -vV)
if ($LASTEXITCODE -ne 0) {
    throw "rustc -vV failed with exit code $LASTEXITCODE"
}
$hostLine = $rustcInfo | Where-Object { $_ -like "host:*" } | Select-Object -First 1
if ([string]::IsNullOrWhiteSpace($hostLine) -or $hostLine -notmatch "windows-msvc") {
    throw "kVNC Windows builds require a Rust MSVC host toolchain. Detected: $hostLine. Install or select stable-*-pc-windows-msvc, then retry."
}

if (Test-Path -LiteralPath $installedBinary -PathType Leaf) {
    $runningInstalledProcess = Get-CimInstance Win32_Process -Filter "Name = 'kvnc.exe'" -ErrorAction SilentlyContinue |
        Where-Object {
            -not [string]::IsNullOrWhiteSpace($_.ExecutablePath) -and
            [string]::Equals(
                [System.IO.Path]::GetFullPath($_.ExecutablePath),
                $installedBinary,
                [System.StringComparison]::OrdinalIgnoreCase
            )
        } |
        Select-Object -First 1
    if ($null -ne $runningInstalledProcess) {
        throw "kVNC is running from $installedBinary (PID $($runningInstalledProcess.ProcessId)). Stop that instance before replacing the executable; this installer will not terminate it."
    }
}

Write-Output "Building kVNC with the locked dependency set..."
Push-Location $repositoryRoot
try {
    & cargo build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $builtBinary -PathType Leaf)) {
    throw "release build completed without the expected executable: $builtBinary"
}

New-Item -ItemType Directory -Force -Path $binaryDirectory | Out-Null
Copy-Item -LiteralPath $builtBinary -Destination $installedBinary -Force

& $installedBinary --version
if ($LASTEXITCODE -ne 0) {
    throw "installed kVNC version check failed with exit code $LASTEXITCODE"
}
Write-Output "Installed standalone executable: $installedBinary"

if ($SkipConfig) {
    Write-Output "Configuration creation skipped. Create one later with: & '$installedBinary' --init"
    return
}

$Config = [System.IO.Path]::GetFullPath($Config)
if (Test-Path -LiteralPath $Config -PathType Leaf) {
    Write-Output "Preserved existing configuration: $Config"
    Write-Output "Review it with: & '$installedBinary' --config '$Config' --print-config"
} else {
    $initArguments = @("--config", $Config, "--init")
    if ($AllowInput) {
        $initArguments += "--allow-input"
    }

    & $installedBinary @initArguments
    if ($LASTEXITCODE -ne 0) {
        throw "initial configuration creation failed with exit code $LASTEXITCODE"
    }
    if ($AllowInput) {
        Write-Warning "Input control was enabled in the new configuration. Use only with trusted clients and a trusted listener."
    } else {
        Write-Output "Created safe configuration with local input disabled: $Config"
    }
}

Write-Output "Review the listener and input settings, then start with: & '$installedBinary' --config '$Config'"
