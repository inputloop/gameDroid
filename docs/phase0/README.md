# Phase 0: spikes

Started 2026-11-10. Goal: learn, on real hardware, whether a Cuttlefish-derived
Android 17 guest boots with GPU acceleration on Linux and on Windows, and choose
the VMM. Outcome is recorded in `docs/adr/0001-vmm-choice.md`.

Nothing in this phase is product code. Spikes are thrown away; findings are kept.

## Status

| Step | State | Evidence |
|---|---|---|
| 0.1 Host probe scripts | Done. Both run green on GitHub's Linux and Windows runners | `tools/hwtest/`, workflow `hwtest-probe` run 2 |
| 0.2 Owner runs the probes on both 3080 Ti machines | Waiting on owner | result files |
| 0.3a Linux boot, crosvm, software GPU, in CI | Done: Android 17 boots in 77 s | `results/2026-11-10-ci-linux-crosvm.md` |
| 0.3b Linux boot, QEMU, in CI | Attempted, inconclusive (script fault, QEMU not installed) | same file |
| 0.3c Linux boot with gfxstream on the 3080 Ti | Not started; needs 0.2 | |
| 0.4 Windows control: stock Android Emulator | Not started; needs 0.2 | |
| 0.5a Windows: build crosvm with WHPX in CI | Not started. The Windows runner has the hypervisor platform on, so this can be tried without the owner | |
| 0.5b Windows boot with gfxstream on the 3080 Ti | Not started; needs 0.2 and 0.5a | |
| 0.6 Overlay or rebuild: classify guest changes | Draft below | |
| 0.7 ADR 0001 decided | Open | |

What CI can and cannot do, learned from the probes: GitHub's Linux runner has
KVM and its Windows runner has the Windows Hypervisor Platform enabled. Neither
has a GPU. So VMM boot spikes can run in CI; anything about graphics still
needs the owner's machines.

## What research says before any hardware is touched

Web searches on 2026-11-10; summaries only, sources at the end.

Linux looks straightforward:
- Cuttlefish publishes prebuilt images. Target `aosp_cf_x86_64_only_phone`,
  userdebug, from ci.android.com, plus `cvd-host_package.tar.gz` from the same build.
- Host packages (`cuttlefish-base`, `cuttlefish-user`) are Debian packages
  built from the android-cuttlefish repository. The user joins the `kvm`,
  `cvdnetwork` and `render` groups and reboots.
- `launch_cvd` starts the device under crosvm. GPU mode and VM manager are
  launch flags; their exact names were not confirmed by search and will be
  read from `launch_cvd --help` during the spike.
- QEMU has rutabaga and gfxstream support since 8.2, documented for Linux hosts.

Windows is the real unknown, as expected:
- crosvm lists WHPX as a Windows hypervisor and documents a source build
  (`cargo build --features all-msvc64,whpx` after `tools/install-deps.ps1`).
  Its own hypervisor table says WHPX is not tested upstream. No prebuilt
  Windows binary was found. Nothing was found on gfxstream in that build.
- QEMU's gfxstream documentation speaks only of Linux hosts. No evidence
  either way for a Windows host.
- Cuttlefish's launcher tooling is Linux only (from memory, not searched). On
  Windows we would start the VMM ourselves against the same disk images.

That is why step 0.4 exists. The stock Android Emulator already ships
gfxstream and WHPX on Windows as a prebuilt download. Running it first on the
Windows 3080 Ti machine tells us whether the driver, Vulkan and hypervisor
stack is healthy before we spend effort on crosvm or QEMU there. If the control
fails, the fault is in the machine setup, not in our choices.

## Spike matrix

Each cell is one run of a stage 2 script, producing one result file.

| | Linux, 3080 Ti | Windows, 3080 Ti |
|---|---|---|
| Control | Cuttlefish as shipped (`launch_cvd` defaults) | Stock Android Emulator, Android 17 x86_64 image |
| crosvm + gfxstream | `launch_cvd`, gfxstream GPU mode | crosvm built from source with WHPX, started by hand |
| QEMU + gfxstream | `launch_cvd` with the QEMU VM manager, or QEMU by hand | QEMU for Windows with WHPX, if a rutabaga build is possible at all |

Recorded per cell: boots to home screen (yes or no), time to boot, GLES and
Vulkan renderer strings reported inside the guest (this is the proof of GPU
acceleration: the string must name the gfxstream path, not a software
renderer), frame rate of one fixed 3D benchmark APK, anything that had to be
patched or worked around, and how long the setup took.

## How the VMM gets chosen

In order; the first rule that separates the candidates decides.

1. Must boot with GPU acceleration on both hosts. A candidate that cannot do
   Windows is out, however good it is on Linux.
2. Fewest patches we would have to carry.
3. Frame rate and boot time, if they differ by more than run-to-run noise.
4. License fit. crosvm (BSD) can be linked or patched in our tree; QEMU
   (GPLv2) stays a separate process with its patches kept apart.

If neither works on Windows, the fallback is to drive the Android Emulator's
own engine on Windows for the first release and keep crosvm on Linux. That
costs us one engine per host and would be written up as its own ADR.

## Guest changes: overlay or rebuild (draft)

We have no AOSP build machine, so each planned guest change is sorted by
whether it can ride on a prebuilt Cuttlefish image. To be confirmed in step
0.3; the "how" column is expectation, not test.

| Change | Overlay on prebuilt? | How |
|---|---|---|
| gamedroid-agent (foreground app, rotation, clipboard) | Yes | Installed as an APK; privileged bits through adb on userdebug |
| Display size, density, refresh rates | Likely yes | Launch flags and system properties |
| Virtual touch, keyboard, mouse, gamepads | Yes | Host-side virtio-input devices; key layout files pushed to a writable partition |
| Landscape, tablet defaults, no soft keyboard | Yes | Settings and properties at first boot |
| microG or Google apps, ARM translator | Yes on userdebug | Remount and push, as community scripts do; user-fetched only |
| Sensors fed from the host | Unknown | Depends on what the Cuttlefish sensor HAL accepts |
| Haptics back to the controller | Likely yes | Agent listens for vibration and reports over the control channel; may miss some sources |
| Custom device identity (model, fingerprint) | Partly | Some properties are read-only without a rebuilt image |
| Release build without root or debugging | No | A user build of our own device target needs a rebuild |
| New HALs, framework patches | No | Rebuild |

Consequence already visible: the honest, rootless release image that section
8.2 of the design plan calls for cannot be made without a build machine. That
does not block phases 0 to 2, which can run on userdebug, but it must be
solved before any public release.

## What the owner does now

Run the stage 1 probe on each machine and hand back the two files. See
`tools/hwtest/README.md`. About a minute per machine.

## Sources

- Cuttlefish get started: https://source.android.com/docs/devices/cuttlefish/get-started
- crosvm on Windows: https://crosvm.dev/book/building_crosvm/windows.html
- crosvm hypervisors: https://crosvm.dev/book/hypervisors.html
- QEMU virtio-gpu: https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html
- Building QEMU with rutabaga: https://linaro.atlassian.net/wiki/spaces/ORKO/pages/28985622530/Building+QEMU+with+virtio-gpu+and+rutabaga+gfx
- GitHub-hosted runners: https://docs.github.com/en/actions/concepts/runners/github-hosted-runners
