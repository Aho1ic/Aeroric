#!/usr/bin/env bash
# verify-bundle.sh — post-signature integrity gate (P4-01 Action 7).
#
# Fails non-zero unless the outer bundle verifies strictly and every helper
# inside Contents/MacOS has a valid ad-hoc signature.
#
# Usage: verify-bundle.sh <path-to-.app>
set -euo pipefail
shopt -s nullglob

if [[ $# -ne 1 || ! -d "$1" ]]; then
  echo "usage: $0 <path-to-.app>" >&2
  exit 2
fi

APP="$1"
CONTENTS="$APP/Contents"

codesign --verify --strict --verbose=2 "$APP"
codesign --display "$APP" > /dev/null

for helper in "$CONTENTS/MacOS/"*; do
  [[ "$(basename "$helper")" == "$(basename "${APP%.app}")" ]] && continue
  codesign --verify --strict "$helper"
done

echo "verify-bundle: intact — $(basename "$APP") and all helpers signed"
