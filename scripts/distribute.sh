#!/bin/bash
set -euo pipefail
trap 'echo "Build interrupted"; exit 1' INT TERM

# ╔══════════════════════════════════════════════════════════════╗
# ║          ZipherX — Build, Test & Distribute                  ║
# ║                                                              ║
# ║  Builds all platforms, runs tests, packages distributable    ║
# ║  artifacts, and collects them in a single output directory.  ║
# ║                                                              ║
# ║  Usage: ./scripts/distribute.sh [OPTIONS]                    ║
# ║    --skip-android     Skip Android build                     ║
# ║    --skip-windows     Skip Windows cross-compile             ║
# ║    --skip-ios         Skip iOS build                         ║
# ║    --skip-linux       Skip Linux desktop (Docker) build      ║
# ║    --skip-tests       Skip Rust cargo tests                  ║
# ║    --release-build    Sign macOS DMG (requires identity)     ║
# ║    --help             Show this help                         ║
# ╚══════════════════════════════════════════════════════════════╝

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${REPO_ROOT}"

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/' || echo "0.1.0")
if ! echo "${VERSION}" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$'; then
    echo "FATAL: Invalid version string: ${VERSION}"
    exit 1
fi
DIST_DIR="${REPO_ROOT}/dist/zipherx-${VERSION}"
# NOTE: Fixed LOG_DIR path used for easy log review between runs.
# On shared/multi-user systems, verify ownership to prevent symlink attacks.
LOG_DIR="/tmp/zipherx-dist"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)

SKIP_ANDROID=false
SKIP_WINDOWS=false
SKIP_IOS=false
SKIP_LINUX=false
SKIP_TESTS=false
RELEASE_BUILD=false

for arg in "$@"; do
    case "$arg" in
        --skip-android) SKIP_ANDROID=true ;;
        --skip-win|--skip-windows) SKIP_WINDOWS=true ;;
        --skip-ios) SKIP_IOS=true ;;
        --skip-linux) SKIP_LINUX=true ;;
        --skip-tests) SKIP_TESTS=true ;;
        --release-build) RELEASE_BUILD=true ;;
        --help|-h)
            echo "Usage: ./scripts/distribute.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --skip-android     Skip Android build (requires cargo-ndk + NDK)"
            echo "  --skip-windows     Skip Windows cross-compile (requires cargo-xwin)"
            echo "  --skip-ios         Skip iOS build"
            echo "  --skip-linux       Skip Linux desktop build (Docker)"
            echo "  --skip-tests       Skip Rust cargo tests"
            echo "  --release-build    Sign macOS DMG (requires signing identity)"
            echo "  --help             Show this help"
            exit 0
            ;;
    esac
done

mkdir -p "${DIST_DIR}" "${LOG_DIR}"
if [ "$(stat -f '%u' "${LOG_DIR}" 2>/dev/null || stat -c '%u' "${LOG_DIR}" 2>/dev/null)" != "$(id -u)" ]; then
    LOG_DIR="$(mktemp -d /tmp/zipherx-dist.XXXXXX)"
    echo "Warning: /tmp/zipherx-dist owned by another user, using ${LOG_DIR}"
fi

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║            ZipherX Distribution Builder v${VERSION}              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "  Version:    ${VERSION}"
echo "  Output:     ${DIST_DIR}/"
echo "  Logs:       ${LOG_DIR}/"
echo ""

START_TIME=$(date +%s)
STEP=0
TOTAL_STEPS=0
ERRORS=()

# Count steps
TOTAL_STEPS=$((TOTAL_STEPS + 1))  # egui native desktop (always)
[ "$SKIP_TESTS" = false ] && TOTAL_STEPS=$((TOTAL_STEPS + 1))
TOTAL_STEPS=$((TOTAL_STEPS + 1))  # macOS desktop (always)
[ "$SKIP_IOS" = false ] && TOTAL_STEPS=$((TOTAL_STEPS + 1))
[ "$SKIP_ANDROID" = false ] && TOTAL_STEPS=$((TOTAL_STEPS + 1))
TOTAL_STEPS=$((TOTAL_STEPS + 1))  # Native CLI
[ "$SKIP_LINUX" = false ] && TOTAL_STEPS=$((TOTAL_STEPS + 1))  # Linux desktop (Docker)
[ "$SKIP_WINDOWS" = false ] && TOTAL_STEPS=$((TOTAL_STEPS + 1))
TOTAL_STEPS=$((TOTAL_STEPS + 1))  # Desktop packaging
TOTAL_STEPS=$((TOTAL_STEPS + 1))  # Collect & verify

# ╔══════════════════════════════════════════════════════════════╗
# ║  SECURITY: Cargo dependency audit                             ║
# ╚══════════════════════════════════════════════════════════════╝

echo ">>> Running cargo audit..."
if ! command -v cargo-audit &>/dev/null; then
    echo "FATAL: cargo-audit is required for release builds."
    echo "Install: cargo install cargo-audit"
    exit 1
fi
if ! cargo audit --deny warnings 2>&1; then
    echo "FATAL: cargo audit found vulnerabilities. Fix before release."
    exit 1
fi
echo "  [OK] No known vulnerabilities found"
echo ""

# ╔══════════════════════════════════════════════════════════════╗
# ║  SECURITY: Strip sensitive data from distribution            ║
# ╚══════════════════════════════════════════════════════════════╝

# Ensure NO personal/sensitive data is included in any artifact
SENSITIVE_PATTERNS=(
    "*.env"
    "*.pem"
    "*.key"
    "*.keystore"
    "*.jks"
    "*.p12"
    "*.mobileprovision"
    "local.properties"
    "credentials*"
    "*.secret"
    "*.password"
    "debug.keystore"
)

check_no_sensitive_files() {
    local dir="$1"
    local found=false
    for pattern in "${SENSITIVE_PATTERNS[@]}"; do
        if find "$dir" -name "$pattern" 2>/dev/null | grep -q .; then
            echo "  [!!] SENSITIVE FILE FOUND in distribution: $(find "$dir" -name "$pattern")"
            found=true
        fi
    done
    if [ "$found" = true ]; then
        echo ""
        echo "  ABORTING: Sensitive files detected in distribution output!"
        echo "  Remove them before distributing."
        return 1
    fi
    return 0
}

# ══════════════════════════════════════════════════════════════
#  STEP: Build egui Desktop (native binary — macOS/Linux/Windows)
# ══════════════════════════════════════════════════════════════

STEP=$((STEP + 1))
echo "── [${STEP}/${TOTAL_STEPS}] Building egui Desktop (native) ──"
echo ""

# macOS (native)
if cargo build --release -p zipherx-gui > "${LOG_DIR}/egui-macos.log" 2>&1; then
    GUI_BIN="target/release/zipherx-gui"
    if [ -f "${GUI_BIN}" ]; then
        case "$(uname -s)" in
            Darwin) GUI_SUFFIX="macos-arm64" ;;
            Linux)  GUI_SUFFIX="linux-x86_64" ;;
            *)      GUI_SUFFIX="$(uname -s | tr '[:upper:]' '[:lower:]')" ;;
        esac
        cp "${GUI_BIN}" "${DIST_DIR}/zipherx-gui-${GUI_SUFFIX}"
        strip "${DIST_DIR}/zipherx-gui-${GUI_SUFFIX}" 2>/dev/null || true
        echo "  [OK] egui: zipherx-gui-${GUI_SUFFIX} ($(du -h "${GUI_BIN}" | cut -f1))"
    fi
else
    echo "  [!!] egui build failed — see ${LOG_DIR}/egui-macos.log"
    ERRORS+=("egui Desktop")
fi

# Windows cross-compile (if cargo-xwin available)
if [ "$SKIP_WINDOWS" = false ] && command -v cargo-xwin &>/dev/null; then
    echo "  Cross-compiling egui for Windows..."
    if cargo xwin build --release -p zipherx-gui --target x86_64-pc-windows-msvc > "${LOG_DIR}/egui-windows.log" 2>&1; then
        WIN_GUI="target/x86_64-pc-windows-msvc/release/zipherx-gui.exe"
        if [ -f "${WIN_GUI}" ]; then
            cp "${WIN_GUI}" "${DIST_DIR}/zipherx-gui-windows-x86_64.exe"
            echo "  [OK] egui: zipherx-gui-windows-x86_64.exe ($(du -h "${WIN_GUI}" | cut -f1))"
        fi
    else
        echo "  [--] egui Windows cross-compile failed — see ${LOG_DIR}/egui-windows.log"
    fi
fi

echo ""

# ══════════════════════════════════════════════════════════════
#  STEP: Cargo Tests
# ══════════════════════════════════════════════════════════════

if [ "$SKIP_TESTS" = false ]; then
    STEP=$((STEP + 1))
    echo "── [${STEP}/${TOTAL_STEPS}] Running Rust tests ──"
    echo ""

    if cargo test \
        -p zipherx-platform \
        -p zipherx-crypto \
        -p zipherx-network \
        -p zipherx-storage \
        -p zipherx-core \
        -p zipherx-ffi \
        -p zipherx-tor \
        -p zipherx-cli \
        --release > "${LOG_DIR}/tests.log" 2>&1; then
        PASSED=$(grep -c "^test .* ok$" "${LOG_DIR}/tests.log" 2>/dev/null || echo "?")
        echo "  [OK] All tests passed (${PASSED} tests)"
    else
        echo "  [!!] Tests FAILED — see ${LOG_DIR}/tests.log"
        echo ""
        echo "  Last 10 lines:"
        tail -10 "${LOG_DIR}/tests.log" | sed 's/^/    /'
        echo ""
        echo "  Fix failing tests before distributing."
        exit 1
    fi
    echo ""
fi

# ══════════════════════════════════════════════════════════════
#  STEP: Build macOS Rust FFI + Desktop GUI
# ══════════════════════════════════════════════════════════════

STEP=$((STEP + 1))
echo "── [${STEP}/${TOTAL_STEPS}] Building macOS Rust FFI ──"
echo ""

if ./scripts/build-macos.sh > "${LOG_DIR}/macos-ffi.log" 2>&1; then
    echo "  [OK] macOS FFI (aarch64-apple-darwin)"
else
    echo "  [!!] macOS FFI build failed — see ${LOG_DIR}/macos-ffi.log"
    ERRORS+=("macOS FFI")
fi

# Copy dylib to desktop resources for packaging
echo "  Copying dylib to desktop resources..."
RESOURCES_DIR="${REPO_ROOT}/platforms/desktop/src/main/resources"
mkdir -p "${RESOURCES_DIR}"
MACOS_DYLIB="target/aarch64-apple-darwin/release/libzipherx_ffi.dylib"
if [ -f "${MACOS_DYLIB}" ]; then
    cp "${MACOS_DYLIB}" "${RESOURCES_DIR}/libzipherx_ffi.dylib"
    cp "${MACOS_DYLIB}" "${RESOURCES_DIR}/libuniffi_zipherx.dylib"
    echo "  [OK] dylib copied to desktop resources"
fi
echo ""

# ══════════════════════════════════════════════════════════════
#  STEP: iOS Build
# ══════════════════════════════════════════════════════════════

if [ "$SKIP_IOS" = false ]; then
    STEP=$((STEP + 1))
    echo "── [${STEP}/${TOTAL_STEPS}] Building iOS ──"
    echo ""

    # Build iOS Simulator library
    if ./scripts/build-ios-sim.sh > "${LOG_DIR}/ios-sim.log" 2>&1; then
        echo "  [OK] iOS Simulator FFI (aarch64-apple-ios-sim)"
    else
        echo "  [!!] iOS Sim build failed — see ${LOG_DIR}/ios-sim.log"
        ERRORS+=("iOS Simulator")
    fi

    # Build iOS Device library (for release distribution)
    echo "  Building iOS device target (aarch64-apple-ios)..."
    if rustup target list --installed | grep -q "aarch64-apple-ios"; then
        if IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p zipherx-ffi --release --target aarch64-apple-ios > "${LOG_DIR}/ios-device.log" 2>&1; then
            IOS_DEVICE_LIB="target/aarch64-apple-ios/release/libzipherx_ffi.a"
            if [ -f "${IOS_DEVICE_LIB}" ]; then
                IOS_DEVICE_DIR="${REPO_ROOT}/platforms/apple/Generated/lib-ios-device"
                mkdir -p "${IOS_DEVICE_DIR}"
                cp "${IOS_DEVICE_LIB}" "${IOS_DEVICE_DIR}/libzipherx_ffi.a"
                echo "  [OK] iOS Device FFI (aarch64-apple-ios)"
            fi
        else
            echo "  [--] iOS device build failed (non-fatal) — see ${LOG_DIR}/ios-device.log"
        fi
    else
        echo "  [--] iOS device target not installed — run: rustup target add aarch64-apple-ios"
    fi

    # Try Xcode archive (only if xcodebuild is available)
    XCODEPROJ="${REPO_ROOT}/platforms/apple/ZipherXApp.xcodeproj"
    if [ -d "${XCODEPROJ}" ] && command -v xcodebuild &>/dev/null; then
        echo "  Archiving iOS app..."
        IOS_ARCHIVE="${DIST_DIR}/ZipherX-iOS.xcarchive"
        if xcodebuild archive \
            -project "${XCODEPROJ}" \
            -scheme "ZipherXApp-iOS" \
            -destination "generic/platform=iOS" \
            -archivePath "${IOS_ARCHIVE}" \
            CODE_SIGN_IDENTITY="-" \
            CODE_SIGNING_ALLOWED=NO \
            > "${LOG_DIR}/ios-archive.log" 2>&1; then
            echo "  [OK] iOS archive: ${IOS_ARCHIVE}"
        else
            echo "  [--] iOS archive failed (signing required) — see ${LOG_DIR}/ios-archive.log"
            echo "       To build a signed IPA, use Xcode with your provisioning profile."
        fi
    fi
    echo ""
fi

# ══════════════════════════════════════════════════════════════
#  STEP: Android Build
# ══════════════════════════════════════════════════════════════

if [ "$SKIP_ANDROID" = false ]; then
    STEP=$((STEP + 1))
    echo "── [${STEP}/${TOTAL_STEPS}] Building Android ──"
    echo ""

    # Check prerequisites
    if ! command -v cargo-ndk &>/dev/null; then
        echo "  [--] cargo-ndk not found — skipping Android"
        SKIP_ANDROID=true
    else
        # Build Rust FFI for Android
        if ./scripts/build-android.sh > "${LOG_DIR}/android-ffi.log" 2>&1; then
            echo "  [OK] Android FFI (arm64-v8a + x86_64)"
        else
            echo "  [!!] Android FFI build failed — see ${LOG_DIR}/android-ffi.log"
            ERRORS+=("Android FFI")
        fi

        # Build release APK
        ANDROID_DIR="${REPO_ROOT}/platforms/android"
        if [ -f "${ANDROID_DIR}/gradlew" ]; then
            echo "  Building Android release APK..."
            if (cd "${ANDROID_DIR}" && ./gradlew assembleRelease) > "${LOG_DIR}/android-apk.log" 2>&1; then
                # Find the APK
                APK=$(find "${ANDROID_DIR}/build" -name "*-release*.apk" -o -name "*-release*.apk" 2>/dev/null | head -1)
                if [ -n "${APK}" ] && [ -f "${APK}" ]; then
                    cp "${APK}" "${DIST_DIR}/ZipherX-${VERSION}-release.apk"
                    echo "  [OK] APK: ZipherX-${VERSION}-release.apk ($(du -h "${APK}" | cut -f1))"
                    # Verify APK is signed
                    if command -v apksigner &>/dev/null; then
                        if ! apksigner verify "${DIST_DIR}/ZipherX-${VERSION}-release.apk" 2>/dev/null; then
                            echo "  [!!] APK is NOT signed — set ZIPHERX_KEYSTORE_* env vars for release signing"
                        else
                            echo "  [OK] APK signature verified"
                        fi
                    fi
                else
                    echo "  [--] APK not found in build output"
                fi
            else
                echo "  [--] Android APK build failed — see ${LOG_DIR}/android-apk.log"
                echo "       (May need signing config in build.gradle.kts)"
            fi

            # Try AAB (App Bundle) for Play Store
            echo "  Building Android App Bundle..."
            if (cd "${ANDROID_DIR}" && ./gradlew bundleRelease) > "${LOG_DIR}/android-aab.log" 2>&1; then
                AAB=$(find "${ANDROID_DIR}/build" -name "*-release*.aab" 2>/dev/null | head -1)
                if [ -n "${AAB}" ] && [ -f "${AAB}" ]; then
                    cp "${AAB}" "${DIST_DIR}/ZipherX-${VERSION}-release.aab"
                    echo "  [OK] AAB: ZipherX-${VERSION}-release.aab ($(du -h "${AAB}" | cut -f1))"
                fi
            else
                echo "  [--] AAB build failed — see ${LOG_DIR}/android-aab.log"
            fi
        else
            echo "  [--] No gradlew found in platforms/android"
        fi
    fi
    echo ""
fi

# ══════════════════════════════════════════════════════════════
#  STEP: Linux / Native CLI
# ══════════════════════════════════════════════════════════════

STEP=$((STEP + 1))
echo "── [${STEP}/${TOTAL_STEPS}] Building CLI binary ──"
echo ""

if ./scripts/build-linux.sh desktop > "${LOG_DIR}/linux.log" 2>&1; then
    CLI_BIN="target/release/zipherx-cli"
    if [ -f "${CLI_BIN}" ]; then
        # Determine platform suffix
        case "$(uname -s)" in
            Darwin) CLI_SUFFIX="macos-arm64" ;;
            Linux)  CLI_SUFFIX="linux-x86_64" ;;
            *)      CLI_SUFFIX="$(uname -s | tr '[:upper:]' '[:lower:]')" ;;
        esac
        cp "${CLI_BIN}" "${DIST_DIR}/zipherx-cli-${CLI_SUFFIX}"
        echo "  [OK] CLI: zipherx-cli-${CLI_SUFFIX} ($(du -h "${CLI_BIN}" | cut -f1))"
    fi
else
    echo "  [!!] CLI build failed — see ${LOG_DIR}/linux.log"
    ERRORS+=("CLI")
fi
echo ""

# ══════════════════════════════════════════════════════════════
#  STEP: Windows Cross-Compile
# ══════════════════════════════════════════════════════════════

if [ "$SKIP_WINDOWS" = false ]; then
    STEP=$((STEP + 1))
    echo "── [${STEP}/${TOTAL_STEPS}] Building Windows ──"
    echo ""

    if ! command -v cargo-xwin &>/dev/null; then
        echo "  [--] cargo-xwin not found — skipping Windows"
    else
        if ./scripts/build-windows.sh desktop > "${LOG_DIR}/windows.log" 2>&1; then
            WIN_CLI="target/x86_64-pc-windows-msvc/release/zipherx-cli.exe"
            WIN_DLL="target/x86_64-pc-windows-msvc/release/zipherx_ffi.dll"
            if [ -f "${WIN_CLI}" ]; then
                cp "${WIN_CLI}" "${DIST_DIR}/zipherx-cli-windows-x86_64.exe"
                echo "  [OK] CLI: zipherx-cli-windows-x86_64.exe ($(du -h "${WIN_CLI}" | cut -f1))"
            fi
            if [ -f "${WIN_DLL}" ]; then
                cp "${WIN_DLL}" "${DIST_DIR}/zipherx_ffi-windows-x86_64.dll"
                echo "  [OK] DLL: zipherx_ffi-windows-x86_64.dll ($(du -h "${WIN_DLL}" | cut -f1))"
            fi
        else
            echo "  [!!] Windows build failed — see ${LOG_DIR}/windows.log"
            ERRORS+=("Windows")
        fi
    fi
    echo ""
fi

# ══════════════════════════════════════════════════════════════
#  STEP: Linux Desktop GUI (via Docker)
# ══════════════════════════════════════════════════════════════

if [ "$SKIP_LINUX" = false ]; then
    STEP=$((STEP + 1))
    echo "── [${STEP}/${TOTAL_STEPS}] Building Linux Desktop GUI (Docker) ──"
    echo ""

    if ! command -v docker &>/dev/null; then
        echo "  [--] Docker not found — skipping Linux desktop build"
        echo "       Install Docker Desktop or run on a Linux machine."
    elif ! docker info &>/dev/null 2>&1; then
        echo "  [--] Docker daemon not running — skipping Linux desktop build"
        echo "       Start Docker Desktop and retry."
    else
        # Build Linux FFI .so + CLI + Desktop DEB/RPM inside a container
        echo "  Building Rust FFI + CLI + Desktop packages inside Docker..."

        DOCKER_IMAGE="zipherx-linux-builder"

        # Create a Dockerfile for the build environment
        DOCKERFILE="${LOG_DIR}/Dockerfile.linux"
        cat > "${DOCKERFILE}" <<'DOCKEREOF'
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# System deps
RUN apt-get update && apt-get install -y \
    curl build-essential pkg-config libssl-dev \
    openjdk-17-jdk \
    fakeroot dpkg-dev rpm \
    && rm -rf /var/lib/apt/lists/*

# Install Rust (pinned toolchain for reproducible builds)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.77.0
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /work
DOCKEREOF

        # Build the Docker image (cached after first run)
        if docker build -t "${DOCKER_IMAGE}" -f "${DOCKERFILE}" . > "${LOG_DIR}/docker-build.log" 2>&1; then
            echo "  [OK] Docker image ready"
        else
            echo "  [!!] Docker image build failed — see ${LOG_DIR}/docker-build.log"
            ERRORS+=("Linux Docker image")
        fi

        # Run the build inside the container
        # Mount the repo read-write, output artifacts to dist/
        LINUX_DIST="/work/dist-linux"
        if docker run --rm \
            --user "$(id -u):$(id -g)" \
            -v "${REPO_ROOT}:/work" \
            -e "LINUX_DIST=${LINUX_DIST}" \
            "${DOCKER_IMAGE}" \
            bash -c '
                set -euo pipefail
                mkdir -p ${LINUX_DIST}

                echo ">>> Building Rust FFI .so (release)..."
                cargo build -p zipherx-ffi --release
                cp target/release/libzipherx_ffi.so ${LINUX_DIST}/
                cp target/release/libzipherx_ffi.so target/release/libuniffi_zipherx.so

                echo ">>> Building CLI (release)..."
                cargo build -p zipherx-cli --release
                cp target/release/zipherx-cli ${LINUX_DIST}/zipherx-cli-linux-x86_64

                echo ">>> Copying FFI to desktop resources..."
                mkdir -p platforms/desktop/src/main/resources
                cp target/release/libzipherx_ffi.so platforms/desktop/src/main/resources/
                cp target/release/libuniffi_zipherx.so platforms/desktop/src/main/resources/

                echo ">>> Packaging desktop GUI (DEB/RPM)..."
                cd platforms/desktop
                chmod +x gradlew
                ./gradlew packageDistributionForCurrentOS || echo "Desktop packaging failed (non-fatal)"
                cd /work

                # Collect DEB/RPM if produced
                find platforms/desktop/build -name "*.deb" -exec cp {} ${LINUX_DIST}/ \; 2>/dev/null || true
                find platforms/desktop/build -name "*.rpm" -exec cp {} ${LINUX_DIST}/ \; 2>/dev/null || true

                echo ">>> Linux build complete"
            ' > "${LOG_DIR}/linux-docker.log" 2>&1; then

            echo "  [OK] Linux Docker build complete"

            # Collect artifacts from the mounted dist-linux dir
            LINUX_OUT="${REPO_ROOT}/dist-linux"
            if [ -d "${LINUX_OUT}" ]; then
                # CLI
                if [ -f "${LINUX_OUT}/zipherx-cli-linux-x86_64" ]; then
                    cp "${LINUX_OUT}/zipherx-cli-linux-x86_64" "${DIST_DIR}/"
                    echo "  [OK] CLI: zipherx-cli-linux-x86_64"
                fi
                # FFI .so
                if [ -f "${LINUX_OUT}/libzipherx_ffi.so" ]; then
                    cp "${LINUX_OUT}/libzipherx_ffi.so" "${DIST_DIR}/libzipherx_ffi-linux-x86_64.so"
                    echo "  [OK] FFI: libzipherx_ffi-linux-x86_64.so"
                fi
                # DEB
                DEB=$(find "${LINUX_OUT}" -name "*.deb" 2>/dev/null | head -1)
                if [ -n "${DEB}" ] && [ -f "${DEB}" ]; then
                    cp "${DEB}" "${DIST_DIR}/ZipherX-${VERSION}-linux.deb"
                    echo "  [OK] DEB: ZipherX-${VERSION}-linux.deb ($(du -h "${DEB}" | cut -f1))"
                fi
                # RPM
                RPM=$(find "${LINUX_OUT}" -name "*.rpm" 2>/dev/null | head -1)
                if [ -n "${RPM}" ] && [ -f "${RPM}" ]; then
                    cp "${RPM}" "${DIST_DIR}/ZipherX-${VERSION}-linux.rpm"
                    echo "  [OK] RPM: ZipherX-${VERSION}-linux.rpm ($(du -h "${RPM}" | cut -f1))"
                fi
                # Clean up
                rm -rf "${LINUX_OUT}"
            fi
        else
            echo "  [!!] Linux Docker build failed — see ${LOG_DIR}/linux-docker.log"
            tail -10 "${LOG_DIR}/linux-docker.log" | sed 's/^/    /'
            ERRORS+=("Linux Desktop")
            # Clean up partial output
            rm -rf "${REPO_ROOT}/dist-linux"
        fi
    fi
    echo ""
fi

# ══════════════════════════════════════════════════════════════
#  STEP: Desktop GUI Packaging — macOS DMG (native)
# ══════════════════════════════════════════════════════════════

STEP=$((STEP + 1))
echo "── [${STEP}/${TOTAL_STEPS}] Packaging Desktop GUI ──"
echo ""

DESKTOP_DIR="${REPO_ROOT}/platforms/desktop"
if [ -f "${DESKTOP_DIR}/gradlew" ]; then
    # On macOS this produces DMG, on Linux DEB/RPM, on Windows MSI
    # Linux DEB/RPM is handled separately via Docker above
    CURRENT_OS="$(uname -s)"
    echo "  Running Gradle packageDistributionForCurrentOS (${CURRENT_OS})..."
    if (cd "${DESKTOP_DIR}" && ./gradlew packageDistributionForCurrentOS) > "${LOG_DIR}/desktop-package.log" 2>&1; then
        echo "  [OK] Desktop packaging complete"

        # DMG (macOS)
        DMG=$(find "${DESKTOP_DIR}/build" -name "*.dmg" 2>/dev/null | head -1)
        if [ -n "${DMG}" ] && [ -f "${DMG}" ]; then
            cp "${DMG}" "${DIST_DIR}/ZipherX-${VERSION}-macos.dmg"
            echo "  [OK] DMG: ZipherX-${VERSION}-macos.dmg ($(du -h "${DMG}" | cut -f1))"
        fi

        # MSI (Windows — only if running on Windows)
        MSI=$(find "${DESKTOP_DIR}/build" -name "*.msi" 2>/dev/null | head -1)
        if [ -n "${MSI}" ] && [ -f "${MSI}" ]; then
            cp "${MSI}" "${DIST_DIR}/ZipherX-${VERSION}-windows.msi"
            echo "  [OK] MSI: ZipherX-${VERSION}-windows.msi ($(du -h "${MSI}" | cut -f1))"
        fi
    else
        echo "  [!!] Desktop packaging failed — see ${LOG_DIR}/desktop-package.log"
        tail -5 "${LOG_DIR}/desktop-package.log" | sed 's/^/    /'
        ERRORS+=("Desktop packaging")
    fi
else
    echo "  [--] No gradlew in platforms/desktop — skipping GUI packaging"
fi
echo ""

# ══════════════════════════════════════════════════════════════
#  STEP: Collect, Verify & Checksum
# ══════════════════════════════════════════════════════════════

STEP=$((STEP + 1))
echo "── [${STEP}/${TOTAL_STEPS}] Verifying & creating checksums ──"
echo ""

# Security check: ensure no sensitive files leaked into dist
echo "  Scanning for sensitive files..."
if check_no_sensitive_files "${DIST_DIR}"; then
    echo "  [OK] No sensitive files found in distribution"
else
    exit 1
fi

# Strip debug symbols from CLI binaries to reduce size
echo "  Stripping debug symbols from CLI binaries..."
for bin in "${DIST_DIR}"/zipherx-cli-*; do
    if [ -f "$bin" ] && file "$bin" | grep -q "Mach-O\|ELF"; then
        strip "$bin" 2>/dev/null && echo "    Stripped: $(basename "$bin")"
    fi
done

# Generate SHA256 checksums
echo "  Generating checksums..."
(cd "${DIST_DIR}" && shasum -a 256 * 2>/dev/null > SHA256SUMS.txt)
echo "  [OK] SHA256SUMS.txt created"
echo ""

# ══════════════════════════════════════════════════════════════
#  Summary
# ══════════════════════════════════════════════════════════════

END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
MINS=$((ELAPSED / 60))
SECS=$((ELAPSED % 60))

echo "╔══════════════════════════════════════════════════════════════╗"
if [ ${#ERRORS[@]} -eq 0 ]; then
    echo "║     DISTRIBUTION COMPLETE — v${VERSION} (${MINS}m ${SECS}s)                  ║"
else
    echo "║     DISTRIBUTION PARTIAL — ${#ERRORS[@]} error(s) (${MINS}m ${SECS}s)                ║"
fi
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

echo "── Artifacts ──"
echo ""
echo "  Directory: ${DIST_DIR}/"
echo ""
if [ -d "${DIST_DIR}" ]; then
    ls -lh "${DIST_DIR}/" | tail -n +2 | sed 's/^/  /'
fi
echo ""

if [ ${#ERRORS[@]} -gt 0 ]; then
    echo "── Errors ──"
    echo ""
    for err in "${ERRORS[@]}"; do
        echo "  [!!] ${err}"
    done
    echo ""
    echo "  Check logs in ${LOG_DIR}/ for details."
    echo ""
fi

echo "── Distribution Checklist ──"
echo ""
echo "  [x] No sensitive files (keys, env, credentials) in dist/"
echo "  [x] SHA256 checksums generated"
echo "  [ ] Code-sign macOS DMG (if distributing publicly)"
echo "  [ ] Sign Android APK with release keystore"
echo "  [ ] Notarize macOS app via Apple notarytool"
echo "  [ ] Upload AAB to Google Play Console"
echo "  [ ] Archive iOS via Xcode for App Store / TestFlight"
echo ""
