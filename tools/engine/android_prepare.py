#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Turn a stock Android (Cuttlefish) image set into what bvmm boots directly.

Cuttlefish normally starts Android through a bootloader that reads the boot
partitions, builds the boot configuration and loads the kernel. bvmm loads a
kernel directly, so this does the bootloader's part ahead of time:

  kernel        the kernel from boot.img
  initrd.img    vendor ramdisk + generic ramdisk + boot configuration trailer
  cmdline.txt   the kernel command line
  os.layout     a GPT-partitioned disk in bvmm's disk-layout format, built
                around the partition images without copying them

Usage:
  android_prepare.py <image-dir> <out-dir> [--boot-device pci0000:00/0000:00:01.0]
                     [--flat]   also write os.raw, a plain raw copy (testing)

Formats: Android boot image headers v3/v4 and vendor boot v3/v4 (AOSP
bootimg.h), kernel boot configuration trailer (Linux
Documentation/admin-guide/bootconfig.rst), GPT (UEFI 2.10 section 5.3).
"""
import argparse
import os
import struct
import sys
import uuid
import zlib

PAGE = 4096
SECTOR = 512
MIB = 1 << 20
LINUX_FS = uuid.UUID("0fc63daf-8483-4772-8e79-3d69d8477de4")
NAMESPACE = uuid.UUID("6f0c5b0a-9d0e-4d5f-8a52-67616d656472")
BOOTCONFIG_MAGIC = b"#BOOTCONFIG\n"


def align(n, to):
    return (n + to - 1) // to * to


def read_boot_image(path):
    """Return (kernel, ramdisk, cmdline) from a boot.img / init_boot.img (v3 or v4)."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"ANDROID!":
        raise ValueError(f"{path}: not an Android boot image")
    kernel_size, ramdisk_size = struct.unpack_from("<2I", data, 8)
    version = struct.unpack_from("<I", data, 40)[0]
    if version not in (3, 4):
        raise ValueError(f"{path}: boot image header v{version} is not handled")
    cmdline = data[44:44 + 1536].split(b"\0", 1)[0].decode()
    kernel = data[PAGE:PAGE + kernel_size]
    start = PAGE + align(kernel_size, PAGE)
    ramdisk = data[start:start + ramdisk_size]
    if len(kernel) != kernel_size or len(ramdisk) != ramdisk_size:
        raise ValueError(f"{path}: truncated")
    return kernel, ramdisk, cmdline


def read_vendor_boot(path):
    """Return (vendor ramdisk, cmdline, bootconfig text) from vendor_boot.img."""
    with open(path, "rb") as f:
        data = f.read()
    if data[:8] != b"VNDRBOOT":
        raise ValueError(f"{path}: not a vendor boot image")
    version, page = struct.unpack_from("<2I", data, 8)
    if version not in (3, 4):
        raise ValueError(f"{path}: vendor boot header v{version} is not handled")
    ramdisk_size = struct.unpack_from("<I", data, 24)[0]
    cmdline = data[28:28 + 2048].split(b"\0", 1)[0].decode()
    header_size, dtb_size = struct.unpack_from("<2I", data, 2096)
    at = align(header_size, page)
    ramdisk = data[at:at + ramdisk_size]
    if len(ramdisk) != ramdisk_size:
        raise ValueError(f"{path}: truncated")
    bootconfig = ""
    if version == 4:
        table_size, _num, _entry, bootconfig_size = struct.unpack_from("<4I", data, 2112)
        at += align(ramdisk_size, page) + align(dtb_size, page) + align(table_size, page)
        bootconfig = data[at:at + bootconfig_size].decode()
    return ramdisk, cmdline, bootconfig


def bootconfig_trailer(text, initrd_len):
    """The bytes to append to an initrd of `initrd_len` bytes to carry `text`."""
    body = text.encode()
    if not body.endswith(b"\n"):
        body += b"\n"
    body += b"\0"
    # The whole file must end 4-byte aligned; padding counts as data.
    total = initrd_len + len(body) + 8 + len(BOOTCONFIG_MAGIC)
    body += b"\0" * (align(total, 4) - total)
    checksum = sum(body) & 0xFFFFFFFF
    return body + struct.pack("<2I", len(body), checksum) + BOOTCONFIG_MAGIC


def read_bootconfig_trailer(blob):
    """Inverse of bootconfig_trailer, as the kernel does it. For tests."""
    if not blob.endswith(BOOTCONFIG_MAGIC):
        return None
    size, checksum = struct.unpack_from("<2I", blob, len(blob) - len(BOOTCONFIG_MAGIC) - 8)
    end = len(blob) - len(BOOTCONFIG_MAGIC) - 8
    body = blob[end - size:end]
    if sum(body) & 0xFFFFFFFF != checksum:
        return None
    return body.rstrip(b"\0").decode()


def gpt(parts, disk_guid):
    """Build GPT structures for `parts`: a list of (name, bytes, guid), each
    1 MiB aligned, placed from 1 MiB onwards. Returns (head, tail, total).

    head is LBA 0 to 33 (protective MBR, header, entries). tail is the last
    1 MiB of the disk, ending with the backup entries and header.
    """
    entries = bytearray(128 * 128)
    lba = MIB // SECTOR
    for i, (name, size, guid) in enumerate(parts):
        sectors = align(size, MIB) // SECTOR
        struct.pack_into("<16s16sQQQ", entries, i * 128, LINUX_FS.bytes_le, guid.bytes_le,
                         lba, lba + sectors - 1, 0)
        encoded = name.encode("utf-16-le")
        if len(encoded) > 72:
            raise ValueError(f"partition name too long: {name}")
        entries[i * 128 + 56:i * 128 + 56 + len(encoded)] = encoded
        lba += sectors
    total = lba * SECTOR + MIB
    last = total // SECTOR - 1
    entries_crc = zlib.crc32(entries)

    def header(this_lba, other_lba, entries_lba):
        h = bytearray(92)
        struct.pack_into("<8sIIIIQQQQ16sQIII", h, 0, b"EFI PART", 0x00010000, 92, 0, 0,
                         this_lba, other_lba, MIB // SECTOR, last - 33,
                         disk_guid.bytes_le, entries_lba, 128, 128, entries_crc)
        struct.pack_into("<I", h, 16, zlib.crc32(h))
        return bytes(h) + b"\0" * (SECTOR - 92)

    mbr = bytearray(SECTOR)
    # One protective partition of type 0xEE covering the disk.
    struct.pack_into("<B3sB3sII", mbr, 446, 0, b"\x00\x02\x00", 0xEE, b"\xff\xff\xff", 1,
                     min(last, 0xFFFFFFFF))
    mbr[510:512] = b"\x55\xaa"
    head = bytes(mbr) + header(1, last, 2) + bytes(entries)
    tail = b"\0" * (MIB - 33 * SECTOR) + bytes(entries) + header(last, 1, last - 32)
    return head, tail, total


# Partitions Android's first-stage init looks for, in Cuttlefish's order.
# (name, source file or None for a blank partition, blank size, writable)
PARTITIONS = [
    ("misc", None, 1 * MIB, True),
    ("boot_a", "boot.img", 0, False),
    ("init_boot_a", "init_boot.img", 0, False),
    ("vendor_boot_a", "vendor_boot.img", 0, False),
    ("vbmeta_a", "vbmeta.img", 0, False),
    ("vbmeta_system_a", "vbmeta_system.img", 0, False),
    ("vbmeta_vendor_dlkm_a", "vbmeta_vendor_dlkm.img", 0, False),
    ("vbmeta_system_dlkm_a", "vbmeta_system_dlkm.img", 0, False),
    ("super", "super.img", 0, True),
    ("userdata", "userdata.img", 0, True),
    ("metadata", None, 64 * MIB, True),
    ("frp", None, 1 * MIB, True),
]


def runtime_bootconfig(boot_device, vbmeta_guid, memory_mib, display):
    """What the bootloader would add: the virtual hardware Android is on.

    Taken from a stock Cuttlefish boot under crosvm (docs/engine/reference),
    keeping what describes the device and dropping what named crosvm-only
    host services.
    """
    width, height, dpi = display
    lines = [
        'androidboot.hardware = "cutf_cvm"',
        'androidboot.slot_suffix = "_a"',
        'androidboot.force_normal_boot = "1"',
        'androidboot.fstab_suffix = "cf.f2fs.hctr2"',
        f'androidboot.boot_devices = "{boot_device}"',
        'androidboot.serialno = "GAMEDROID0001"',
        f'androidboot.ddr_size = "{memory_mib}MB"',
        f'androidboot.lcd_density = "{dpi}"',
        'androidboot.setupwizard_mode = "DISABLED"',
        'androidboot.enable_bootanimation = "0"',
        'androidboot.hw_timeout_multiplier = "3"',
        # Graphics: the guest draws with the CPU until the host GPU is wired in.
        'androidboot.hardware.hwcomposer = "ranchu"',
        'androidboot.hardware.hwcomposer.display_finder_mode = "drm"',
        'androidboot.hardware.hwcomposer.display_framebuffer_format = "rgba"',
        'androidboot.hardware.gralloc = "minigbm"',
        'androidboot.hardware.egl = "angle"',
        'androidboot.hardware.vulkan = "pastel"',
        'androidboot.hardware.angle_feature_overrides_enabled = "mapUnspecifiedColorSpaceToPassThrough"',
        'androidboot.hardware.angle_feature_overrides_disabled = "exposeES32ForTesting"',
        'androidboot.cpuvulkan.version = "4206592"',
        'androidboot.opengles.version = "196609"',
        # Security services that run inside the guest, needing no host peer.
        'androidboot.vendor.apex.com.android.hardware.keymint = "com.android.hardware.keymint.rust_nonsecure"',
        'androidboot.vendor.apex.com.android.hardware.gatekeeper = "com.android.hardware.gatekeeper.nonsecure"',
        'androidboot.vendor.apex.com.android.hardware.graphics.composer = "com.android.hardware.graphics.composer.ranchu"',
        'androidboot.vendor.apex.com.android.hardware.secure_element = "com.android.hardware.secure_element"',
        'androidboot.vendor.apex.com.android.hardware.strongbox = "none"',
        'androidboot.vendor.apex.com.android.hardware.weaver = "none"',
        'androidboot.vendor.apex.com.google.emulated.camera.provider.hal = "com.google.emulated.camera.provider.hal"',
        'androidboot.hypervisor.vm.supported = "0"',
        'androidboot.hypervisor.protected_vm.supported = "0"',
        'androidboot.hypervisor.version = "gamedroid-bvmm"',
        'androidboot.serialconsole = "0"',
        # Verified boot: unlocked, as on the stock virtual device.
        f'androidboot.vbmeta.device = "PARTUUID={vbmeta_guid}"',
        'androidboot.vbmeta.avb_version = "1.2"',
        'androidboot.vbmeta.device_state = "unlocked"',
        'androidboot.vbmeta.hash_alg = "sha256"',
        'androidboot.vbmeta.invalidate_on_error = "yes"',
        'androidboot.veritymode = "enforcing"',
        'androidboot.verifiedbootstate = "orange"',
    ]
    return "\n".join(lines) + "\n"


def kernel_cmdline(vendor_cmdline):
    """The command line: the image's own, then what the bootloader adds.

    The console goes to the 16550 port bvmm emulates. Android's kernel has no
    such ports unless asked (8250.nr_uarts).
    """
    theirs = [p for p in vendor_cmdline.split() if not p.startswith("console=")]
    ours = [
        "console=ttyS0", "8250.nr_uarts=1", "earlycon=uart8250,io,0x3f8",
        "printk.devkmsg=on", "audit=1", "binder.impl=rust", "cma=0",
        "firmware_class.path=/vendor/etc/", "loop.max_part=7", "init=/init", "bootconfig",
    ]
    seen, out = set(), []
    for p in theirs + ours:
        if p not in seen:
            seen.add(p)
            out.append(p)
    return " ".join(out)


def prepare(image_dir, out_dir, boot_device, flat=False, memory_mib=4096,
            display=(720, 1280, 280)):
    os.makedirs(out_dir, exist_ok=True)
    kernel, _none, _boot_cmdline = read_boot_image(os.path.join(image_dir, "boot.img"))
    _nokernel, generic_ramdisk, _ = read_boot_image(os.path.join(image_dir, "init_boot.img"))
    vendor_ramdisk, vendor_cmdline, vendor_bootconfig = read_vendor_boot(
        os.path.join(image_dir, "vendor_boot.img"))

    def guid(name):
        return uuid.uuid5(NAMESPACE, name)

    bootconfig = vendor_bootconfig
    if bootconfig and not bootconfig.endswith("\n"):
        bootconfig += "\n"
    bootconfig += runtime_bootconfig(boot_device, guid("vbmeta_a"), memory_mib, display)

    # The kernel unpacks concatenated ramdisks in order, later files winning.
    initrd = vendor_ramdisk + generic_ramdisk
    initrd += bootconfig_trailer(bootconfig, len(initrd))
    with open(os.path.join(out_dir, "kernel"), "wb") as f:
        f.write(kernel)
    with open(os.path.join(out_dir, "initrd.img"), "wb") as f:
        f.write(initrd)
    with open(os.path.join(out_dir, "bootconfig.txt"), "w") as f:
        f.write(bootconfig)
    with open(os.path.join(out_dir, "cmdline.txt"), "w") as f:
        f.write(kernel_cmdline(vendor_cmdline) + "\n")

    parts, pieces = [], []
    for name, source, blank_size, writable in PARTITIONS:
        if source is None:
            path = os.path.join(out_dir, f"{name}.img")
            if not os.path.exists(path):
                with open(path, "wb") as f:
                    f.truncate(blank_size)
            size = blank_size
        else:
            path = os.path.abspath(os.path.join(image_dir, source))
            size = os.path.getsize(path)
        parts.append((name, size, guid(name)))
        pieces.append((align(size, MIB), "rw" if writable else "ro", os.path.abspath(path)))

    head, tail, total = gpt(parts, guid("gamedroid-os-disk"))
    with open(os.path.join(out_dir, "gpt-head.bin"), "wb") as f:
        f.write(head)
    with open(os.path.join(out_dir, "gpt-tail.bin"), "wb") as f:
        f.write(tail)
    lines = ["bvmm-disk-layout 1", "# Written by gameDroid tools/engine/android_prepare.py",
             f"{MIB} rw gpt-head.bin"]
    lines += [f"{length} {mode} {path}" for length, mode, path in pieces]
    lines.append(f"{MIB} rw gpt-tail.bin")
    with open(os.path.join(out_dir, "os.layout"), "w") as f:
        f.write("\n".join(lines) + "\n")
    assert MIB + sum(p[0] for p in pieces) + MIB == total

    if flat:
        with open(os.path.join(out_dir, "os.raw"), "wb") as f:
            f.write(head)
            at = MIB
            for length, _mode, path in pieces:
                f.seek(at)
                with open(path, "rb") as src:
                    while chunk := src.read(8 * MIB):
                        f.write(chunk)
                at += length
            f.seek(at)
            f.write(tail)

    return {
        "kernel_bytes": len(kernel),
        "initrd_bytes": len(initrd),
        "disk_bytes": total,
        "partitions": [p[0] for p in parts],
        "vendor_bootconfig_bytes": len(vendor_bootconfig),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("image_dir")
    ap.add_argument("out_dir")
    ap.add_argument("--boot-device", default="pci0000:00/0000:00:01.0")
    ap.add_argument("--memory-mib", type=int, default=4096)
    ap.add_argument("--flat", action="store_true")
    args = ap.parse_args()
    info = prepare(args.image_dir, args.out_dir, args.boot_device, args.flat, args.memory_mib)
    for key, value in info.items():
        print(f"{key}={value}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
