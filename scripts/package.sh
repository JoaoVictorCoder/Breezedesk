#!/usr/bin/env bash
# Builds the release files in dist/:
#   Breezedesk-Setup.exe           installer (also used by the automatic updater)
#   Breezedesk-Setup.exe.sha256    its checksum (checked by the updater)
#   Breezedesk-X.Y.Z-portable.zip  portable copy (keeps widgets next to the .exe)
#   Plume-Setup.exe (+ .sha256)    same installer, for copies from before the rename
#
# Usage: scripts/package.sh VERSION [path/to/breezedesk.exe] [path/to/breezedesk-editor.exe]
# (the editor defaults to breezedesk-editor.exe next to breezedesk.exe, if it exists)
# Needs: makensis (NSIS), zip, sha256sum.
set -euo pipefail

version="$1"
exe="${2:-target/x86_64-pc-windows-gnu/release/breezedesk.exe}"
editor="${3:-$(dirname "$exe")/breezedesk-editor.exe}"
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

makensis -V2 -DVERSION="$version" -DEXE="$exe" "${editor_flag[@]}" -DOUTFILE="$dist/Breezedesk-Setup.exe" "$root/installer/breezedesk.nsi"
(cd "$dist" && sha256sum Breezedesk-Setup.exe > Breezedesk-Setup.exe.sha256)
# Copies under the old name (Plume): versions released before the rename look
# for these to update themselves. Can be removed once nobody runs Plume <= 0.6.
cp "$dist/Breezedesk-Setup.exe" "$dist/Plume-Setup.exe"
(cd "$dist" && sha256sum Plume-Setup.exe > Plume-Setup.exe.sha256)

tmp="$(mktemp -d)"
mkdir -p "$tmp/Breezedesk"
cp "$exe" "$tmp/Breezedesk/breezedesk.exe"
if [ -n "$editor" ]; then cp "$editor" "$tmp/Breezedesk/breezedesk-editor.exe"; fi
cp "$root/README.md" "$root/CHANGELOG.md" "$root/LICENSE" "$tmp/Breezedesk/"
cp -r "$root/widgets" "$tmp/Breezedesk/widgets"
rm -f "$tmp"/Breezedesk/widgets/*.log "$tmp"/Breezedesk/widgets/*.state
(cd "$tmp" && zip -qr "$dist/Breezedesk-$version-portable.zip" Breezedesk)
rm -rf "$tmp"

ls -l "$dist"
