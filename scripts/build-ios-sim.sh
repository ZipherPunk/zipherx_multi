#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ZipherX Multi-Platform — iOS Simulator Build Script
# Builds the Rust FFI static library for iOS Simulator (ARM64).
#
# Usage: ./scripts/build-ios-sim.sh
# Run from the repository root
#
# Prerequisites:
#   rustup target add aarch64-apple-ios-sim

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="aarch64-apple-ios-sim"
FFI_CRATE="zipherx-ffi"

GENERATED_DIR="${REPO_ROOT}/platforms/apple/Generated"
LIB_DIR="${GENERATED_DIR}/lib-ios-sim"
SWIFT_DIR="${GENERATED_DIR}/swift"
INCLUDE_DIR="${GENERATED_DIR}/include"

echo "=== ZipherX iOS Simulator Build ==="
echo "Repository: ${REPO_ROOT}"
echo "Target:     ${TARGET}"
echo ""

cd "${REPO_ROOT}"

# Step 1: Build the Rust static library for iOS Simulator
echo ">>> Step 1/4: Building Rust static library (release, iOS Simulator)..."
IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p "${FFI_CRATE}" --release --target "${TARGET}"
echo "    Done."

# Step 2: Create output directory
echo ">>> Step 2/4: Creating output directory..."
mkdir -p "${LIB_DIR}"

# Step 3: Copy the static library
echo ">>> Step 3/4: Copying static library..."
cp "target/${TARGET}/release/libzipherx_ffi.a" "${LIB_DIR}/libzipherx_ffi.a"
echo "    -> ${LIB_DIR}/libzipherx_ffi.a"

# Step 4: Verify Swift bindings exist (shared with macOS build)
echo ">>> Step 4/4: Verifying shared Swift bindings..."
if [ ! -f "${SWIFT_DIR}/zipherx.swift" ]; then
    echo "    Swift bindings not found — generating from UDL..."
    mkdir -p "${SWIFT_DIR}" "${INCLUDE_DIR}"
    cargo run -p uniffi-bindgen -- generate \
        "crates/zipherx-ffi/src/zipherx.udl" \
        --language swift \
        --out-dir "${SWIFT_DIR}"
    mv "${SWIFT_DIR}/zipherxFFI.h" "${INCLUDE_DIR}/zipherxFFI.h"
    mv "${SWIFT_DIR}/zipherxFFI.modulemap" "${INCLUDE_DIR}/module.modulemap"
    sed -i '' 's/module zipherxFFI/module ZipherXFFI/' "${INCLUDE_DIR}/module.modulemap"
    sed -i '' 's/canImport(zipherxFFI)/canImport(ZipherXFFI)/' "${SWIFT_DIR}/zipherx.swift"
    sed -i '' 's/import zipherxFFI/import ZipherXFFI/' "${SWIFT_DIR}/zipherx.swift"
    echo "    Generated."
else
    echo "    Swift bindings already exist (shared with macOS)."
fi

echo ""
echo "=== iOS Simulator Build Complete ==="
echo ""
echo "  Static library:  ${LIB_DIR}/libzipherx_ffi.a"
echo "  Swift bindings:  ${SWIFT_DIR}/zipherx.swift (shared)"
echo "  C header:        ${INCLUDE_DIR}/zipherxFFI.h (shared)"
echo ""

# Print library info
echo "Library:"
file "${LIB_DIR}/libzipherx_ffi.a" | sed 's/.*: /  /'
echo ""

echo "Next steps:"
echo "  1. cd platforms/apple && xcodegen generate"
echo "  2. open ZipherXApp.xcodeproj"
echo "  3. Select ZipherXApp-iOS scheme → iPhone Simulator → Cmd+R"
