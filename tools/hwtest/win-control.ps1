# SPDX-License-Identifier: GPL-3.0-or-later
# gameDroid phase 0, step 0.4: Windows control run.
# Boots Google's stock Android Emulator (which already ships gfxstream and WHPX
# support) so we learn whether this machine's hypervisor, driver and Vulkan
# stack are healthy before any crosvm or QEMU work is tried here.
#
# Everything is downloaded into one folder (.hwtest in the repository by
# default). Nothing is installed system-wide. Delete that folder to undo it all.
#
# Usage, from the repository folder:
#   powershell -ExecutionPolicy Bypass -File tools\hwtest\win-control.ps1 -AcceptLicenses
#
# -AcceptLicenses   You accept the Android SDK license terms. Required: the
#                   SDK tools will not download without it. Read them first at
#                   https://developer.android.com/studio/terms
# -Gpu host         (default) use the real GPU. swiftshader_indirect = software.
# -Headless         no emulator window (used by CI).
param(
  [string]$WorkDir = "",
  [ValidateSet("host", "swiftshader_indirect")][string]$Gpu = "host",
  [switch]$AcceptLicenses,
  [switch]$Headless,
  [switch]$ForceLocalJdk,
  [string]$Image = "",
  [int]$BootTimeoutSec = 900,
  [string]$OutDir = "."
)

$ErrorActionPreference = "Continue"
$ProgressPreference = "SilentlyContinue"
$ControlVersion = 1
$CmdlineToolsUrl = "https://dl.google.com/android/repository/commandlinetools-win-11076708_latest.zip"
$JdkUrl = "https://aka.ms/download-jdk/microsoft-jdk-17-windows-x64.zip"

if (-not $WorkDir) { $WorkDir = Join-Path (Resolve-Path (Join-Path $PSScriptRoot "..\..")) ".hwtest" }
New-Item -ItemType Directory -Force -Path $WorkDir, $OutDir | Out-Null
$WorkDir = (Resolve-Path $WorkDir).Path
$stamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ")
$out = Join-Path (Resolve-Path $OutDir).Path "hwtest-wincontrol-$stamp.txt"
$lines = New-Object System.Collections.Generic.List[string]
function KV([string]$k, $v) { $script:lines.Add(("{0}: {1}" -f $k, $v)); Write-Host ("{0}: {1}" -f $k, $v) }
function Section([string]$n) { $script:lines.Add(""); $script:lines.Add("[$n]"); Write-Host "`n[$n]" }
function Block([string]$label, $text) {
  $script:lines.Add("${label}:")
  foreach ($l in @($text)) { if ("$l".Trim()) { $script:lines.Add("    " + "$l".Trim()) } }
}
function Finish([string]$verdict, [int]$code) {
  Section "verdict"
  KV "control_result" $verdict
  $script:lines | Set-Content -Path $out -Encoding UTF8
  Write-Host "`nResult file: $out"
  $global:LASTEXITCODE = $code
  exit $code
}
function Fetch([string]$url, [string]$dest) {
  & curl.exe -fsSL --retry 3 -o $dest $url
  return ($LASTEXITCODE -eq 0 -and (Test-Path $dest))
}
function Run([string]$cmdline) {  # run through cmd so stderr merges without PowerShell error records
  return (& cmd.exe /c "$cmdline 2>&1")
}

KV "probe" "gamedroid-hwtest-wincontrol"
KV "control_version" $ControlVersion
KV "utc" ((Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ"))
KV "gpu_mode_requested" $Gpu
KV "headless" ([bool]$Headless)

if (-not $AcceptLicenses) {
  Write-Host "`nThis run downloads the Android SDK tools, which have license terms:"
  Write-Host "  https://developer.android.com/studio/terms"
  Write-Host "If you accept them, run again with -AcceptLicenses."
  Finish "NOT RUN (licenses not accepted)" 2
}

Section "setup"
$free = [math]::Round((Get-Item $WorkDir).PSDrive.Free / 1GB, 1)
KV "free_gb" $free
if ($free -lt 15) { KV "blocker" "need about 15 GB free in $WorkDir"; Finish "NOT RUN (disk)" 3 }

# Java, needed only by the SDK's install tools.
$haveJava = [bool](Get-Command java -ErrorAction SilentlyContinue)
if ($ForceLocalJdk -or -not $haveJava) {
  $jdkDir = Join-Path $WorkDir "jdk"
  if (-not (Get-ChildItem $jdkDir -Directory -ErrorAction SilentlyContinue)) {
    Write-Host "Downloading a private JDK into $jdkDir ..."
    $zip = Join-Path $WorkDir "jdk.zip"
    if (-not (Fetch $JdkUrl $zip)) { KV "blocker" "JDK download failed"; Finish "FAILED (setup)" 4 }
    Expand-Archive -Path $zip -DestinationPath $jdkDir -Force
    Remove-Item $zip
  }
  $env:JAVA_HOME = (Get-ChildItem $jdkDir -Directory | Select-Object -First 1).FullName
  $env:Path = "$env:JAVA_HOME\bin;$env:Path"
  KV "java" "private copy in .hwtest"
} else { KV "java" "from PATH" }
KV "java_version" (Run "java -version" | Select-Object -First 1)

$sdk = Join-Path $WorkDir "android-sdk"
$env:ANDROID_SDK_ROOT = $sdk
$env:ANDROID_HOME = $sdk
$env:ANDROID_AVD_HOME = Join-Path $WorkDir "avd"
$env:ANDROID_USER_HOME = Join-Path $WorkDir "android-user"
New-Item -ItemType Directory -Force -Path $sdk, $env:ANDROID_AVD_HOME, $env:ANDROID_USER_HOME | Out-Null
$tools = Join-Path $sdk "cmdline-tools\latest\bin"
if (-not (Test-Path (Join-Path $tools "sdkmanager.bat"))) {
  Write-Host "Downloading Android command-line tools ..."
  $zip = Join-Path $WorkDir "cmdline-tools.zip"
  if (-not (Fetch $CmdlineToolsUrl $zip)) { KV "blocker" "command-line tools download failed"; Finish "FAILED (setup)" 4 }
  $tmp = Join-Path $WorkDir "cmdline-tmp"
  Expand-Archive -Path $zip -DestinationPath $tmp -Force
  New-Item -ItemType Directory -Force -Path (Join-Path $sdk "cmdline-tools") | Out-Null
  Move-Item (Join-Path $tmp "cmdline-tools") (Join-Path $sdk "cmdline-tools\latest")
  Remove-Item $zip; Remove-Item $tmp -Recurse -Force
}
$sdkmanager = Join-Path $tools "sdkmanager.bat"
$avdmanager = Join-Path $tools "avdmanager.bat"

Write-Host "Recording license acceptance ..."
# sdkmanager is a batch file; answers are fed from a file through cmd, which is
# the only form that reliably reaches it from Windows PowerShell.
$yesFile = Join-Path $WorkDir "yes.txt"
(1..60 | ForEach-Object { "y" }) | Set-Content -Path $yesFile -Encoding ASCII
Run "`"$sdkmanager`" --licenses --sdk_root=`"$sdk`" < `"$yesFile`"" | Out-Null

if (-not $Image) {
  $list = Run "`"$sdkmanager`" --list --sdk_root=`"$sdk`""
  $candidates = @($list | ForEach-Object { if ("$_" -match '(system-images;android-(\d+)[^;\s]*;([^;\s]+);x86_64)\s') { $Matches[1] } } | Sort-Object -Unique)
  # Prefer the newest API level, and plain AOSP images over ones with Google apps.
  $ranked = $candidates | Sort-Object -Property @(
    @{ Expression = { if ($_ -match 'android-(\d+)') { [int]$Matches[1] } else { 0 } }; Descending = $true },
    @{ Expression = { switch -Regex ($_) { ';default;' { 0 } ';aosp_atd;' { 1 } ';google_apis;' { 2 } default { 3 } } } }
  )
  $apiTop = if ($ranked.Count) { if ($ranked[0] -match 'android-(\d+)') { $Matches[1] } } else { "" }
  $Image = @($ranked | Where-Object { $_ -match "android-$apiTop[^;]*;(default|aosp_atd|google_apis);" } | Select-Object -First 1)[0]
  Block "images_offered_newest" ($ranked | Select-Object -First 8)
}
if (-not $Image) { KV "blocker" "no x86_64 system image found in the SDK list"; Finish "FAILED (setup)" 4 }
KV "system_image" $Image

Write-Host "Installing emulator, platform-tools and $Image (a few GB) ..."
$inst = Run "`"$sdkmanager`" --sdk_root=`"$sdk`" `"platform-tools`" `"emulator`" `"$Image`" < `"$yesFile`""
$emulator = Join-Path $sdk "emulator\emulator.exe"
$adb = Join-Path $sdk "platform-tools\adb.exe"
if (-not (Test-Path $emulator) -or -not (Test-Path $adb)) {
  Block "sdkmanager_output_tail" ($inst | Select-Object -Last 15)
  KV "blocker" "emulator or adb missing after install"; Finish "FAILED (setup)" 4
}
KV "emulator_version" (Run "`"$emulator`" -version" | Select-String -Pattern "emulator version" | Select-Object -First 1)

$avd = "gamedroid-control"
$noFile = Join-Path $WorkDir "no.txt"
"no" | Set-Content -Path $noFile -Encoding ASCII
$avdOut = Run "`"$avdmanager`" create avd -n $avd -k `"$Image`" --force < `"$noFile`""
$avdOk = Test-Path (Join-Path $env:ANDROID_AVD_HOME "$avd.ini")
KV "avd_created" $avdOk
if (-not $avdOk) { Block "avdmanager_output_tail" ($avdOut | Select-Object -Last 12); Finish "FAILED (setup)" 4 }

Section "hypervisor"
$accel = Run "`"$emulator`" -accel-check"
Block "accel_check" $accel
$accelOk = [bool]($accel | Select-String -Pattern "is installed and usable|WHPX.*usable|AEHD.*usable")
KV "accel_usable" $accelOk

Section "boot"
$log = Join-Path $WorkDir "emulator-$stamp.log"
$emuArgs = @("-avd", $avd, "-gpu", $Gpu, "-no-snapshot", "-no-boot-anim", "-no-audio", "-verbose")
if ($Headless) { $emuArgs += "-no-window" }
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Process -FilePath $emulator -ArgumentList $emuArgs -RedirectStandardOutput $log -RedirectStandardError "$log.err" -PassThru -WindowStyle Hidden
$booted = $false
while ($sw.Elapsed.TotalSeconds -lt $BootTimeoutSec) {
  Start-Sleep -Seconds 5
  if ($proc.HasExited) { KV "emulator_exited_early" $proc.ExitCode; break }
  $b = (Run "`"$adb`" shell getprop sys.boot_completed" | Select-Object -First 1)
  if ("$b".Trim() -eq "1") { $booted = $true; break }
}
$sw.Stop()
KV "booted" $booted
KV "seconds_to_boot" ([math]::Round($sw.Elapsed.TotalSeconds))

if ($booted) {
  Section "guest"
  foreach ($p in @("ro.build.fingerprint", "ro.build.version.release", "ro.build.version.sdk", "ro.product.cpu.abilist",
                   "ro.hardware.egl", "ro.hardware.vulkan", "ro.hardware.gralloc", "ro.kernel.qemu.gles", "ro.boot.qemu.gltransport.name")) {
    KV $p (Run "`"$adb`" shell getprop $p" | Select-Object -First 1)
  }
  Block "gles_renderer" (Run "`"$adb`" shell dumpsys SurfaceFlinger" | Select-String -Pattern "GLES:" | Select-Object -First 2 | ForEach-Object { $_.Line })
  Block "vulkan_device" (Run "`"$adb`" shell cmd gpu vkjson" | Select-String -Pattern '"deviceName"|"apiVersion"|"driverVersion"|"vendorID"' | Select-Object -First 6 | ForEach-Object { $_.Line })
}

Section "emulator_log"
$logText = @()
if (Test-Path $log) { $logText += Get-Content $log }
if (Test-Path "$log.err") { $logText += Get-Content "$log.err" }
Block "graphics_and_hypervisor_lines" ($logText | Select-String -Pattern "WHPX|hypervisor|HAXM|AEHD|Vulkan|vulkan|GPU|renderer|Renderer|gfxstream|ANGLE|swiftshader|GeForce|Parsec|Adapter|ERROR|FATAL" | Select-Object -First 60 | ForEach-Object { $_.Line })
if (-not $booted) { Block "log_tail" ($logText | Select-Object -Last 25) }

Run "`"$adb`" emu kill" | Out-Null
Start-Sleep -Seconds 5
if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
Run "`"$adb`" kill-server" | Out-Null

if ($booted) { Finish "BOOTED" 0 } else { Finish "DID NOT BOOT" 1 }
