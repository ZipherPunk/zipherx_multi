# ZipherX Multi -- User Guide

> *"We must defend our own privacy if we expect to have any."*
> -- Eric Hughes, 1993

Welcome to ZipherX Multi. This guide will get you from zero to private in minutes.

---

## Table of Contents

- [First Launch](#first-launch)
- [Create a New Wallet](#create-a-new-wallet)
- [Restore a Wallet](#restore-a-wallet)
- [Syncing](#syncing)
- [Receiving ZCL](#receiving-zcl)
- [Sending ZCL](#sending-zcl)
- [Tor -- Hide Your IP](#tor----hide-your-ip)
- [Settings & Security](#settings--security)
- [Backup -- Read This or Cry Later](#backup----read-this-or-cry-later)
- [Peer Management](#peer-management)
- [Export Private Key](#export-private-key)
- [Delete Everything](#delete-everything)
- [CLI Mode](#cli-mode)
- [Troubleshooting](#troubleshooting)
- [Glossary](#glossary)

---

## First Launch

When you first open ZipherX Multi, you'll see a disclaimer. **Read it.** Scroll all the way to the bottom (the Accept button won't activate until you do -- we know the tricks).

Accept it, and you're in.

---

## Create a New Wallet

1. Tap/click **"Create New Wallet"**
2. You'll see a **24-word recovery phrase** (also called a mnemonic or seed phrase)

**STOP. THIS IS THE MOST IMPORTANT STEP.**

3. Write those 24 words down. On paper. With a pen. In order.
4. **Do NOT screenshot them.** Do NOT paste them in Notes. Do NOT email them to yourself. Do NOT store them in iCloud/Google Drive/Dropbox. Do NOT tattoo them on your arm (seriously).
5. Store that paper somewhere safe. A fireproof safe. A bank safety deposit box. Your paranoid uncle's bunker.
6. Verify the words when prompted
7. Set a password to encrypt your wallet on disk

**If you lose your 24 words, your money is gone forever. Not "call support" gone. Not "reset password" gone. Gone-gone. Entropy-of-the-universe gone.**

---

## Restore a Wallet

Already have a wallet? Got those 24 words?

1. Tap/click **"Restore Wallet"**
2. Enter your 24-word recovery phrase (in the correct order)
3. Set a password
4. The wallet will sync from the beginning of the Sapling era -- this takes a few minutes thanks to boost sync

Your balance and transaction history will appear as sync progresses.

---

## Syncing

ZipherX Multi syncs directly with the Zclassic peer-to-peer network. No central server. Here's what happens:

1. **Boost sync** -- Downloads a compact commitment tree snapshot (fast, gets you up to speed quickly)
2. **Header sync** -- Downloads and validates block headers from peers
3. **Block scan** -- Scans blocks for transactions belonging to your wallet using your viewing key
4. **Tree building** -- Reconstructs the Sapling commitment tree for witness computation

**First sync:** A few minutes with boost sync enabled (default).

**Subsequent syncs:** Usually seconds, only scanning new blocks since your last sync.

The sync phase is shown in the status bar. You can also see the current sync phase in **Settings > About**.

### Auto-sync after sending

After you send a transaction, ZipherX Multi automatically syncs every 30 seconds (up to 3 minutes) to detect confirmation. You'll see a banner showing the pending transaction status.

---

## Receiving ZCL

1. Go to the **Balance** or **Receive** screen
2. Your **shielded address** (starts with `zs1...`) is displayed
3. Tap/click to copy it
4. Share it with whoever is sending you ZCL

That's it. When someone sends ZCL to your address, it'll appear after the next sync.

**Pro tip:** Your shielded address is safe to share publicly. Unlike transparent addresses, nobody can look up your balance or transaction history from it. That's the whole point.

---

## Sending ZCL

1. Go to the **Send** screen
2. Enter the recipient's shielded address (`zs1...`)
3. Enter the amount (tap **MAX** to send everything minus the fee)
4. The network fee is 10,000 zatoshis (0.0001 ZCL) -- displayed automatically
5. Review the details
6. Hit **Send**

The first send of each session loads the Sapling prover parameters (~50MB, cached on disk after first download). After that, sends are instant.

**Sending is IRREVERSIBLE.** Double-check the address. Triple-check it. There is no undo button. There is no customer support. There is no chargeback. If you send to the wrong address, those coins are gone.

---

## Tor -- Hide Your IP

By default, ZipherX Multi connects directly to Zclassic peers. Your IP address is visible to the nodes you connect to. If that bothers you (it should), enable Tor:

1. Go to **Settings**
2. Toggle **Tor** on
3. Wait for the connection (you'll see the state: CONNECTING -> BOOTSTRAPPING -> CONNECTED)
4. Once connected, your onion address is displayed
5. All traffic now routes through the Tor network

**Tor states:**
- **DISCONNECTED** -- Tor is off (red)
- **CONNECTING** -- Establishing Tor circuit (yellow)
- **BOOTSTRAPPING** -- Almost there (orange)
- **CONNECTED** -- You're anonymous (green)

**Note:** Tor is slower than direct connections. Sync will take longer. That's the price of network-level privacy. Worth it.

---

## Settings & Security

### Connected Peers

The green/red dot next to the peer count tells you if you're connected to the network:
- **Green dot + number**: You're connected to X peers
- **Red dot + 0**: No connection. Check your internet.

### Screenshot Protection

Toggle this on to prevent screen capture on mobile. Useful if you don't want someone looking over your shoulder to snap your balance.

### Security Audit Report

Tap **"Security Audit Report"** to see a real-time assessment of your wallet's security posture:

- Is the database encrypted?
- Are keys stored in hardware-backed storage (Secure Enclave / StrongBox)?
- Is biometric authentication available?
- Is Tor enabled?
- How many peers are connected?
- Is screenshot protection on?

Green = good. Red = fix it.

---

## Backup -- Read This or Cry Later

Your wallet's security boils down to one thing: **your 24-word recovery phrase.**

### The Rules

1. **Write it on paper.** Physical paper. Analog. Old school.
2. **Store it offline.** No cloud. No phone. No computer.
3. **Make copies.** Store them in different physical locations.
4. **Tell no one.** Not your best friend. Not your spouse. Not your dog. (Okay, your dog is fine. Dogs can't read.)
5. **Test your backup.** Restore from the 24 words on a different device to verify it works.

### What happens if you lose your 24 words?

Your funds become a permanent, irrecoverable donation to the void. No one can help you. Not us. Not God. Not even quantum computers (probably).

### What happens if someone else gets your 24 words?

They have your money now. All of it. Immediately.

---

## Peer Management

ZipherX Multi connects to multiple Zclassic nodes for reliability. In Settings, expand **Peer Management** to see:

- **Connected peers** -- currently active connections
- **Banned peers** -- nodes that misbehaved (sent bad data, timed out repeatedly)

You can:
- **Add a custom peer** -- connect to a specific node by IP:port
- **Disconnect a peer** -- drop a specific connection
- **Unban a peer** -- give a banned node another chance

Banned peers show their ban duration (permanent or time-remaining).

---

## Export Private Key

Need your raw private key (spending key)?

1. Go to **Settings > Security > Export Private Key**
2. Authenticate with biometrics (Face ID / Touch ID / fingerprint)
3. The key is displayed in encoded form (truncated for safety)
4. Tap **Copy** to copy the full key
5. **The clipboard auto-clears after 30 seconds**
6. The key display auto-dismisses after 60 seconds

**WARNING:** Your private key is equivalent to your 24-word recovery phrase. Anyone who has it controls your funds. Handle with extreme care.

---

## Delete Everything

Nuclear option. In **Settings > Danger Zone > Delete All Data**:

1. Authenticate with biometrics
2. Confirm the deletion dialog
3. All wallet data, keys, and app data are permanently erased
4. The app closes

This is irreversible. Make sure you have your 24-word backup before doing this.

---

## CLI Mode

For terminal enthusiasts:

```bash
cargo run -p zipherx-cli
```

The CLI provides the same core functionality as the GUI: create wallet, restore, sync, send, receive, check balance. No mouse required.

---

## Troubleshooting

### "Sync is stuck"
- Check your internet connection
- Check the peer count in Settings (should be > 0)
- Try stopping and restarting sync
- Enable/disable Tor

### "Balance seems wrong"
- Let sync complete fully (check sync phase in Settings > About)
- Try **Repair Database** in Settings > Maintenance
- As a last resort, try **Full Rescan** (re-scans the entire chain)

### "Send failed"
- Make sure you have enough balance (amount + 0.0001 ZCL fee)
- Make sure the recipient address is a valid shielded address (`zs1...`)
- Check that sync is complete (pending sync can cause stale witnesses)

### "Can't connect to peers"
- Check your firewall (port 8033 for Zclassic mainnet)
- If using Tor, wait for the connection to establish
- Try adding a known peer manually in Peer Management

### "App is slow on first send"
- The first send loads Sapling prover parameters (~50MB). This is normal and only happens once per session. After the first sync completes, the prover is pre-loaded in the background.

---

## Glossary

| Term | Meaning |
|------|---------|
| **Shielded address** | A `zs1...` address. Transactions to/from it are private (encrypted on-chain). |
| **Sapling** | The privacy protocol used by Zclassic. Uses zk-SNARKs (zero-knowledge proofs). |
| **zk-SNARK** | Zero-Knowledge Succinct Non-Interactive Argument of Knowledge. Proves you own funds without revealing which funds. Math magic. |
| **Recovery phrase** | 24 words that encode your wallet. Back it up. Seriously. |
| **Spending key** | The cryptographic key that authorizes transactions. Derived from your recovery phrase. |
| **Viewing key** | A key that can see your transactions but NOT spend your funds. Useful for watch-only wallets. |
| **Non-custodial** | You hold your own keys. No third party can access or freeze your funds. |
| **Boost sync** | Fast initial sync using a pre-computed commitment tree snapshot. |
| **Commitment tree** | A Merkle tree of all Sapling note commitments on the blockchain. Required for creating transactions. |
| **Nullifier** | A unique identifier revealed when spending a note. Prevents double-spending without revealing which note was spent. |
| **Witness** | A Merkle path proving your note exists in the commitment tree. Required for spending. |
| **Tor** | The Onion Router. Routes your traffic through multiple relays to hide your IP address. |
| **Zatoshi** | The smallest unit of ZCL. 1 ZCL = 100,000,000 zatoshis. Like satoshis for Bitcoin. |

---

> *"Privacy in an open society requires anonymous transaction systems. An anonymous system empowers individuals to reveal their identity when desired and only when desired; this is the essence of privacy."*
>
> -- Eric Hughes, *A Cypherpunk's Manifesto* (1993)

---

**Questions? Issues? Feature requests?** Open an issue on [GitHub](https://github.com/ZipherPunk/zipherx_multi/issues).

**Found a security vulnerability?** Please report it responsibly via GitHub.

---

*Stay private. Stay free. Stay punk.*
