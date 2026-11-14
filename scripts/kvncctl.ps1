[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet("start", "stop", "restart", "status", "logs")]
    [string]$Action = "status",
    [string]$Config = "$env:APPDATA\kvnc\config.toml",
    [string]$PidFile = (Join-Path $PSScriptRoot "..\var\kvnc.pid"),
    [string]$LogFile = (Join-Path $PSScriptRoot "..\var\kvnc.log"),
    [ValidateRange(1, 65535)]
    [int]$Port = 5900
)

$ErrorActionPreference = "Stop"
$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$kvncBinary = Join-Path $repositoryRoot "bin\kvnc.exe"
$PidFile = [System.IO.Path]::GetFullPath($PidFile)
$LogFile = [System.IO.Path]::GetFullPath($LogFile)
$Config = [System.IO.Path]::GetFullPath($Config)

function Get-ManagedKvncProcess {
    if (-not (Test-Path -LiteralPath $PidFile)) {
        return $null
    }

    $pidText = (Get-Content -LiteralPath $PidFile -Raw).Trim()
    $processId = 0
    if (-not [int]::TryParse($pidText, [ref]$processId) -or $processId -le 0) {
        throw "PID file is invalid: $PidFile"
    }

    $process = Get-CimInstance Win32_Process -Filter "ProcessId = $processId" -ErrorAction SilentlyContinue
    if ($null -eq $process) {
        return $null
    }
    if ([string]::IsNullOrWhiteSpace($process.CommandLine) -or
        $process.CommandLine.IndexOf($kvncBinary, [System.StringComparison]::OrdinalIgnoreCase) -lt 0) {
        throw "PID file names PID $processId, but it is not the expected kVNC executable; refusing to manage it."
    }
    return $process
}

function Show-Listener {
    Write-Output "Listeners on TCP port $Port:"
    $listeners = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue
    if ($null -eq $listeners) {
        Write-Output "  none found"
        return
    }
    $listeners | Format-Table -AutoSize LocalAddress, LocalPort, OwningProcess
}

function Start-ManagedKvnc {
    if (-not (Test-Path -LiteralPath $kvncBinary)) {
        throw "kVNC executable does not exist: $kvncBinary"
    }
    if (-not (Test-Path -LiteralPath $Config)) {
        throw "configuration does not exist: $Config; run `& $kvncBinary --init` first"
    }
    if (Select-String -LiteralPath $Config -Pattern '^\s*daemon\s*=\s*true\s*(#.*)?$' -Quiet) {
        throw "Windows controller starts kVNC directly; set daemon = false or remove it from $Config"
    }

    $existing = Get-ManagedKvncProcess
    if ($null -ne $existing) {
        Write-Output "Managed kVNC is already running: PID $($existing.ProcessId)"
        return
    }
    if (Test-Path -LiteralPath $PidFile) {
        Remove-Item -LiteralPath $PidFile -Force
        Write-Output "Removed stale PID file: $PidFile"
    }

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $PidFile) | Out-Null
    $process = Start-Process -FilePath $kvncBinary -ArgumentList @("--config", ('"{0}"' -f $Config)) -WindowStyle Hidden -PassThru
    Set-Content -LiteralPath $PidFile -Value $process.Id -NoNewline
    Start-Sleep -Milliseconds 500
    $managed = Get-ManagedKvncProcess
    if ($null -eq $managed) {
        throw "kVNC exited during startup; inspect $LogFile"
    }
    Write-Output "Started managed kVNC: PID $($managed.ProcessId)"
}

function Stop-ManagedKvnc {
    $managed = Get-ManagedKvncProcess
    if ($null -eq $managed) {
        if (Test-Path -LiteralPath $PidFile) {
            Remove-Item -LiteralPath $PidFile -Force
            Write-Output "Removed stale PID file: $PidFile"
        } else {
            Write-Output "No managed kVNC PID file; nothing to stop."
        }
        return
    }

    Stop-Process -Id $managed.ProcessId
    try {
        Wait-Process -Id $managed.ProcessId -Timeout 10
    } catch {
        throw "kVNC PID $($managed.ProcessId) did not exit within 10 seconds; inspect it before forcing termination."
    }
    Remove-Item -LiteralPath $PidFile -Force -ErrorAction SilentlyContinue
    Write-Output "Stopped managed kVNC: PID $($managed.ProcessId)"
}

switch ($Action) {
    "start" { Start-ManagedKvnc }
    "stop" { Stop-ManagedKvnc }
    "restart" { Stop-ManagedKvnc; Start-ManagedKvnc }
    "status" {
        $managed = Get-ManagedKvncProcess
        if ($null -eq $managed) {
            Write-Output "No managed kVNC process: $PidFile"
        } else {
            Write-Output "Managed kVNC is running: PID $($managed.ProcessId)"
            Write-Output "Command: $($managed.CommandLine)"
        }
        Show-Listener
    }
    "logs" {
        if (-not (Test-Path -LiteralPath $LogFile)) {
            throw "log file does not exist: $LogFile"
        }
        Get-Content -LiteralPath $LogFile -Tail 100 -Wait
    }
}
