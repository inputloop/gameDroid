# ADR 0002: gameDroid is delivered as a single file

Status: DECIDED 2026-11-10 by the owner: "When it's done I'm going to need a
single file solution to start the entire thing. (An exe or bat file that
starts it or installs it)".

## What this requires

One file the user downloads and runs. No Python, no Linux-only helper tools,
no manual steps. On first run it sets itself up; after that it starts.

| Need | Today | To do |
|---|---|---|
| One program that does everything | Three: Google's Linux-only `cvd fetch` downloads the image, `tools/engine/android_prepare.py` prepares it, `gamedroid` runs it | Fold download and preparation into `gamedroid` itself, in Rust |
| Getting the Android image | `cvd fetch`, Linux only | Download the image archive directly over HTTPS; unpack it; convert Android's sparse images to plain ones |
| Windows | The engine runs Linux guests on WHP with one processor. Android's kernel boots there with a coarse clock. Android itself has never been tried on Windows | Boot Android on WHP in CI; fix what stops it; several processors; a proper clock |
| A file to download | Nothing is packaged | CI builds `gamedroid.exe` (and a Linux binary) as artifacts |
| The engine inside the file | Already so: bvmm is a library linked into `gamedroid` | |

An `.exe` means Windows is the platform this will be judged on, and Windows
is where the engine is furthest behind. Windows work therefore moves ahead of
further Linux polish.

## Shape

`gamedroid.exe` with no arguments:

1. Checks the machine (hypervisor available, disk space) and says plainly
   what to turn on if something is missing.
2. First run: downloads the Android image to the user's data folder, with
   progress, and prepares it. Later runs skip this.
3. Boots Android and opens the window.

A `.bat` is not needed if the `.exe` does this; one may be added only to turn
on the Windows Hypervisor Platform feature, which needs an administrator
prompt and a reboot and cannot be done silently.

## What the single file will not contain

- The Android image (about 1 GB compressed): downloaded on first run, from
  Google's public build server, not redistributed by us.
- Google apps or an ARM translator: opt-in, as the design plan says.

## Honest risks

- Whether Android boots on WHP at all is unknown until tried.
- One processor and CPU rendering on Windows will be slow even when it works.
- The image download depends on Google's build server keeping public builds
  available at a stable address.
