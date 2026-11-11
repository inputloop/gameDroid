# Firefox and gameDroid

Date: 2026-11-11. Status: looked into; one experiment run. A decision is
needed before more is built.

## What was tried

Live-verified in CI on Linux (the `firefox` workflow, gameDroid a60088f).

"Fennec F-Droid" (`org.mozilla.fennec_fdroid`) is F-Droid's own build of
Firefox for Android from Mozilla's source, under the MPL-2.0, with Mozilla's
proprietary parts and trademarks taken out; hence the name. F-Droid
publishes it once per processor kind. The newest release was version code
1590120 for ARM and **1590110 for x86-64**, which is the one gameDroid's
Android can run.

That APK (134 MB) installed on gameDroid, started, and was the front
activity, using about 226 MB of memory at its welcome screen, with no crash
in the log:

<img src="../screenshots/firefox-welcome.png" width="240" alt="Fennec's welcome screen on gameDroid">

Not tried: anything past the welcome screen (loading a page, video, sound,
the camera or microphone in a page, add-ons).

## What is known about making it fit

- **Its home page cannot be set to gameDroid's help page.** Firefox for
  Android has no setting for a home page address (inferred from its
  settings, not verified in source). The help page could still be opened in
  it on request.
- **It is large.** 134 MB to download against Fulguris's 9 MB, and it
  brings its own browser engine where Fulguris uses Android's.
- **Its welcome screen says it "sends diagnostic and interaction data to
  Mozilla"**, with a link to manage that. F-Droid's build is meant to have
  telemetry off; whether this text is only left-over wording was not
  checked. It should be, before gameDroid installs it for anyone unasked.
- **Why one might want it anyway:** a full, current browser engine; add-ons
  (an ad blocker, say); and it does not depend on the image's WebView,
  which is whatever version Google's build happened to ship.
- The official Mozilla build also exists for x86-64, but carries Google's
  services and Mozilla's trademarks; gameDroid ships free software only.

## Ways to integrate it

| Option | What the person gets | Cost |
|---|---|---|
| **A. Offered, not installed.** gameDroid installs Fennec when asked (a command now, a button in a launcher later), beside Fulguris | A choice; nothing extra downloaded unless wanted | Small: the fetch-and-install path already exists for Fulguris |
| **B. Installed by default, beside Fulguris**, Fulguris staying the default browser with the help page | Both browsers there from the first start | 134 MB more on every first run |
| **C. Fennec replaces Fulguris** as the browser gameDroid sets up | One browser, the best-known one | The help page can no longer be the start page; 134 MB; the telemetry question must be settled first |

## Recommendation

**A.** It costs almost nothing to build, keeps the first run small, and
keeps the help page where it is. If Firefox turns out to be what people
reach for, B is one line away.

**The question for the owner:** A, B or C.
