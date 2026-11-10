# ADR 0001: which virtual machine monitor

Status: OPEN. Opened 2026-11-10. No decision yet.

## Question

Which VMM runs the gameDroid guest on Linux and Windows: crosvm, upstream
QEMU, or a different engine per host?

## Candidates

| | crosvm | QEMU |
|---|---|---|
| License | BSD-3 | GPLv2 |
| Linux + KVM | Cuttlefish's default | Supported by Cuttlefish as an alternative |
| Windows + WHPX | Builds from source; not tested upstream | Mature WHPX support |
| gfxstream | First class through rutabaga | Since 8.2, documented for Linux hosts, experimental for Android guests |
| gfxstream on Windows | Unknown | Unknown |

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
