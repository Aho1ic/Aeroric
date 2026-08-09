#!/usr/bin/env bash

set -euo pipefail

if [[ "$#" -lt 2 || "$#" -gt 3 ]]; then
  echo "Usage: $0 <Aeroric.app> <output.dmg> [volume-name]" >&2
  exit 2
fi

app_path="$1"
output_path="$2"
volume_name="${3:-Aeroric}"

if [[ ! -d "$app_path" || ! -x "$app_path/Contents/MacOS/aeroric" ]]; then
  echo "Invalid Aeroric app bundle: $app_path" >&2
  exit 2
fi
if [[ "$output_path" != *.dmg ]]; then
  echo "DMG output must end in .dmg: $output_path" >&2
  exit 2
fi
if ! command -v hdiutil >/dev/null 2>&1; then
  echo "hdiutil is required to build a macOS DMG" >&2
  exit 2
fi

mkdir -p "$(dirname "$output_path")"
working_dir="$(mktemp -d "${TMPDIR:-/tmp}/aeroric-dmg.XXXXXX")"
mount_dir="$working_dir/volume"
sparse_image="$working_dir/Aeroric.sparseimage"
mounted=false
cleanup() {
  if [[ "$mounted" == true ]]; then
    hdiutil detach -force "$mount_dir" >/dev/null 2>&1 || true
  fi
  chmod -R u+w "$working_dir" 2>/dev/null || true
  rm -rf "$working_dir"
}
trap cleanup EXIT

# Build the writable image first, then create the Applications symlink inside
# it. hdiutil's -srcfolder mode follows external symlinks and would otherwise
# attempt to copy the host's complete /Applications directory.
app_kib="$(du -sk "$app_path" | awk '{print $1}')"
image_mib="$(( (app_kib + 1023) / 1024 + 48 ))"
mkdir -p "$mount_dir"
hdiutil create \
  -size "${image_mib}m" \
  -fs HFS+ \
  -volname "$volume_name" \
  -type SPARSE \
  -ov \
  "$sparse_image" >/dev/null
hdiutil attach \
  -nobrowse \
  -noautoopen \
  -mountpoint "$mount_dir" \
  "$sparse_image" >/dev/null
mounted=true

# ditto preserves the app bundle's resource forks, extended attributes, and
# code-signing layout. No Finder process or AppleScript is involved.
ditto "$app_path" "$mount_dir/$(basename "$app_path")"
ln -s /Applications "$mount_dir/Applications"
sync
hdiutil detach "$mount_dir" >/dev/null
mounted=false

hdiutil convert "$sparse_image" \
  -format UDZO \
  -imagekey zlib-level=9 \
  -ov \
  -o "$output_path" >/dev/null
hdiutil verify "$output_path"

echo "Created verified DMG: $output_path"
