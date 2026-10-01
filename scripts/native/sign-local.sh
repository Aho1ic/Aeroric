#!/usr/bin/env bash
# sign-local.sh — account-free ad-hoc signing of a built .app bundle (P4-01).
#
# Order matters: sign helpers first, then the outer bundle, so each signature
# is computed over already-signed contents (inside-out). Nested frameworks or
# XPC services would go between these two steps; today the bundle has none.
#
# Usage: sign-local.sh <path-to-.app>
set -euo pipefail
shopt -s nullglob

if [[ $# -ne 1 || ! -d "$1" ]]; then
  echo "usage: $0 <path-to-.app>" >&2
  exit 2
fi

APP="$1"
CONTENTS="$APP/Contents"

for helper in "$CONTENTS/MacOS/"*; do
  # Skip the main executable; it is re-signed by the bundle signature below.
  if [[ "$(basename "$helper")" == "$(basename "${APP%.app}")" ]]; then
    continue
  fi
  echo "sign-local: helper $(basename "$helper")"
  codesign --force --sign - "$helper"
done

echo "sign-local: bundle $(basename "$APP")"
codesign --force --sign - "$APP"
