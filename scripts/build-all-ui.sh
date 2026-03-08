#!/bin/bash
set -euo pipefail

# ╔══════════════════════════════════════════════════════════════╗
# ║       ZipherX — Build ALL UI Platforms (no CLI-only)        ║
# ║                                                              ║
# ║  Builds: macOS Desktop, iOS, Android, Windows Desktop,      ║
# ║          Linux Desktop — all with GUI support.               ║
# ║                                                              ║
# ║  Usage: ./scripts/build-all-ui.sh [OPTIONS]                 ║
# ║    --skip-android   Skip Android (requires cargo-ndk + NDK) ║
# ║    --skip-windows   Skip Windows (requires cargo-xwin)      ║
# ║    --skip-linux     Skip Linux desktop                      ║
# ║    --skip-ios       Skip iOS Simulator                      ║
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
SKIP_LINUX=false
SKIP_IOS=false

for arg in "$@"; do
    case "$arg" in
        --skip-android)  SKIP_ANDROID=true ;;
        --skip-win*)     SKIP_WINDOWS=true ;;
        --skip-linux)    SKIP_LINUX=true ;;
        --skip-ios)      SKIP_IOS=true ;;
        --help|-h)
            echo "Usage: ./scripts/build-all-ui.sh [OPTIONS]"
            echo ""
            echo "Builds all UI platforms (Desktop + Mobile)."
            echo ""
            echo "Options:"
            echo "  --skip-android   Skip Android (requires cargo-ndk + NDK)"
            echo "  --skip-windows   Skip Windows desktop (requires cargo-xwin + mingw-w64)"
            echo "  --skip-linux     Skip Linux desktop"
            echo "  --skip-ios       Skip iOS Simulator"
            echo "  --help           Show this help"
            exit 0
            ;;
    esac
done

START_TIME=$(date +%s)

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║           ZipherX — Build All UI Platforms                  ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "  Repository: ${REPO_ROOT}"
echo "  Logs:       ${LOG_DIR}/"
echo ""

# ── Prerequisites ────────────────────────────────────────────────

echo "── Checking prerequisites ──"
echo ""

PREREQ_OK=true

if command -v cargo &>/dev/null; then
    echo "  [OK] cargo $(cargo --version | awk '{print $2}')"
else
    echo "  [!!] cargo not found — install Rust: https://rustup.rs"
    PREREQ_OK=false
fi

# iOS
if [ "$SKIP_IOS" = false ]; then
    if rustup target list --installed 2>/dev/null | grep -q "aarch64-apple-ios-sim"; then
        echo "  [OK] rustup target aarch64-apple-ios-sim"
    else
        echo "  [--] iOS Simulator target missing — skipping"
        echo "       Install with: rustup target add aarch64-apple-ios-sim"
        SKIP_IOS=true
    fi
fi

# Android
if [ "$SKIP_ANDROID" = false ]; then
    if command -v cargo-ndk &>/dev/null; then
        echo "  [OK] cargo-ndk"
    else
        echo "  [--] cargo-ndk not found — skipping Android"
        echo "       Install with: cargo install cargo-ndk"
        SKIP_ANDROID=true
    fi
    if [ "$SKIP_ANDROID" = false ]; then
        if [ -n "${ANDROID_NDK_HOME:-}" ]; then
            echo "  [OK] ANDROID_NDK_HOME=${ANDROID_NDK_HOME}"
        elif [ -n "${NDK_HOME:-}" ]; then
            echo "  [OK] NDK_HOME=${NDK_HOME}"
        else
            NDK_PATH=""
            if [ -d "$HOME/Library/Android/sdk/ndk" ]; then
                NDK_PATH=$(find "$HOME/Library/Android/sdk/ndk" -maxdepth 1 -type d 2>/dev/null | sort -V | tail -1 || true)
            fi
            if [ -n "$NDK_PATH" ] && [ -d "$NDK_PATH" ]; then
                export ANDROID_NDK_HOME="$NDK_PATH"
                echo "  [OK] Auto-detected NDK: ${NDK_PATH}"
            else
                echo "  [--] Android NDK not found — skipping Android"
                SKIP_ANDROID=true
            fi
        fi
    fi
fi

# Windows
if [ "$SKIP_WINDOWS" = false ]; then
    if command -v cargo-xwin &>/dev/null; then
        echo "  [OK] cargo-xwin"
    else
        echo "  [--] cargo-xwin not found — skipping Windows"
        echo "       Install with: cargo install cargo-xwin"
        SKIP_WINDOWS=true
    fi
    if [ "$SKIP_WINDOWS" = false ]; then
        if command -v x86_64-w64-mingw32-gcc &>/dev/null; then
            echo "  [OK] mingw-w64 (for OpenSSL cross-build)"
        else
            echo "  [--] mingw-w64 not found — skipping Windows"
            echo "       Install with: brew install mingw-w64"
            SKIP_WINDOWS=true
        fi
    fi
fi

echo ""

if [ "$PREREQ_OK" = false ]; then
    echo "  FATAL: Missing required prerequisites. Fix the [!!] items above."
    exit 1
fi

# ── Count platforms ──────────────────────────────────────────────

PLATFORM_COUNT=1  # macOS Desktop always builds
[ "$SKIP_IOS" = false ]     && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))
[ "$SKIP_ANDROID" = false ] && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))
[ "$SKIP_WINDOWS" = false ] && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))
[ "$SKIP_LINUX" = false ]   && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))

echo "── Building ${PLATFORM_COUNT} UI platforms ──"
echo ""

PIDS=()
NAMES=()
LOGS=()
N=1

# ── 1. macOS (SwiftUI + Compose Desktop) ────────────────────────

echo "  [${N}/${PLATFORM_COUNT}] macOS — SwiftUI app + Compose Desktop dylib..."
(
    # Build FFI static lib + Swift bindings for Xcode
    ./scripts/build-macos.sh
    # Also build dylib for Compose Desktop on macOS
    cargo build -p zipherx-ffi --release
    DESKTOP_RES="platforms/desktop/src/main/resources"
    mkdir -p "${DESKTOP_RES}"
    cp target/release/libzipherx_ffi.dylib "${DESKTOP_RES}/"
) > "${LOG_DIR}/macos.log" 2>&1 &
PIDS+=($!)
NAMES+=("macOS (SwiftUI + Desktop)")
LOGS+=("${LOG_DIR}/macos.log")
N=$((N + 1))

# ── 2. iOS Simulator ────────────────────────────────────────────

if [ "$SKIP_IOS" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] iOS Simulator (aarch64-apple-ios-sim)..."
    ./scripts/build-ios-sim.sh > "${LOG_DIR}/ios-sim.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("iOS Simulator")
    LOGS+=("${LOG_DIR}/ios-sim.log")
    N=$((N + 1))
fi

# ── 3. Android ──────────────────────────────────────────────────

if [ "$SKIP_ANDROID" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Android (arm64-v8a + x86_64)..."
    ./scripts/build-android.sh > "${LOG_DIR}/android.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Android")
    LOGS+=("${LOG_DIR}/android.log")
    N=$((N + 1))
fi

# ── 4. Windows Desktop (cross-compile FFI DLL) ──────────────────

if [ "$SKIP_WINDOWS" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Windows Desktop (x86_64-pc-windows-msvc DLL)..."
    ./scripts/build-windows.sh desktop > "${LOG_DIR}/windows-desktop.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Windows Desktop")
    LOGS+=("${LOG_DIR}/windows-desktop.log")
    N=$((N + 1))
fi

# ── 5. Linux Desktop (native FFI .so) ───────────────────────────

if [ "$SKIP_LINUX" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Linux Desktop (native .so)..."
    ./scripts/build-linux.sh desktop > "${LOG_DIR}/linux-desktop.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Linux Desktop")
    LOGS+=("${LOG_DIR}/linux-desktop.log")
fi

# ── Wait ────────────────────────────────────────────────────────

echo ""
echo "── Waiting for ${#PIDS[@]} builds... ──"
echo ""

PASSED=0
FAILED=0
FAILED_NAMES=()

for i in "${!PIDS[@]}"; do
    if wait "${PIDS[$i]}" 2>/dev/null; then
        echo "  OK  ${NAMES[$i]}"
        PASSED=$((PASSED + 1))
    else
        echo "  FAIL  ${NAMES[$i]}  ->  ${LOGS[$i]}"
        FAILED=$((FAILED + 1))
        FAILED_NAMES+=("${NAMES[$i]}")
    fi
done

# ── Summary ─────────────────────────────────────────────────────

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
MINS=$((ELAPSED / 60))
SECS=$((ELAPSED % 60))

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
if [ $FAILED -eq 0 ]; then
    echo "║  ALL UI BUILDS COMPLETE — ${PASSED}/${PLATFORM_COUNT} platforms (${MINS}m ${SECS}s)             ║"
else
    echo "║  PARTIAL — ${PASSED} passed, ${FAILED} failed (${MINS}m ${SECS}s)                     ║"
fi
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ── Output Artifacts ────────────────────────────────────────────

echo "── Output Artifacts ──"
echo ""

# macOS SwiftUI
MACOS_LIB="platforms/apple/Generated/lib/libzipherx_ffi.a"
[ -f "$MACOS_LIB" ] && echo "  macOS SwiftUI:     ${MACOS_LIB} ($(du -h "$MACOS_LIB" | cut -f1))"

# macOS Desktop (Compose)
DESKTOP_DYLIB="platforms/desktop/src/main/resources/libzipherx_ffi.dylib"
[ -f "$DESKTOP_DYLIB" ] && echo "  macOS Desktop:     ${DESKTOP_DYLIB} ($(du -h "$DESKTOP_DYLIB" | cut -f1))"

# iOS
IOS_LIB="platforms/apple/Generated/lib-ios-sim/libzipherx_ffi.a"
[ -f "$IOS_LIB" ] && echo "  iOS Simulator:     ${IOS_LIB} ($(du -h "$IOS_LIB" | cut -f1))"

# Android
ANDROID_ARM="platforms/android/src/main/jniLibs/arm64-v8a/libzipherx_ffi.so"
[ -f "$ANDROID_ARM" ] && echo "  Android arm64:     ${ANDROID_ARM} ($(du -h "$ANDROID_ARM" | cut -f1))"
ANDROID_X86="platforms/android/src/main/jniLibs/x86_64/libzipherx_ffi.so"
[ -f "$ANDROID_X86" ] && echo "  Android x86_64:    ${ANDROID_X86} ($(du -h "$ANDROID_X86" | cut -f1))"

# Windows Desktop
WIN_DLL="target/x86_64-pc-windows-msvc/release/zipherx_ffi.dll"
[ -f "$WIN_DLL" ] && echo "  Windows Desktop:   ${WIN_DLL} ($(du -h "$WIN_DLL" | cut -f1))"

# Linux Desktop
LINUX_SO="platforms/desktop/src/main/resources/libzipherx_ffi.so"
[ -f "$LINUX_SO" ] && echo "  Linux Desktop:     ${LINUX_SO} ($(du -h "$LINUX_SO" | cut -f1))"

# Bindings
SWIFT_BINDINGS="platforms/apple/Generated/swift/zipherx.swift"
[ -f "$SWIFT_BINDINGS" ] && echo "  Swift bindings:    ${SWIFT_BINDINGS}"
KT_DIR="platforms/android/src/main/kotlin/uniffi/zipherx"
[ -d "$KT_DIR" ] && echo "  Kotlin bindings:   ${KT_DIR}/"

echo ""

# ── Failures ────────────────────────────────────────────────────

if [ $FAILED -gt 0 ]; then
    echo "── Failed Builds (last 10 lines) ──"
    echo ""
    for name in "${FAILED_NAMES[@]}"; do
        LOG_NAME=$(echo "$name" | tr '[:upper:]' '[:lower:]' | tr ' ()' '--' | sed 's/--*/-/g; s/-$//')
        for LOG_FILE in "${LOG_DIR}"/*.log; do
            if echo "$name" | grep -qi "$(basename "$LOG_FILE" .log | tr '-' ' ')"; then
                echo "  ${name}:"
                tail -10 "$LOG_FILE" | sed 's/^/    /'
                echo ""
                break
            fi
        done
    done
    exit 1
fi

# ── Next Steps ──────────────────────────────────────────────────

echo "── Next Steps ──"
echo ""
echo "  macOS SwiftUI:   open platforms/apple/ZipherXApp.xcodeproj  ->  Cmd+R"
echo "  macOS Desktop:   cd platforms/desktop && ./gradlew run"
echo "  iOS Simulator:   Xcode -> ZipherXApp-iOS scheme -> iPhone Simulator -> Cmd+R"
echo "  Android:         open -a 'Android Studio' platforms/android -> Run"
[ "$SKIP_WINDOWS" = false ] && echo "  Windows Desktop:  Copy platforms/desktop/ + DLL to Windows -> gradlew.bat run"
[ "$SKIP_LINUX" = false ]   && echo "  Linux Desktop:    cd platforms/desktop && ./gradlew run  (on Linux)"
echo ""
echo "  Package installers (run on target OS):"
echo "    cd platforms/desktop && ./gradlew packageDistributionForCurrentOS"
echo "    -> .dmg (macOS) | .msi (Windows) | .deb/.rpm (Linux)"
echo ""
