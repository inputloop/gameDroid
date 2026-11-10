# Track 1 guesses: how crosvm or QEMU would have gone

Written 2026-11-10, when the owner chose to lead with the engine track (bvmm)
instead of running the crosvm and QEMU spikes to the end.

Everything below marked GUESS is a prediction, not a finding. None of it was
tested. The percentages are my confidence that the statement would have turned
out true. They are kept so that (a) planning has something to stand on, and
(b) if any of this is ever tested, we can see how good the guesses were.

Rule for this file: guesses are never edited to match later results. A result
is appended under the guess it tests, with a date.

## What is actually known (not guesses)

- crosvm boots the prebuilt Android 17 Cuttlefish image under KVM on Linux
  with software graphics, 77 s on a 2-core CI runner.
  `docs/phase0/results/2026-11-10-ci-linux-crosvm.md`
- That image exposes a multitouch touchscreen and keyboard as virtual input
  devices, and lists `arm64-v8a` among its ABIs.
- crosvm documents a Windows source build with WHPX and says that path is not
  tested upstream. QEMU documents gfxstream for Linux hosts only.
- The Windows laptop has a working hypervisor, an RTX 3080 Ti Laptop GPU on
  driver 617.14 with Vulkan 1.4, Rust and Visual Studio.

## Guesses

### Linux

| # | GUESS | Confidence |
|---|---|---|
| L1 | crosvm with gfxstream boots Android with real GPU acceleration on an NVIDIA 3080 Ti under the proprietary driver, using Cuttlefish's launcher as shipped | 70% |
| L2 | If L1 fails, the cause is host display or EGL setup with NVIDIA (Wayland versus X11, headless surface creation), not gfxstream itself | 65% |
| L3 | Vulkan inside the guest works through gfxstream on that setup | 65% |
| L4 | Frame rate in a 3D benchmark reaches at least half of native for the same scene | 55% |
| L5 | QEMU from the distribution boots the same image through Cuttlefish's QEMU mode with software graphics | 75% |
| L6 | QEMU with gfxstream needs a custom QEMU build (rutabaga enabled); the distribution's package does not have it | 85% |
| L7 | After that custom build, QEMU with gfxstream gives accelerated Android on Linux | 55% |
| L8 | On Linux, crosvm would have beaten QEMU on the decision rule (fewer patches, first-class gfxstream) | 80% |

### Windows

| # | GUESS | Confidence |
|---|---|---|
| W1 | The stock Android Emulator boots Android 17 with `-gpu host` on the laptop, with the NVIDIA card as renderer | 90% |
| W2 | The Parsec virtual display adapter causes a wrong-adapter or black-window problem in at least one tool we would have tried | 35% |
| W3 | crosvm builds on Windows from public source with the documented command, after no more than a day of fixes | 60% |
| W4 | That crosvm boots a Linux kernel to a serial console under WHPX | 50% |
| W5 | That crosvm boots Android to the home screen with software graphics | 35% |
| W6 | That crosvm boots Android with gfxstream GPU acceleration | 20% |
| W7 | The main blocker for W5 and W6 is not the hypervisor but everything around it: Cuttlefish's launcher and its helper processes (vhost-user input, networking, adb proxy) are Linux only, so the roughly forty device arguments would have to be assembled and substituted by hand | 75% |
| W8 | QEMU for Windows with WHPX boots Android to the home screen with software graphics | 60% |
| W9 | QEMU for Windows gives gfxstream acceleration | 10% |
| W10 | QEMU for Windows gives Android acceleration through virgl (the path bContain uses for Linux guests) | 30% |
| W11 | Track 1 would have ended with a different engine per host: crosvm on Linux, and on Windows either the stock emulator's engine or a long patch series | 55% |

### Across both

| # | GUESS | Confidence |
|---|---|---|
| X1 | Track 1 gets a GPU-accelerated Android on Linux well before the engine track does | 90% |
| X2 | Track 1 gets a GPU-accelerated Android on Windows before the engine track does | 50% |
| X3 | The ARM translator inside the stock image is a prebuilt binary without a clear redistribution grant, not an open component we can ship | 55% |
| X4 | gfxstream on the host side works on the laptop's NVIDIA driver on Windows (it is what the stock emulator uses) | 85% |
| X5 | The Android guest needs virtio over PCI, not virtio-mmio, with the stock kernel | 70% |

## What these guesses are used for

Planning assumptions the engine track takes from them:

1. gfxstream stays the graphics stack (from L1, X4). bvmm embeds it through
   rutabaga instead of inventing a transport.
2. The Cuttlefish prebuilt image stays the guest (known, not guessed).
3. crosvm on Linux is the reference to compare bvmm against: same image, same
   host, and its device list is the checklist of what Android expects (from
   L1, L8).
4. Windows is where our own engine is most worth having (from W4 to W7, W11).
5. bvmm should plan for a PCI transport (from X5) and confirm that early.

## What was dropped

The remaining phase 0 spikes for crosvm and QEMU are parked, not deleted:
`.github/workflows/spike-linux-boot.yml` and `tools/hwtest/win-control.ps1`
stay in the repository and now run only when started by hand.

## Results appended

Guesses above are left as written. Results go here, by guess number.

- **X5** (guess: Android needs virtio over PCI, 70%). 2026-11-10: TRUE. The
  stock kernel has `CONFIG_VIRTIO_PCI=m` and no `CONFIG_VIRTIO_MMIO`; all 38
  virtio devices are on PCI. `docs/engine/reference/README.md`
- **X3** (guess: the ARM translator in the stock image is a prebuilt binary
  without a clear grant, 55%). 2026-11-10: the premise was wrong. There is no
  translator in the image at all: native bridge is disabled and none of the
  known translator libraries are present, although the ABI list names
  `arm64-v8a`. Scored as a miss. Same source.
