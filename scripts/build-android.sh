#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ZipherX Multi-Platform — Android Build Script
# Builds the Rust FFI shared library for Android and generates Kotlin bindings.
#
# Usage: ./scripts/build-android.sh
# Run from the repository root
#
# Prerequisites:
#   cargo install cargo-ndk
#   Android NDK installed (via Android Studio SDK Manager)
#   Set ANDROID_NDK_HOME if not auto-detected

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FFI_CRATE="zipherx-ffi"
UDL_PATH="crates/zipherx-ffi/src/zipherx.udl"
ANDROID_DIR="${REPO_ROOT}/platforms/android"
JNILIB_DIR="${ANDROID_DIR}/src/main/jniLibs"
KOTLIN_DIR="${ANDROID_DIR}/src/main/kotlin"

echo "=== ZipherX Android Build ==="
echo "Repository: ${REPO_ROOT}"
echo ""

cd "${REPO_ROOT}"

# Step 1: Build native libraries for Android ABIs
echo ">>> Step 1/3: Building Rust shared library (release, Android)..."
cargo ndk \
    -t arm64-v8a \
    -t x86_64 \
    -o "${JNILIB_DIR}" \
    build -p "${FFI_CRATE}" --release
echo "    Done."

# Step 1b: Create symlinks so JNA finds the library as "uniffi_zipherx"
# UniFFI-generated Kotlin uses JNA to load "uniffi_zipherx" but cargo produces "libzipherx_ffi.so"
echo ">>> Step 1b: Creating JNA-compatible library symlinks..."
for ABI_DIR in "${JNILIB_DIR}"/*/; do
    if [ -f "${ABI_DIR}libzipherx_ffi.so" ]; then
        cp "${ABI_DIR}libzipherx_ffi.so" "${ABI_DIR}libuniffi_zipherx.so"
        echo "    $(basename "${ABI_DIR}"): libuniffi_zipherx.so"
    fi
done
echo "    Done."

# Step 2: Generate UniFFI Kotlin bindings
echo ">>> Step 2/3: Generating UniFFI Kotlin bindings..."
mkdir -p "${KOTLIN_DIR}"
cargo run -p uniffi-bindgen -- generate \
    "${UDL_PATH}" \
    --language kotlin \
    --out-dir "${KOTLIN_DIR}"
echo "    Done."

# Step 3: Verify output
echo ">>> Step 3/3: Verifying output..."
echo ""
echo "=== Android Build Complete ==="
echo ""
echo "  JNI Libraries:"
find "${JNILIB_DIR}" -name "*.so" -exec sh -c 'for f; do echo "    $f  ($(du -h "$f" | cut -f1))"; done' _ {} +
echo ""
echo "  Kotlin Bindings:"
find "${KOTLIN_DIR}" -name "*.kt" -exec sh -c 'for f; do echo "    $f"; done' _ {} +
echo ""
echo "Next steps:"
echo "  1. open -a 'Android Studio' platforms/android"
echo "  2. Select Pixel emulator → Run"
