#!/usr/bin/env bash
# probe-batch2.sh — AP0 batch-2 real App Sandbox probe driver.
#
# MIGRATION_PLAN.md §D AP0; .trellis/tasks/10-12-ap0-store-local-feasibility.
# 1. Tests temporary-exception contrast against baseline.
# 2. Runs the matrix across four agent CLIs, system runtimes, package managers,
#    LSP, home configs, Keychain, and dynamic libraries.
# Records every command + result under .artifacts/native/ap0/batch2/.
set -euo pipefail
shopt -s nullglob

REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
OUT="$REPO/.artifacts/native/ap0/batch2"
PROJ="$REPO/macos/AeroricNative.xcodeproj"
DERIVED="$REPO/.artifacts/native/DerivedData"
BUILT="$OUT/BuiltProducts"
BASE_ENTITLEMENTS="$REPO/macos/SandboxProbe/AeroricSandboxProbe.entitlements"
TEMPEXC_ENTITLEMENTS="$REPO/macos/SandboxProbe/AeroricSandboxProbe.tempexc.entitlements"
APP="$BUILT/AeroricSandboxProbe.app"
CLI_BINARY="$REPO/core/target/debug/aeroric-cli"

mkdir -p "$OUT"
ENV_FILE="$OUT/env.txt"
TRACE="$OUT/trace.log"
: >"$TRACE"

log() { printf '%s\n' "$*" | tee -a "$TRACE"; }
die() { log "probe-batch2: FATAL: $*"; exit 1; }

record_env() {
  {
    echo "# AP0 batch-2 probe environment ($(date -u +%Y-%m-%dT%H:%M:%SZ))"
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

log "== AP0 batch-2 probe run: $(date -u +%Y-%m-%dT%H:%M:%SZ) =="
record_env

# 1. regenerate Xcode project
run_step generate node "$REPO/scripts/native/generate.mjs" \
  || die "xcodegen generate failed — see $TRACE"

# 2. build aeroric-cli if missing or outdated
log "[step] cargo build aeroric-cli"
if ! (cd "$REPO" && cargo build --manifest-path "$REPO/core/Cargo.toml" --locked -p aeroric-cli) \
     >>"$TRACE" 2>&1; then
  die "cargo build aeroric-cli failed — see $TRACE"
fi
[[ -x "$CLI_BINARY" ]] || die "helper binary missing: $CLI_BINARY"

# 3. xcodebuild the probe app
rm -rf "$APP" "$BUILT"
log "[step] xcodebuild AeroricSandboxProbe"
if ! xcodebuild -project "$PROJ" -target AeroricSandboxProbe -configuration Debug \
  SYMROOT="$DERIVED" OBJROOT="$DERIVED/Intermediates" \
  CONFIGURATION_BUILD_DIR="$BUILT" \
  CODE_SIGNING_ALLOWED=NO build >>"$TRACE" 2>&1; then
  die "xcodebuild failed — see $TRACE"
fi
[[ -d "$APP" ]] || die "probe bundle missing after build: $APP"

# 4. embed helpers
mkdir -p "$APP/Contents/MacOS"
cp "$CLI_BINARY" "$APP/Contents/MacOS/aeroric-cli"
clang -O2 "$REPO/macos/SandboxProbe/fake-agent.c" -o "$APP/Contents/MacOS/fake-agent"

# 5. create outside-container exec target
rm -rf /tmp/ap0
mkdir -p /tmp/ap0
cp /usr/bin/true /tmp/ap0/fake-agent
chmod +x /tmp/ap0/fake-agent

CONTAINER_DATA="${HOME}/Library/Containers/com.aeroric.native.sandboxprobe/Data"
rm -rf "$CONTAINER_DATA/ap0-data" "$CONTAINER_DATA/.ap0-in.txt" \
       "${HOME}/.ap0-outside-write.txt"

# 6. Part A: Contrast test with temporary-exception
log "[contrast] sign with tempexc entitlements"
codesign --force --sign - "$APP/Contents/MacOS/aeroric-cli" >>"$TRACE" 2>&1
codesign --force --sign - "$APP/Contents/MacOS/fake-agent" >>"$TRACE" 2>&1
codesign --force --sign - --entitlements "$TEMPEXC_ENTITLEMENTS" "$APP" >>"$TRACE" 2>&1

log "[contrast] run probe under temporary-exception"
set +e
"$APP/Contents/MacOS/AeroricSandboxProbe" \
  >"$OUT/contrast-tempexc-stdout.log" 2>"$OUT/contrast-tempexc-stderr.log"
CONTRAST_RC=$?
set -e
log "[contrast] tempexc probe exit=$CONTRAST_RC"

# Clean container data before Part B
rm -rf "$CONTAINER_DATA/ap0-data" "$CONTAINER_DATA/.ap0-in.txt" \
       "${HOME}/.ap0-outside-write.txt"

# 7. Part B: Baseline matrix run
log "[matrix] sign with baseline sandbox entitlements"
codesign --force --sign - "$APP/Contents/MacOS/aeroric-cli" >>"$TRACE" 2>&1
codesign --force --sign - "$APP/Contents/MacOS/fake-agent" >>"$TRACE" 2>&1
codesign --force --sign - --entitlements "$BASE_ENTITLEMENTS" "$APP" >>"$TRACE" 2>&1

{
  echo "# embedded entitlements (baseline probe carrier)"
  codesign -d --entitlements - --xml "$APP" 2>&1 || true
  echo "# verification"
  codesign --verify --strict --verbose=2 "$APP" 2>&1 || true
} >"$OUT/codesign-record.txt"

# Collect installed targets dynamically
ARGS=(
  "exec:claude:${HOME}/.local/bin/claude"
  "exec:codex:${HOME}/.local/bin/codex"
  "exec:omp:${HOME}/.local/bin/omp"
  "exec:dsh:${HOME}/.local/bin/dsh"
  "exec:node:${HOME}/.local/bin/node"
  "exec:npm:${HOME}/.local/bin/npm"
  "exec:brew:/opt/homebrew/bin/brew"
  "exec:python_conda:/opt/miniconda3/bin/python3"
  "exec:python_sys:/usr/bin/python3"
  "exec:git_sys:/usr/bin/git"
  "exec:rust_analyzer:${HOME}/.cargo/bin/rust-analyzer"
  "read:home_claude:${HOME}/.claude"
  "read:home_codex:${HOME}/.codex"
  "read:home_omp:${HOME}/.omp"
  "read:home_aeroric:${HOME}/.aeroric"
  "read:brew_prefix:/opt/homebrew"
  "read:cargo_home:${HOME}/.cargo"
  "dlopen:sys_sqlite:/usr/lib/libsqlite3.dylib"
  "dlopen:sys_zlib:/usr/lib/libz.dylib"
)

log "[matrix] run AeroricSandboxProbe with real-path targets"
set +e
"$APP/Contents/MacOS/AeroricSandboxProbe" "${ARGS[@]}" \
  >"$OUT/matrix-stdout.log" 2>"$OUT/matrix-stderr.log"
MATRIX_RC=$?
set -e
log "[matrix] matrix run exit=$MATRIX_RC"

{
  echo "# probe matrix exit code: $MATRIX_RC"
  echo "# container scratch data dir:"
  ls -la "$CONTAINER_DATA/ap0-data" 2>/dev/null || true
} >"$OUT/post-run-facts.txt"

log "== batch-2 complete: results in $OUT/ =="
