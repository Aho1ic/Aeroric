#!/usr/bin/env bash
# review-live-session.sh — build an ad-hoc Aeroric.app and an isolated data
# root whose built-in Codex is the scripted fake app-server, for looking at
# the native session page (N4 D8). Never touches ~/.aeroric, ~/.codex or the
# Keychain; prints the command that opens the app on the sandbox.
#
# Usage: scripts/native/review-live-session.sh [sandbox-dir]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SANDBOX="${1:-$HOME/Library/Caches/aeroric-n4-review}"
DERIVED="$ROOT/.artifacts/native/DerivedData"
APP="$DERIVED/Build/Products/Debug/AeroricNative.app"
FAKE="$ROOT/scripts/native/fake-codex-app-server.py"

echo "== build aeroric-cli"
(cd "$ROOT/core" && cargo build --locked -p aeroric-cli)
echo "== build app"
xcodegen generate -s "$ROOT/macos/project.yml" -p "$ROOT/macos" >/dev/null
xcodebuild -project "$ROOT/macos/AeroricNative.xcodeproj" -scheme AeroricNative -configuration Debug \
  -destination platform=macOS -derivedDataPath "$DERIVED" CODE_SIGNING_ALLOWED=NO CODE_SIGN_IDENTITY= build \
  | grep -E "error:|BUILD (SUCCEEDED|FAILED)"
cp "$ROOT/core/target/debug/aeroric-cli" "$APP/Contents/MacOS/"
bash "$ROOT/scripts/native/sign-local.sh" "$APP"
bash "$ROOT/scripts/native/verify-bundle.sh" "$APP"

echo "== sandbox $SANDBOX"
mkdir -p "$SANDBOX/data" "$SANDBOX/projects/demo-app/Sources/App"
chmod +x "$FAKE"
python3 - "$SANDBOX/data/settings.json" "$FAKE" <<'PY'
import json, os, sys
path, fake = sys.argv[1], sys.argv[2]
settings = json.load(open(path)) if os.path.exists(path) else {}
settings["codex_path"] = fake
json.dump(settings, open(path, "w"), ensure_ascii=False, indent=2)
PY
printf '# demo-app\n\n用于目检原生会话页的示例项目。\n' > "$SANDBOX/projects/demo-app/README.md"
printf 'print("hello")\n' > "$SANDBOX/projects/demo-app/Sources/App/main.swift"

cat <<MSG

Ready. Open the app on the sandbox with:

  open -n "$APP" --env AERORIC_DATA_DIR="$SANDBOX/data" --env AERORIC_TEST_OPEN_PROJECT="$SANDBOX/projects/demo-app"

Then start a new session in demo-app with the built-in "Codex" configuration.
MSG
