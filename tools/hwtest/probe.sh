#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# gameDroid phase 0 host probe (Linux). Read-only: installs nothing, changes nothing.
# Usage: tools/hwtest/probe.sh [output-dir]
# Writes hwtest-probe-linux-<date>.txt and prints its path.
set -u
PROBE_VERSION=1
outdir="${1:-.}"
mkdir -p "$outdir"
out="$outdir/hwtest-probe-linux-$(date -u +%Y%m%dT%H%M%SZ).txt"

kv()      { printf '%s: %s\n' "$1" "$2" >>"$out"; }
section() { printf '\n[%s]\n' "$1" >>"$out"; }
have()    { command -v "$1" >/dev/null 2>&1; }
first()   { "$@" 2>/dev/null | head -n 1; }
block()   { # block <label> <command...>: indented multi-line output, capped
  printf '%s:\n' "$1" >>"$out"; shift
  { "$@" 2>&1 || true; } | head -n 40 | sed 's/^/    /' >>"$out"
}
tool()    { # tool <name> <version-arg>
  if have "$1"; then kv "$1" "$(first "$1" "$2")"; else kv "$1" "MISSING"; fi
}

: >"$out"
kv probe "gamedroid-hwtest-probe"
kv probe_version "$PROBE_VERSION"
kv platform linux
kv utc "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

section os
if [ -r /etc/os-release ]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  kv distro "${PRETTY_NAME:-unknown}"
  kv distro_id "${ID:-unknown}"
  kv distro_like "${ID_LIKE:-}"
else
  kv distro unknown
fi
kv kernel "$(uname -r)"
kv arch "$(uname -m)"
kv session_type "${XDG_SESSION_TYPE:-unknown}"
kv desktop "${XDG_CURRENT_DESKTOP:-unknown}"

section cpu
kv model "$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ //')"
kv logical_cores "$(nproc 2>/dev/null || echo unknown)"
virt_flags="$(grep -m1 -oE 'vmx|svm' /proc/cpuinfo 2>/dev/null || true)"
kv virt_extension "${virt_flags:-NONE}"
kv mem_total_gb "$(awk '/MemTotal/ {printf "%.1f", $2/1048576}' /proc/meminfo 2>/dev/null)"

section kvm
if [ -c /dev/kvm ]; then
  kv dev_kvm present
  if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then kv dev_kvm_access ok; else kv dev_kvm_access "DENIED (add user to the kvm group)"; fi
else
  kv dev_kvm MISSING
  kv dev_kvm_access n/a
fi
kv groups "$(id -nG 2>/dev/null)"
kv nested "$(cat /sys/module/kvm_intel/parameters/nested 2>/dev/null || cat /sys/module/kvm_amd/parameters/nested 2>/dev/null || echo unknown)"

section gpu
if have lspci; then
  block pci_display sh -c "lspci -nnk | grep -A3 -Ei 'vga|3d|display' || echo 'none found'"
else
  kv pci_display "lspci MISSING"
fi
if have nvidia-smi; then
  kv nvidia "$(first nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader)"
else
  kv nvidia "nvidia-smi MISSING"
fi
kv nvidia_kernel_module "$(cat /proc/driver/nvidia/version 2>/dev/null | head -n 1 || true)"
if have vulkaninfo; then
  block vulkan sh -c "vulkaninfo --summary 2>&1 | grep -Ei 'deviceName|deviceType|apiVersion|driverName|driverInfo|Vulkan Instance Version'"
else
  kv vulkan "vulkaninfo MISSING (package vulkan-tools)"
fi
if have glxinfo; then
  block opengl sh -c "glxinfo -B 2>&1 | grep -Ei 'renderer string|core profile version|direct rendering'"
else
  kv opengl "glxinfo MISSING (package mesa-utils)"
fi

section disk
kv free_here "$(df -h . 2>/dev/null | awk 'NR==2 {print $4}')"
kv free_home "$(df -h "${HOME:-/}" 2>/dev/null | awk 'NR==2 {print $4}')"

section tools
tool git --version
tool cargo --version
tool rustc --version
tool qemu-system-x86_64 --version
tool crosvm --version
tool adb --version
tool docker --version
tool podman --version
tool curl --version
tool unzip -v
if have dpkg-query; then
  kv cuttlefish_base "$(dpkg-query -W -f='${Version}' cuttlefish-base 2>/dev/null || echo MISSING)"
  kv cuttlefish_user "$(dpkg-query -W -f='${Version}' cuttlefish-user 2>/dev/null || echo MISSING)"
else
  kv cuttlefish_base "n/a (not a Debian-family system)"
fi

section verdict
ok=1
[ -n "$virt_flags" ] || { kv blocker "CPU virtualization extension not visible (enable VT-x/AMD-V in firmware)"; ok=0; }
[ -c /dev/kvm ] || { kv blocker "/dev/kvm missing"; ok=0; }
{ [ -r /dev/kvm ] && [ -w /dev/kvm ]; } || [ ! -c /dev/kvm ] || { kv blocker "/dev/kvm not accessible to this user"; ok=0; }
have vulkaninfo || kv note "install vulkan-tools so the GPU's Vulkan support can be recorded"
if [ "$ok" = 1 ]; then kv ready_for_boot_spike yes; else kv ready_for_boot_spike no; fi

echo "$out"
