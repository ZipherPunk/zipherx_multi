#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ╔══════════════════════════════════════════════════════════════╗
# ║         ZipherX Multi-Platform — Build ALL Platforms         ║
# ║                                                              ║
# ║  Builds macOS, iOS Simulator, Android, Linux CLI,            ║
# ║  and Windows CLI from a single command.                      ║
# ║                                                              ║
# ║  Usage: ./scripts/build-all.sh [--skip-android] [--skip-win] ║
# ║  Run from the repository root.                               ║
# ╚══════════════════════════════════════════════════════════════╝

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

# NOTE: Fixed LOG_DIR path used for easy log review between runs.
# On shared/multi-user systems, verify ownership to prevent symlink attacks.
LOG_DIR="/tmp/zipherx-build"
mkdir -p "${LOG_DIR}"
if [ "$(stat -f '%u' "${LOG_DIR}" 2>/dev/null || stat -c '%u' "${LOG_DIR}" 2>/dev/null)" != "$(id -u)" ]; then
    LOG_DIR="$(mktemp -d /tmp/zipherx-build.XXXXXX)"
    echo "Warning: /tmp/zipherx-build owned by another user, using ${LOG_DIR}"
fi

SKIP_ANDROID=false
SKIP_WINDOWS=false

for arg in "$@"; do
    case "$arg" in
        --skip-android) SKIP_ANDROID=true ;;
        --skip-win|--skip-windows) SKIP_WINDOWS=true ;;
        --help|-h)
            echo "Usage: ./scripts/build-all.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --skip-android   Skip Android build (requires cargo-ndk + NDK)"
            echo "  --skip-windows   Skip Windows cross-compile (requires cargo-xwin)"
            echo "  --help           Show this help"
            exit 0
            ;;
    esac
done

START_TIME=$(date +%s)

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║              ZipherX Multi-Platform Build                   ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "  Repository: ${REPO_ROOT}"
echo "  Logs:       ${LOG_DIR}/"
echo ""

# ── Prerequisites Check ──────────────────────────────────────────

echo "── Checking prerequisites ──"
echo ""

PREREQ_OK=true

# Rust toolchain
if command -v cargo &>/dev/null; then
    echo "  [OK] cargo $(cargo --version | awk '{print $2}')"
else
    echo "  [!!] cargo not found — install Rust: https://rustup.rs"
    PREREQ_OK=false
fi

# iOS: check target
if rustup target list --installed 2>/dev/null | grep -q "aarch64-apple-ios-sim"; then
    echo "  [OK] rustup target aarch64-apple-ios-sim"
else
    echo "  [!!] iOS Simulator target missing — run: rustup target add aarch64-apple-ios-sim"
    PREREQ_OK=false
fi

# Android: cargo-ndk
if [ "$SKIP_ANDROID" = false ]; then
    if command -v cargo-ndk &>/dev/null; then
        echo "  [OK] cargo-ndk"
    else
        echo "  [--] cargo-ndk not found — Android build will be skipped"
        echo "       Install with: cargo install cargo-ndk"
        SKIP_ANDROID=true
    fi

    if [ -n "${ANDROID_NDK_HOME:-}" ]; then
        echo "  [OK] ANDROID_NDK_HOME=${ANDROID_NDK_HOME}"
    elif [ -n "${NDK_HOME:-}" ]; then
        echo "  [OK] NDK_HOME=${NDK_HOME}"
    else
        # Try to find NDK automatically
        NDK_PATH=""
        if [ -d "$HOME/Library/Android/sdk/ndk" ]; then
            NDK_PATH=$(find "$HOME/Library/Android/sdk/ndk" -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1 || true)
        fi
        if [ -n "$NDK_PATH" ] && [ -d "$NDK_PATH" ]; then
            export ANDROID_NDK_HOME="$NDK_PATH"
            echo "  [OK] Auto-detected NDK: ${NDK_PATH}"
        elif [ "$SKIP_ANDROID" = false ]; then
            echo "  [--] Android NDK not found — Android build will be skipped"
            echo "       Install via: Android Studio → SDK Manager → NDK"
            SKIP_ANDROID=true
        fi
    fi
fi

# Windows: cargo-xwin
if [ "$SKIP_WINDOWS" = false ]; then
    if command -v cargo-xwin &>/dev/null; then
        echo "  [OK] cargo-xwin"
    else
        echo "  [--] cargo-xwin not found — Windows build will be skipped"
        echo "       Install with: cargo install cargo-xwin"
        SKIP_WINDOWS=true
    fi
fi

echo ""

if [ "$PREREQ_OK" = false ]; then
    echo "  FATAL: Missing required prerequisites. Fix the [!!] items above."
    exit 1
fi

# ── Security: Cargo dependency audit ────────────────────────

if ! command -v cargo-audit &>/dev/null; then
    echo "  [!!] cargo-audit not found — required for security scanning"
    echo "       Install with: cargo install cargo-audit"
    exit 1
fi

echo ">>> Running cargo audit..."
if ! cargo audit --deny warnings 2>&1; then
    echo "FATAL: cargo audit found vulnerabilities. Fix before release."
    exit 1
fi
echo "  [OK] No known vulnerabilities found"
echo ""

# ── Count platforms ──────────────────────────────────────────────

PLATFORM_COUNT=3  # macOS + iOS + Linux always build
[ "$SKIP_ANDROID" = false ] && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))
[ "$SKIP_WINDOWS" = false ] && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))

echo "── Building ${PLATFORM_COUNT} platforms ──"
echo ""

PIDS=()
NAMES=()
LOGS=()

# ── 1. macOS ─────────────────────────────────────────────────────

echo "  [1/${PLATFORM_COUNT}] macOS (aarch64-apple-darwin)..."
./scripts/build-macos.sh > "${LOG_DIR}/macos.log" 2>&1 &
PIDS+=($!)
NAMES+=("macOS")
LOGS+=("${LOG_DIR}/macos.log")

# ── 2. iOS Simulator ─────────────────────────────────────────────

echo "  [2/${PLATFORM_COUNT}] iOS Simulator (aarch64-apple-ios-sim)..."
./scripts/build-ios-sim.sh > "${LOG_DIR}/ios-sim.log" 2>&1 &
PIDS+=($!)
NAMES+=("iOS Simulator")
LOGS+=("${LOG_DIR}/ios-sim.log")

# ── 3. Android ───────────────────────────────────────────────────

N=3
if [ "$SKIP_ANDROID" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Android (arm64-v8a + x86_64)..."
    ./scripts/build-android.sh > "${LOG_DIR}/android.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Android")
    LOGS+=("${LOG_DIR}/android.log")
    N=$((N + 1))
fi

# ── 4. Linux CLI ─────────────────────────────────────────────────

echo "  [${N}/${PLATFORM_COUNT}] Linux/macOS CLI (native)..."
./scripts/build-linux.sh > "${LOG_DIR}/linux.log" 2>&1 &
PIDS+=($!)
NAMES+=("Linux CLI")
LOGS+=("${LOG_DIR}/linux.log")
N=$((N + 1))

# ── 5. Windows CLI ───────────────────────────────────────────────

if [ "$SKIP_WINDOWS" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Windows CLI (x86_64-pc-windows-msvc)..."
    ./scripts/build-windows.sh > "${LOG_DIR}/windows.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Windows CLI")
    LOGS+=("${LOG_DIR}/windows.log")
fi

# ── Wait for all builds ──────────────────────────────────────────

echo ""
echo "── Waiting for ${#PIDS[@]} builds to complete... ──"
echo ""

PASSED=0
FAILED=0
FAILED_NAMES=()

for i in "${!PIDS[@]}"; do
    if wait "${PIDS[$i]}" 2>/dev/null; then
        echo "  ✓  ${NAMES[$i]}"
        PASSED=$((PASSED + 1))
    else
        echo "  ✗  ${NAMES[$i]}  →  ${LOGS[$i]}"
        FAILED=$((FAILED + 1))
        FAILED_NAMES+=("${NAMES[$i]}")
    fi
done

# ── Desktop: copy dylib for JNA ──────────────────────────────────

DESKTOP_RES="platforms/desktop/src/main/resources"
# The macOS build uses --target aarch64-apple-darwin, so the dylib is in the target-specific dir
DESKTOP_DYLIB="target/aarch64-apple-darwin/release/libzipherx_ffi.dylib"
# Fallback to target/release if the target-specific one doesn't exist
if [ ! -f "$DESKTOP_DYLIB" ]; then
    DESKTOP_DYLIB="target/release/libzipherx_ffi.dylib"
fi
if [ -d "$DESKTOP_RES" ] && [ -f "$DESKTOP_DYLIB" ]; then
    cp "$DESKTOP_DYLIB" "$DESKTOP_RES/libzipherx_ffi.dylib"
    echo "  ✓  Desktop dylib copied to ${DESKTOP_RES}/"
fi

# ── Summary ──────────────────────────────────────────────────────

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
MINS=$((ELAPSED / 60))
SECS=$((ELAPSED % 60))

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"

if [ $FAILED -eq 0 ]; then
    echo "║  BUILD COMPLETE — ${PASSED}/${PLATFORM_COUNT} platforms succeeded (${MINS}m ${SECS}s)        ║"
else
    echo "║  BUILD PARTIAL — ${PASSED} passed, ${FAILED} failed (${MINS}m ${SECS}s)              ║"
fi

echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# Print output paths
echo "── Output Artifacts ──"
echo ""

# macOS
MACOS_LIB="platforms/apple/Generated/lib/libzipherx_ffi.a"
[ -f "$MACOS_LIB" ] && echo "  macOS:         ${MACOS_LIB} ($(du -h "$MACOS_LIB" | cut -f1))"

# iOS
IOS_LIB="platforms/apple/Generated/lib-ios-sim/libzipherx_ffi.a"
[ -f "$IOS_LIB" ] && echo "  iOS Simulator: ${IOS_LIB} ($(du -h "$IOS_LIB" | cut -f1))"

# Android
if [ "$SKIP_ANDROID" = false ]; then
    ANDROID_ARM="platforms/android/src/main/jniLibs/arm64-v8a/libzipherx_ffi.so"
    [ -f "$ANDROID_ARM" ] && echo "  Android arm64: ${ANDROID_ARM} ($(du -h "$ANDROID_ARM" | cut -f1))"
    ANDROID_X86="platforms/android/src/main/jniLibs/x86_64/libzipherx_ffi.so"
    [ -f "$ANDROID_X86" ] && echo "  Android x86:   ${ANDROID_X86} ($(du -h "$ANDROID_X86" | cut -f1))"
fi

# Linux CLI
CLI_BIN="target/release/zipherx-cli"
[ -f "$CLI_BIN" ] && echo "  Linux CLI:     ${CLI_BIN} ($(du -h "$CLI_BIN" | cut -f1))"

# Windows CLI
WIN_BIN="target/x86_64-pc-windows-msvc/release/zipherx-cli.exe"
[ -f "$WIN_BIN" ] && echo "  Windows CLI:   ${WIN_BIN} ($(du -h "$WIN_BIN" | cut -f1))"

# Desktop
DESKTOP_LIB="platforms/desktop/src/main/resources/libzipherx_ffi.dylib"
[ -f "$DESKTOP_LIB" ] && echo "  Desktop dylib: ${DESKTOP_LIB} ($(du -h "$DESKTOP_LIB" | cut -f1))"

# Swift bindings
SWIFT_BINDINGS="platforms/apple/Generated/swift/zipherx.swift"
[ -f "$SWIFT_BINDINGS" ] && echo "  Swift bindings: ${SWIFT_BINDINGS} ($(du -h "$SWIFT_BINDINGS" | cut -f1))"

# Kotlin bindings
KT_DIR="platforms/android/src/main/kotlin/uniffi/zipherx"
[ -d "$KT_DIR" ] && echo "  Kotlin bindings: ${KT_DIR}/"

echo ""

# Print failures if any
if [ $FAILED -gt 0 ]; then
    echo "── Failed Builds ──"
    echo ""
    for name in "${FAILED_NAMES[@]}"; do
        LOG_NAME=$(echo "$name" | tr '[:upper:]' '[:lower:]' | tr ' ' '-')
        LOG_FILE="${LOG_DIR}/${LOG_NAME}.log"
        if [ -f "$LOG_FILE" ]; then
            echo "  ${name}:"
            tail -5 "$LOG_FILE" | sed 's/^/    /'
            echo ""
        fi
    done
    exit 1
fi

echo "── Next Steps ──"
echo ""
echo "  macOS:   open platforms/apple/ZipherXApp.xcodeproj  →  Cmd+R"
echo "  iOS:     Xcode → ZipherXApp-iOS scheme → iPhone Simulator → Cmd+R"
echo "  Android: open -a 'Android Studio' platforms/android → Run on emulator"
echo "  Desktop: cd platforms/desktop && ./gradlew run"
echo "  Linux:   ./target/release/zipherx-cli"
[ "$SKIP_WINDOWS" = false ] && echo "  Windows: Copy target/x86_64-pc-windows-msvc/release/zipherx-cli.exe to Windows"
echo ""
