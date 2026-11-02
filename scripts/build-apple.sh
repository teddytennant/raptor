#!/usr/bin/env bash
# Build the Rust core for Apple platforms and package ShrewFFI.xcframework
# plus Swift bindings for apps/ios/ShrewKit.
#   scripts/build-apple.sh          # macOS slice always; iOS slices when an iOS SDK exists
#   TARGETS="macos ios sim" scripts/build-apple.sh
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE=${PROFILE:-release}
KIT=apps/ios/ShrewKit
OUT=$KIT/ShrewFFI.xcframework
if [ -d "$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin" ]; then
  export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
fi

have_ios_sdk() { xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1; }
if [ -z "${TARGETS:-}" ]; then
  TARGETS="macos"
  if have_ios_sdk; then TARGETS="macos ios sim"; else echo "note: no iOS SDK (install Xcode); building the macOS slice only" >&2; fi
fi

flag=(); [ "$PROFILE" = release ] && flag=(--release)
dir=$PROFILE; [ "$PROFILE" = dev ] && dir=debug

declare -a slices=()
for t in $TARGETS; do
  case $t in
    macos) triple=aarch64-apple-darwin; id=macos-arm64; plat=macos; variant="" ;;
    ios)   triple=aarch64-apple-ios; id=ios-arm64; plat=ios; variant="" ;;
    sim)   triple=aarch64-apple-ios-sim; id=ios-arm64-simulator; plat=ios; variant=simulator ;;
    *) echo "unknown target $t" >&2; exit 1 ;;
  esac
  [ "$t" = ios ] || [ "$t" = sim ] && export IPHONEOS_DEPLOYMENT_TARGET=17.0
  [ "$t" = macos ] && export MACOSX_DEPLOYMENT_TARGET=14.0
  cargo build -p shrew-ffi --lib --target "$triple" "${flag[@]}"
  slices+=("$triple|$id|$plat|$variant")
done

# Swift bindings (from the host library's metadata).
cargo build -p shrew-ffi --features bindgen --bin uniffi-bindgen
gen=$(mktemp -d)
./target/debug/uniffi-bindgen generate --library "target/aarch64-apple-darwin/$dir/libshrew_ffi.dylib" --language swift --out-dir "$gen"
mkdir -p "$KIT/Sources/ShrewKit/Generated"
cp "$gen/shrew_ffi.swift" "$KIT/Sources/ShrewKit/Generated/shrew_ffi.swift"

rm -rf "$OUT"
mkdir -p "$OUT"
libs=""
for s in "${slices[@]}"; do
  IFS='|' read -r triple id plat variant <<<"$s"
  d="$OUT/$id"
  mkdir -p "$d/Headers"
  cp "target/$triple/$dir/libshrew_ffi.a" "$d/libshrew_ffi.a"
  cp "$gen/shrew_ffiFFI.h" "$d/Headers/"
  printf 'module shrew_ffiFFI {\n  header "shrew_ffiFFI.h"\n  export *\n}\n' > "$d/Headers/module.modulemap"
  v=""; [ -n "$variant" ] && v="<key>SupportedPlatformVariant</key><string>$variant</string>"
  libs+="<dict><key>LibraryIdentifier</key><string>$id</string><key>LibraryPath</key><string>libshrew_ffi.a</string><key>HeadersPath</key><string>Headers</string><key>SupportedArchitectures</key><array><string>arm64</string></array><key>SupportedPlatform</key><string>$plat</string>$v</dict>"
done
cat > "$OUT/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>AvailableLibraries</key><array>$libs</array>
<key>CFBundlePackageType</key><string>XFWK</string>
<key>XCFrameworkFormatVersion</key><string>1.0</string>
</dict></plist>
PLIST
rm -rf "$gen"
echo "Apple core ready: $OUT ($TARGETS)"
