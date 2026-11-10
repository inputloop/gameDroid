#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Download the current release of a free-software app from F-Droid.

Usage: fetch_fdroid_apk.py <package> <out.apk>
Prints the version code and which native-code ABIs the APK carries (none
means pure Java/Kotlin, which runs on any processor).
"""
import json
import sys
import urllib.request
import zipfile

API = "https://f-droid.org/api/v1/packages/{}"
REPO = "https://f-droid.org/repo/{}_{}.apk"


def main():
    package, out = sys.argv[1], sys.argv[2]
    with urllib.request.urlopen(API.format(package), timeout=60) as r:
        info = json.load(r)
    code = info["suggestedVersionCode"]
    url = REPO.format(package, code)
    print(f"package={package}\nversion_code={code}\nurl={url}")
    urllib.request.urlretrieve(url, out)
    with zipfile.ZipFile(out) as z:
        abis = sorted({n.split("/")[1] for n in z.namelist() if n.startswith("lib/") and n.count("/") >= 2})
        size = sum(i.file_size for i in z.infolist())
    print(f"abis={','.join(abis) or 'none (pure Java/Kotlin)'}")
    print(f"unpacked_bytes={size}")
    if abis and "x86_64" not in abis:
        print("WARNING: no x86_64 code in this APK; it needs an ARM translator the image does not have")
    return 0


if __name__ == "__main__":
    sys.exit(main())
