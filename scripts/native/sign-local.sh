#!/usr/bin/env bash
# sign-local.sh — account-free signing of a built .app bundle (P4-01).
#
# Order matters: sign helpers first, then the outer bundle, so each signature
# is computed over already-signed contents (inside-out). Nested frameworks or
# XPC services would go between these two steps; today the bundle has none.
#
# Identity: ad-hoc ("-") by default. An ad-hoc signature changes with every
# build, so macOS asks again for the keychain each time `aeroric-cli` reads
# the credential vault after a rebuild. Set AERORIC_SIGN_IDENTITY to the name
# of a code-signing certificate in your login keychain (a self-signed one from
# Keychain Access → Certificate Assistant is enough) and "Always Allow" sticks
# across rebuilds. A name that is not in the keychain falls back to ad-hoc.
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

IDENTITY="-"
if [[ -n "${AERORIC_SIGN_IDENTITY:-}" ]]; then
  if security find-identity -p codesigning | grep -Fq "\"$AERORIC_SIGN_IDENTITY\""; then
    IDENTITY="$AERORIC_SIGN_IDENTITY"
  else
    echo "sign-local: no code-signing identity named \"$AERORIC_SIGN_IDENTITY\"; signing ad-hoc" >&2
  fi
fi
echo "sign-local: identity ${IDENTITY/#-/ad-hoc}"

for helper in "$CONTENTS/MacOS/"*; do
  # Skip the main executable; it is re-signed by the bundle signature below.
  if [[ "$(basename "$helper")" == "$(basename "${APP%.app}")" ]]; then
    continue
  fi
  echo "sign-local: helper $(basename "$helper")"
  codesign --force --sign "$IDENTITY" "$helper"
done

echo "sign-local: bundle $(basename "$APP")"
codesign --force --sign "$IDENTITY" "$APP"
