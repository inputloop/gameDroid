# ADR 0001: which virtual machine monitor

Status: OPEN. Opened 2026-11-10. No decision yet.

## Question

Which VMM runs the gameDroid guest on Linux and Windows: crosvm, upstream
QEMU, INPUTLOOP's own bvmm, or a different engine per host?

## Candidates

| | crosvm | QEMU |
|---|---|---|
| License | BSD-3 | GPLv2 |
| Linux + KVM | Cuttlefish's default | Supported by Cuttlefish as an alternative |
| Windows + WHPX | Builds from source; not tested upstream | Mature WHPX support |
| gfxstream | First class through rutabaga | Since 8.2, documented for Linux hosts, experimental for Android guests |
| gfxstream on Windows | Unknown | Unknown |

## A third candidate: bvmm, INPUTLOOP's own hypervisor

Raised by the owner on 2026-11-10. Source read at `inputloop/bcontain` commit
35eed22 (`crates/bvmm`, about 2,700 lines of Rust, and `docs/NATIVE-VMM.md`).
Read-verified only; nothing was built or run.

What it has today:
- KVM and WHP backends. The WHP one passed its self test on 2026-10-26 on the
  same reference laptop gameDroid targets. That is the exact spot where crosvm
  is weakest (its WHPX path is not tested upstream).
- Direct 64-bit Linux kernel boot, serial console, CMOS clock, I/O APIC.
- License GPL-3.0-or-later OR LGPL-3.0-or-later, the same family as gameDroid,
  so it can be linked into our code. No process boundary needed.

What Android needs that it does not have yet:

| Needed for Android 17 with graphics | bvmm today |
|---|---|
| A Linux kernel boot confirmed on real hardware | Not yet (its own doc lists this as the next test) |
| More than one vCPU | One |
| More than 3 GiB of RAM, memory above the 4 GiB hole | 64 MiB to 3 GiB |
| A virtio transport (MMIO or PCI) | None |
| virtio-blk for the system, vendor and data images | None |
| virtio-gpu with gfxstream (through rutabaga) and a host window | None |
| virtio-input for touch, keys and gamepads | None |
| virtio-net, virtio-snd, vsock (adb, control channel) | None |
| ACPI or a device tree, clean shutdown | MP table only |
| Boot path Android expects (bootloader or direct kernel plus ramdisks, verified boot settings) | Direct kernel only |

So bvmm cannot boot Android today, and closing that gap is most of what makes
a VMM a VMM. It is not a phase 0 candidate. It is a serious long-term one:

- Much of the missing list exists as permissively licensed building blocks we
  may link: rust-vmm crates (virtio queues, guest memory, kernel loader,
  vhost-user) and rutabaga, which exists precisely so other VMMs can embed
  gfxstream.
- bContain's own roadmap already heads the same way (virtio-mmio, network,
  several vCPUs, disks). Android would add the GPU, input and sound devices.

Also noted: bContain's main engine is a bundled QEMU with WHPX on Windows and
`virtio-vga-gl` (virgl) graphics. That gives a fourth, cheap experiment: boot
the Cuttlefish image through bContain's QEMU on the Windows laptop with virgl
graphics. Whether Android's guest drivers accept that path on a Windows host
is unknown.

### Recommendation (not yet a decision)

Two tracks, with the seam kept clean:

1. Ship track: pick crosvm or QEMU by the rule below and get Android running
   with GPU acceleration on both hosts. gameDroid talks to the VMM only through
   a small engine interface (start, stop, displays, input devices, control
   channel), which was already the plan.
2. Engine track: grow bvmm toward that same interface in the bContain
   repository, milestone by milestone, and switch gameDroid to it per host once
   it boots Android with graphics. Windows is where the switch would pay first.

What would change this recommendation: if both crosvm and QEMU fail to give
GPU-accelerated Android on Windows, bvmm plus rutabaga stops being the long
road and becomes the only road we fully control, and phase 1 would be
re-planned around it.

## Decision rule

See "How the VMM gets chosen" in `docs/phase0/README.md`.

## Evidence

- 2026-11-10: crosvm boots the prebuilt Android 17 Cuttlefish image under KVM
  on Linux with software graphics, in CI.
  `docs/phase0/results/2026-11-10-ci-linux-crosvm.md`
- QEMU on Linux: attempted, inconclusive.
- Windows: nothing yet. This is the deciding evidence.

## Decision

Pending.
