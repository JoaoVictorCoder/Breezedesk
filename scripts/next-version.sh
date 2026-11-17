#!/usr/bin/env bash
# Prints "version=X.Y.Z" for the next release:
# - the version in Cargo.toml, if no release has it yet (bump it by hand for
#   new features or breaking changes);
# - otherwise the latest release with the last number + 1.
set -euo pipefail

cargo_version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
latest=$(git tag -l 'v[0-9]*.[0-9]*.[0-9]*' | sed 's/^v//' | sort -V | tail -n1)

if [ -z "$latest" ]; then
  next="$cargo_version"
elif [ "$(printf '%s\n%s\n' "$latest" "$cargo_version" | sort -V | tail -n1)" = "$cargo_version" ] \
     && [ "$cargo_version" != "$latest" ]; then
  next="$cargo_version"
else
  IFS=. read -r major minor patch <<<"$latest"
  next="$major.$minor.$((patch + 1))"
fi
echo "version=$next"
