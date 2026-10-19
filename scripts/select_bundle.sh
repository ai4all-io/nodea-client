#!/usr/bin/env bash
# Print the one bundle matching PATTERN under the first existing ROOT.
#
# Written as shell rather than Python because the Linux release job runs in a
# bare ubuntu:22.04 container that installs no interpreter.
#
# Exists because `find ... | head -n1` and `ls GLOB | head -n1` silently pick
# an arbitrary file when the bundle directory holds more than one candidate,
# and the release target dirs are persistent, so they accumulate. Renaming the
# app from AI4All to Nodea once left AI4All.app beside Nodea.app; `find` returned the stale one
# and `ls` sorts "AI4All_..." before "Nodea_...", so the old build would have
# been verified and published under the new name. Ambiguity is a failure here,
# never a coin toss.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $(basename "$0") PATTERN ROOT [ROOT...]" >&2
  exit 2
fi

pattern=$1
shift

root=""
for candidate in "$@"; do
  if [ -d "$candidate" ]; then
    root=$candidate
    break
  fi
done

if [ -z "$root" ]; then
  echo "No bundle directory found; looked for: $*" >&2
  exit 1
fi

matches=()
for match in "$root"/$pattern; do
  [ -e "$match" ] && matches+=("$match")
done

if [ "${#matches[@]}" -eq 0 ]; then
  echo "No bundle matching '$pattern' under $root" >&2
  exit 1
fi

if [ "${#matches[@]}" -gt 1 ]; then
  {
    echo "Ambiguous: ${#matches[@]} files match '$pattern' under $root"
    printf '  %s\n' "${matches[@]}"
    echo "The bundle directory should hold only this run's output. A leftover"
    echo "from a previous name or version means the wrong artifact could be"
    echo "signed, notarised or published."
  } >&2
  exit 1
fi

printf '%s\n' "${matches[0]}"
