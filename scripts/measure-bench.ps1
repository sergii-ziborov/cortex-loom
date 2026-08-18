# Sample CPU/RAM (and GPU if nvidia-smi exists) while cortex-bench runs.
# Usage:
#   powershell -File scripts/measure-bench.ps1 -Set probe -Budget 4000 -Stamp local -Out .cortex-loom/bench/probe.json

param(
    [string]$Set = "probe",
    [int]$Budget = 4000,
    [string]$Stamp = "measured",
    [string]$Out = ".cortex-loom/bench/report.json",
    [string]$Repo = ".",
    [string]$Task = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$exe = Join-Path $root "target\release\cortex-bench.exe"
if (-not (Test-Path $exe)) {
    throw "build release cortex-bench first: cargo build -p cortex-bench --release"
}

$args = @("--repo", $Repo, "--budget", "$Budget", "--set", $Set, "--stamp", $Stamp, "--out", $Out)
if ($Task) {
    $args = @("--repo", $Repo, "--budget", "$Budget", "--task", $Task, "--stamp", $Stamp, "--out", $Out)
}

$hostInfo = [ordered]@{
    os           = [System.Environment]::OSVersion.VersionString
    arch         = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    logicalCpus  = [System.Environment]::ProcessorCount
    cpuName      = (Get-CimInstance Win32_Processor | Select-Object -First 1 -ExpandProperty Name)
    ramBytes     = [int64](Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
    gpus         = @(Get-CimInstance Win32_VideoController | ForEach-Object { $_.Name })
    rustc        = (rustc --version)
}

$gpuSmi = $null
if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) {
    $gpuSmi = (nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader).Trim()
}

$started = Get-Date
$proc = Start-Process -FilePath $exe -ArgumentList $args -WorkingDirectory $root -PassThru -NoNewWindow
$peakWs = 0L
$peakPaged = 0L
$samples = 0
$cpuSamples = New-Object System.Collections.Generic.List[double]

while (-not $proc.HasExited) {
    try {
        $proc.Refresh()
        if ($proc.WorkingSet64 -gt $peakWs) { $peakWs = $proc.WorkingSet64 }
        if ($proc.PagedMemorySize64 -gt $peakPaged) { $peakPaged = $proc.PagedMemorySize64 }
        $cpuSamples.Add([double]$proc.CPU)
    } catch {
        # process may exit between HasExited and Refresh
    }
    $samples += 1
    Start-Sleep -Milliseconds 250
}
$proc.WaitForExit()
$elapsedMs = [int]((Get-Date) - $started).TotalMilliseconds
$cpuSeconds = if ($cpuSamples.Count -gt 0) { [math]::Round($cpuSamples[-1], 3) } else { 0 }

$sidecar = [ordered]@{
    stamp            = $Stamp
    set              = $Set
    task             = $Task
    budget           = $Budget
    report           = $Out
    exitCode         = $proc.ExitCode
    elapsedMs        = $elapsedMs
    cpuSeconds       = $cpuSeconds
    peakWorkingSetMb = [math]::Round($peakWs / 1MB, 1)
    peakPagedMb      = [math]::Round($peakPaged / 1MB, 1)
    samples          = $samples
    gpu              = $gpuSmi
    host             = $hostInfo
    note             = "Default context bench is CPU-only (graph + compile). GPU is unused unless CORTEX_SEMANTIC=1 or a gated local profile is on."
}

$sidecarPath = [System.IO.Path]::ChangeExtension($Out, ".resources.json")
$dir = Split-Path -Parent $sidecarPath
if ($dir) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
$sidecar | ConvertTo-Json -Depth 6 | Set-Content -Path $sidecarPath -Encoding utf8
Write-Host "resources: $sidecarPath  peak=$($sidecar.peakWorkingSetMb) MB  cpu=$cpuSeconds s  wall=$elapsedMs ms  exit=$($proc.ExitCode)"
if ($proc.ExitCode -ne 0) { exit $proc.ExitCode }
