#!/usr/bin/env bash
# Print the next CalVer release version, YY.M.X, for the month YY.M (default:
# the current month in UTC).
#
# X is one more than the highest vYY.M.X tag in the current repository, or 1
# when that month has none. The tags are the only record of which numbers are
# taken, so the caller must have fetched them all. Nothing is reserved until
# the caller pushes the tag, so runs must not overlap.
#
# The month is not zero-padded because the version must be valid semver (no
# leading zeros) for Cargo, npm and Tauri; YY rather than YYYY because the
# Windows MSI caps the major version at 255.
set -euo pipefail

if [ "$#" -gt 1 ]; then
  echo "usage: $(basename "$0") [YY.M]" >&2
  exit 2
fi

month=${1:-$(date -u +%y).$((10#$(date -u +%m)))}

if ! [[ $month =~ ^[0-9]{2}\.([1-9]|1[0-2])$ ]]; then
  echo "Expected the month as YY.M (e.g. 26.10, not 26.01), got '$month'" >&2
  exit 2
fi

highest=0
while IFS= read -r tag; do
  if [[ $tag =~ ^v${month//./\\.}\.([1-9][0-9]*)$ ]]; then
    n=${BASH_REMATCH[1]}
    if [ "$n" -gt "$highest" ]; then
      highest=$n
    fi
  fi
done < <(git tag --list "v$month.*")

printf '%s.%d\n' "$month" "$((highest + 1))"
