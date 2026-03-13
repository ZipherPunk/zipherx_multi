#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ZipherX Multi-Platform — Windows Build Script
# Cross-compiles the CLI binary and FFI library for Windows x86_64 from macOS/Linux.
#
# Usage: ./scripts/build-windows.sh [cli|desktop|all]
#   cli      — Build CLI .exe only (default)
#   desktop  — Build CLI + FFI DLL (for Compose Desktop)
#   all      — Build everything
#
# Prerequisites:
#   cargo install cargo-xwin
#   rustup target add x86_64-pc-windows-msvc

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUST_TARGET="x86_64-pc-windows-msvc"
BUILD_TARGET="${1:-cli}"

echo "=== ZipherX Windows Build (cross-compile) ==="
echo "Repository: ${REPO_ROOT}"
echo "Target:     ${RUST_TARGET}"
echo "Build:      ${BUILD_TARGET}"
echo ""

cd "${REPO_ROOT}"

# Check cargo-xwin is installed
if ! command -v cargo-xwin &> /dev/null; then
    echo "Error: cargo-xwin not found."
    echo "Install with: cargo install cargo-xwin"
    exit 1
fi

# ── OpenSSL cross-compilation strategy ────────────────────────────
# SQLCipher requires OpenSSL for AES encryption. Building OpenSSL from
# source for the VC-WIN64A target on macOS fails because:
#   1. macOS perl doesn't produce Windows-style paths
#   2. The generated nmake Makefile is incompatible with GNU make
#
# Solution: Build OpenSSL separately using the mingw64 target (which
# generates GNU make compatible makefiles), then point openssl-sys to
# the pre-built libraries.
#
# Prerequisites: brew install mingw-w64
# ───────────────────────────────────────────────────────────────────

OPENSSL_WIN_DIR="${REPO_ROOT}/target/.openssl-win64"
OPENSSL_VERSION="3.3.2"

# Detect pre-built OpenSSL (check both lib/ and lib64/)
OPENSSL_BUILT=false
for _d in "${OPENSSL_WIN_DIR}/lib" "${OPENSSL_WIN_DIR}/lib64"; do
    [ -f "${_d}/libcrypto.a" ] && OPENSSL_BUILT=true && break
done

if [ "${OPENSSL_BUILT}" = false ]; then
    echo ">>> Building OpenSSL ${OPENSSL_VERSION} for Windows x64..."

    # Check for mingw-w64 cross-compiler
    if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
        echo ""
        echo "Error: mingw-w64 cross-compiler not found."
        echo "Install with: brew install mingw-w64"
        echo ""
        echo "Alternative: set OPENSSL_DIR to a pre-built Windows OpenSSL:"
        echo "  export X86_64_PC_WINDOWS_MSVC_OPENSSL_DIR=/path/to/openssl"
        echo "  export X86_64_PC_WINDOWS_MSVC_OPENSSL_STATIC=1"
        exit 1
    fi

    # Always start from a clean source tree to avoid stale object files
    OPENSSL_SRC_DIR="${REPO_ROOT}/target/.openssl-src"
    rm -rf "${OPENSSL_SRC_DIR}"
    echo "  Downloading OpenSSL source..."
    mkdir -p "${OPENSSL_SRC_DIR}"
    OPENSSL_TARBALL="${REPO_ROOT}/target/.openssl-${OPENSSL_VERSION}.tar.gz"
    OPENSSL_EXPECTED_HASH="2e8a40b01979afe8be0bbfb3de5dc1c6709fedb46d6c89c10da114ab5fc3d281"
    curl -sL -o "${OPENSSL_TARBALL}" \
        "https://github.com/openssl/openssl/releases/download/openssl-${OPENSSL_VERSION}/openssl-${OPENSSL_VERSION}.tar.gz"
    OPENSSL_ACTUAL_HASH="$(shasum -a 256 "${OPENSSL_TARBALL}" | awk '{print $1}')"
    if [ "${OPENSSL_ACTUAL_HASH}" != "${OPENSSL_EXPECTED_HASH}" ]; then
        echo "Error: OpenSSL checksum mismatch!"
        echo "  Expected: ${OPENSSL_EXPECTED_HASH}"
        echo "  Got:      ${OPENSSL_ACTUAL_HASH}"
        rm -f "${OPENSSL_TARBALL}"
        exit 1
    fi
    echo "  SHA-256 checksum verified."
    tar xzf "${OPENSSL_TARBALL}" -C "${OPENSSL_SRC_DIR}" --strip-components=1
    rm -f "${OPENSSL_TARBALL}"

    cd "${OPENSSL_SRC_DIR}"

    echo "  Configuring..."
    # - _WIN32_WINNT=0x0600 (Vista+): use native ws2_32 freeaddrinfo/getaddrinfo
    # - no-sock: exclude BIO socket code (bio_addr.c) — eliminates Wspiapi/gai_strerror refs
    # - no-asm: avoid NASM dependency on cross-compile
    # - -mno-stack-arg-probe: prevent ___chkstk_ms calls (mingw runtime, not in MSVC CRT)
    perl ./Configure \
        --prefix="${OPENSSL_WIN_DIR}" \
        --cross-compile-prefix=x86_64-w64-mingw32- \
        -D_WIN32_WINNT=0x0600 \
        no-shared no-module no-tests no-comp no-zlib no-zlib-dynamic \
        no-ssl3 no-md2 no-rc5 no-weak-ssl-ciphers \
        no-camellia no-idea no-seed no-capieng no-asm \
        no-engine no-dso no-apps no-docs no-sock \
        mingw64 \
        -mno-stack-arg-probe

    echo "  Compiling (this takes a few minutes)..."
    make -j$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4) build_libs
    make install_sw

    # OpenSSL may install to lib64/ instead of lib/ — normalize
    if [ -d "${OPENSSL_WIN_DIR}/lib64" ] && [ ! -d "${OPENSSL_WIN_DIR}/lib" ]; then
        ln -s lib64 "${OPENSSL_WIN_DIR}/lib"
    fi

    # mingw64 produces .a archives; the MSVC linker (lld-link) looks for .lib
    # Create .lib symlinks so openssl-sys can find them
    LIB_ACTUAL="${OPENSSL_WIN_DIR}/lib64"
    [ ! -d "${LIB_ACTUAL}" ] && LIB_ACTUAL="${OPENSSL_WIN_DIR}/lib"
    for lib in ssl crypto; do
        if [ -f "${LIB_ACTUAL}/lib${lib}.a" ] && [ ! -f "${LIB_ACTUAL}/lib${lib}.lib" ]; then
            ln -s "lib${lib}.a" "${LIB_ACTUAL}/lib${lib}.lib"
        fi
    done

    # Safety net: strip any problematic objects that survived no-sock
    # (bio_addr.o references Wspiapi/gai_strerror symbols absent in MSVC)
    for _a in "${LIB_ACTUAL}/libcrypto.a" "${LIB_ACTUAL}/libssl.a"; do
        if [ -f "${_a}" ]; then
            for _obj in bio_addr bio_sock bio_sock2; do
                if x86_64-w64-mingw32-ar t "${_a}" 2>/dev/null | grep -q "${_obj}"; then
                    echo "  Stripping ${_obj} from $(basename "${_a}")..."
                    x86_64-w64-mingw32-ar d "${_a}" "${_obj}.o" 2>/dev/null || true
                    # OpenSSL 3 uses libcrypto-lib-bio_addr.o naming
                    x86_64-w64-mingw32-ar d "${_a}" "libcrypto-lib-${_obj}.o" 2>/dev/null || true
                fi
            done
        fi
    done

    cd "${REPO_ROOT}"
    echo "  Done: ${OPENSSL_WIN_DIR}"
fi

# Tell openssl-sys and libsqlite3-sys to use our pre-built OpenSSL
# Set LIB_DIR explicitly to lib64/ so neither crate needs the lib/ symlink
OPENSSL_LIB="${OPENSSL_WIN_DIR}/lib64"
[ ! -d "${OPENSSL_LIB}" ] && OPENSSL_LIB="${OPENSSL_WIN_DIR}/lib"

export X86_64_PC_WINDOWS_MSVC_OPENSSL_DIR="${OPENSSL_WIN_DIR}"
export X86_64_PC_WINDOWS_MSVC_OPENSSL_LIB_DIR="${OPENSSL_LIB}"
export X86_64_PC_WINDOWS_MSVC_OPENSSL_INCLUDE_DIR="${OPENSSL_WIN_DIR}/include"
export X86_64_PC_WINDOWS_MSVC_OPENSSL_STATIC=1
export X86_64_PC_WINDOWS_MSVC_OPENSSL_NO_VENDOR=1
# Also set generic vars as fallback
export OPENSSL_DIR="${OPENSSL_WIN_DIR}"
export OPENSSL_LIB_DIR="${OPENSSL_LIB}"
export OPENSSL_INCLUDE_DIR="${OPENSSL_WIN_DIR}/include"
export OPENSSL_STATIC=1
export OPENSSL_NO_VENDOR=1

# Build CLI (skip for desktop-only builds)
if [ "${BUILD_TARGET}" = "cli" ] || [ "${BUILD_TARGET}" = "all" ]; then
    echo ">>> Building zipherx-cli (release, Windows)..."
    cargo xwin build -p zipherx-cli --release --target "${RUST_TARGET}"
    echo "    Done."

    CLI_BINARY="target/${RUST_TARGET}/release/zipherx-cli.exe"
    if [ -f "${CLI_BINARY}" ]; then
        echo "  CLI Binary: ${CLI_BINARY} ($(du -h "${CLI_BINARY}" | cut -f1))"
    fi
fi

# Build FFI DLL for desktop GUI
if [ "${BUILD_TARGET}" = "desktop" ] || [ "${BUILD_TARGET}" = "all" ]; then
    echo ""
    echo ">>> Building zipherx-ffi (release, Windows DLL)..."
    cargo xwin build -p zipherx-ffi --release --target "${RUST_TARGET}"
    echo "    Done."

    FFI_LIB="target/${RUST_TARGET}/release/zipherx_ffi.dll"
    if [ -f "${FFI_LIB}" ]; then
        echo "  FFI DLL: ${FFI_LIB} ($(du -h "${FFI_LIB}" | cut -f1))"

        # Copy to desktop resources
        RESOURCES_DIR="${REPO_ROOT}/platforms/desktop/src/main/resources"
        mkdir -p "${RESOURCES_DIR}"
        cp "${FFI_LIB}" "${RESOURCES_DIR}/"
        echo "  Copied to desktop resources"
    fi

    echo ""
    echo "To package as .msi on Windows:"
    echo "  cd platforms/desktop && gradlew.bat packageDistributionForCurrentOS"
fi

echo ""
echo "=== Windows Build Complete ==="
echo "Copy files to a Windows machine to run."
