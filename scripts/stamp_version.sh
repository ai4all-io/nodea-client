#!/usr/bin/env bash
# Write VERSION into every file the app's version is read from, under ROOT
# (default: the repository root).
#
# The repository keeps a placeholder version and the release workflow stamps
# the real one into the build's checkout only, so nothing is committed back.
# tauri.conf.json sets the bundle and installer version, Cargo.toml the
# app_version the app reports to the backend, Cargo.lock keeps the lock in
# step with Cargo.toml, and package.json keeps the frontend consistent.
#
# Shell and perl rather than Python or Node because the Linux release job runs
# in a bare ubuntu:22.04 container (perl-base is always present) and the
# Windows job runs Git Bash, which ships perl.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "usage: $(basename "$0") VERSION [ROOT]" >&2
  exit 2
fi

version=$1
root=${2:-$(cd "$(dirname "$0")/.." && pwd)}

if ! [[ $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "Expected a plain semver version such as 26.10.1, got '$version'" >&2
  exit 2
fi

crate=$(perl -ne 'if (/^name = "([^"]+)"/) { print $1; exit }' "$root/src-tauri/Cargo.toml")

# Replaces the first match of PATTERN's capture group 1 + quoted value, and
# fails unless exactly that one replacement happened.
stamp() {
  local file=$1 pattern=$2
  if ! VERSION="$version" PATTERN="$pattern" perl -0pi -e '
      $n = s/$ENV{PATTERN}/$1"$ENV{VERSION}"/;
      END { exit($n ? 0 : 3) }
    ' "$root/$file"; then
    echo "No version found to replace in $file" >&2
    exit 1
  fi
  echo "$file -> $version"
}

stamp package.json '("version":\s*)"[^"]*"'
stamp src-tauri/tauri.conf.json '("version":\s*)"[^"]*"'
stamp src-tauri/Cargo.toml '(?m)(^version = )"[^"]*"'
stamp src-tauri/Cargo.lock "(name = \"$crate\"\\r?\\nversion = )\"[^\"]*\""
