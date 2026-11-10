#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Pull the kernel out of an Android boot.img (header version 3 or 4).

Usage: extract-boot-kernel.py boot.img kernel-out
Prints what it found. Exits non-zero if the image is not what we expect.
"""
import struct
import sys

PAGE = 4096


def main() -> int:
    src, dst = sys.argv[1], sys.argv[2]
    data = open(src, "rb").read()
    if data[:8] != b"ANDROID!":
        print("not an Android boot image (bad magic)")
        return 1
    kernel_size, ramdisk_size, os_version, header_size = struct.unpack_from("<4I", data, 8)
    header_version = struct.unpack_from("<I", data, 40)[0]
    print(f"header_version={header_version} kernel_size={kernel_size} ramdisk_size={ramdisk_size} header_size={header_size}")
    if header_version < 3:
        print("boot image header older than v3 is not handled")
        return 1
    kernel = data[PAGE:PAGE + kernel_size]
    if len(kernel) != kernel_size or kernel_size == 0:
        print("kernel is missing or truncated")
        return 1
    is_bzimage = kernel[0x202:0x206] == b"HdrS"
    print(f"first_bytes={kernel[:8].hex()} bzimage={is_bzimage}")
    if is_bzimage:
        proto = struct.unpack_from("<H", kernel, 0x206)[0]
        print(f"boot_protocol={proto >> 8}.{proto & 0xff:02d}")
    open(dst, "wb").write(kernel)
    return 0 if is_bzimage else 2


if __name__ == "__main__":
    sys.exit(main())
