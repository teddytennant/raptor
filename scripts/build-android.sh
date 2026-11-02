#!/usr/bin/env bash
# Build the Rust core for Android and generate Kotlin bindings.
#   scripts/build-android.sh            # arm64-v8a + x86_64, release
#   ABIS="arm64-v8a" PROFILE=debug scripts/build-android.sh
set -euo pipefail
cd "$(dirname "$0")/.."

ABIS=${ABIS:-"arm64-v8a x86_64"}
PROFILE=${PROFILE:-release}
APP=apps/android/app/src/main
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Library/Android/sdk}
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  ANDROID_NDK_HOME=$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)
  export ANDROID_NDK_HOME
fi
# Cross-compiling needs rustup's toolchain (a distro/Homebrew rustc has no Android std).
if [ -d "$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin" ]; then
  export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
fi
export PATH="$HOME/.cargo/bin:$PATH"

flags=()
for abi in $ABIS; do flags+=(-t "$abi"); done
release=()
[ "$PROFILE" = release ] && release=(--release)

cargo ndk "${flags[@]}" --platform 28 -o "$APP/jniLibs" build -p shrew-ffi "${release[@]}"

# Bindings are generated from the host build's metadata (identical across targets).
cargo build -p shrew-ffi --features bindgen --bin uniffi-bindgen
cargo build -p shrew-ffi
lib=target/debug/libshrew_ffi.dylib
[ -f "$lib" ] || lib=target/debug/libshrew_ffi.so
./target/debug/uniffi-bindgen generate --library "$lib" --language kotlin --no-format --out-dir "$APP/java"
echo "Android core ready: $APP/jniLibs, $APP/java/uniffi"
