# ZipherX

**Privacy-first, multi-platform Zclassic (ZCL) shielded wallet.**

ZipherX is a non-custodial wallet that connects directly to the Zclassic peer-to-peer network. No central servers. No data collection. No intermediaries. Your keys never leave your device.

## Features

- **Sapling shielded transactions** -- full zk-SNARK privacy
- **Non-custodial** -- your keys, your coins
- **Direct P2P** -- connects to Zclassic nodes directly, no central server
- **Tor support** -- optional onion routing for network-level privacy
- **Boost sync** -- fast initial sync from commitment tree snapshots
- **Cross-platform** -- macOS, Windows, Linux, Android, iOS

## Architecture

```
crates/
  zipherx-core/       Core wallet logic (sync, send, scan)
  zipherx-crypto/     Sapling cryptography, commitment tree, provers
  zipherx-network/    P2P networking, header sync, block fetcher
  zipherx-storage/    SQLite database, encrypted storage
  zipherx-ffi/        UniFFI bridge (Rust -> Kotlin/Swift)
  zipherx-tor/        Tor client integration
  zipherx-platform/   Platform abstraction layer

platforms/
  desktop/            Compose Desktop (macOS, Windows, Linux)
  android/            Android (Kotlin + Jetpack Compose)
  apple/              iOS / macOS (SwiftUI)
  cli/                Command-line interface
```

The Rust core is shared across all platforms via [UniFFI](https://mozilla.github.io/uniffi-rs/). Platform-specific UI is written in Compose (Desktop/Android) and SwiftUI (iOS/macOS).

## Prerequisites

- **Rust** (stable, via [rustup](https://rustup.rs/))
- **Platform-specific:**
  - Desktop: JDK 17+, Gradle
  - Android: Android SDK, NDK, `cargo-ndk`
  - iOS: Xcode 15+, `xcodegen`

## Building

### Desktop (macOS / Windows / Linux)

```bash
# 1. Build the Rust FFI library
./scripts/build-all.sh

# 2. Run the desktop app
cd platforms/desktop && ./gradlew run
```

### Android

```bash
# 1. Build Rust for Android targets
./scripts/build-android.sh

# 2. Build APK
cd platforms/android && ./gradlew assembleDebug
```

The APK will be at `platforms/android/build/outputs/apk/debug/`.

### iOS

```bash
# 1. Build Rust for iOS Simulator
./scripts/build-ios-sim.sh

# 2. Open in Xcode
open platforms/apple/ZipherXApp.xcodeproj
```

### CLI

```bash
cargo run -p zipherx-cli
```

## Usage

1. **First launch** -- read and accept the disclaimer
2. **Create or restore** -- generate a new wallet or restore from a 24-word recovery phrase
3. **Set a password** -- encrypts your wallet on disk
4. **Sync** -- the wallet syncs with the Zclassic network (first sync uses boost for speed)
5. **Receive** -- copy your shielded address
6. **Send** -- enter a destination address and amount

### Important

- **Back up your recovery phrase** -- write it down on paper, store it offline. If you lose it, your funds are gone forever.
- **This is beta software** -- do not use with funds you cannot afford to lose.

## Testing

```bash
# Run Rust tests (per-crate to avoid feature conflicts)
cargo test -p zipherx-platform
cargo test -p zipherx-crypto
cargo test -p zipherx-storage
cargo test -p zipherx-network
cargo test -p zipherx-core
cargo test -p zipherx-ffi
```

## Security

ZipherX is beta software. If you discover a security vulnerability, please report it responsibly.

- **Non-custodial**: Private keys are stored on-device with hardware-backed encryption (Keychain on Apple, Keystore on Android, encrypted file on Desktop)
- **No telemetry**: Zero data collection, no analytics, no tracking
- **Tor optional**: Route all network traffic through the Tor network
- **Open source**: Full source code available for audit

## License

[MIT License](LICENSE)

## Disclaimer

**Read the full [DISCLAIMER](DISCLAIMER.md) before using this software.**

ZipherX is provided "as is" without warranty of any kind. You are solely responsible for your use of this software and for securing your private keys. Do not use with funds you cannot afford to lose.

---

> *"Privacy is necessary for an open society in the electronic age."*
> -- Eric Hughes, *A Cypherpunk's Manifesto* (1993)
