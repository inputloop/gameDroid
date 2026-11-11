# Engine track: bvmm to Android

Decided by the owner on 2026-11-10: gameDroid leads with INPUTLOOP's own
hypervisor, bvmm (`inputloop/bcontain`, `crates/bvmm`), as its engine. The
crosvm and QEMU track is parked; its expected outcomes are kept as guesses in
`TRACK1-GUESSES.md`.

Where the work lives: engine code in the bContain repository; gameDroid's
launcher, input mapping and profiles here. gameDroid depends on bvmm as a
library (the licenses match, so linking is allowed).

## Starting point (read from the source, 2026-11-10)

bvmm boots a Linux kernel directly on KVM or Windows Hypervisor Platform with
one vCPU, up to 3 GiB RAM, a serial console, a CMOS clock and an I/O APIC. Its
WHP self test passed on the reference laptop. A real kernel boot has not been
confirmed on hardware yet. It has no virtio devices.

## Milestones

Each milestone ends in a test that runs without a person where possible. KVM
tests can run on GitHub's Linux runners, which have KVM. WHP tests may run on
GitHub's Windows runners (confirmed 2026-11-10: bvmm runs a guest there) and
on the laptop for anything involving the GPU or Intel-specific behaviour.

| # | Milestone | Done when | Verified by |
|---|---|---|---|
| E0 | A real Linux kernel boots on KVM and on WHP | A container prints `uname -a` through `--engine native` | **Done in CI on both, 2026-11-10** (`results/2026-11-10-E0-kernel-boot.md`). Still to repeat on the Intel laptop |
| E0b | The Android kernel boots on both hypervisors | A stock GKI 6.12 kernel runs a container on KVM and WHP | **Done in CI, 2026-11-10** (`results/2026-11-10-E1-and-E0b.md`). It boots, but on WHP it has only a coarse clock |
| E0c | The Android kernel gets a precise clock on Windows | On WHP the stock GKI kernel reports `clocksource tsc` and still shuts down cleanly | **Clock done in CI, 2026-11-11** (`results/2026-11-11-windows-smp-clock.md`): `clocksource tsc`. Clean shutdown not yet checked |
| E1 | Device transport and a disk | PCI host bridge, modern virtio-pci, virtio-blk; a guest reads and writes a disk image | **Done in CI on both, 2026-11-10** (same file) |
| E2 | Several vCPUs and big memory | Several vCPUs and more than 4 GiB visible in the guest, memory above the 4 GiB hole | **KVM: done in CI, 2026-11-10** (4 vCPUs, 5 GiB). **WHP: done in CI, 2026-11-11** (2 vCPUs with Android; `results/2026-11-11-windows-smp-clock.md`) |
| E3 | ACPI and clean power-off | Guest shuts down and bvmm exits 0, no reset trick | CI |
| E4 | Network and vsock | virtio-net with user-mode NAT; vsock stream to the host | CI |
| E5 | Android boots | Stock Android 17 image reaches `sys.boot_completed=1` | **KVM: done in CI, 2026-11-10** (`results/2026-11-10-E5-E6-android-boots.md`). Without adb; boot is read from the console |
| E6 | A picture | virtio-gpu 2D scanout; Android screen visible with guest software rendering | **KVM: done in CI, 2026-11-10**, frames saved to files. A host window is gameDroid's job and not built |
| E7 | Touch, keys, gamepad | virtio-input devices; taps and keys arrive in Android | **Touch: done in CI on KVM, 2026-11-10** (a swipe unlocks, another opens the app drawer). Keyboard present, untested. No gamepad device yet |
| E8 | GPU acceleration | gfxstream through rutabaga; guest renderer string names the host GPU | Plan in `E8-gpu-plan.md`, steps G1 to G6. G1 (build the host library on both hosts) in progress. Final proof is on the laptop only |
| E9 | Sound | virtio-snd playback | Laptop |

gameDroid's own phases hang off these: the playable shell (phase 1) needs E6
and E7; the input work (phase 2) can start on its pure-logic parts now and
needs E7 to be tried against a real game; performance work needs E8.

## Settled by measurement, 2026-11-10

From `reference/README.md` (stock Android 17 inspected under crosvm):

- Transport is virtio over PCI. The Android kernel has no virtio-mmio driver.
  E1 therefore builds a PCI host bridge and modern virtio-pci, not MMIO.
- The Android kernel has no Hyper-V support, so on Windows it cannot use the
  enlightenments bvmm depends on today for timer frequencies. New early
  milestone E0b: boot a stock GKI kernel (not the linuxkit one) to a shell on
  WHP. This is the first Windows risk for Android and costs little to test.
- Android here boots through a bootloader. E5 will unpack the kernel and
  ramdisks and supply the command line and boot configuration directly.

## Choices to settle early

1. Transport: virtio over PCI or virtio-mmio. bContain's notes plan for MMIO,
   which is far simpler. The stock Android kernel may only carry the PCI
   driver built in (guess X5, 70%). E1 starts by checking the Cuttlefish
   kernel's configuration; if PCI is required, E1 builds a minimal PCI host
   bridge instead.
2. How Android is booted. Cuttlefish normally goes through a bootloader. bvmm
   boots a kernel directly, so E5 must unpack the kernel and ramdisks from the
   boot images and pass the boot configuration itself. crosvm supports this
   style, which suggests it is workable.
3. What to reuse. rust-vmm crates (virtio queues, guest memory, ACPI tables,
   kernel loader) and rutabaga are permissively licensed and may be linked.
   Each adoption is a choice between speed and keeping bvmm small; decide per
   crate, in the bContain repository.
4. The engine interface gameDroid codes against: create and start a VM from an
   image set, stop it, get frames or a window surface, attach input devices
   and push events, open a control stream. Drafted during E5 to E7, when the
   real shape is visible.

## Honest size of this

E0 to E4 is general VMM work that bContain wants anyway. E5 is the first
Android-specific step and the first real unknown. E8 is the hardest single
item: embedding gfxstream on Windows inside a new VMM has, as far as I know,
no public example to follow. Until E8 is reached, gameDroid has no accelerated
graphics on either host. That is the cost of leading with this track, accepted
on 2026-11-10.

## Reference oracle

crosvm on Linux boots the same image today. Whenever bvmm and Android
disagree, compare with what crosvm presents to the guest (devices, kernel
command line, boot configuration). Read its behaviour; its code is BSD
licensed and may also be read and reused with attribution.
