# E5 and E6: stock Android 17 boots on bvmm and shows its lock screen

Date: 2026-11-10. Host: GitHub `ubuntu-24.04` runner, KVM (nested, AMD, 2
cores, no GPU). Confidence: live-verified from the run's published log and
screenshot (`ci-logs` branch, `engine-android/`).
Run: https://github.com/inputloop/gameDroid/actions/runs/42912776068
gameDroid ba4fe7d, bContain 8115c98.

## Result

Android reported boot completed 38.9 seconds after the kernel started:

    [   38.886850] init: processing action (sys.boot_completed=1) from (/system/etc/init/hw/init.rc:1237)

and presented its lock screen to bvmm's display device, 720 by 1280: the date,
the clock, the battery indicator and "Unlock for all features and data".

This is the unmodified stock image (`aosp_cf_x86_64_only_phone-userdebug`,
build 16644040, Android 17). Nothing in it was rebuilt or patched. Everything
that differs from Google's own way of running it is on the host side.

No GPU acceleration is involved: Android draws with the CPU inside the guest
and bvmm copies finished frames out.

## What it took, in the order the boot demanded it

Each item is something the previous attempt stopped on.

| Attempt | Got as far as | Stopped because | Fix |
|---|---|---|---|
| 1 | Kernel, first-stage init, drivers loaded, disk found with all 12 partitions | (the job's own summary script failed; no display yet) | fixed the script, added a display |
| 2 | Recovery mode, looping | The kernel rejected the boot configuration: a key was set both by the image and by us ("Value is redefined"), so Android got no settings at all | merge the two, ours replacing theirs |
| 3 | System and data mounted, zygote, surfaceflinger and system_server running, one blank frame presented | system_server waited for the lights service, which crash-looped without vsock; eight other services crash-looped for want of console ports; the random-number seeder had no hardware source | silent console ports, a minimal vsock, a random source |
| 4 | Past the lights service | system_server waited about 200 s for the sensors service, then Android's watchdog restarted it. The sensors service was waiting for a host helper to answer "list-sensors" | answer it (`core/device/src/cuttlefish.rs`) |
| 5 | **Boot completed, lock screen shown** | | |

## What is in the machine

One PCI bus carrying: the system disk (a GPT disk assembled from the image's
partition files without copying them), a 2D display, a touchscreen, a
keyboard, 20 console ports (19 silent, one answering the sensors service), a
vsock address with no host services, and a random source. Two processors,
4 GiB. A 16550 serial port carries the kernel and init log.

The host does the bootloader's work ahead of time
(`tools/engine/android_prepare.py`): it takes the kernel out of the boot
image, joins the vendor and generic ramdisks, appends the boot configuration,
and writes the partition table.

## Known defects and limits

- **Colours.** In this first frame red and blue were swapped. The guest's
  composer writes pixels in an order chosen by a boot setting, and the stock
  launcher's choice assumes its own viewer. Changed to the order the buffer's
  label says; re-run pending at the time of writing.
- **One service still crash-loops**: `vendor.threadnetwork_hal` (Thread
  radio), restarting every five seconds. It did not block the boot.
- **No network**, no sound, no adb. Wi-Fi and mobile data have no device
  behind them.
- **Verified boot runs unlocked**, as on the stock virtual device, and
  reports a verification error it is allowed to ignore in that state.
- **The system partition is opened read-write** and the user data image is
  written in place. Fine for CI; a real install needs copy-on-write.
- **Userdebug build**: rooted and debuggable. Not what would be shipped.
- **Linux and KVM only.** On Windows the engine has one processor and the
  Android kernel has a coarse clock; Android has not been tried there.
- **Performance is not measured.** 39 seconds to boot on two shared cores
  says little about a real machine either way.

## Why this counts as E5 and E6

E5 was "Android boots headless, `sys.boot_completed=1`". E6 was "a picture:
virtio-gpu 2D scanout, Android screen visible with guest software rendering".
Both are met on KVM. E6's "in a host window" part is not: frames are read by a
headless runner and saved to a file. A window belongs to gameDroid's launcher,
which does not exist yet.
