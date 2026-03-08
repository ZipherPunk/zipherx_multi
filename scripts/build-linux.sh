#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ZipherX Multi-Platform — Linux Build Script
# Builds the CLI binary and FFI library for Linux.
#
# Usage: ./scripts/build-linux.sh [cli|desktop|all]
#   cli      — Build CLI binary only (default)
#   desktop  — Build CLI + FFI library (for Compose Desktop)
#   all      — Build everything
#
# For cross-compilation from macOS:
#   docker run --rm -v "$PWD":/work -w /work rust:latest \
#     cargo build -p zipherx-cli -p zipherx-ffi --release

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${1:-cli}"

echo "=== ZipherX Linux Build ==="
echo "Repository: ${REPO_ROOT}"
echo "Target:     ${TARGET}"
echo ""

cd "${REPO_ROOT}"

# Always build CLI
echo ">>> Building zipherx-cli (release)..."
cargo build -p zipherx-cli --release
echo "    Done."

CLI_BINARY="target/release/zipherx-cli"
if [ -f "${CLI_BINARY}" ]; then
    echo "  CLI Binary: ${CLI_BINARY} ($(du -h "${CLI_BINARY}" | cut -f1))"
fi

# Build FFI library for desktop GUI
if [ "${TARGET}" = "desktop" ] || [ "${TARGET}" = "all" ]; then
    echo ""
    echo ">>> Building zipherx-ffi (release) for desktop GUI..."
    cargo build -p zipherx-ffi --release
    echo "    Done."

    FFI_LIB="target/release/libzipherx_ffi.so"
    if [ -f "${FFI_LIB}" ]; then
        echo "  FFI Library: ${FFI_LIB} ($(du -h "${FFI_LIB}" | cut -f1))"

        # Copy to desktop resources
        RESOURCES_DIR="${REPO_ROOT}/platforms/desktop/src/main/resources"
        mkdir -p "${RESOURCES_DIR}"
        cp "${FFI_LIB}" "${RESOURCES_DIR}/"
        echo "  Copied to desktop resources"
    fi

    echo ""
    echo "To run the desktop GUI:"
    echo "  cd platforms/desktop && ./gradlew run"
    echo ""
    echo "To package as .deb/.rpm:"
    echo "  cd platforms/desktop && ./gradlew packageDistributionForCurrentOS"
fi

echo ""
echo "=== Linux Build Complete ==="
