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

## Progress, 2026-11-10

Live-verified in CI (gameDroid commit 0dadcf4, runs 42923343208 and
42923343124):

- **Download without a key.** The public build site's ordinary link,
  `https://ci.android.com/builds/submitted/<build>/<target>/latest/raw/<zip>`,
  serves the 1.16 GB archive with no API key. gameDroid pins build 16644040.
  Asking the site for "the latest build" returns 403 without a key, so
  moving to a newer build is a deliberate change of the pinned number, not
  automatic. Google's key-based build API is not used.
- **Setup in Rust, inside the program** (`core/device/src/setup.rs`,
  `prepare/`): download, unzip, expand the sparse partitions, build the boot
  files and disk layout. 12 s on Linux and 16 s on Windows after the download.
- **Android on Windows.** `gamedroid-run --setup` on a Windows runner boots
  Android 17 through WHP (boot completed at 76 s of guest time, one
  processor), unlocks by touch, installs and starts Shattered Pixel Dungeon.
  The risk "whether Android boots on WHP at all" is retired.
- **The file.** `gamedroid.exe` is 3.1 MB, statically linked, and asks
  Windows only for system libraries. In demo mode it opens its window on the
  Windows runner and draws 120 frames.

Not yet verified:

- `gamedroid.exe` with no arguments end to end (window with Android behind
  it). The parts are proven separately.
- Anything on a real Windows machine. CI runners have no GPU and are not the
  reference laptop.
- A disk-space check before the download; resuming an interrupted download;
  a checksum of the archive.
- The clock on Windows is still coarse (`refined-jiffies`) and there is one
  processor there.

Later the same day (commit 37e2f2a, run 42925479807): `gamedroid` and
`gamedroid.exe`, started with no arguments, were live-verified end to end in
CI on Linux and Windows. Each downloaded Android, set it up, booted it and
showed it in its window; on Linux a mouse swipe unlocked it. The first item
under "Not yet verified" is therefore done. The first Windows attempt failed
because the default asked for two processors; the default now follows what
the host engine supports.

## Progress, 2026-11-11: setup

Live-verified in CI unless marked (runs 42927730377, 42928416616,
42928586313):

- **Where and how much.** The first run asks where to keep Android, offering
  gameDroid's own folder and a `gameDroid` folder on every other fixed drive
  with the free space of each, then how many GB games and their data may
  use. The answers are saved in `settings.txt` in gameDroid's own folder;
  deleting that file asks again. `GAMEDROID_DATA` and `GAMEDROID_STORAGE_GB`
  override them. With nobody at the keyboard it takes the defaults.
  The storage limit is honoured: asked for 20 GB on Windows, Android reports
  a 20 GB data partition (12 GB asked and seen on Linux). The questions
  themselves are unit-tested, not exercised by a person in CI.
- **The limit is not a reservation.** The partition file is sparse, so disk
  is used as Android fills it. Android formats it on first boot.
- **Free space** is checked before the download (5 GB needed to set up).
  Unit-tested only.
- **Interrupted downloads continue** from where they stopped. Tested against
  a local web server, not against a real dropped connection.
- **The archive is checked**: its size and SHA-256 are pinned. Both hosts
  downloaded the same bytes.

Not done: choosing a different place after setup (the data would need
moving); showing any of this in a window, not a console.
