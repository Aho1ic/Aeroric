#!/usr/bin/env bash
set -euo pipefail

mobile_root="$(cd "$(dirname "$0")/.." && pwd)"
workspace="$mobile_root/ios/Aeroric.xcworkspace"
artifact_dir="$mobile_root/build/ios"
stamp="$(date +%Y%m%d-%H%M%S)"
output_ipa="$artifact_dir/Aeroric-unsigned-$stamp.ipa"
derived_dir="$(mktemp -d "${TMPDIR:-/tmp}/aeroric-derived.XXXXXX")"
package_dir="$(mktemp -d "${TMPDIR:-/tmp}/aeroric-package.XXXXXX")"

cleanup() {
  # Both paths are private directories created above with mktemp.
  rm -rf -- "$derived_dir" "$package_dir"
}
trap cleanup EXIT

if [[ ! -d "$workspace" ]]; then
  echo "缺少 $workspace，请先运行 pnpm expo prebuild --clean --platform ios" >&2
  exit 1
fi

mkdir -p "$artifact_dir" "$package_dir/Payload"

xcodebuild \
  -quiet \
  -workspace "$workspace" \
  -scheme Aeroric \
  -configuration Release \
  -destination "generic/platform=iOS" \
  -sdk iphoneos \
  -derivedDataPath "$derived_dir" \
  CODE_SIGNING_ALLOWED=NO \
  CODE_SIGNING_REQUIRED=NO \
  build

app_path="$derived_dir/Build/Products/Release-iphoneos/Aeroric.app"
if [[ ! -d "$app_path" ]]; then
  echo "未找到 Release .app: $app_path" >&2
  exit 1
fi

bundle_id="$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$app_path/Info.plist")"
expected_bundle_id="$(node -p "require('$mobile_root/app.json').expo.ios.bundleIdentifier")"
if [[ "$bundle_id" != "$expected_bundle_id" ]]; then
  echo "Bundle ID 不符合 app.json: $bundle_id != $expected_bundle_id" >&2
  exit 1
fi
if [[ ! -f "$app_path/main.jsbundle" ]]; then
  echo "Release App 中缺少 main.jsbundle" >&2
  exit 1
fi
if [[ -e "$app_path/embedded.mobileprovision" ]]; then
  echo "unsigned App 不应包含 embedded.mobileprovision" >&2
  exit 1
fi

ditto "$app_path" "$package_dir/Payload/Aeroric.app"
ditto -c -k --keepParent "$package_dir/Payload" "$output_ipa"

if zipinfo -1 "$output_ipa" | grep -q '^__MACOSX/'; then
  echo "IPA 中不应包含 __MACOSX 元数据目录" >&2
  exit 1
fi

echo "$output_ipa"
