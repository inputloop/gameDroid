#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Prepare Linphone's Android source for gameDroid's build of it.

Linphone publishes its app for ARM processors only, and gameDroid's Android
runs on Intel and AMD ones. So gameDroid builds the app itself, from an
unchanged release of the source plus exactly what this script does:

  1. build for x86-64 instead of ARM;
  2. take out Google's Firebase libraries (push messages and crash
     reports), which are not free software and do nothing on an Android
     without Google's services. The changes follow F-Droid's recipe for
     the same app;
  3. sign with the key named in the environment.

Usage: prepare.py <linphone-android checkout>
Environment: LINPHONE_KEYSTORE (a path), LINPHONE_KEY_ALIAS,
LINPHONE_KEYSTORE_PASSWORD, LINPHONE_KEY_PASSWORD.
"""
import os
import pathlib
import re
import sys


def edit(path, changes):
    """Apply (pattern, replacement, least number of matches) to a file."""
    text = path.read_text(encoding="utf-8")
    for pattern, replacement, least in changes:
        text, n = re.subn(pattern, replacement, text, flags=re.M | re.S)
        if n < least:
            sys.exit(f"prepare: {path}: expected {pattern!r} at least {least} time(s), found {n}")
        print(f"prepare: {path.name}: {n} x {pattern[:60]!r}")
    path.write_text(text, encoding="utf-8")


def main():
    root = pathlib.Path(sys.argv[1])
    app = root / "app"
    main_src = app / "src" / "main"

    (app / "google-services.json").unlink(missing_ok=True)

    edit(
        app / "build.gradle.kts",
        [
            (r'abiFilters \+= listOf\([^)]*\)', 'abiFilters += listOf("x86_64")', 1),
            (r"^import com\.google\.(firebase|gms)\.[^\n]*\n", "", 3),
            (r"^\s*apply<(GoogleServicesPlugin|CrashlyticsPlugin)>\(\)\n", "", 2),
            (r"^\s*if \(crashlyticsAvailable\) \{\s*val path = [^\n]*\n\s*configure<CrashlyticsExtension> \{[^}]*\}\s*\}\n", "", 2),
            (r"^\s*implementation\(platform\(libs\.google\.firebase\.bom\)\)\n", "", 1),
            (r"^\s*implementation\(libs\.google\.firebase\.messaging\)\n", "", 1),
            (r"^\s*if \(crashlyticsAvailable\) \{\s*implementation\(libs\.google\.firebase\.crashlytics\)\s*\} else \{\s*compileOnly\(libs\.google\.firebase\.crashlytics\)\s*\}\n", "", 1),
            # The source's own style check rewrites files as it builds.
            (r'^project\.tasks\.preBuild\.dependsOn\("ktlintFormat"\)\n', "", 1),
        ],
    )
    edit(
        root / "build.gradle.kts",
        [(r"^\s*alias\(libs\.plugins\.(googleGmsServices|crashlytics)\) apply false\n", "", 2)],
    )
    edit(
        main_src / "AndroidManifest.xml",
        [(r'\s*<service android:name="org\.linphone\.core\.tools\.firebase\.FirebaseMessaging".*?</service>', "", 1)],
    )
    edit(
        main_src / "java/org/linphone/core/CoreContext.kt",
        [(r"^[^\n]*FirebaseCrashlytics[^\n]*\n", "", 2)],
    )
    edit(
        main_src / "java/org/linphone/ui/main/help/viewmodel/HelpViewModel.kt",
        [(r"^[^\n]*firebase[^\n]*\n", "", 2)],
    )
    edit(
        main_src / "res/layout/help_debug_fragment.xml",
        [(r"<[^<]+firebase[^<]+/>", "", 1)],
    )
    # The published Linphone SDK still holds a few classes that name
    # Firebase; with the libraries gone they are never loaded.
    with open(app / "proguard-rules.pro", "a", encoding="utf-8") as rules:
        rules.write("\n-dontwarn com.google.firebase.**\n-dontwarn com.google.android.gms.**\n")

    keystore = os.environ["LINPHONE_KEYSTORE"]
    (root / "keystore.properties").write_text(
        "storeFile={}\nkeyAlias={}\nstorePassword={}\nkeyPassword={}\n".format(
            keystore,
            os.environ["LINPHONE_KEY_ALIAS"],
            os.environ["LINPHONE_KEYSTORE_PASSWORD"],
            os.environ["LINPHONE_KEY_PASSWORD"],
        ),
        encoding="utf-8",
    )

    left = []
    for path in [app / "build.gradle.kts", root / "build.gradle.kts", main_src / "AndroidManifest.xml"]:
        left += [f"{path.name}: {l.strip()}" for l in path.read_text().splitlines()
                 if re.search(r"com\.google\.(firebase|gms)|GoogleServicesPlugin|CrashlyticsPlugin", l)]
    for path in (main_src / "java").rglob("*.kt"):
        left += [f"{path.name}: {l.strip()}" for l in path.read_text().splitlines()
                 if "com.google.firebase" in l or "FirebaseCrashlytics" in l or "FirebaseApp." in l]
    if left:
        sys.exit("prepare: Firebase is still named in:\n  " + "\n  ".join(left))
    print("prepare: done; nothing in the app's build names Firebase any more")


if __name__ == "__main__":
    main()
