# hwtest

Scripts the project owner runs on real hardware. The building agent has no GPU,
no KVM and no Windows machine, so anything about booting, graphics or speed is
only ever claimed on the strength of a result file produced here.

## Stage 1: host probe (ready)

Read-only. Installs nothing and changes nothing. Takes a few seconds.

Linux machine:

```bash
tools/hwtest/probe.sh
```

Windows machine, from an Administrator PowerShell in the repository folder:

```powershell
powershell -ExecutionPolicy Bypass -File tools\hwtest\probe.ps1
```

Each prints the path of a `hwtest-probe-<platform>-<date>.txt` file. Hand both
files back (paste them, or commit them under `docs/phase0/results/`).

What the file contains: OS and kernel version, CPU model, memory, whether
hardware virtualization is usable, GPU model and driver versions, Vulkan and
OpenGL summaries, free disk, and versions of build tools. It does not record
your user name, host name, network details or serial numbers. Read it before
sharing; it is plain text.

For a fuller Linux report, first install `pciutils`, `vulkan-tools` and
`mesa-utils` (names on Debian and Ubuntu; other distributions differ).

## Stage 2: boot spikes (not written yet)

Written after the stage 1 results are in, because the steps depend on the
Linux distribution and on which Windows features are on. See
`docs/phase0/README.md`.
