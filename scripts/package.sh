#!/usr/bin/env bash
# Builds the release files in dist/:
#   Plume-Setup.exe           installer (also used by the automatic updater)
#   Plume-Setup.exe.sha256    its checksum (checked by the updater)
#   Plume-X.Y.Z-portable.zip  portable copy (keeps widgets next to the .exe)
#
# Usage: scripts/package.sh VERSION [path/to/plume.exe] [path/to/plume-editor.exe]
# (the editor defaults to plume-editor.exe next to plume.exe, if it exists)
# Needs: makensis (NSIS), zip, sha256sum.
set -euo pipefail

version="$1"
exe="${2:-target/x86_64-pc-windows-gnu/release/plume.exe}"
editor="${3:-$(dirname "$exe")/plume-editor.exe}"
root="$(cd "$(dirname "$0")/.." && pwd)"
dist="$root/dist"
exe="$(cd "$(dirname "$exe")" && pwd)/$(basename "$exe")"
editor_flag=()
if [ -f "$editor" ]; then
  editor="$(cd "$(dirname "$editor")" && pwd)/$(basename "$editor")"
  editor_flag=(-DEDITOR="$editor")
else
  echo "note: no editor at $editor, packaging without it" >&2
  editor=""
fi

rm -rf "$dist"
mkdir -p "$dist"

makensis -V2 -DVERSION="$version" -DEXE="$exe" "${editor_flag[@]}" -DOUTFILE="$dist/Plume-Setup.exe" "$root/installer/plume.nsi"
(cd "$dist" && sha256sum Plume-Setup.exe > Plume-Setup.exe.sha256)

tmp="$(mktemp -d)"
mkdir -p "$tmp/Plume"
cp "$exe" "$tmp/Plume/plume.exe"
if [ -n "$editor" ]; then cp "$editor" "$tmp/Plume/plume-editor.exe"; fi
cp "$root/README.md" "$root/CHANGELOG.md" "$root/LICENSE" "$tmp/Plume/"
cp -r "$root/widgets" "$tmp/Plume/widgets"
rm -f "$tmp/Plume/widgets/plume.log" "$tmp/Plume/widgets/positions.state"
(cd "$tmp" && zip -qr "$dist/Plume-$version-portable.zip" Plume)
rm -rf "$tmp"

ls -l "$dist"
