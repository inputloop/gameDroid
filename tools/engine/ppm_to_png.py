#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Convert a binary PPM (P6, 8 bits per channel) to PNG. No dependencies.

Usage: ppm_to_png.py in.ppm out.png
bvmm writes its screenshots as PPM because that needs no library; this turns
one into a PNG for documents.
"""
import struct
import sys
import zlib


def read_ppm(data):
    fields, at = [], 0
    while len(fields) < 4:
        while data[at:at + 1].isspace():
            at += 1
        if data[at:at + 1] == b"#":
            at = data.index(b"\n", at) + 1
            continue
        end = at
        while not data[end:end + 1].isspace():
            end += 1
        fields.append(data[at:end])
        at = end
    magic, width, height, maxval = fields[0], int(fields[1]), int(fields[2]), int(fields[3])
    if magic != b"P6" or maxval != 255:
        raise ValueError("only binary PPM with 8 bits per channel is handled")
    pixels = data[at + 1:at + 1 + width * height * 3]
    if len(pixels) != width * height * 3:
        raise ValueError("PPM is truncated")
    return width, height, pixels


def png(width, height, rgb):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    stride = width * 3
    raw = b"".join(b"\0" + rgb[y * stride:(y + 1) * stride] for y in range(height))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b""))


def main():
    with open(sys.argv[1], "rb") as f:
        width, height, rgb = read_ppm(f.read())
    with open(sys.argv[2], "wb") as f:
        f.write(png(width, height, rgb))
    distinct = len(set(rgb[i:i + 3] for i in range(0, len(rgb), 3 * 97)))
    print(f"{width}x{height}, {distinct} distinct colours in a sample")
    return 0


if __name__ == "__main__":
    sys.exit(main())
