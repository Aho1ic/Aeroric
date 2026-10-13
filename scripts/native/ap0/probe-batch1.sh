#!/usr/bin/env bash
# probe-batch1.sh — AP0 batch-1 real App Sandbox probe driver.
#
# MIGRATION_PLAN.md §D AP0; .trellis/tasks/10-12-ap0-store-local-feasibility.
# Builds the AeroricSandboxProbe carrier from macos/project.yml, embeds the
# real aeroric-cli helper, signs inside-out ad-hoc WITH the App Sandbox
# entitlement, runs the battery, and records every command + result under
# .artifacts/native/ap0/batch1/. The xcodebuild/Tauri signing baselines
# (Debug/ReleaseLocal, check-signing-baseline.mjs) are never touched.
#
# Isolation contract (prd Q3/Q5): scratch data only (container-internal
# ap0-data, /tmp/ap0); no writes to real ~/.aeroric, ~/.claude, ~/.codex,
# real npm/brew prefixes; no credentials.
set -euo pipefail
shopt -s nullglob

REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="$REPO/.artifacts/native/ap0/batch1"
PROJ="$REPO/macos/AeroricNative.xcodeproj"
DERIVED="$REPO/.artifacts/native/DerivedData"
BUILT="$OUT/BuiltProducts"
ENTITLEMENTS="$REPO/macos/SandboxProbe/AeroricSandboxProbe.entitlements"
APP="$BUILT/AeroricSandboxProbe.app"
CLI_BINARY="$REPO/core/target/debug/aeroric-cli"

mkdir -p "$OUT"
ENV_FILE="$OUT/env.txt"
TRACE="$OUT/trace.log"
: >"$TRACE"

log() { printf '%s\n' "$*" | tee -a "$TRACE"; }
die() { log "probe-batch1: FATAL: $*"; exit 1; }

record_env() {
  {
    echo "# AP0 batch-1 probe environment ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
    sw_vers
    uname -a
    echo "arch: $(uname -m)"
    xcodebuild -version
    clang --version | head -1
    xcodegen --version 2>&1 | head -1
    echo "codesign: $(codesign --version 2>&1)"
    security find-identity -v -p codesigning 2>/dev/null | tail -3
  } >"$ENV_FILE"
}

run_step() {
  # run_step <label> <cmd...> — echo into trace, propagate exit code
  local label="$1"; shift
  log "[step] $label: $*"
  if "$@" >>"$TRACE" 2>&1; then
    log "[ok]   $label"
  else
    local rc=$?
    log "[fail] $label (exit=$rc)"
    return "$rc"
  fi
}

log "== AP0 batch-1 probe run: $(date -u +%Y-%m-%dT%H:%M:%SZ) =="
record_env

# 1. regenerate the Xcode project (adds the probe target from project.yml)
run_step generate node "$REPO/scripts/native/generate.mjs" \
  || die "xcodegen generate failed — see $TRACE"

# 2. host helper binary (real aeroric-cli)
log "[step] cargo build aeroric-cli"
if ! (cd "$REPO" && cargo build --manifest-path "$REPO/core/Cargo.toml" --locked -p aeroric-cli) \
  >>"$TRACE" 2>&1; then
  die "cargo build failed — see $TRACE"
fi
[[ -x "$CLI_BINARY" ]] || die "helper binary missing: $CLI_BINARY"

# 3. xcodebuild the probe app (no signing — the script owns the signature)
rm -rf "$APP" "$BUILT"
log "[step] xcodebuild AeroricSandboxProbe"
if ! xcodebuild -project "$PROJ" -target AeroricSandboxProbe -configuration Debug \
  SYMROOT="$DERIVED" OBJROOT="$DERIVED/Intermediates" \
  CONFIGURATION_BUILD_DIR="$BUILT" \
  CODE_SIGNING_ALLOWED=NO build >>"$TRACE" 2>&1; then
  die "xcodebuild failed — see $TRACE"
fi
[[ -d "$APP" ]] || die "probe bundle missing after build: $APP"

# 4. embed the helper and fake agent
mkdir -p "$APP/Contents/MacOS"
cp "$CLI_BINARY" "$APP/Contents/MacOS/aeroric-cli"
clang -O2 "$REPO/macos/SandboxProbe/fake-agent.c" -o "$APP/Contents/MacOS/fake-agent"
# 5. outside-container exec target (user-installed CLI shape): /tmp/ap0/fake-agent
rm -rf /tmp/ap0
mkdir -p /tmp/ap0
cp /usr/bin/true /tmp/ap0/fake-agent
chmod +x /tmp/ap0/fake-agent

# 5b. container scratch hygiene — prior-run state would corrupt observations
CONTAINER_DATA="${HOME}/Library/Containers/com.aeroric.native.sandboxprobe/Data"
rm -rf "$CONTAINER_DATA/ap0-data" "$CONTAINER_DATA/ap3-ver" "$CONTAINER_DATA/ap3-bg" "$CONTAINER_DATA/.ap0-in.txt" \
       "${HOME}/.ap0-outside-write.txt"

# 6. inside-out ad-hoc signing: helpers first (plain ad-hoc), then bundle + entitlements
log "[step] codesign helpers (plain ad-hoc)"
codesign --force --sign - "$APP/Contents/MacOS/aeroric-cli" >>"$TRACE" 2>&1
codesign --force --sign - "$APP/Contents/MacOS/fake-agent" >>"$TRACE" 2>&1
log "[step] codesign bundle (ad-hoc + App Sandbox entitlements)"
codesign --force --sign - --entitlements "$ENTITLEMENTS" "$APP" >>"$TRACE" 2>&1

{
  echo "# embedded entitlements (probe carrier)"
  codesign -d --entitlements - --xml "$APP" 2>&1 || true
  echo "# verification"
  codesign --verify --strict --verbose=2 "$APP" 2>&1 || true
} >"$OUT/codesign-record.txt"

# 7. run the battery (stdout/stderr captured — the sandboxed process itself
#    cannot write outside its container)
log "[step] run AeroricSandboxProbe battery"
set +e
"$APP/Contents/MacOS/AeroricSandboxProbe" \
  >"$OUT/probe-stdout.log" 2>"$OUT/probe-stderr.log"
PROBE_RC=$?
set -e
log "[done] battery exit=$PROBE_RC"

# 8. post-run facts a sandboxed process cannot record itself
{
  echo "# probe exit code: $PROBE_RC"
  echo "# container dir listing (read from outside the sandbox):"
  ls -la "${HOME}/Library/Containers/com.aeroric.native.sandboxprobe/Data" 2>&1 || true
  echo "# scratch data dir:"
  ls -la "${HOME}/Library/Containers/com.aeroric.native.sandboxprobe/Data/ap0-data" 2>&1 || true
} >"$OUT/post-run-facts.txt"

log "== batch-1 complete: results in $OUT/probe-stdout.log =="
