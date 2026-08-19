#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
dbx_dir="${DBX_DIR:-$repo_root/../dbx}"
patch_file="$repo_root/patches/dbx-security.patch"
ref_file="$script_dir/dbx-ref.txt"

if [[ ! -d "$dbx_dir/.git" ]]; then
  echo "DBX checkout not found at $dbx_dir" >&2
  echo "Clone the pinned DBX dependency first, then rerun this script." >&2
  exit 1
fi

# `Cargo.toml` 用的是 `path = "../dbx/..."`,这条依赖的来源不受 Cargo.lock 约束,
# 所以本地 checkout 漂到别的提交时构建的其实已经不是 CI 验证过的那份代码。
# 这里只告警不失败:本地有意跟进 DBX 上游是正常的开发动作。
if [[ -f "$ref_file" ]]; then
  pinned_ref="$(tr -d '[:space:]' <"$ref_file")"
  current_ref="$(git -C "$dbx_dir" rev-parse HEAD 2>/dev/null || echo "")"
  if [[ -n "$pinned_ref" && -n "$current_ref" && "$current_ref" != "$pinned_ref" ]]; then
    echo "warning: DBX checkout is at $current_ref but CI pins $pinned_ref." >&2
    echo "warning: run 'git -C $dbx_dir checkout $pinned_ref' to match CI builds." >&2
  fi
fi

if git -C "$dbx_dir" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
  echo "DBX security patch is already applied."
  exit 0
fi

if ! git -C "$dbx_dir" apply --check "$patch_file"; then
  echo "DBX security patch does not apply cleanly to $dbx_dir." >&2
  echo "Reset DBX to the commit in scripts/dbx-ref.txt and retry." >&2
  exit 1
fi

git -C "$dbx_dir" apply "$patch_file"
echo "Applied DBX security patch to $dbx_dir."
