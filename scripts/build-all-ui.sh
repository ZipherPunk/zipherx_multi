#!/bin/bash
set -euo pipefail

# ╔══════════════════════════════════════════════════════════════╗
# ║          ZipherX — Build ALL UI Platforms                    ║
# ║                                                              ║
# ║  Desktop: egui (macOS / Linux / Windows)                     ║
# ║  Mobile:  Android (Kotlin via FFI)                           ║
# ║                                                              ║
# ║  Usage: ./scripts/build-all-ui.sh [OPTIONS]                 ║
# ║    --skip-android    Skip Android (requires cargo-ndk + NDK)║
# ║    --skip-windows    Skip Windows egui cross-compile        ║
# ║    --skip-linux      Skip Linux egui cross-compile          ║
# ║    --skip-egui       Skip native egui desktop build         ║
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
SKIP_EGUI=false

for arg in "$@"; do
    case "$arg" in
        --skip-android)  SKIP_ANDROID=true ;;
        --skip-win*)     SKIP_WINDOWS=true ;;
        --skip-linux)    SKIP_LINUX=true ;;
        --skip-egui)     SKIP_EGUI=true ;;
        --help|-h)
            echo "Usage: ./scripts/build-all-ui.sh [OPTIONS]"
            echo ""
            echo "Builds all UI platforms:"
            echo "  Desktop: egui (macOS/Linux/Windows)"
            echo "  Mobile:  Android (Kotlin)"
            echo ""
            echo "Options:"
            echo "  --skip-android   Skip Android (requires cargo-ndk + NDK)"
            echo "  --skip-windows   Skip Windows egui cross-compile (requires cargo-xwin + mingw-w64)"
            echo "  --skip-linux     Skip Linux egui cross-compile (requires cross or linux target)"
            echo "  --skip-egui      Skip native egui desktop build"
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

# Windows cross-compile
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

# Linux cross-compile
if [ "$SKIP_LINUX" = false ]; then
    HOST_OS="$(uname -s)"
    if [ "$HOST_OS" = "Linux" ]; then
        echo "  [OK] Linux native — egui will build natively"
        # On Linux, the native egui build IS the Linux build — skip duplicate
        SKIP_LINUX=true
    elif command -v cross &>/dev/null; then
        echo "  [OK] cross (Linux cross-compile)"
    else
        if rustup target list --installed 2>/dev/null | grep -q "x86_64-unknown-linux-gnu"; then
            echo "  [OK] rustup target x86_64-unknown-linux-gnu"
            echo "  [!!] Note: Cross-compiling to Linux from macOS requires a linker + sysroot"
            echo "       Consider using 'cross' or building on actual Linux / Linux CI"
            SKIP_LINUX=true
        else
            echo "  [--] Linux cross-compile requires 'cross' — skipping"
            echo "       Install with: cargo install cross"
            SKIP_LINUX=true
        fi
    fi
fi

echo ""

if [ "$PREREQ_OK" = false ]; then
    echo "  FATAL: Missing required prerequisites. Fix the [!!] items above."
    exit 1
fi

# ── Count platforms ──────────────────────────────────────────────

PLATFORM_COUNT=0
[ "$SKIP_EGUI" = false ]    && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))
[ "$SKIP_ANDROID" = false ] && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))
[ "$SKIP_WINDOWS" = false ] && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))
[ "$SKIP_LINUX" = false ]   && PLATFORM_COUNT=$((PLATFORM_COUNT + 1))

if [ "$PLATFORM_COUNT" -eq 0 ]; then
    echo "  Nothing to build — all platforms skipped."
    exit 0
fi

echo "── Building ${PLATFORM_COUNT} UI platforms ──"
echo ""

PIDS=()
NAMES=()
LOGS=()
N=1

# ── 1. Desktop egui (native — macOS/Linux) ────────────────────────

if [ "$SKIP_EGUI" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Desktop egui (native --release)..."
    cargo build -p zipherx-gui --release > "${LOG_DIR}/egui.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Desktop egui (native)")
    LOGS+=("${LOG_DIR}/egui.log")
    N=$((N + 1))
fi

# ── 2. Android ──────────────────────────────────────────────────

if [ "$SKIP_ANDROID" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Android (Kotlin — arm64-v8a + x86_64)..."
    ./scripts/build-android.sh > "${LOG_DIR}/android.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Android")
    LOGS+=("${LOG_DIR}/android.log")
    N=$((N + 1))
fi

# ── 3. Windows egui (cross-compile) ─────────────────────────────
# NOTE: Windows cross-compile from macOS/Linux may fail because OpenSSL's
# build system requires a Windows-compatible Perl (Strawberry Perl).
# The rusqlite bundled-sqlcipher-vendored-openssl feature triggers this.
# Build on actual Windows or Windows CI if this fails.

if [ "$SKIP_WINDOWS" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Windows egui (x86_64-pc-windows-msvc)..."
    echo "    NOTE: May fail on macOS — OpenSSL requires Windows Perl for cross-compile"
    cargo xwin build -p zipherx-gui --release --target x86_64-pc-windows-msvc > "${LOG_DIR}/egui-windows.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Windows egui")
    LOGS+=("${LOG_DIR}/egui-windows.log")
    N=$((N + 1))
fi

# ── 4. Linux egui (cross-compile via `cross`) ───────────────────
# Only runs when NOT on Linux (on Linux the native build covers it)

if [ "$SKIP_LINUX" = false ]; then
    echo "  [${N}/${PLATFORM_COUNT}] Linux egui (x86_64-unknown-linux-gnu via cross)..."
    cross build -p zipherx-gui --release --target x86_64-unknown-linux-gnu > "${LOG_DIR}/egui-linux.log" 2>&1 &
    PIDS+=($!)
    NAMES+=("Linux egui")
    LOGS+=("${LOG_DIR}/egui-linux.log")
    N=$((N + 1))
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

# Desktop egui (native)
EGUI_BIN="target/release/zipherx-gui"
[ -f "$EGUI_BIN" ] && echo "  Desktop egui:      ${EGUI_BIN} ($(du -h "$EGUI_BIN" | cut -f1))"

# Desktop egui (Windows)
EGUI_WIN="target/x86_64-pc-windows-msvc/release/zipherx-gui.exe"
[ -f "$EGUI_WIN" ] && echo "  Windows egui:      ${EGUI_WIN} ($(du -h "$EGUI_WIN" | cut -f1))"

# Desktop egui (Linux)
EGUI_LINUX="target/x86_64-unknown-linux-gnu/release/zipherx-gui"
[ -f "$EGUI_LINUX" ] && echo "  Linux egui:        ${EGUI_LINUX} ($(du -h "$EGUI_LINUX" | cut -f1))"

# Android
ANDROID_ARM="platforms/android/src/main/jniLibs/arm64-v8a/libzipherx_ffi.so"
[ -f "$ANDROID_ARM" ] && echo "  Android arm64:     ${ANDROID_ARM} ($(du -h "$ANDROID_ARM" | cut -f1))"
ANDROID_X86="platforms/android/src/main/jniLibs/x86_64/libzipherx_ffi.so"
[ -f "$ANDROID_X86" ] && echo "  Android x86_64:    ${ANDROID_X86} ($(du -h "$ANDROID_X86" | cut -f1))"

# Bindings
KT_DIR="platforms/android/src/main/kotlin/uniffi/zipherx"
[ -d "$KT_DIR" ] && echo "  Kotlin bindings:   ${KT_DIR}/"

echo ""

# ── Failures ────────────────────────────────────────────────────

if [ $FAILED -gt 0 ]; then
    echo "── Failed Builds (last 10 lines) ──"
    echo ""
    for i in "${!FAILED_NAMES[@]}"; do
        name="${FAILED_NAMES[$i]}"
        for j in "${!NAMES[@]}"; do
            if [ "${NAMES[$j]}" = "$name" ]; then
                echo "  ${name}:"
                tail -10 "${LOGS[$j]}" | sed 's/^/    /'
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
[ "$SKIP_EGUI" = false ]    && echo "  Desktop (egui):     ./target/release/zipherx-gui"
[ "$SKIP_ANDROID" = false ] && echo "  Android:            open -a 'Android Studio' platforms/android -> Run"
[ "$SKIP_WINDOWS" = false ] && echo "  Windows (egui):     Copy target/x86_64-pc-windows-msvc/release/zipherx-gui.exe to Windows"
[ "$SKIP_LINUX" = false ]   && echo "  Linux (egui):       Copy target/x86_64-unknown-linux-gnu/release/zipherx-gui to Linux"
echo ""
