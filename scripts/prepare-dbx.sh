#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
dbx_dir="${DBX_DIR:-$repo_root/../dbx}"
patch_file="$repo_root/patches/dbx-security.patch"

if [[ ! -d "$dbx_dir/.git" ]]; then
  echo "DBX checkout not found at $dbx_dir" >&2
  echo "Clone the pinned DBX dependency first, then rerun this script." >&2
  exit 1
fi

if git -C "$dbx_dir" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
  echo "DBX security patch is already applied."
  exit 0
fi

if ! git -C "$dbx_dir" apply --check "$patch_file"; then
  echo "DBX security patch does not apply cleanly to $dbx_dir." >&2
  echo "Reset DBX to the commit documented in README.md and retry." >&2
  exit 1
fi

git -C "$dbx_dir" apply "$patch_file"
echo "Applied DBX security patch to $dbx_dir."
