#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Load gfxstream's host library and ask it to start, as a VMM would.

GPU track, step G2a (docs/engine/E8-gpu-plan.md). Before any device work:
does the library we built initialise on this machine, with which renderers,
and what capability sets does it then offer a guest?

Usage: probe_gfxstream.py <library> [gles] [vulkan]

Each mode is tried in a child process, because a failed start can abort the
process rather than return an error. Written from gfxstream's
host/include/gfxstream/virtio-gpu-gfxstream-renderer.h (version 0.1.2).
"""
import ctypes
import subprocess
import sys

USE_EGL = 1 << 0
USE_SURFACELESS = 1 << 3
USE_GLES = 1 << 4
USE_VK = 1 << 5

PARAM_USER_DATA = 1
PARAM_RENDERER_FLAGS = 2
PARAM_FENCE_CALLBACK = 3
PARAM_WIN0_WIDTH = 4
PARAM_WIN0_HEIGHT = 5
PARAM_DEBUG_CALLBACK = 6

CAPSETS = {3: "GFXSTREAM_VULKAN", 8: "GFXSTREAM_GLES", 9: "GFXSTREAM_COMPOSER"}


class Param(ctypes.Structure):
    _fields_ = [("key", ctypes.c_uint64), ("value", ctypes.c_uint64)]


class Debug(ctypes.Structure):
    _fields_ = [("debug_type", ctypes.c_uint32), ("message", ctypes.c_char_p)]


FENCE_CB = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p)
DEBUG_CB = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.POINTER(Debug))


def start(library, modes):
    lib = ctypes.CDLL(library)
    flags = USE_SURFACELESS
    if "gles" in modes:
        flags |= USE_EGL | USE_GLES
    if "vulkan" in modes:
        flags |= USE_VK

    def on_debug(_user, debug):
        kind = {1: "error", 2: "warn", 3: "info", 4: "debug"}.get(debug.contents.debug_type, "?")
        if kind in ("error", "warn"):
            text = (debug.contents.message or b"").decode("utf-8", "replace").strip()
            print(f"  renderer {kind}: {text[:200]}", flush=True)

    fence_cb = FENCE_CB(lambda _user, _fence: None)
    debug_cb = DEBUG_CB(on_debug)
    params = (Param * 6)(
        Param(PARAM_USER_DATA, 0),
        Param(PARAM_RENDERER_FLAGS, flags),
        Param(PARAM_FENCE_CALLBACK, ctypes.cast(fence_cb, ctypes.c_void_p).value),
        Param(PARAM_WIN0_WIDTH, 720),
        Param(PARAM_WIN0_HEIGHT, 1280),
        Param(PARAM_DEBUG_CALLBACK, ctypes.cast(debug_cb, ctypes.c_void_p).value),
    )
    lib.stream_renderer_init.restype = ctypes.c_int
    rc = lib.stream_renderer_init(params, ctypes.c_uint64(len(params)))
    print(f"  stream_renderer_init returned {rc}", flush=True)
    if rc != 0:
        return 1
    for number, name in CAPSETS.items():
        version, size = ctypes.c_uint32(0), ctypes.c_uint32(0)
        lib.stream_renderer_get_cap_set(ctypes.c_uint32(number), ctypes.byref(version), ctypes.byref(size))
        offered = "offered" if size.value else "not offered"
        print(f"  capability set {number} {name}: {offered} (version {version.value}, {size.value} bytes)", flush=True)
    # A context, as the guest's first act would be.
    lib.stream_renderer_context_create.restype = ctypes.c_int
    rc = lib.stream_renderer_context_create(ctypes.c_uint32(1), ctypes.c_uint32(5), b"probe", ctypes.c_uint32(0))
    print(f"  context_create returned {rc}", flush=True)
    lib.stream_renderer_context_destroy(ctypes.c_uint32(1))
    lib.stream_renderer_teardown()
    print("  teardown done", flush=True)
    return 0


def main():
    if len(sys.argv) >= 3 and sys.argv[1] == "--child":
        return start(sys.argv[2], sys.argv[3:])
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    library = sys.argv[1]
    worst = 0
    for modes in (["gles"], ["vulkan"], ["gles", "vulkan"]):
        label = "+".join(modes)
        print(f"--- {label}", flush=True)
        try:
            done = subprocess.run([sys.executable, __file__, "--child", library, *modes], timeout=120)
            code = done.returncode
        except subprocess.TimeoutExpired:
            code = "timed out"
        print(f"GFXSTREAM_START_{label.upper().replace('+', '_')}={'PASS' if code == 0 else f'FAIL ({code})'}", flush=True)
        worst = worst or (0 if code == 0 else 1)
    return worst


if __name__ == "__main__":
    sys.exit(main())
