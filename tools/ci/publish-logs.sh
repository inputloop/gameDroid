#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Publish files from a CI run to the `ci-logs` branch, under <name>/, replacing
# what the previous run of the same job left there. The branch holds only
# logs; it shares no history with the code.
#
# Usage: tools/ci/publish-logs.sh <name> <file>...
# Needs GITHUB_TOKEN with contents: write, and the usual GITHUB_* variables.
set -euo pipefail
name="$1"; shift
work="$(mktemp -d)"
url="https://x-access-token:${GITHUB_TOKEN}@github.com/${GITHUB_REPOSITORY}.git"
git -C "$work" init -q
git -C "$work" config user.name "github-actions[bot]"
git -C "$work" config user.email "41898282+github-actions[bot]@users.noreply.github.com"
for attempt in 1 2 3; do
  if git -C "$work" fetch -q --depth 1 "$url" ci-logs 2>/dev/null; then
    git -C "$work" checkout -q -B ci-logs FETCH_HEAD
  else
    git -C "$work" checkout -q --orphan ci-logs 2>/dev/null || true
  fi
  # An older run that finishes late must not replace a newer run's logs.
  last="$(sed -n 's/^run: //p' "$work/$name/RUN.txt" 2>/dev/null || true)"
  if [ -n "$last" ] && [ "$last" -gt "${GITHUB_RUN_ID}" ]; then
    echo "publish-logs: ci-logs already holds newer run $last; not publishing"; exit 0
  fi
  rm -rf "${work:?}/$name"; mkdir -p "$work/$name"
  for f in "$@"; do
    [ -f "$f" ] && cp "$f" "$work/$name/" || echo "publish-logs: no file $f"
  done
  printf 'run: %s\nsha: %s\nworkflow: %s\nutc: %s\n' "${GITHUB_RUN_ID}" "${GITHUB_SHA}" \
    "${GITHUB_WORKFLOW}" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$work/$name/RUN.txt"
  git -C "$work" add -A
  git -C "$work" commit -q -m "$name: run ${GITHUB_RUN_ID}" || true
  if git -C "$work" push -q "$url" ci-logs; then
    echo "publish-logs: pushed $name to ci-logs"; exit 0
  fi
  sleep $((attempt * 3))
done
echo "publish-logs: could not push"; exit 0
