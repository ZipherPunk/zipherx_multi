#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ZipherX Multi-Platform — macOS Build Script
# Builds the Rust FFI static library and generates UniFFI Swift bindings.
#
# Usage: ./scripts/build-macos.sh
# Run from the repository root

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="aarch64-apple-darwin"
FFI_CRATE="zipherx-ffi"
UDL_PATH="crates/zipherx-ffi/src/zipherx.udl"

GENERATED_DIR="${REPO_ROOT}/platforms/apple/Generated"
LIB_DIR="${GENERATED_DIR}/lib"
SWIFT_DIR="${GENERATED_DIR}/swift"
INCLUDE_DIR="${GENERATED_DIR}/include"

echo "=== ZipherX macOS Build ==="
echo "Repository: ${REPO_ROOT}"
echo "Target:     ${TARGET}"
echo ""

cd "${REPO_ROOT}"

# Step 1: Build the Rust static library
echo ">>> Step 1/6: Building Rust static library (release)..."
MACOSX_DEPLOYMENT_TARGET=14.0 cargo build -p "${FFI_CRATE}" --release --target "${TARGET}"
echo "    Done."

# Step 2: Create output directories
echo ">>> Step 2/6: Creating output directories..."
mkdir -p "${LIB_DIR}" "${SWIFT_DIR}" "${INCLUDE_DIR}"

# Step 3: Copy the static library
echo ">>> Step 3/6: Copying static library..."
cp "target/${TARGET}/release/libzipherx_ffi.a" "${LIB_DIR}/libzipherx_ffi.a"
echo "    -> ${LIB_DIR}/libzipherx_ffi.a"

# Step 4: Generate UniFFI Swift bindings
echo ">>> Step 4/6: Generating UniFFI Swift bindings..."
cargo run -p uniffi-bindgen -- generate \
    "${UDL_PATH}" \
    --language swift \
    --out-dir "${SWIFT_DIR}"
echo "    Done."

# Step 5: Organize headers and modulemap
# UniFFI names files after the UDL namespace ("zipherx"), not the crate name.
# Generated files: zipherx.swift, zipherxFFI.h, zipherxFFI.modulemap
echo ">>> Step 5/6: Organizing headers and modulemap..."
mv "${SWIFT_DIR}/zipherxFFI.h" "${INCLUDE_DIR}/zipherxFFI.h"
mv "${SWIFT_DIR}/zipherxFFI.modulemap" "${INCLUDE_DIR}/module.modulemap"

# Step 6: Patch module name for ZipherXFFI compatibility
# The existing Swift wrapper uses `#if canImport(ZipherXFFI)` and `import ZipherXFFI`.
# UniFFI generates `zipherxFFI` as the C module name. Rename it so the guards work.
echo ">>> Step 6/6: Patching module name (zipherxFFI -> ZipherXFFI)..."
sed -i '' 's/module zipherxFFI/module ZipherXFFI/' "${INCLUDE_DIR}/module.modulemap"
sed -i '' 's/canImport(zipherxFFI)/canImport(ZipherXFFI)/' "${SWIFT_DIR}/zipherx.swift"
sed -i '' 's/import zipherxFFI/import ZipherXFFI/' "${SWIFT_DIR}/zipherx.swift"

echo ""
echo "=== Build Complete ==="
echo ""
echo "  Static library:  ${LIB_DIR}/libzipherx_ffi.a"
echo "  Swift bindings:  ${SWIFT_DIR}/zipherx.swift"
echo "  C header:        ${INCLUDE_DIR}/zipherxFFI.h"
echo "  Modulemap:       ${INCLUDE_DIR}/module.modulemap"
echo ""

# Print library info
echo "Library:"
file "${LIB_DIR}/libzipherx_ffi.a" | sed 's/.*: /  /'
echo ""

echo "Generated Swift API (first 20 public functions):"
grep '^public func' "${SWIFT_DIR}/zipherx.swift" | head -20 | sed 's/^/  /'
echo ""

echo "Next steps:"
echo "  1. cd platforms/apple && xcodegen generate"
echo "  2. open ZipherXApp.xcodeproj"
echo "  3. Cmd+R to build and run"
