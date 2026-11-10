#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tests for android_prepare.py, using small synthetic images.

Run: python3 tools/engine/test_android_prepare.py
"""
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import android_prepare as ap  # noqa: E402

PAGE = 4096


def pad(b, to=PAGE):
    return b + b"\0" * (ap.align(len(b), to) - len(b))


def boot_image(kernel=b"", ramdisk=b"", cmdline=b""):
    h = bytearray(PAGE)
    h[:8] = b"ANDROID!"
    struct.pack_into("<2I", h, 8, len(kernel), len(ramdisk))
    struct.pack_into("<I", h, 20, 1584)
    struct.pack_into("<I", h, 40, 4)
    h[44:44 + len(cmdline)] = cmdline
    return bytes(h) + pad(kernel) + pad(ramdisk)


def vendor_boot(ramdisk, cmdline, bootconfig):
    h = bytearray(PAGE)
    h[:8] = b"VNDRBOOT"
    struct.pack_into("<2I", h, 8, 4, PAGE)
    struct.pack_into("<I", h, 24, len(ramdisk))
    h[28:28 + len(cmdline)] = cmdline
    dtb, table = b"D" * 100, b"T" * 108
    struct.pack_into("<2I", h, 2096, 2128, len(dtb))
    struct.pack_into("<4I", h, 2112, len(table), 1, 108, len(bootconfig))
    return bytes(h) + pad(ramdisk) + pad(dtb) + pad(table) + pad(bootconfig)


class PrepareTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="gd-prepare-")
        self.images = os.path.join(self.dir, "images")
        self.out = os.path.join(self.dir, "out")
        os.makedirs(self.images)
        files = {
            "boot.img": boot_image(kernel=b"KERNEL" * 1000),
            "init_boot.img": boot_image(ramdisk=b"GENERIC-RAMDISK"),
            "vendor_boot.img": vendor_boot(
                b"VENDOR-RAMDISK!", b"console=ttynull stack_depot_disable=on bootconfig",
                b'androidboot.boot_devices = "from-vendor"\nkernel.x = "1"\n'),
            "vbmeta.img": b"V" * 65536,
            "vbmeta_system.img": b"S" * 65536,
            "vbmeta_vendor_dlkm.img": b"W" * 65536,
            "vbmeta_system_dlkm.img": b"X" * 65536,
            "super.img": b"SUPER" + b"\0" * (3 * ap.MIB - 5),
            "userdata.img": b"DATA" + b"\0" * (ap.MIB + 4096 - 4),  # not MiB aligned
        }
        for name, data in files.items():
            with open(os.path.join(self.images, name), "wb") as f:
                f.write(data)

    def tearDown(self):
        shutil.rmtree(self.dir)

    def read(self, name, mode="rb"):
        with open(os.path.join(self.out, name), mode) as f:
            return f.read()

    def test_kernel_initrd_and_cmdline(self):
        info = ap.prepare(self.images, self.out, "pci0000:00/0000:00:01.0")
        self.assertEqual(self.read("kernel"), b"KERNEL" * 1000)
        initrd = self.read("initrd.img")
        self.assertTrue(initrd.startswith(b"VENDOR-RAMDISK!GENERIC-RAMDISK"))
        self.assertEqual(len(initrd) % 4, 0, "the kernel wants the file 4-byte aligned")
        self.assertEqual(info["initrd_bytes"], len(initrd))
        config = ap.read_bootconfig_trailer(initrd)
        self.assertIsNotNone(config, "trailer must parse and checksum")
        self.assertEqual(config, self.read("bootconfig.txt", "r").rstrip("\n") + "\n")
        self.assertIn('kernel.x = "1"', config, "the image's own bootconfig is kept")
        self.assertIn('androidboot.slot_suffix = "_a"', config)
        # The image set boot_devices too. The kernel rejects a key set twice,
        # so only ours may remain.
        self.assertNotIn("from-vendor", config)
        self.assertIn("pci0000:00/0000:00:01.0", config)
        keys = [ap.bootconfig_key(l) for l in config.splitlines()]
        keys = [k for k in keys if k]
        self.assertEqual(len(keys), len(set(keys)), "no key is set twice")
        cmdline = self.read("cmdline.txt", "r").split()
        self.assertIn("console=ttyS0", cmdline)
        self.assertNotIn("console=ttynull", cmdline)
        self.assertIn("8250.nr_uarts=1", cmdline)
        self.assertIn("stack_depot_disable=on", cmdline)
        self.assertEqual(cmdline.count("bootconfig"), 1)

    def test_bootconfig_merge(self):
        merged = ap.merge_bootconfig('a = 1\n# note\nb = "x"\nc := 3\n', 'b = "y"\nd = 4\n')
        self.assertEqual(merged, 'a = 1\n# note\nc := 3\nb = "y"\nd = 4\n')
        self.assertEqual(ap.merge_bootconfig("", "a = 1\n"), "a = 1\n")
        with self.assertRaises(ValueError):
            ap.merge_bootconfig("", "a = 1\na = 2\n")
        with self.assertRaises(ValueError):
            ap.merge_bootconfig("z = 1\nz = 2\n", "a = 1\n")

    def test_trailer_layout_matches_the_kernel_format(self):
        for initrd_len in (0, 1, 2, 3, 4097):
            t = ap.bootconfig_trailer("a = 1", initrd_len)
            self.assertTrue(t.endswith(b"#BOOTCONFIG\n"))
            self.assertEqual((initrd_len + len(t)) % 4, 0)
            size, checksum = struct.unpack_from("<2I", t, len(t) - 20)
            body = t[:size]
            self.assertEqual(size, len(t) - 20)
            self.assertEqual(checksum, sum(body) & 0xFFFFFFFF)
            self.assertTrue(body.startswith(b"a = 1\n\0"))
        self.assertIsNone(ap.read_bootconfig_trailer(b"no trailer here"))

    def test_disk_layout_and_gpt(self):
        info = ap.prepare(self.images, self.out, "x", flat=True)
        layout = self.read("os.layout", "r").splitlines()
        self.assertEqual(layout[0], "bvmm-disk-layout 1")
        pieces = [l.split(" ", 2) for l in layout if l and l[0].isdigit()]
        self.assertEqual(sum(int(p[0]) for p in pieces), info["disk_bytes"])
        self.assertTrue(all(int(p[0]) % ap.MIB == 0 for p in pieces))
        by_name = {os.path.basename(p[2]): p for p in pieces}
        self.assertEqual(by_name["super.img"][1], "rw")
        self.assertEqual(by_name["boot.img"][1], "ro")
        self.assertEqual(int(by_name["userdata.img"][0]), 2 * ap.MIB, "rounded up to 1 MiB")
        self.assertTrue(os.path.isabs(by_name["super.img"][2]))
        self.assertEqual(os.path.getsize(os.path.join(self.out, "metadata.img")), 64 * ap.MIB)

        raw = self.read("os.raw")
        self.assertEqual(len(raw), info["disk_bytes"])
        self.assertEqual(raw[510:512], b"\x55\xaa")
        self.assertEqual(raw[450], 0xEE, "protective MBR")
        hdr = raw[512:512 + 92]
        self.assertEqual(hdr[:8], b"EFI PART")
        crc = struct.unpack_from("<I", hdr, 16)[0]
        self.assertEqual(zlib.crc32(hdr[:16] + b"\0\0\0\0" + hdr[20:]), crc)
        entries = raw[1024:1024 + 128 * 128]
        self.assertEqual(zlib.crc32(entries), struct.unpack_from("<I", hdr, 88)[0])
        # Each partition's first bytes are where its entry says.
        want = {"boot_a": b"ANDROID!", "super": b"SUPER", "userdata": b"DATA",
                "vbmeta_a": b"VVVV", "vendor_boot_a": b"VNDRBOOT"}
        found = {}
        for i in range(128):
            e = entries[i * 128:(i + 1) * 128]
            first, last = struct.unpack_from("<2Q", e, 32)
            name = e[56:].decode("utf-16-le").rstrip("\0")
            if not name:
                continue
            found[name] = (first, last)
            self.assertEqual(first * 512 % ap.MIB, 0)
            if name in want:
                self.assertEqual(raw[first * 512:first * 512 + len(want[name])], want[name], name)
        self.assertEqual(list(found), [p[0] for p in ap.PARTITIONS])
        spans = sorted(found.values())
        for (a_first, a_last), (b_first, _b) in zip(spans, spans[1:]):
            self.assertEqual(a_last + 1, b_first, "partitions are back to back")
        # The backup header at the last sector points back at the primary.
        backup = raw[-512:][:92]
        self.assertEqual(backup[:8], b"EFI PART")
        self.assertEqual(struct.unpack_from("<2Q", backup, 24), (len(raw) // 512 - 1, 1))

    def test_an_independent_tool_accepts_the_partition_table(self):
        if not shutil.which("sfdisk"):
            self.skipTest("sfdisk is not installed")
        ap.prepare(self.images, self.out, "x", flat=True)
        raw = os.path.join(self.out, "os.raw")
        verify = subprocess.run(["sfdisk", "--verify", raw], capture_output=True, text=True)
        self.assertEqual(verify.returncode, 0, verify.stdout + verify.stderr)
        self.assertIn("No errors detected", verify.stdout)
        dump = subprocess.run(["sfdisk", "--dump", raw], capture_output=True, text=True).stdout
        self.assertIn("label: gpt", dump)
        for name, _s, _b, _w in ap.PARTITIONS:
            self.assertIn(f'name="{name}"', dump)

    def test_bad_images_are_refused(self):
        with open(os.path.join(self.images, "boot.img"), "wb") as f:
            f.write(b"not a boot image")
        with self.assertRaises(ValueError):
            ap.prepare(self.images, self.out, "x")


if __name__ == "__main__":
    unittest.main()
