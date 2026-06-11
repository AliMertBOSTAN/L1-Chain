# QuantumVault L1 User Guide

> **Last updated: 2026-06-10.** This guide matches the actual `qv-wallet`
> CLI and web UI as implemented in `crates/qv-wallet`. Run
> `qv-wallet --help` (or `qv-wallet <command> --help`) for the
> authoritative flag list of your build.

Welcome to QuantumVault, a post-quantum secure blockchain designed for fast, private, and fair transactions. This guide covers everything you need to get started and use QV safely.

## 1. Getting Started

### Installing qv-wallet

For prebuilt binaries (Windows / macOS), checksum verification, and the
guided first-run flow, follow **[docs/INSTALL.md](INSTALL.md)** — it is the
canonical installation guide and requires no Rust toolchain.

To build from source instead:

```bash
nix develop
just build
./target/release/qv-wallet --version
```

`--version` prints `<cargo_version> (<release_tag>, git <commit_hash>)`.

### Global Options

Every command accepts these global flags:

| Flag | Default | Meaning |
|------|---------|---------|
| `--keystore <path>` | `wallet.json` | Encrypted wallet file to use |
| `--rpc <url>` | `http://localhost:8080` | Node JSON-RPC endpoint |
| `--network <devnet\|local>` | (unset) | Shortcut to a well-known RPC endpoint. `devnet` = the official public devnet RPC, `local` = `http://127.0.0.1:8545`. When set, it overrides `--rpc`. |

### Creating Your Wallet

```bash
qv-wallet init
```

This:

1. Generates a fresh **24-word BIP-39 mnemonic** and prints it **once** —
   write it down on paper, offline. If it leaks, your funds can be stolen;
   if you lose it, the wallet is unrecoverable.
2. Prompts you for a keystore password (minimum 8 characters).
3. Saves the encrypted keystore (default `wallet.json`; change with
   `--out <path>` or the global `--keystore`). The keystore is encrypted
   with **Argon2id + AES-256-GCM** — the password never leaves your machine.
4. Prints the account-0 stealth address (fingerprint + full form).

Your wallet uses two key families per account:

- **Spend key** (ML-DSA, FIPS 204 — Dilithium family, Level 3): signs
  transactions. Derived deterministically from the mnemonic.
- **View key** (Kyber KEM): lets the wallet detect incoming stealth
  payments. Generated from OS randomness and stored inside the keystore —
  see the backup caveat below.

### Importing an Existing Mnemonic

```bash
qv-wallet import-mnemonic "word1 word2 ... word24"
```

Prompts for a new keystore password and saves the keystore (use `--out` for
an alternate path). Refuses to overwrite an existing keystore file.

### Devnet Quick Start

```bash
qv-wallet devnet-import
```

One-shot bootstrap: imports the well-known public devnet test mnemonic and
saves it as your keystore. The matching `qv-node --network devnet` genesis
pre-funds this wallet, so `qv-wallet balance` immediately shows a non-zero
balance. You can pass `--password <pw>` non-interactively (≥ 8 chars) and
`--out <path>` for an alternate keystore location.

**Never use this on mainnet** — the mnemonic is public; anyone can spend
those funds.

Note: devnet-imported funds sit in **plain p2pkh outputs** (visible
recipient). Your first transfer spends them into stealth outputs, after
which your funds are private by default.

You can also get test funds from the faucet via the web UI: start
`qv-wallet serve`, open the **Balance** panel, and press
**"Get devnet test funds"** (see [INSTALL.md](INSTALL.md)).

### Backing Up Your Wallet — Read This Carefully

There is no separate `backup` command. Your backup is:

1. **The 24-word mnemonic** (written down offline) — restores your spend
   key and therefore your ability to spend.
2. **The keystore file itself** (`wallet.json`) — additionally contains
   your **view keys**.

**Known limitation (C-05):** view keys are generated from OS entropy, not
derived from the mnemonic. If you restore a wallet from the mnemonic alone
(`import-mnemonic` on a new machine), the wallet derives the same spend key
but a **new** view key — stealth payments received under the old view key
**may not be detectable** by the restored wallet. To preserve full
visibility of past incoming stealth payments, also keep a copy of the
encrypted keystore file (it is safe to store as long as the password is
strong — it is Argon2id + AES-256-GCM encrypted). A deterministic view-key
derivation is planned.

---

## 2. Receiving QV

### Your QV Address

Print your account-0 receiving address:

```bash
qv-wallet address
```

Use `qv-wallet address 1` (positional argument) for account index 1, etc.

QuantumVault addresses come in two forms:

- **Full payable address** — starts with `qvst1` and encodes your Kyber
  view public key + Dilithium spend public key. Because post-quantum keys
  are large, the full address is **~6.4 KB** of encoded payload. This is
  the only form someone can actually *send to*.
- **Fingerprint** — starts with `qvfp1` followed by 40 hex characters
  (a 20-byte hash of the key material). Short and human-comparable, used
  for display and verification. **You cannot send to a fingerprint** — it
  doesn't contain the keys.

### Sharing the Full Address

Since the full address is too long to read over the phone, the wallet
offers three transports:

```bash
# 1. Save a .qvaddr JSON sidecar file — send it to your counterparty
qv-wallet address --save myaddr.qvaddr

# 2. Small ASCII QR of the fingerprint (for visual verification)
qv-wallet address --qr

# 3. Multi-part ASCII QR of the FULL address (default 2 parts; needs a
#    wide terminal). The receiver scans all parts to reassemble.
qv-wallet address --full-qr --qr-parts 2
```

### Understanding Stealth Addresses

Unlike traditional blockchains where anyone can see incoming transactions, QuantumVault hides the recipient:

1. The sender uses your **Kyber view public key** (inside your `qvst1`
   address) to derive a one-time output lock via a KEM encapsulation.
2. Only you, with your view secret key, can detect that the output is yours.
3. The blockchain shows a transaction but not who received it.

Every output created by `send-stealth` is stealth — including your own
change. No configuration needed.

### View Keys for Auditors

To prove your incoming payments (e.g., to a tax auditor) without giving up
spending power:

```bash
qv-wallet export-view-key --out audit.qvview --label "2026 tax audit"
```

The `.qvview` file contains the view keypair + spend *public* key — never
the mnemonic or spend secret. The auditor runs, with no keystore at all:

```bash
qv-wallet audit-scan --view-key audit.qvview --network devnet
```

This lists every incoming stealth UTXO (outpoint + value) the view key can
detect. The auditor **cannot spend** anything.

### Selective Disclosure (Single Payment)

To prove you received one specific payment — instead of revealing your
whole incoming history — create a self-contained proof file:

```bash
qv-wallet disclose --utxo <tx_id_hex>:<index> --out proof.qvdisclose
# add --amount <value> to also reveal the plaintext amount
```

Anyone can verify it offline, with no keystore or RPC:

```bash
qv-wallet verify-disclosure --proof proof.qvdisclose
```

---

## 3. Sending QV

### Stealth Transfer (Recommended)

```bash
qv-wallet --network devnet send-stealth --to-address qvst1... --amount 50000
```

Exactly one recipient option is required (they are mutually exclusive):

| Option | Recipient source |
|--------|------------------|
| `--to-address <qvst1...>` | Full stealth address pasted inline |
| `--to-qvaddr <path>` | A `.qvaddr` file the recipient sent you |
| `--to-contact <label>` | A saved contact from your address book |

Other flags:

- `--amount <units>` — amount in **smallest units** (integer, required)
- `--fee <units>` — transaction fee, default `1000`
- `--account <n>` — account index to spend from, default `0`

The wallet automatically scans the node for your spendable UTXOs (both
stealth and plain p2pkh), selects inputs largest-first, creates a stealth
output for the recipient and a stealth change output back to yourself,
signs everything with your ML-DSA spend key, prints a summary (inputs,
amount, change, fee, local tx id, size), and broadcasts via the node RPC.

### Address Book (Contacts)

Save frequently used recipients so you never paste a 6 KB address twice:

```bash
qv-wallet contacts add --label alice --address qvst1... --notes "work"
qv-wallet contacts list
qv-wallet contacts show --label alice
qv-wallet contacts remove --label alice

qv-wallet send-stealth --to-contact alice --amount 50000
```

The address book is stored **encrypted** (Argon2id + AES-256-GCM) next to
your keystore, under your wallet password.

### Plain Transfer (Devnet / Legacy)

`send` is the low-level, pre-stealth devnet flow — it pays to a raw
Dilithium public key with a transparent `p2pkh_pqc` output and requires
you to name the input UTXO manually:

```bash
qv-wallet send \
  --to-pubkey <recipient_dilithium_pk_hex> \
  --amount 1000 \
  --input <txid_hex>:<output_index> \
  --input-value 5000 \
  --broadcast
```

Without `--broadcast` it just prints the signed transaction hex for manual
submission via the `qv_sendTransaction` RPC. Prefer `send-stealth` for
everything new — plain outputs reveal the recipient.

### Transaction Confirmation

After sending, the wallet prints a local **transaction id** (txid) and the
RPC broadcast result. QuantumVault finalizes blocks at **k=50 depth
(~100 seconds)**; after that the transaction cannot be reversed. To see the
result land, re-run `qv-wallet balance` or `qv-wallet scan` after a few
blocks.

### Transaction Fees

Fees are set explicitly with `--fee` (default 1000 smallest units) and are
distributed to **stake pool operators** and **delegators** — there is no
burning. Higher fees get priority during high network activity.

---

## 4. Checking Your Balance and UTXOs

### Balance

```bash
qv-wallet --network devnet balance
```

Shows three lines for the account (default 0; use `--account <n>`):

- `stealth` — sum of stealth UTXOs your view key detects
- `plain` — sum of transparent p2pkh UTXOs locked to your spend key
  (e.g., fresh devnet-import funds), with UTXO count
- `total` — both combined

### Listing UTXOs

```bash
qv-wallet --network devnet scan
```

Lists every stealth and plain UTXO the wallet can spend, as
`<txid>:<index> value=<units>` lines. Optional flags: `--from`, `--to`
(height bounds), `--account`.

QuantumVault is **UTXO-based**: your balance is a set of discrete "coins".
`send-stealth` selects and combines them automatically — you normally never
need to think about individual UTXOs.

---

## 5. Privacy Features

### Default: Stealth Outputs

Every output created by `send-stealth` (payment *and* change) is stealth by
default. You get strong recipient privacy automatically.

- **Visible on-chain**: transaction amounts and the existence of outputs
- **Hidden**: who the recipient is (only the holder of the matching view
  key can detect an output as theirs)

The one exception: funds obtained via `devnet-import` start as plain
p2pkh outputs (transparent recipient). Your first `send-stealth` converts
them — including change — into stealth outputs.

### Confidential Amounts — Not Yet Available

The protocol design includes opt-in confidential amounts (Bulletproofs
range proofs — classical, not post-quantum cryptography, hence opt-in).
This is **not yet exposed through `qv-wallet`** — there is no flag to hide
amounts today. Until it ships, all transaction amounts are visible
on-chain.

### What's Private Today, Concretely

- Amount: **visible** on-chain
- Recipient: **private** (stealth outputs)
- Network metadata (your IP, timing): may leak through the node you talk
  to; run your own node or use a transport proxy if this matters to you

---

## 6. The Web UI (`serve`)

The wallet ships an embedded browser UI served over a local HTTP API:

```bash
qv-wallet --network devnet serve
# qv-wallet UI listening at http://127.0.0.1:7777
```

Open the printed URL in your browser. The UI covers wallet
creation/import/unlock, address display (with QR and `.qvaddr` download),
balance, UTXOs, sending, contacts, history, view-key export, selective
disclosure, and the devnet faucet button.

### Single-User Mode (Default)

No extra flags. The server fronts the one keystore named by `--keystore`.
For safety, single-user mode **refuses to bind to a non-loopback address**
— it only serves `127.0.0.1` / `::1`.

### Multi-Tenant Mode (Devnet/Demo Only)

```bash
qv-wallet --network devnet serve \
  --bind 0.0.0.0:7777 \
  --wallets-dir ./wallets \
  --session-ttl-secs 3600
```

- Each user registers with a username + password via
  `/api/auth/register`, then `/api/auth/login` returns a Bearer token used
  by the UI; `/api/auth/logout` and `/api/auth/me` complete the set.
- The server creates `<wallets-dir>/<username>/wallet.json` per user.
- Idle sessions expire after `--session-ttl-secs` (default 3600 s) and
  drop their in-memory secrets.

**This mode is CUSTODIAL**: while a user is logged in, their spend secret
lives in the server process's RAM. LAN binding is only permitted in
multi-tenant mode, and it is acceptable for devnet/demo use only — never
run it as a production custody service.

---

## 7. Staking, Delegation, and DeFi

QuantumVault's consensus is **Ouroboros Praos proof-of-stake** (2-second
slots, 12-hour epochs, fees paid to pool operators and delegators), and the
protocol roadmap includes AMM swaps, lending, and MEV-protected intents on
the eUTXO model.

**Staking and lending are not operable through `qv-wallet` yet.** There
are no `delegate`, `pools`, or lending commands in the current CLI; stake
pool operation is the domain of the `qv-miner` binary. The first DeFi
flow — AMM swaps — landed in Faz 6 and is described below.

### Swap (devnet)

`swap` trades against an on-chain constant-product AMM pool (a single
UTXO locked by the `amm_pool_lock` covenant, reserves tracked in its
datum). You name the pool UTXO; the wallet fetches its datum and script
via `qv_getUtxo`, computes the output, builds and signs the transaction:

```bash
qv-wallet --network local swap \
  --pool <pool_txid_hex>#0 \
  --direction a-to-b \
  --amount 1000 \
  --min-receive 900 \
  --fee 1000 \
  --broadcast
```

- `--pool` — the live pool UTXO as `<txid_hex>#<idx>` (or `txid:idx`).
- `--direction` — `a-to-b` sells token A for token B; `b-to-a` the
  reverse.
- `--amount` — how much of the input token you sell (smallest units).
- `--min-receive` — slippage floor; the wallet refuses to build the
  transaction if the computed output is below this.
- `--input <txid:idx>` / `--input-value <units>` — optional explicit
  funding UTXO (same convention as `send`). If `--input-value` is
  omitted the wallet resolves it via `qv_getUtxo`; if `--input` is
  omitted entirely, the wallet auto-selects the smallest plain p2pkh
  UTXO of `--account` (default 0) that covers the fee via
  `qv_scanP2pkh`.
- `--fee` — network fee (default 1000); `--broadcast` — submit via
  `qv_sendTransaction`, otherwise the signed hex is printed for manual
  submission (same as `send`).

The wallet prints a summary before broadcasting: direction, amount in,
computed amount out (vs. your floor), the pool's swap fee, network fee,
and the pool's post-swap reserves.

Devnet notes: token accounting is currently **datum-level** — reserves
move inside the pool datum rather than as separate native token outputs,
so the funding input only needs to cover the network fee, and change is
returned to your own transparent p2pkh address (this flow is not
stealth). Requires a node with the Faz 6 `qv_getUtxo` extension
(`script_hex`/`datum_hex` fields); against an older node the command
stops with an explicit error.

Don't know any pool outpoints? The node's `qv_listPools` RPC enumerates
every live pool (outpoint, reserves, fee, lp_total), and the web UI's
**Swap** panel populates its pool dropdown from it automatically.

### Create a pool (devnet)

`create-pool` bootstraps a brand-new AMM pool UTXO on chain — the
"genesis" of a trading pair:

```bash
qv-wallet --network local create-pool \
  --token-a <32_byte_hex> \
  --token-b <32_byte_hex> \
  --fee-bps 30 \
  --reserve-a 1000000 \
  --reserve-b 1000000 \
  --broadcast
```

- `--token-a` / `--token-b` — 32-byte token identifiers (64 hex chars),
  baked into both the pool's locking script and its datum.
- `--fee-bps` — swap fee in basis points (30 = 0.3%), also baked in.
- `--reserve-a` / `--reserve-b` — initial reserves (smallest units,
  datum-level accounting). Both must be positive.
- `--pool-value` — native value locked into the pool UTXO (default
  1000); subsequent swaps carry it through unchanged.
- `--input` / `--input-value` / `--account` / `--fee` / `--broadcast` —
  same conventions as `send` and `swap`. The funding input must cover
  `--pool-value` + `--fee`; the rest comes back as p2pkh change.

The command prints the new **pool outpoint** (`<txid>#0`) — that is what
you pass to `swap --pool` — plus a datum summary (reserves, fee,
`lp_total`). The genesis LP total is `⌊sqrt(reserve_a · reserve_b)⌋`,
identical to the empty-pool add-liquidity formula, so future liquidity
flows stay consistent.

**Scope note (read before locking real reserves).** LP shares exist
*only* as the `lp_total` field inside the pool datum — there is **no
on-chain LP token**, and no add/remove-liquidity spend path yet (D-6+
work). The pool covenant's `x·y ≥ k` invariant would let an
add-liquidity-shaped transition through (the product grows) but can
never pass a remove-liquidity one (the product shrinks) — meaning
reserves locked at creation **cannot be withdrawn** until a dedicated
spend path ships. On devnet this is a deliberate, documented boundary,
not an oversight.

### Swap in the Web UI

`qv-wallet serve` includes a **Swap** panel: it lists every live pool
(via `GET /api/defi/pools`, backed by `qv_listPools`), shows the selected
pool's reserves/fee, and submits the swap through `POST /api/defi/swap`
— the exact same signing path as the CLI command. A
`POST /api/defi/create-pool` endpoint mirrors `create-pool` for
programmatic use. All three endpoints follow the server's normal session
rules (Bearer token in multi-tenant mode; unlocked wallet in single-user
mode).

---

## 8. Security Best Practices

### Key Management
- **Mnemonic first**: the 24 words on paper, offline, are your root
  backup. No mnemonic = no recovery, ever.
- **Keep the keystore file too**: because of the view-key limitation
  (C-05, see Section 1), the keystore file is what preserves visibility of
  already-received stealth payments. Back it up alongside the mnemonic.
- **Passwords**: keystore password must be ≥ 8 characters; use far more —
  16+ mixed characters. It gates Argon2id + AES-256-GCM decryption.
- Do not email wallet files or store them unencrypted in cloud services.

### Phishing Awareness
- Verify recipients by comparing the short `qvfp1...` **fingerprint**
  out-of-band (call, video) before sending large amounts — that's exactly
  what it's for.
- Prefer `--to-qvaddr` files or saved `--to-contact` entries over
  hand-pasting 6 KB addresses.
- Never share your mnemonic, keystore file, or password. A `.qvview` file
  is safe to share with an auditor (view-only); a `.qvdisclose` file
  reveals only the single payment you chose.

### Server Mode
- Keep `serve` on `127.0.0.1` for personal use.
- Multi-tenant LAN mode is custodial and devnet-only — see Section 6.

### Hardware Wallets (Future)

Hardware wallet support is planned for post-launch. Until then, keep
high-value wallets on a dedicated machine and rely on the encrypted
keystore + offline mnemonic.

### Quantum Safety

QuantumVault signs transactions with **ML-DSA (FIPS 204, Dilithium family,
Level 3)** and detects stealth payments with the **Kyber KEM** — both
post-quantum schemes. Your on-chain funds are resistant to attacks by
future quantum computers.

---

## 9. Frequently Asked Questions

**Q: How long until my transaction is final?**
A: Approximately 100 seconds (k=50 blocks). After that, it cannot be reversed.

**Q: Why are full addresses so long (~6.4 KB)?**
A: They embed a Kyber view public key and a Dilithium spend public key —
post-quantum keys are simply large. Use `.qvaddr` files, multi-part QR
codes (`address --full-qr`), or contacts to move them around; use the
short `qvfp1` fingerprint to verify them.

**Q: Can I send to a `qvfp1...` fingerprint?**
A: No. The fingerprint is a hash for display/verification only; it doesn't
contain the keys needed to construct a stealth output. Always obtain the
full `qvst1...` address (or a `.qvaddr` file).

**Q: What if I forget my keystore password?**
A: Re-create the keystore from your 24-word mnemonic with
`qv-wallet import-mnemonic` and choose a new password. Caveat: the
restored wallet gets a fresh view key, so stealth payments received before
the restore may not be visible (limitation C-05) — your spend key and any
plain UTXOs are unaffected.

**Q: I restored from my mnemonic and my stealth balance looks lower. Why?**
A: Limitation C-05: view keys are not derived from the mnemonic, so a
restored wallet can't detect stealth outputs addressed to the old view
key. This is why backing up the keystore *file* matters, not just the
words. A deterministic view-key scheme is planned.

**Q: Are amounts hidden?**
A: Not currently. Recipient privacy (stealth) is on by default; confidential
amounts (Bulletproofs) are designed but not yet exposed in the wallet.

**Q: Is the multi-tenant web server safe for real funds?**
A: No. It is explicitly custodial (spend secrets in server RAM during
sessions) and intended for devnet/demo only.

**Q: How do I point the wallet at a different node?**
A: `--network devnet` (official public devnet), `--network local`
(`http://127.0.0.1:8545`), or an explicit `--rpc <url>`. `--network`
takes precedence over `--rpc`.

**Q: How do I prove a payment to a third party?**
A: For full incoming history, give an auditor a `.qvview` file
(`export-view-key` + `audit-scan`). For one specific payment, use
`disclose` / `verify-disclosure`.

---

## 10. Glossary

| Term | Definition |
|------|-----------|
| **UTXO** | Unspent Transaction Output. A "coin" you own that hasn't been spent. Multiple UTXOs make up your balance. |
| **Stealth Address** | A privacy-preserving address (`qvst1...`) where only the recipient can detect incoming outputs. Default on QuantumVault. |
| **Fingerprint** | Short `qvfp1...` hash (20 bytes / 40 hex chars) of an address's key material. For display and verification only — not payable. |
| **`.qvaddr` file** | JSON sidecar carrying a full stealth address, produced by `address --save` and consumed by `send-stealth --to-qvaddr`. |
| **`.qvview` file** | View-key export for auditors (`export-view-key`). Grants read access to incoming stealth payments; cannot spend. |
| **`.qvdisclose` file** | Self-contained proof of ownership of one specific stealth UTXO (`disclose`), optionally revealing its amount. |
| **Spend Key** | Your private signing key (ML-DSA, FIPS 204 — Dilithium Level 3). Deterministic from the mnemonic. Guard it absolutely. |
| **View Key** | Kyber keypair used to detect incoming stealth payments. Stored in the keystore; not derivable from the mnemonic (C-05). |
| **Keystore** | Encrypted wallet file (`wallet.json` by default): mnemonic + view keys under Argon2id + AES-256-GCM. |
| **Epoch** | A period of 12 hours (21,600 slots). |
| **Slot** | A 2-second opportunity for a leader to propose a block. |
| **VRF** | Verifiable Random Function. Used to fairly elect stake pool leaders without a central authority. |
| **KES** | Key Evolving Signature. A forward-secure signature scheme protecting against long-term key compromise. |
| **Finality** | The point at k=50 blocks (~100 seconds) when a transaction is irreversible. |
| **MEV** | Maximal Extractable Value. Profit extracted by reordering transactions. QuantumVault's design counters it with an encrypted mempool. |
| **ML-DSA** | Module-Lattice Digital Signature Algorithm (FIPS 204), from the CRYSTALS-Dilithium family. QuantumVault uses Level 3 by default. |
| **Kyber** | A post-quantum key encapsulation mechanism (KEM). Used for stealth-address detection. |

---

## Getting Help

- **Install & first run**: [docs/INSTALL.md](INSTALL.md)
- **Architecture**: [docs/ARCHITECTURE_V2.md](ARCHITECTURE_V2.md)
- **Stealth design**: [ADR-011](ADR/011-stealth-address-integration.md)
- **Bugs & questions**: GitHub Issues on the project repository

Good luck, and welcome to the post-quantum future of blockchain.
