# SPDX-License-Identifier: GPL-3.0-or-later
# gameDroid phase 0 host probe (Windows). Read-only: installs nothing, changes nothing.
# Usage:  powershell -ExecutionPolicy Bypass -File tools\hwtest\probe.ps1 [-OutDir <dir>]
# Run from an elevated (Administrator) prompt if you can: the Windows feature
# checks need it. Without elevation those lines read UNKNOWN and the rest still works.
param([string]$OutDir = ".")

$ErrorActionPreference = "Continue"
$ProbeVersion = 1
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$out = Join-Path $OutDir "hwtest-probe-windows-$stamp.txt"
$lines = New-Object System.Collections.Generic.List[string]

function KV([string]$k, $v) { $script:lines.Add(("{0}: {1}" -f $k, $v)) }
function Section([string]$n) { $script:lines.Add(""); $script:lines.Add("[$n]") }
function Have([string]$c) { return [bool](Get-Command $c -ErrorAction SilentlyContinue) }
function Tool([string]$name, [string[]]$verArgs) {
  if (Have $name) {
    try { $v = (& $name @verArgs 2>&1 | Select-Object -First 1) } catch { $v = "present, version call failed" }
    KV $name $v
  } else { KV $name "MISSING" }
}
function Feature([string]$name) {
  try {
    $f = Get-WindowsOptionalFeature -Online -FeatureName $name -ErrorAction Stop
    return $f.State
  } catch { return "UNKNOWN (needs Administrator, or feature not on this edition)" }
}

KV "probe" "gamedroid-hwtest-probe"
KV "probe_version" $ProbeVersion
KV "platform" "windows"
KV "utc" ((Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"))
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
KV "elevated" $admin

Section "os"
$os = Get-CimInstance Win32_OperatingSystem
KV "caption" $os.Caption
KV "version" $os.Version
KV "build" $os.BuildNumber
KV "arch" $os.OSArchitecture

Section "cpu"
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$cs = Get-CimInstance Win32_ComputerSystem
KV "model" $cpu.Name
KV "logical_cores" $cs.NumberOfLogicalProcessors
KV "mem_total_gb" ([math]::Round($cs.TotalPhysicalMemory / 1GB, 1))
# Note: when a hypervisor is already running, firmware flags can read False
# because the hypervisor hides them. hypervisor_present is the one to trust then.
KV "virt_firmware_enabled" $cpu.VirtualizationFirmwareEnabled
KV "slat" $cpu.SecondLevelAddressTranslationExtensions
KV "hypervisor_present" $cs.HypervisorPresent

Section "features"
$whp = Feature "HypervisorPlatform"
KV "HypervisorPlatform" $whp
KV "VirtualMachinePlatform" (Feature "VirtualMachinePlatform")
KV "Hyper-V" (Feature "Microsoft-Hyper-V-All")
KV "winhvplatform_dll" (Test-Path "$env:SystemRoot\System32\WinHvPlatform.dll")

Section "gpu"
$i = 0
foreach ($g in (Get-CimInstance Win32_VideoController)) {
  KV "gpu$i" ("{0} | driver {1} | {2}" -f $g.Name, $g.DriverVersion, $g.DriverDate)
  $i++
}
if (Have "nvidia-smi") {
  KV "nvidia" (& nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader 2>&1 | Select-Object -First 1)
} else { KV "nvidia" "nvidia-smi MISSING" }
if (Have "vulkaninfo") {
  $lines.Add("vulkan:")
  & vulkaninfo --summary 2>&1 |
    Select-String -Pattern "deviceName|deviceType|apiVersion|driverName|driverInfo|Vulkan Instance Version" |
    Select-Object -First 40 | ForEach-Object { $lines.Add("    " + $_.Line.Trim()) }
} else {
  KV "vulkan" "vulkaninfo MISSING (comes with the Vulkan SDK; vulkan-1.dll check below still applies)"
}
KV "vulkan_loader_dll" (Test-Path "$env:SystemRoot\System32\vulkan-1.dll")

Section "disk"
try {
  $drive = (Get-Location).Drive
  KV "free_here_gb" ([math]::Round($drive.Free / 1GB, 1))
} catch { KV "free_here_gb" "unknown" }

Section "tools"
Tool "git" @("--version")
Tool "cargo" @("--version")
Tool "rustc" @("--version")
Tool "qemu-system-x86_64" @("--version")
Tool "crosvm" @("--version")
Tool "adb" @("--version")
Tool "emulator" @("-version")
Tool "python" @("--version")
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
if (Test-Path $vswhere) {
  KV "visual_studio" ((& $vswhere -latest -products * -property catalog_productDisplayVersion 2>&1 | Select-Object -First 1))
} else { KV "visual_studio" "MISSING (needed to build crosvm with MSVC)" }
KV "android_sdk_root" $(if ($env:ANDROID_SDK_ROOT) { $env:ANDROID_SDK_ROOT } elseif ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { "not set" })

Section "verdict"
$ok = $true
if (-not $cs.HypervisorPresent -and -not $cpu.VirtualizationFirmwareEnabled) {
  KV "blocker" "virtualization looks disabled in firmware (enable VT-x/AMD-V)"; $ok = $false
}
if ("$whp" -eq "Disabled") {
  KV "blocker" "Windows Hypervisor Platform feature is off (Turn Windows features on or off)"; $ok = $false
} elseif ("$whp" -ne "Enabled") {
  KV "note" "could not read the Windows Hypervisor Platform state; rerun elevated"
}
KV "ready_for_boot_spike" $(if ($ok) { "yes" } else { "no" })

$lines | Set-Content -Path $out -Encoding UTF8
Write-Output $out
