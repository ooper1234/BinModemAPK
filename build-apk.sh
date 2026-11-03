#!/usr/bin/env bash
# Build BinModem as an Android APK (arm64-v8a).
#
# Needs: Rust with the aarch64-linux-android target, an Android SDK with
# build-tools and a platform, and the NDK. Paths come from the environment:
#   ANDROID_HOME   (or ANDROID_SDK_ROOT)  the SDK
#   ANDROID_NDK_ROOT (or ANDROID_NDK_HOME) the NDK; defaults to the newest in $ANDROID_HOME/ndk
#   KEYSTORE, KEYSTORE_PASS, KEY_ALIAS   optional release key; a debug key is made otherwise
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:?set ANDROID_HOME}}"
ndk="${ANDROID_NDK_ROOT:-${ANDROID_NDK_HOME:-$(ls -d "$sdk"/ndk/* | sort -V | tail -1)}}"
bt="$(ls -d "$sdk"/build-tools/* | sort -V | tail -1)"
platform="$(ls -d "$sdk"/platforms/android-* | sort -V | tail -1)"
api=26
tc="$ndk/toolchains/llvm/prebuilt/linux-x86_64/bin"

[ -f "$here/BinModem/Cargo.toml" ] || git -C "$here" submodule update --init --recursive

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$tc/aarch64-linux-android$api-clang"
export CC_aarch64_linux_android="$tc/aarch64-linux-android$api-clang"
export AR_aarch64_linux_android="$tc/llvm-ar"
export ANDROID_NDK_ROOT="$ndk"

rustup target add aarch64-linux-android >/dev/null 2>&1 || true
cargo build --release --target aarch64-linux-android --manifest-path "$here/android/Cargo.toml"

out="$here/android/target/apk"
rm -rf "$out" && mkdir -p "$out/staging/lib/arm64-v8a"
cp "$here/android/target/aarch64-linux-android/release/libbinmodem.so" "$out/staging/lib/arm64-v8a/"

"$bt/aapt2" link -o "$out/unaligned.apk" -I "$platform/android.jar" \
    --manifest "$here/android/AndroidManifest.xml"
# Native libraries are compressed to keep the download small; Android extracts
# them at install (extractNativeLibs="true" in the manifest).
(cd "$out/staging" && zip -q -9 -r "$out/unaligned.apk" lib)
"$bt/zipalign" -f -p 4 "$out/unaligned.apk" "$out/aligned.apk"

keystore="${KEYSTORE:-$here/android/debug.keystore}"
pass="${KEYSTORE_PASS:-android}"
alias="${KEY_ALIAS:-androiddebugkey}"
if [ ! -f "$keystore" ]; then
    keytool -genkeypair -keystore "$keystore" -storepass "$pass" -keypass "$pass" \
        -alias "$alias" -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=BinModem Debug" >/dev/null 2>&1
fi
"$bt/apksigner" sign --ks "$keystore" --ks-pass "pass:$pass" --ks-key-alias "$alias" \
    --out "$here/BinModem.apk" "$out/aligned.apk"
"$bt/apksigner" verify "$here/BinModem.apk"
echo "built $here/BinModem.apk"
