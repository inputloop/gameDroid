#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build gfxstream's host library at the commit gameDroid is written for.
# The library is what lets Android draw with the computer's graphics card.
#
# Usage: tools/ci/build-gfxstream.sh <work-dir>
# Prints the built library's path as its last line. Linux needs cmake,
# ninja and the X11, EGL, GLES, Vulkan and DRM development packages;
# Windows needs Visual Studio with the ClangCL toolset, and a bash shell.
set -euo pipefail
COMMIT=b8537e6d4db20caaa581d769a8543bddb377a30b
work="$1"; mkdir -p "$work"
src="$work/gfxstream"
if [ ! -d "$src/.git" ]; then
  git clone -q --filter=blob:none https://github.com/google/gfxstream "$src"
fi
git -C "$src" checkout -q "$COMMIT"
# A bug in gfxstream's Windows-only code at this commit: a std::string handed
# to a printf-style log call. Harmless to apply everywhere.
sed -i 's/mFeatures.VulkanAllocateHostMemory.getReason());/mFeatures.VulkanAllocateHostMemory.getReason().c_str());/' \
  "$src/host/vulkan/vk_common_operations.cpp"
mkdir -p "$src/build" && cd "$src/build"
case "$(uname -s)" in
  Linux)
    cmake .. -G Ninja -DCMAKE_BUILD_TYPE=Release > "$work/gfxstream-build.log" 2>&1
    ninja gfxstream_backend >> "$work/gfxstream-build.log" 2>&1
    lib="$PWD/libgfxstream_backend.so"
    # The symbols are nine tenths of the file.
    strip --strip-unneeded "$lib"
    ;;
  *)
    # windows.h must not pull in the old winsock.h ahead of WinSock2.h;
    # gfxstream's Bazel build defines this, its CMake build does not.
    export CFLAGS="-DWIN32_LEAN_AND_MEAN" CXXFLAGS="-DWIN32_LEAN_AND_MEAN"
    cmake .. -A x64 -T ClangCL > "$work/gfxstream-build.log" 2>&1
    cmake --build . --config Release --target gfxstream_backend -- -m -v:m >> "$work/gfxstream-build.log" 2>&1
    lib="$(find "$PWD" -iname 'gfxstream_backend.dll' | head -n 1)"
    lib="$(cygpath -m "$lib")"
    ;;
esac
[ -f "$lib" ] || { echo "the library was not built; see $work/gfxstream-build.log" >&2; tail -n 30 "$work/gfxstream-build.log" >&2; exit 1; }
echo "$lib"
