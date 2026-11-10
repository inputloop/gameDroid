# gameDroid design plan

Status: draft v0.2, 2026-11-10. License: GPL-3.0-or-later (see COPYING). Nothing here has been built or benchmarked yet.

gameDroid is a free and open source Android emulator for desktop, built for
playing games: first-class keymaps, controller support, high frame rates, and
per-game profiles.

## 1. Decisions taken

These came from the project owner on 2026-11-10 and shape everything below.

| Question | Decision |
|---|---|
| Host OS | Linux and Windows together, from the start |
| Engine | A virtual machine running our own AOSP build |
| Proprietary parts | FOSS core. Google apps and ARM translation are an opt-in download the user performs, never bundled |
| Game targets | Offline and single-player; online gacha, RPG and strategy; competitive shooters and MOBAs; our own and sideloaded builds |
| License | GPLv3 or later |
| Android version | The current release (android17-release at the time of writing) |
| Who builds it | One AI coding agent with subagents; the owner runs hardware tests |
| Hardware | Variable; an NVIDIA RTX 3080 Ti must work and is the reference machine |

One consequence to state plainly: competitive shooters and MOBAs cannot be
promised. See section 8.

## 2. What exists today

Confidence column: "searched" means confirmed by a web search on 2026-11-10
(summaries, not a reading of the source); "memory" means general knowledge that
was not re-checked and should be verified before we rely on it.

| Project | Model | License | State | Useful to us | Confidence |
|---|---|---|---|---|---|
| Trinity | QEMU 5.0 fork, "graphics projection" between guest and host | GPLv2 (repo also lists LGPL-2.1) | Research code, OSDI 2022. Repo last updated around March 2025. Launcher targets HAXM and Hyper-V, so Windows-oriented | The idea: cut guest-to-host graphics copies with a shared projection space. Paper claims 93.3% of native performance | searched |
| AOSP Android Emulator | Old QEMU 2.12 fork plus gfxstream; KVM, WHPX, HVF | GPLv2 (QEMU), Apache 2.0 (gfxstream) | Active, developer-focused. Google is working toward upstream QEMU | gfxstream, the goldfish and ranchu device HALs | searched |
| gfxstream | GLES and Vulkan API streaming over virtio-gpu | Apache 2.0 | Active. Ships in the Android Emulator and Google Play Games on PC. Standalone CMake build, Windows build supported | Our graphics transport | searched |
| virglrenderer / Venus | Vulkan streaming over virtio-gpu, renderer in an isolated host process | MIT | Active. Near-native in good cases. Needs host Vulkan. Used by Anbox Cloud for Android guests | Second graphics backend, Linux strength | searched |
| crosvm | Rust VMM from ChromeOS; Cuttlefish runs on it | BSD-3 | Active. Has a Windows port (said to be the base of Google Play Games on PC) | Candidate VMM | memory for the Windows port |
| QEMU upstream | General VMM; KVM and WHPX | GPLv2 | Active. virtio-gpu has Venus and experimental gfxstream capsets through rutabaga | Candidate VMM | searched |
| Cuttlefish | AOSP's reference virtual device | Apache 2.0 | Active, tracks each AOSP release | Base for our device tree | memory |
| Waydroid | Linux container (LXC), shares host kernel, Wayland | GPLv3 | Active (1.6.x in 2026). No official NVIDIA support; community Venus workarounds exist | Proof of demand, helper-script pattern for opt-in GApps and ARM translation | searched |
| redroid | Android in Docker | Apache 2.0 | Active, images up to Android 16 | Cloud and CI use, not desktop gaming | searched |
| Bliss OS / Android-x86 | Bare-metal or VM x86 Android | Apache 2.0 / GPL mix | Bliss active; Android-x86 largely dormant | Bliss ships XtMapper; x86 device experience | searched / memory |
| XtMapper | In-guest keymapper reading kernel input events | GPLv3 | Active. Works on Bliss; fragile in containers | Feature reference: multi-touch emulation, mouse aim, key to touch | searched |
| scrcpy | Mirrors a real device; UHID keyboard, mouse (2.4) and gamepad (2.7) | Apache 2.0 | Active | Reference for clean input injection and its Android-version caveats | searched |
| Anbox, WSA | Container; Windows subsystem | n/a | Both discontinued | Cautionary tales | memory |
| BlueStacks, LDPlayer, MuMu, Google Play Games on PC | Proprietary VMs | Closed | Active | The feature bar we are measured against | memory |

What the survey says: nobody ships a FOSS, cross-platform, gaming-first
emulator. Waydroid is Linux only and has no real input mapping. Trinity is fast
but unmaintained research code. The keymapping, controller and profile layer
that makes the commercial emulators popular has no good open equivalent. That
layer is where gameDroid should put most of its original work, while reusing
existing open pieces for virtualization and graphics.

## 3. Goals and non-goals

Goals
- Run Android games on Linux and Windows x86_64 with GPU acceleration.
- Keyboard and mouse play for touch-only games, through an on-screen mapping editor.
- Controllers work two ways: as real gamepads in the guest, and mapped to touch.
- Per-game profiles that are shareable, versioned, and resolution independent.
- High refresh rates, low input latency, quick boot.
- Entirely free software as shipped.

Non-goals (for now)
- macOS and ARM hosts. The design keeps the door open; see section 9.
- Defeating anti-cheat, Play Integrity, or emulator detection. Not now, not later.
- App development tooling. The AOSP emulator already serves that.
- Multi-instance farming. Possible later, not a design driver.

## 4. Architecture

```
+--------------------------- host (Linux / Windows) ---------------------------+
|  Launcher + library UI          Mapping editor overlay        Settings       |
|        |                              |                          |           |
|  +-----v------------------------------v--------------------------v--------+  |
|  |  gamedroid-core (Rust)                                                 |  |
|  |  instance manager | profile store | input engine | adb/agent client    |  |
|  +-----+----------------------+----------------------+--------------------+  |
|        |                      |                      |                       |
|  window + presenter     SDL3 input capture     control channel               |
|  (swapchain, overlay)   (kbd, raw mouse,       (virtio-serial / vsock)       |
|        |                gamepads, gyro)              |                       |
|  +-----v----------------------v----------------------v--------------------+  |
|  |  VMM (crosvm or QEMU)  KVM on Linux, WHPX on Windows                   |  |
|  |  virtio-gpu (gfxstream, Venus) | virtio-input | virtio-snd | virtio-blk|  |
|  |  virtio-net | virtio-fs or 9p shared folder | vsock                    |  |
|  +-----+------------------------------------------------------------------+  |
+--------|---------------------------------------------------------------------+
+--------v----------------------- guest (AOSP x86_64) -------------------------+
|  gamedroid device tree (derived from Cuttlefish)                             |
|  gfxstream guest drivers (GLES + Vulkan) | input HAL configs (.idc/.kl)      |
|  gamedroid-agent (system app): package events, foreground app, rotation,     |
|  IME state, clipboard, haptics back-channel                                  |
|  Optional, user-fetched: microG or GApps, ARM native bridge                  |
+------------------------------------------------------------------------------+
```

### 4.1 Virtual machine monitor

Two candidates, to be settled by a spike in phase 0 rather than by argument.

- crosvm. Rust, BSD licensed, what Cuttlefish is developed against, gfxstream
  integration is first class through rutabaga. Risk: the Windows port is less
  travelled outside Google and must be confirmed to build and run from public
  sources.
- Upstream QEMU. Most mature on both hosts (KVM, WHPX), huge community. Risk:
  its gfxstream path is marked experimental, and GPLv2 constrains how we link.

Either way the VMM runs as a separate process that gamedroid-core supervises.
That keeps our code license-independent of the VMM and lets us swap it.

### 4.2 Graphics

- Primary: gfxstream for both GLES and Vulkan. It is the Android-native path,
  multi-threaded, and proven on Windows and Linux hosts in shipping products.
- Secondary on Linux: Venus, where host Vulkan drivers are good.
- Display: frames presented in our own window, so the overlay (mapping hints,
  editor, FPS counter) composites on the host without touching the guest.
- Guest advertises selectable modes: resolution, DPI, and 60/90/120/144/240 Hz.
- Later research item: Trinity-style shared projection memory to remove a copy
  from the frame path. Measure first; only pursue if profiling shows the copy matters.

### 4.3 Input: the core of the product

Mapping happens on the host. The guest only ever sees ordinary devices. This
avoids the in-guest event-reading approach that makes XtMapper fragile, keeps
latency low, and means games see nothing unusual.

Virtual devices exposed to the guest through virtio-input:
- Multi-touch screen, 10 slots, protocol B.
- Keyboard, and a relative mouse for games that support pointer capture.
- Up to 4 gamepads with standard identity so Android's built-in key layouts apply.
- Sensors (accelerometer, gyroscope) and a haptics return path through the agent.

Host capture uses SDL3: gamepad database, HIDAPI drivers for Xbox, DualSense
and Switch controllers, rumble, gyro, hotplug, raw relative mouse.

Mapping primitives (each binds any key, mouse button, or controller control):

| Primitive | Behaviour |
|---|---|
| Tap / hold | Touch down and up at a point; hold while pressed |
| Virtual joystick | WASD or a stick drives a touch drag inside a circle, with radius and dead zone |
| Look / aim | Captured mouse or right stick becomes a continuous drag in a region, re-centering invisibly at the edges; sensitivity and acceleration per axis |
| Fire on click | Mouse buttons mapped to touch points while look mode is active |
| Directional skill | MOBA-style: hold key, aim with mouse relative to character, release to cast; cancel zone |
| Swipe / gesture | Recorded or parametric path |
| Pinch / zoom | Scroll wheel or triggers to a two-finger gesture |
| Toggle and layers | Switch layout sets (on foot, vehicle, menu) by hotkey |
| Tilt | Keys or controller gyro to guest sensors |
| Passthrough | Send the controller as a real gamepad, no mapping |
| Macro | Timed sequences. Off by default and labelled, since many games forbid them |

Mapping editor: an overlay on the live game. Pause, drag controls onto the
frame, bind by pressing a key, test immediately. Hint labels can be shown at
adjustable opacity during play.

Profiles:
- One text file per game (TOML), keyed by package name, with a schema version.
- Coordinates normalized 0 to 1 with an aspect ratio and orientation tag, so a
  profile survives resolution changes.
- Loaded automatically when the agent reports the foreground package.
- A community profile repository, plain git, reviewed by pull request.

### 4.4 Guest image

- Base: the current AOSP release branch (android-latest-release, which today
  points at android17-release). Google now
  publishes source twice a year, which gives us a predictable rebase rhythm.
- Device tree derived from Cuttlefish, x86_64, 64-bit primary with 32-bit app support if needed for older games.
- Tablet-class defaults, landscape, hardware keyboard present so the IME stays out of the way.
- Disk as a read-only system image plus a writable overlay per instance, which
  makes reset, snapshot and later multi-instance cheap.

### 4.5 Opt-in proprietary components

Shipped image is AOSP only. A guided "compatibility pack" step lets the user
choose to fetch, on their own machine and under their own acceptance of terms:
- Play services: microG (free software) or Google apps.
- ARM translation: libndk_translation or libhoudini, extracted from sources the
  user is entitled to use, the way Waydroid's community scripts do it.

Open research item: AOSP's own binary translator (Berberis) as a free
alternative. Its arm64 guest support was not verified in this pass.

Redistribution terms for the ARM translators could not be confirmed by search.
Until someone reads the actual license texts, we treat them as not redistributable.

### 4.6 Quality-of-life features

APK drag and drop, shared folder, clipboard sync, screenshots and recording,
snapshot fast boot, FPS and frame-time overlay, per-game settings (resolution,
frame rate, CPU and memory), and a controller-navigable "big picture" launcher
for handheld PCs.

## 5. Technology choices

| Area | Choice | Reason |
|---|---|---|
| Host core | Rust | Matches crosvm and rutabaga; safe for an input and IPC heavy daemon |
| Input and window | SDL3 | Best cross-platform controller support available |
| Overlay and editor | Immediate-mode UI drawn in our presenter | Must composite over the game frame on both hosts |
| Guest agent | Kotlin system app plus a small native service | Standard Android |
| Profiles | TOML with a JSON Schema | Human-editable, diff-friendly |
| Build | Cargo for host; AOSP Soong for guest; CI builds both hosts | |

## 6. Repository layout (proposed)

```
core/        gamedroid-core: instances, profiles, input engine
ui/          launcher, editor overlay
vmm/         VMM integration and patches (submodules, not copies)
guest/       AOSP local manifest, device tree, agent
profiles/    schema and starter profiles
docs/        this plan, ADRs, compatibility notes
tools/       image builder, compatibility-pack fetcher
```

## 7. Roadmap

| Phase | Outcome | Exit test |
|---|---|---|
| 0. Spikes | Boot a Cuttlefish-derived image under crosvm and QEMU, on Linux and Windows, with gfxstream. Choose the VMM. Record the decision as an ADR | A 3D benchmark APK renders with GPU acceleration on both hosts on the RTX 3080 Ti reference machine |
| 1. Playable shell | Window, audio, mouse as touch, keyboard, APK install, one instance, settings | An x86-native offline game is playable start to finish on both hosts |
| 2. Input | Virtual devices, mapping engine, editor, profiles, controller passthrough and controller to touch | A touch-only shooter and a MOBA are comfortable on keyboard and mouse and on a controller |
| 3. Compatibility | Compatibility pack (microG or GApps, ARM translation), device profiles, public compatibility database | An ARM-only gacha title installs from its store and runs |
| 4. Performance and polish | High refresh modes, snapshots, recording, big-picture UI, community profile repo | 120 fps sustained in a title that supports it; cold start to home screen under a target we set in phase 0 |
| 5. Reach | ARM hosts and macOS, multi-instance, publisher outreach | Decided after phase 4 |

Phases 1 and 2 are where a small team should expect to spend most of its time.

## 8. Anti-cheat and integrity: an honest position

- Google's Play Integrity fails device checks on emulators. The one virtual
  environment it recognizes (MEETS_VIRTUAL_INTEGRITY) is currently limited to
  Google Play Games on PC.
- Competitive shooters and MOBAs add their own emulator detection. Some ban,
  some place emulator players in separate lobbies, some allow named emulators.
- gameDroid will not spoof device identity, hide that it is an emulator, or
  bypass integrity checks. Doing so risks players' accounts and the project.
- What we can do: identify ourselves honestly, keep a compatibility database
  that records each game's stance, and approach publishers about being
  recognized the way the commercial emulators are.

So the realistic target for the competitive tier is: works where the publisher
permits emulators, clearly marked as unsupported where it does not.

### 8.1 Where shooters stand (searched 2026-11-10)

No publisher policy page was read directly. These come from news and guide
sites, plus one Garena announcement, and must be re-checked per title before
we publish a compatibility entry.

| Game | Stance toward emulators | What that means for us |
|---|---|---|
| Free Fire / Free Fire MAX | Tolerated on any emulator. Detected players go to a separate emulator pool in ranked Battle Royale and Clash Squad; a squad with one emulator player is matched as emulator. Emulator accounts are excluded from leaderboards | Best first target: the policy is about the platform, not a named vendor |
| PUBG Mobile | Detects emulators and matches them only with each other. Tencent's Gameloop is the only sanctioned emulator; temporary bans are reported on others | Playable in principle, real account risk until Krafton or Tencent recognizes us |
| Call of Duty: Mobile | Same model. Activision's wording: no official support outside Gameloop. Bans on BlueStacks were reported in 2021 | Same as PUBG Mobile |
| Arena Breakout | Offered on Gameloop. No policy found for other emulators | Unknown; treat as at-risk |
| Standoff 2 | Developer says emulators are unsupported; no express ban found | Unsupported, at the player's risk |
| Critical Ops, Blood Strike, Farlight 84 | Nothing found | Unknown |

### 8.2 Ways to work with it

1. Be easy to classify. Present one stable, documented, truthful identity
   (device model, hardware name, build fingerprint) so a game's detection puts
   gameDroid in its emulator pool. Unknown or shifting identities are what get
   treated as tampering. Publish the identity so anti-cheat vendors can add it.
2. Ship a clean release image. No root, no debuggable system, adb off by
   default, signed builds with verified boot. Limit to state honestly: anyone
   can rebuild open source with root added, and without hardware-backed
   attestation a publisher cannot prove a given player runs our signed build.
3. Fair-play mode, enforced per game. The compatibility database carries a
   policy per package. For titles marked competitive, the input engine disables
   macros, recoil scripts, turbo and any automation, leaving only one-to-one
   mapping. This is the behaviour publishers ask of sanctioned emulators.
4. Tell the player before they risk an account. First launch of a listed title
   shows its stance: emulator pool, leaderboard exclusion, or reported bans.
   For "at-risk" titles, recommend a secondary account.
5. Approach publishers in order of openness: Garena first, since its pool is
   vendor-neutral; then the Tencent-published titles, where recognition is
   currently tied to Gameloop and is a business conversation, not a technical one.
6. Point to the native PC version when one exists. Several mobile shooters have
   PC clients; the launcher should say so instead of pretending to be the best route.
7. Accept Play Integrity as it is. Titles that require a device-integrity
   verdict will not run, with microG or with Google apps. The database marks
   them "blocked by integrity" and we do not attempt to change the verdict.
8. Offline value still counts. Training grounds, bot matches and custom rooms
   often work where ranked play does not; record that per title.

## 9. Risks

| Risk | Mitigation |
|---|---|
| Windows graphics path harder than expected | Phase 0 exists to find this out before anything else is built |
| crosvm on Windows not buildable from public source | QEMU with WHPX as fallback; VMM kept behind a process boundary |
| ARM translation legally or technically unavailable | Opt-in only; investigate Berberis; an arm64 guest on ARM hosts needs no translation |
| AOSP rebase cost | Thin device tree, patches kept minimal and upstreamable, twice-yearly rhythm |
| Anti-cheat blocks the most popular titles | Stated up front; success is measured on the other three tiers first |
| Scope | Linux and Windows together roughly doubles graphics and test work; the roadmap keeps features minimal until phase 2 |

## 10. License: GPLv3 or later

Decided 2026-11-10. What it means for the parts we reuse:

| Component | Its license | How we may use it |
|---|---|---|
| crosvm | BSD-3 | Link or patch freely |
| gfxstream, AOSP, Cuttlefish, scrcpy | Apache 2.0 | Compatible with GPLv3; link or patch freely |
| virglrenderer, SDL3 | MIT, zlib | Link freely |
| XtMapper | GPLv3 | Code may be borrowed with attribution |
| QEMU | GPLv2, with some files GPLv2 only | Run as a separate process only. Do not copy its code into ours; keep any QEMU patches under QEMU's own license in the vmm/ tree |
| Trinity | GPLv2 per its README | Ideas only, unless its files turn out to say "or later". Not checked |
| Google apps, ARM translators | Proprietary | Never in the repository or the image; user-fetched |

Our AOSP device tree and guest agent are our own work and carry GPLv3+ too.
Patches we hope to send upstream to an Apache or BSD project are written under
that project's license from the start.

## 11. How the work gets done: one agent

The builder is a single AI coding agent that can fan work out to subagents. The
plan has to fit what such an agent can and cannot verify.

Measured in the agent's cloud session on 2026-11-10: 4 CPU cores, 15 GB RAM,
about 31 GB free disk, no /dev/kvm, no GPU, Linux only, and the container is
discarded when the session ends.

| Work | Can the agent verify it alone? | How it gets verified |
|---|---|---|
| Input engine, mapping maths, profile schema and loader, instance manager logic | Yes | Unit and property tests in the session and in CI |
| Host builds for Linux and Windows | Build only | GitHub Actions on Linux and Windows runners |
| Launcher and overlay UI | Partly | Headless render tests and screenshots; feel is judged by the owner |
| Booting the VM, WHPX, KVM | No | Owner's machine |
| GPU acceleration, frame rate, latency | No | Owner's RTX 3080 Ti machine |
| AOSP image build | No. A full build needs several hundred GB of disk and far more CPU and RAM than a session has | A build machine or self-hosted runner the owner provides; until then, start from prebuilt Cuttlefish images and add our device changes as overlays |

Working rules that follow:

1. Logic first. The mapping engine and profile format are the product's core
   and are fully testable without hardware, so they are built first and in
   parallel with the phase 0 spikes rather than after them.
2. Every hardware-dependent step ships with a script the owner runs
   (`tools/hwtest`), which collects versions, logs and a pass or fail result
   into one file to hand back. The agent never reports such a step as working
   on its own word.
3. Subagents take separable, checkable units (one mapping primitive and its
   tests, one CI job, one doc survey). Anything load-bearing is reviewed by the
   main agent before merge.
4. Nothing lives only in a session. Each unit of work ends in a pushed commit.
5. Decisions are recorded as short ADRs in docs/, since the agent has no
   memory between sessions beyond the repository.

### Hardware targets

- Reference and must-pass: NVIDIA RTX 3080 Ti with the proprietary driver, on
  Linux and on Windows. The VM design helps here: the guest never loads the
  host's GPU driver, so the NVIDIA limitation that hurts Waydroid does not apply.
- Should work: recent AMD and Intel GPUs with Vulkan 1.3 drivers.
- Best effort: integrated graphics and older cards. Settings scale down
  (resolution, frame cap); no promise of high refresh.
- CPU: x86_64 with hardware virtualization enabled. On Windows, the Windows
  Hypervisor Platform feature turned on.

## 12. Settled on 2026-11-10

1. No AOSP build machine. The first phases start from prebuilt Cuttlefish
   images, with our device changes layered on as overlays (extra partitions,
   vendor overlays, the agent as an installed app). Limit: changes that need a
   rebuilt system image, such as new HALs or framework patches, wait until a
   build machine or a hosted builder exists. Phase 0 must list which of our
   planned guest changes fall on which side of that line.
2. The RTX 3080 Ti reference covers Linux and Windows on separate machines.
   Each hardware test round is therefore two runs of `tools/hwtest`, one per
   machine, and the two result files are compared side by side.
3. Compatibility-pack components are offered only as legally available. Each
   one (microG, Google apps, an ARM translator) is enabled in the fetcher only
   after its license text has been read and a short note recording the source,
   the terms, and what we may do is committed under docs/legal/. A component
   with no clear grant is not offered; the user can still sideload it.

## 12.1 Still open

Nothing blocks phase 0. To be answered by the phase 0 spikes: which VMM, and
which guest changes can ship as overlays.

## 13. Sources

Gathered by web search on 2026-11-10. Search summaries were used; the
underlying repositories and papers were not read in full.

- Trinity: https://github.com/TrinityEmulator/TrinityEmulator and https://www.usenix.org/conference/osdi22/presentation/gao
- gfxstream: https://github.com/google/gfxstream
- QEMU virtio-gpu: https://www.qemu.org/docs/master/system/devices/virtio/virtio-gpu.html
- virglrenderer and Venus: https://www.collabora.com/news-and-blog/blog/2025/01/15/the-state-of-gfx-virtualization-using-virglrenderer/
- Waydroid 1.6.2: https://ubuntuhandbook.org/index.php/2026/02/waydroid-1-6-2-added-vulkan-support-for-intel-xe-graphics-driver/
- Waydroid on NVIDIA: https://aur.archlinux.org/packages/waydroid-nvidia-bin
- XtMapper: https://www.github.com/Xtr126/XtMapper and https://xtr126.github.io/XtMapper-docs/guides/about
- scrcpy input modes: https://github.com/Genymobile/scrcpy/blob/master/doc/mouse.md
- redroid: https://github.com/remote-android/redroid-doc
- AOSP release schedule: https://www.androidauthority.com/aosp-source-code-schedule-3630018/
- Play Integrity: https://developer.android.com/google/play/integrity/overview and https://developer.android.com/games/playgames/integrity
