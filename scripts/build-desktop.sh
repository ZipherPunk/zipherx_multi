#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ZipherX Multi-Platform — Desktop Build Script
# Builds the Compose Desktop app for the current OS.
#
# Usage: ./scripts/build-desktop.sh [run|package]
#   run      — Build and run the desktop app
#   package  — Create native installer (DMG/MSI/DEB)
#
# Prerequisites:
#   - JDK 17+
#   - Rust toolchain (for building zipherx-ffi)
#   - The zipherx-ffi native library must be built first

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP_DIR="${REPO_ROOT}/platforms/desktop"
ACTION="${1:-run}"

echo "=== ZipherX Desktop Build ==="
echo "Repository: ${REPO_ROOT}"
echo "Action:     ${ACTION}"
echo ""

# Detect OS and set library name
OS="$(uname -s)"
case "$OS" in
    Linux)  LIB_NAME="libzipherx_ffi.so"  ; RUST_TARGET="" ;;
    Darwin) LIB_NAME="libzipherx_ffi.dylib"; RUST_TARGET="" ;;
    MINGW*|MSYS*|CYGWIN*) LIB_NAME="zipherx_ffi.dll"; RUST_TARGET="" ;;
    *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

# Step 1: Build the Rust FFI library
echo ">>> Building zipherx-ffi (release)..."
cd "${REPO_ROOT}"
if [ -n "${RUST_TARGET}" ]; then
    cargo build -p zipherx-ffi --release --target "${RUST_TARGET}"
    RUST_LIB="target/${RUST_TARGET}/release/${LIB_NAME}"
else
    cargo build -p zipherx-ffi --release
    RUST_LIB="target/release/${LIB_NAME}"
fi

if [ ! -f "${RUST_LIB}" ]; then
    echo "Error: Rust library not found at ${RUST_LIB}"
    exit 1
fi
echo "    Rust library: ${RUST_LIB} ($(du -h "${RUST_LIB}" | cut -f1))"

# Step 2: Copy native library to desktop resources
RESOURCES_DIR="${DESKTOP_DIR}/src/main/resources"
mkdir -p "${RESOURCES_DIR}"
cp "${RUST_LIB}" "${RESOURCES_DIR}/${LIB_NAME}"
echo "    Copied to: ${RESOURCES_DIR}/${LIB_NAME}"

# Also copy UniFFI-generated Kotlin bindings
UNIFFI_DIR="${REPO_ROOT}/platforms/android/src/main/kotlin/uniffi"
DESKTOP_UNIFFI="${DESKTOP_DIR}/src/main/kotlin/uniffi"
if [ -d "${UNIFFI_DIR}" ]; then
    mkdir -p "${DESKTOP_UNIFFI}"
    cp -r "${UNIFFI_DIR}/zipherx" "${DESKTOP_UNIFFI}/"
    echo "    Copied UniFFI bindings to desktop"
fi

# Step 3: Build/run the desktop app
cd "${DESKTOP_DIR}"

case "${ACTION}" in
    run)
        echo ""
        echo ">>> Running ZipherX Desktop..."
        ./gradlew run
        ;;
    package)
        echo ""
        echo ">>> Packaging ZipherX Desktop..."
        ./gradlew packageDistributionForCurrentOS
        echo ""
        echo "=== Desktop Package Complete ==="
        echo "Check: ${DESKTOP_DIR}/build/compose/binaries/"
        ;;
    *)
        echo "Unknown action: ${ACTION}"
        echo "Usage: $0 [run|package]"
        exit 1
        ;;
esac
