# What stock Android 17 expects from its virtual machine

Recorded 2026-11-10 by the `engine-android-oracle` CI job: the prebuilt
Cuttlefish image (`aosp_cf_x86_64_only_phone-userdebug`, build 16644040)
booted under crosvm with KVM, then inspected from inside and outside.
Confidence: live-verified, read from the job log
(https://github.com/inputloop/gameDroid/actions/runs/42896026438).
Raw capture: `android17-crosvm-summary.txt` in this folder.

This is bvmm's checklist for the Android milestones. Where bvmm and Android
disagree, this is what a working setup looks like.

## Findings that change the plan

1. **virtio must be on PCI.** The kernel (6.12.74-android16-6, a stock GKI
   build) has `CONFIG_VIRTIO_PCI=m` and no `CONFIG_VIRTIO_MMIO` at all. All 38
   virtio devices in the running guest sit on PCI bus 0. bContain's notes had
   planned virtio-mmio; for Android that is not an option without replacing
   the kernel. bvmm needs a PCI host bridge (the guest sees an Intel 440FX
   style bridge, 8086:1237) and modern virtio-pci devices (ids 0x1041 and up).
2. **No Hyper-V support in the Android kernel.** `CONFIG_HYPERV` is not set.
   On Windows, bvmm currently relies on Hyper-V enlightenments so the kernel
   can read its timer frequencies. Android's kernel will not use them. bvmm on
   WHP will need another way to give the guest a clock: emulate a PIT or HPET,
   or pass the TSC frequency on the command line and rely on the TSC deadline
   timer. This is the first Windows-specific risk for Android and should be
   tested early with a stock GKI kernel, long before graphics.
3. **Android boots through a bootloader here, not a direct kernel load.**
   crosvm is started with `--bios=bootloader.crosvm` (U-Boot) plus a flash
   image, and no kernel, initrd or command line arguments. bvmm boots kernels
   directly, so it must either run that bootloader (more platform emulation)
   or unpack the kernel and ramdisks from the boot images and build the
   command line and boot configuration itself. The second is likely less
   work; the captured command line and boot configuration below are the
   template.
4. **ACPI is available in the kernel** (`CONFIG_ACPI=y`), and so is the MP
   table parser (`CONFIG_X86_MPPARSE=y`), which is what bvmm provides today.
5. **The image has no ARM translator.** It lists `arm64-v8a` among its ABIs,
   but `ro.dalvik.vm.native.bridge` is `0` and none of the usual translator
   libraries exist (ndk_translation, berberis, houdini). ARM-only games will
   not run on this image as shipped. The earlier hope that the image already
   solved this was wrong.

## Devices the guest uses

| Kind | Count | Notes |
|---|---|---|
| virtio-blk | 3 | overlay (system), persistent composite, sdcard. Boot devices are the first two |
| virtio-console | 21 | one is the kernel console (`hvc0`); the rest are channels to host-side services (logcat, keymint, gatekeeper, Bluetooth, GNSS, sensors and others) or sinks |
| virtio-input | 4 | touchscreen, keyboard, rotary, switches; served by separate host processes over vhost-user |
| virtio-net | 2 | mobile data and Ethernet, on host tap devices |
| virtio-gpu | 1 | 2D backend in this run; one 720x1348 display at 280 dpi, 60 Hz |
| virtio-snd | 1 | to a host audio server socket |
| vsock | 1 | guest CID 3; adb and several services ride on it |
| virtio-pmem | 2 | |
| virtio-rng, balloon, virtiofs, Wi-Fi simulator | 1 each | balloon has no guest driver bound |
| 16550 serial | 1 | early console at I/O port 0x3f8 |

For a first Android boot on bvmm, the likely minimum is: PCI bridge, two
virtio-blk disks, one virtio-console, vsock (for adb), virtio-rng, and a
serial port. The boot configuration already selects in-guest "nonsecure"
keymint and gatekeeper, which suggests those do not need a host peer. Which
of the other console channels can be left as sinks is unknown until tried.

## Kernel command line and boot configuration

See the raw capture. Points that matter: `console=hvc0`,
`earlycon=uart8250,io,0x3f8`, `init=/init`, `bootconfig`; the boot
configuration names the boot disks by PCI path
(`pci0000:00/0000:00:13.0` and `14.0`), sets `androidboot.hardware=cutf_cvm`,
and picks the graphics stack (ANGLE over a CPU Vulkan driver in this run).
