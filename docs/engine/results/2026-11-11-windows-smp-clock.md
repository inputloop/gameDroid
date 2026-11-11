# Windows: several processors and a precise clock

Date: 2026-11-11. Confidence: live-verified in CI.
Runs: engine-android 42928416616 (gameDroid a48ec3c) and 42928586313
(91a2601), both on bContain 020c3a7.

## Result

Stock Android 17 on bvmm on a Windows runner, asked for two processors:

    smp: Brought up 1 node, 2 CPUs
    clocksource: Switched to clocksource tsc
    GD_CPUS=2

Boot completed at 53 s of guest time (76 s before, on one processor with the
coarse clock). In run 42928416616 the game was installed and started and the
screen reached frame 949 by the last screenshot; the same script reached
frame 436 the day before. Frame counts are a rough measure, not a benchmark.

## How the clock was fixed

The guest's own log named the cause:

    Hypervisor detected: Microsoft Hyper-V
    tsc: Marking TSC unstable due to running on Hyper-V

Linux reads the time-stamp counter's frequency from Hyper-V whether or not
it has the Hyper-V drivers, but it then refuses the counter as a clock unless
Hyper-V CPUID leaf 0x40000003 sets the "invariant TSC" permission (bit 15 of
EAX). Android's kernel has no Hyper-V clock driver to fall back on, so it was
left with `refined-jiffies`, 250 ticks a second.

The Windows Hypervisor Platform has no setting for that bit. bvmm now asks to
answer that one CPUID leaf itself and adds the bit to the hypervisor's own
answer. The counter under WHP is steady, so the claim is true.

One visible side effect: the kernel then writes a Hyper-V register to turn
the feature on, WHP refuses the write, and the kernel logs once

    unchecked MSR access error: WRMSR to 0x40000118

and carries on. Harmless, and left in the log rather than hidden.

This replaces the attempt of 2026-11-10 (`nopv` plus a measured frequency),
which left the guest with no timer at all and was withdrawn.

## How several processors were added

Each virtual processor runs on its own thread with its own instruction
emulator. Secondary processors are created parked ("startup suspend"), and
the hypervisor's local APIC starts them when the boot processor asks. To
stop, the first processor to finish sets a flag and the others are cancelled
out of the hypervisor.

## What is not yet verified

- Clean shutdown on Windows with several processors.
- More than two processors (the runner has four).
- Anything on a real machine.

## A fault in the test, found here

Run 42928586313 reported PASS on Windows while the game was hidden behind
the lock screen: the unlock swipe was sent 8 s after boot and ignored, and
the check only asked whether the game had been started. The script now waits
longer and requires the game to be the front activity.
