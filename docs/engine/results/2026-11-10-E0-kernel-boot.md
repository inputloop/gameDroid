# E0: a real Linux kernel boots on bvmm, on KVM and on WHP

Date: 2026-11-10. Confidence: live-verified, read from CI job logs.
Run: https://github.com/inputloop/bContain/actions/runs/42894435990
(bContain commit 35eed22 plus a CI-only commit, 050e880).

| | Linux runner, KVM | Windows runner, WHP |
|---|---|---|
| Host | GitHub `ubuntu-latest`, AMD EPYC, 2 cores, nested virtualization | GitHub `windows-latest` (Windows Server 2025), AMD EPYC, 2 cores, nested |
| bvmm unit tests | pass | pass |
| `hypervisor-test` (40-byte guest) | PASS, console `4f 4b 11 0a` | PASS, console `4f 4b 11 0a` |
| `bcontain run --engine native alpine:3.20 -- uname -a` | PASS | PASS |
| Guest output | `Linux run-3856 6.12.59-linuxkit ... x86_64 Linux` | `Linux run-2936 6.12.59-linuxkit ... x86_64 Linux` |
| Container exit code | 0 | 0 |
| Kernel time at reboot | 0.90 s | 1.44 s |

What this confirms that was not confirmed before: on both hypervisors a real
6.12 kernel boots on bvmm, finds its timers and interrupts, runs userspace
from the initramfs, writes to the serial console, and exits through the reset
path. bContain's own notes listed the Linux boot on WHP as untested and named
three likely trouble spots (halt exits, timer frequencies through Hyper-V
enlightenments, no PIC). None of them stopped the boot here.

Limits of this evidence:
- Both hosts are AMD and nested inside GitHub's own hypervisor. The reference
  laptop is Intel and not nested. The laptop has passed `hypervisor-test`
  (2026-10-26, per bContain's notes) but not yet this container boot.
- One vCPU, 2 GiB, serial console only. Nothing here touches the gaps Android
  needs (virtio, several vCPUs, big memory, graphics).
- Idle behaviour, interactive input and long uptimes were not exercised.
