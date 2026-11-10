#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Drive the gameDroid window with real X input and check what it shows.
# Runs the window in demo mode (a test card that draws each finger it is
# sent as a coloured square), so no Android or KVM is needed.
#
# Usage: xvfb-run -a -s "-screen 0 700x900x24" tools/ci/window-test.sh <gamedroid-binary> <out-dir>
# Needs xdotool and ImageMagick.
set -euo pipefail
bin="$1"; out="$2"; mkdir -p "$out"
here="$(cd "$(dirname "$0")/../.." && pwd)"

"$bin" --demo --profile "$here/profiles/example.shooter.toml" --frames 900 2> "$out/window.err" &
app=$!
for _ in $(seq 50); do
  win="$(xdotool search --name '^gameDroid' 2>/dev/null | head -n 1 || true)"
  [ -n "$win" ] && break
  sleep 0.2
done
[ -n "${win:-}" ] || { echo "FAIL: no window appeared"; exit 1; }
xdotool windowfocus "$win" 2>/dev/null || true
eval "$(xdotool getwindowgeometry --shell "$win")"   # X, Y, WIDTH, HEIGHT
echo "window ${WIDTH}x${HEIGHT} at ${X},${Y}: $(xdotool getwindowname "$win")"
at() { xdotool mousemove $((X + $1)) $((Y + $2)); }
has() { convert "$1" -unique-colors txt:- | grep -qi "$2"; }
fail=0
check() { if "$@"; then echo "ok: ${*: -2}"; else echo "FAIL: ${*: -2}"; fail=1; fi; }

# Direct mode: press and drag. The mouse's finger is drawn white.
at 150 300; sleep 0.2; xdotool mousedown 1; sleep 0.3; at 250 500; sleep 0.4
import -window root "$out/1-direct-drag.png"
check has "$out/1-direct-drag.png" '#FFFFFF'
xdotool mouseup 1; sleep 0.3
import -window root "$out/2-released.png"
check [ "$(convert "$out/2-released.png" -unique-colors txt:- | grep -ci '#FFFFFF')" = 0 ]

# Typing in direct mode shows a yellow bar per held key.
xdotool keydown a; sleep 0.3
import -window root "$out/3-typing.png"
check has "$out/3-typing.png" '#E0E040'
xdotool keyup a; sleep 0.2

# Game mode with the example profile: W pushes the stick (finger 0, red),
# the left button fires (finger 1, blue), moving the mouse looks (finger 2, green).
# A quick tap, as a person would do it: the window must not miss it.
xdotool key F1; sleep 0.4
echo "title now: $(xdotool getwindowname "$win")"
check [ "$(xdotool getwindowname "$win" | grep -c 'game mode')" = 1 ]
at 200 400; sleep 0.2
xdotool keydown w; sleep 0.3; xdotool mousedown 1; sleep 0.3; at 215 405; sleep 0.3
import -window root "$out/4-game-mode.png"
check has "$out/4-game-mode.png" '#FF5C5C'
check has "$out/4-game-mode.png" '#5CC8FF'
check has "$out/4-game-mode.png" '#7DFF7A'
# Nothing was typed into the guest: no yellow bar.
check [ "$(convert "$out/4-game-mode.png" -unique-colors txt:- | grep -ci '#E0E040')" = 0 ]
xdotool mouseup 1; xdotool keyup w; sleep 0.8
import -window root "$out/5-let-go.png"
check [ "$(convert "$out/5-let-go.png" -unique-colors txt:- | grep -ciE '#FF5C5C|#5CC8FF|#7DFF7A')" = 0 ]

kill "$app" 2>/dev/null || true
wait "$app" 2>/dev/null || true
if [ "$fail" = 0 ]; then echo "WINDOW_TEST=PASS"; else echo "WINDOW_TEST=FAIL"; exit 1; fi
