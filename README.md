```
 ________ _       _               __   __   __  __       _ _   _
|___  /(_)| |__  | |__   ___ _ __\ \ / /  |  \/  |_   _| | |_(_)
   / / | || '_ \ | '_ \ / _ \ '__|\   /   | |\/| | | | | | __| |
  / /_ | || |_) || | | |  __/ |   / . \   | |  | | |_| | | |_| |
 /____||_|| .__/ |_| |_|\___|_|  /_/ \_\  |_|  |_|\__,_|_|\__|_|
           |_|
```

# ZipherX Multi

**Your keys. Your coins. Your privacy. No exceptions.**

ZipherX Multi is a non-custodial, multi-platform Zclassic (ZCL) shielded wallet built for people who believe financial privacy is a human right -- not a feature request.

No central servers. No data collection. No intermediaries. No compromises.

---

## What is this?

A wallet that connects *directly* to the Zclassic peer-to-peer network using **Sapling shielded transactions** (zk-SNARKs). Your private keys never leave your device. Nobody -- not us, not your ISP, not your government -- can see your balance or transaction history.

**Runs on everything:** macOS, Windows, Linux, Android, iOS.

One Rust core. Native UIs everywhere. Built with [UniFFI](https://mozilla.github.io/uniffi-rs/) because we don't believe in Electron.

## Why?

> *"Privacy is necessary for an open society in the electronic age. Privacy is not secrecy. A private matter is something one doesn't want the whole world to know, but a secret matter is something one doesn't want anybody to know. Privacy is the power to selectively reveal oneself to the world."*
>
> -- Eric Hughes, *A Cypherpunk's Manifesto* (1993)

Because every transaction you make on a transparent blockchain is a confession. Because "I have nothing to hide" is the argument of someone who has never been targeted. Because Satoshi didn't invent Bitcoin so banks could track you better than before.

ZipherX Multi exists because **privacy is the default, not the option.**

---

## Features

| | |
|---|---|
| **Sapling shielded transactions** | Full zk-SNARK privacy. Your balance and transactions are cryptographically hidden. |
| **Transparent addresses** | BIP-44 transparent (t-address) support. Send and receive on both shielded and transparent pools. |
| **Non-custodial** | Your keys live on YOUR device. We can't touch your funds. Nobody can. |
| **Direct P2P** | Connects to Zclassic nodes directly. No central server to subpoena, hack, or shut down. |
| **Tor built-in** | Optional onion routing. Hide your IP from the network itself. |
| **Boost sync** | Fast initial sync from commitment tree snapshots. No waiting days for a full chain download. |
| **Unified backup/export** | Export recovery phrase (recommended), individual shielded key, or all funded transparent WIF keys. |
| **WIF import** | Import transparent private keys (WIF format) from other wallets. Paste multiple keys at once. |
| **Biometric auth** | Face ID / Touch ID / fingerprint to protect key export and dangerous operations. |
| **Hardware-backed encryption** | Keys stored in Secure Enclave (Apple), StrongBox Keystore (Android), or encrypted file (Desktop). |
| **Screenshot protection** | Optional screen capture blocking on mobile. |
| **Security audit** | Built-in audit report showing your wallet's security posture. |
| **Full Node mode** | Optional local `zclassicd` integration — validate blocks yourself, trust no one. |
| **Native desktop** | egui-based native binary. No JVM, no Electron. One binary, runs everywhere. |
| **Auto-sync** | Autonomous background sync detects new blocks even while screen is locked. |
| **Network recovery** | Automatic reconnection and retry when network connectivity drops. |

---

## Quick Start

### 1. Get the code

```bash
git clone https://github.com/ZipherPunk/zipherx_multi.git
cd zipherx_multi
```

### 2. Build for your platform

**Desktop -- Native egui (macOS / Windows / Linux):**
```bash
cargo build --release -p zipherx-gui
# Binary: target/release/zipherx-gui
```

No JVM. No Electron. No runtime dependencies. One native binary.

**Android:**
```bash
./scripts/build-android.sh
# Open platforms/android in Android Studio -> Run
```

**iOS / macOS (SwiftUI):**
```bash
./scripts/build-macos.sh        # macOS
./scripts/build-ios-sim.sh      # iOS Simulator
open platforms/apple/ZipherXApp.xcodeproj   # Cmd+R
```

**CLI (for the true cypherpunks):**
```bash
cargo run -p zipherx-cli
```

**All platforms at once:**
```bash
./scripts/distribute.sh
```

### 3. Use it

See the **[User Guide](USER_GUIDE.md)** for the full walkthrough.

**TL;DR:** Launch -> Accept disclaimer -> Create wallet (or restore from 24 words) -> Set password -> Sync -> You're private.

---

## Architecture

```
                    +------------------+
                    |    ZipherX Multi |
                    +--------+---------+
                             |
       +----------+----------+----------+----------+
       |          |          |          |           |
  +----+----+ +---+----+ +--+---+ +----+----+ +---+---+
  | SwiftUI | |  Jetpack| | egui | |  CLI   | | Full  |
  | iOS /   | | Compose | |Desktop| |Terminal| | Node  |
  | macOS   | | Android | |Native | |        | |       |
  +----+----+ +---+----+ +--+---+ +----+----+ +---+---+
       |          |          |          |           |
       +-----+----+    +----+----+-----+           |
             |         |              |             |
        +----+----+    |         +----+----+        |
        | UniFFI  |    |         | Direct  |        |
        | Bridge  |    |         | Rust    |        |
        +----+----+    |         +----+----+        |
             |         |              |             |
       +-----+---------+--------------+-------------+
       |                Rust Core                    |
       |                                             |
       |  zipherx-core     Sync, send, scan          |
       |  zipherx-crypto   Sapling, trees, proofs    |
       |  zipherx-network  P2P, headers, blocks      |
       |  zipherx-storage  Encrypted SQLite           |
       |  zipherx-tor      Tor client                 |
       |  zipherx-ffi      FFI bridge (mobile/CLI)    |
       +---------------------------------------------+
```

**egui Desktop:** Native binary calling Rust core directly (no FFI bridge, no JVM). Single binary, fastest possible.

**SwiftUI / Jetpack Compose:** Native mobile UIs via UniFFI bridge.

**Full Node:** Optional integration with a local `zclassicd` daemon for maximum sovereignty.

**Why Rust?** Because memory safety isn't optional when you're handling private keys. Because we wanted one codebase that runs everywhere without garbage collection pauses. Because cypherpunks write Rust.

---

## Prerequisites

| Platform | Requirements |
|----------|-------------|
| **All** | [Rust](https://rustup.rs/) (stable) |
| **egui Desktop** | Nothing else. Just Rust. That's the point. |
| **Android** | Android SDK + NDK, `cargo install cargo-ndk` |
| **iOS/macOS** | Xcode 15+, `xcodegen` |
| **Windows** (cross-compile) | `cargo install cargo-xwin`, `brew install mingw-w64` |
| **Linux Desktop** (from macOS) | Docker Desktop |

---

## Testing

```bash
# Per-crate tests (avoids feature conflicts)
cargo test -p zipherx-platform
cargo test -p zipherx-crypto
cargo test -p zipherx-storage
cargo test -p zipherx-network
cargo test -p zipherx-core
cargo test -p zipherx-ffi
```

---

## Security

ZipherX Multi takes security seriously:

- **Non-custodial**: Private keys stored on-device with hardware-backed encryption
- **Zero telemetry**: No analytics, no tracking, no data collection, no phone home
- **Tor integration**: Route all traffic through the Tor network
- **Open source**: Every line of code is auditable
- **Hardened runtime**: macOS builds use Hardened Runtime + App Sandbox
- **Biometric gating**: Key export, key import, and destructive operations require biometric/password authentication
- **Zeroizing secrets**: Spending keys wrapped in `Zeroizing<Vec<u8>>` for automatic secure zeroing on drop, including panic paths
- **Encrypted storage**: SQLCipher (AES-256) for database, AES-256-GCM for key files, Argon2id for key derivation
- **Input validation**: WIF decode validates checksum, version byte, compression flag, and length bounds

**Found a vulnerability?** Open a GitHub issue or contact us responsibly. We take every report seriously.

---

## Distribution

```bash
# Build, test, and package all platforms
./scripts/distribute.sh

# Output: dist/zipherx-VERSION/
#   zipherx-gui-macos-arm64          (egui native desktop — macOS)
#   zipherx-gui-linux-x86_64         (egui native desktop — Linux)
#   zipherx-gui-windows-x86_64.exe   (egui native desktop — Windows)
#   ZipherX-VERSION-release.apk      (Android)
#   ZipherX-VERSION-release.aab      (Android App Bundle)
#   zipherx-cli-macos-arm64          (CLI — macOS)
#   zipherx-cli-linux-x86_64         (CLI — Linux)
#   zipherx-cli-windows-x86_64.exe   (CLI — Windows)
#   SHA256SUMS.txt
```

Pre-built binaries are also available on the [Releases](https://github.com/ZipherPunk/zipherx_multi/releases) page.

---

## Contributing

PRs welcome. Read the code first. Understand the architecture. Write tests. Don't break privacy guarantees.

If you're adding a feature, ask yourself: *"Does this respect the user's sovereignty over their own money?"* If the answer is anything but an unequivocal yes, don't submit it.

---

## License

[MIT License](LICENSE) -- because freedom means freedom.

---

## Disclaimer

**Read the full [DISCLAIMER](DISCLAIMER.md) and [LEGAL NOTICE](LEGAL_NOTICE.md) before using this software.**

ZipherX Multi is provided "as is" without warranty of any kind. You are solely responsible for your use of this software and for securing your private keys.

**This is beta software. Do not use with funds you cannot afford to lose.**

---

<p align="center">

*Built by cypherpunks, for cypherpunks.*

*We write code. We don't keep logs.*

</p>

---

> *"Cypherpunks write code. We know that someone has to write software to defend privacy, and since we can't get privacy unless we all do, we're going to write it."*
>
> -- Eric Hughes, 1993
