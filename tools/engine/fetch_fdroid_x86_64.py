#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Download the newest F-Droid release of an app that runs on x86-64.

Apps with native code are published once per processor kind, each under
its own version code. This tries the newest version codes in turn and
keeps the first APK that carries x86-64 code (or no native code at all).

Usage: fetch_fdroid_x86_64.py <package> <out.apk>
"""
import json
import sys
import urllib.request
import zipfile

API = "https://f-droid.org/api/v1/packages/{}"
REPO = "https://f-droid.org/repo/{}_{}.apk"


def abis(path):
    with zipfile.ZipFile(path) as z:
        return sorted({n.split("/")[1] for n in z.namelist() if n.startswith("lib/") and n.count("/") >= 2})


def main():
    package, out = sys.argv[1], sys.argv[2]
    with urllib.request.urlopen(API.format(package), timeout=60) as r:
        info = json.load(r)
    codes = sorted({p["versionCode"] for p in info["packages"]}, reverse=True)
    print(f"package={package}\nsuggested={info.get('suggestedVersionCode')}\nnewest_codes={codes[:8]}")
    for code in codes[:8]:
        url = REPO.format(package, code)
        try:
            urllib.request.urlretrieve(url, out)
        except Exception as e:  # a listed code whose file is gone
            print(f"version_code={code}: {e}")
            continue
        found = abis(out)
        print(f"version_code={code} abis={','.join(found) or 'none'}")
        if not found or "x86_64" in found:
            print(f"chosen={code}\nurl={url}")
            return 0
    print("no x86-64 build among the newest version codes")
    return 1


if __name__ == "__main__":
    sys.exit(main())
