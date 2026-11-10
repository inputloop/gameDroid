# Result: Linux, crosvm, software GPU, GitHub runner

Date: 2026-11-10. Workflow `spike-linux-boot`, run 6
(https://github.com/inputloop/gameDroid/actions/runs/42888091022).
Confidence: live-verified, read from the CI job log. No GPU on this host, so
this says nothing about graphics acceleration or frame rate.

## Host

GitHub-hosted `ubuntu-24.04` runner: 2 cores (AMD EPYC), about 8 GB RAM, KVM
with nested virtualization available.

## What ran

- Cuttlefish host packages 1.57.14 from Google's apt repository
  (`us-apt.pkg.dev/projects/android-cuttlefish-artifacts`).
- Image fetched with `cvd fetch --default_build=aosp-android-latest-release/aosp_cf_x86_64_only_phone-userdebug`,
  which resolved to build 16644040.
- `launch_cvd --daemon --gpu_mode=guest_swiftshader --cpus=2 --memory_mb=4096`.

## Outcome

| Item | Value |
|---|---|
| Booted | Yes: `VIRTUAL_DEVICE_BOOT_COMPLETED`, `sys.boot_completed=1` |
| Launch to boot complete | 77 s (66 s on the previous run) |
| VMM | crosvm, from the host package |
| Android | 17, SDK 37 |
| Fingerprint | `generic/aosp_cf_x86_64_only_phone/vsoc_x86_64_only:17/CP2A.260605.016/16644040:userdebug/test-keys` |
| ABI list | `x86_64,arm64-v8a` |
| EGL / Vulkan HAL | `angle` / `pastel` |
| Renderer | ANGLE on Vulkan 1.3 SwiftShader (software), OpenGL ES 3.1 |
| Input devices in guest | Multitouch touchscreen, keyboard, rotary, switches (all vhost-user), power and sleep buttons |

## Things learned the hard way

1. Cuttlefish packages 1.57.14 do not install on Ubuntu 26.04 (unmet
   `libxml2` and other dependencies). Ubuntu 24.04 works.
2. Ubuntu 24.04 blocks crosvm's sandbox by default:
   `unshare(CLONE_NEWNS) failed: Operation not permitted`. Fixed with
   `sysctl kernel.apparmor_restrict_unprivileged_userns=0`. A narrower fix (an
   AppArmor profile for crosvm) should replace this before we tell users to do it.
3. Launch needs about 8.6 GB free for the data image on top of roughly 17 GB
   for the two 8 GB sparse images. Plan on 30 GB free.
4. `/dev/kvm` exists on the runner but is not open to the runner user until a
   udev rule is added.

## Worth following up

- The stock x86_64 image lists `arm64-v8a` as a supported ABI. That implies it
  already carries an ARM translator. Which one, how well it runs games, and
  under what license are not yet known. If it is an open AOSP component, the
  ARM translation question in the design plan gets much easier. Unverified.
- Input reaches the guest through vhost-user devices that include a multitouch
  touchscreen. That matches the design's host-side mapping model.

## Not achieved

- QEMU boot: not tested. The attempt failed on my script, not on QEMU: the
  earlier instance group was still registered ("Group is already running, try
  `cvd stop`"), and the host package ships no QEMU binary, so one must be
  installed separately. No conclusion about QEMU can be drawn.
