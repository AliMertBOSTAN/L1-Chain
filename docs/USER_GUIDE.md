# QuantumVault L1 User Guide

Welcome to QuantumVault, a post-quantum secure blockchain designed for fast, private, and fair transactions. This guide covers everything you need to get started and use QV safely.

## 1. Getting Started

### Installing qv-wallet

QuantumVault transactions happen through the `qv-wallet` command-line tool. To install:

```bash
# Clone the repository and build
git clone https://github.com/quantumvault/l1-blockchain.git
cd l1-blockchain
nix develop
just build
./target/release/qv-wallet --version
```

Or use a prebuilt binary from the releases page.

### Creating Your Wallet

```bash
qv-wallet create --name my-wallet
```

This generates three keys:
- **Spend Key** (Dilithium): signs transactions; keep this extremely private
- **View Key** (Kyber): lets you see incoming transactions (stealth addresses)
- **Account Key**: derived from both; your primary wallet identity

The tool will prompt you to create a **passphrase** (optional but recommended). Store the wallet file securely.

### Backing Up Your Keys

After creating a wallet:

```bash
qv-wallet backup --wallet my-wallet --output my-wallet.backup
```

**Store the backup file offline** (USB drive, paper, safe deposit box). Without this, lost keys mean lost funds forever. QuantumVault has no recovery mechanism—only you control your keys.

---

## 2. Receiving QV

### Your QV Address

Generate a receiving address:

```bash
qv-wallet address --wallet my-wallet --new
```

This outputs your **stealth address**—a long string starting with `qv1...`. Share this with anyone who wants to send you QV. Each address is single-use and automatically rotates for privacy.

### Understanding Stealth Addresses

Unlike traditional blockchains where anyone can see incoming transactions, QuantumVault hides the recipient. Here's how:

1. The sender uses your **public view key** to create an encrypted "letter" (called a stealth note)
2. Only you, with your private view key, can decrypt it
3. The blockchain shows a transaction but not who received it

**Example**: If you share your stealth address with a friend, they send QV to that address. On-chain, the transaction is visible, but it doesn't show "(Friend → You)"—only "(Someone → Encrypted)". You see it in your wallet; the world doesn't.

### View Keys for Auditors

If you need to prove your income (e.g., tax auditing) without revealing your spend key:

```bash
qv-wallet export-view-key --wallet my-wallet
```

Share this view key with your auditor. They can see all incoming transactions to your wallet but cannot spend your funds.

---

## 3. Sending QV

### Basic Transfer

```bash
qv-wallet send --wallet my-wallet --to <recipient-address> --amount 1.5 --fee 0.0001
```

- `--amount`: how much QV to send (decimals allowed; 1 QV = 100,000,000 smallest units)
- `--fee`: transaction fee in QV (higher fee = faster inclusion)

The wallet will prompt for your passphrase (if set) and display a confirmation:

```
Sending 1.5 QV to qv1a9b8c7d6e5f4g3h2i1j...
Fee: 0.0001 QV
Total: 1.5001 QV
Confirm? (y/n)
```

### Transaction Confirmation

After sending, you receive a **transaction ID** (txid). To check status:

```bash
qv-wallet status --txid <txid>
```

QuantumVault finalizes transactions in approximately **100 seconds** (roughly 50 blocks). Once finalized, the transaction cannot be reversed.

### Transaction Fees

Fees are distributed to **stake pool operators** (who validate blocks) and **delegators** (who stake coins to pools). There is no burning—all fees reward the network. Higher fees get priority during high network activity.

---

## 4. Privacy Features

### Default: Stealth Addresses

Every receive address is stealth by default. You get strong privacy automatically—no configuration needed.

- **What's visible on-chain**: Transaction amounts, outputs (encrypted)
- **What's hidden**: Who sent it, who received it (only visible to the recipient)

### Optional: Confidential Amounts

For even more privacy, hide the transaction amount:

```bash
qv-wallet send --wallet my-wallet --to <recipient> --amount 5 --confidential
```

With `--confidential`, the amount is encrypted using Bulletproofs (a zero-knowledge proof). Observers see a transaction but not how much was sent.

**Note**: Confidential amounts use classical (non-quantum-safe) cryptography. Standard transactions use post-quantum signatures, so quantum-safe is the default. Use `--confidential` only if hiding amounts outweighs this trade-off.

### What About Transaction Privacy?

Standard QuantumVault transactions are **amount-transparent but recipient-opaque**:
- Amount: visible on-chain
- Sender: private (via stealth addresses)
- Receiver: private (via stealth addresses)

To hide amounts too, add `--confidential`.

---

## 5. Staking & Delegation

QuantumVault uses **Proof of Stake (Ouroboros Praos)**: instead of mining, holders lock coins to validate blocks and earn rewards.

### Choosing a Stake Pool

View available pools:

```bash
qv-wallet pools --list
```

Output shows:
- Pool name, operator address, current stake, fees
- Recent block productivity (uptime indicator)

**Choose by**: low fees (0–5%), high uptime (>95%), and operator reputation.

### Delegating Your Coins

```bash
qv-wallet delegate --wallet my-wallet --pool <pool-name> --amount 10
```

Your 10 QV are now "locked" in the pool but still in your wallet (not transferred). The pool operator uses delegated coins to earn block rewards.

### Earning Rewards

Rewards arrive every **epoch** (12 hours, or 21,600 blocks). Coins stay locked during each epoch; after delegation, you earn rewards proportional to your share of the pool.

**Example**: If the pool has 1000 QV delegated and you delegated 10 QV, you earn 1% of the pool's rewards that epoch.

### Undelegating

To withdraw your coins from a pool:

```bash
qv-wallet undelegate --wallet my-wallet --pool <pool-name>
```

Funds return to your wallet in the next epoch (~12 hours). Undelegated coins do not earn rewards.

---

## 6. Using DeFi

QuantumVault includes **decentralized exchanges (AMM)**, **lending pools**, and **intent-based swaps**—all MEV-protected.

### Swapping Tokens (AMM)

Liquidity pools let you trade tokens trustlessly. For example, swap QV for a wrapped stablecoin:

```bash
qv-wallet swap --wallet my-wallet --from QV --to USDC --amount 5
```

The wallet calculates the exchange rate (including slippage). Confirm, and the swap happens in the next block.

**MEV Protection**: Your swap order is encrypted until the block is finalized, so no one can front-run you.

### Providing Liquidity

Earn fees by depositing two tokens into a liquidity pool:

```bash
qv-wallet liquidity-add --wallet my-wallet --token-a QV --token-b USDC --amount-a 5 --amount-b 5000
```

You receive **LP tokens** representing your share. Swap fees accrue to LP token holders. Remove liquidity anytime:

```bash
qv-wallet liquidity-remove --wallet my-wallet --lp-token <token-id> --amount 100
```

### Lending & Borrowing

Deposit coins into a lending pool to earn interest:

```bash
qv-wallet lending-deposit --wallet my-wallet --token QV --amount 10
```

Or borrow against collateral:

```bash
qv-wallet lending-borrow --wallet my-wallet --token QV --amount 5 --collateral USDC --collateral-amount 7500
```

Loans accrue interest. Repay anytime:

```bash
qv-wallet lending-repay --wallet my-wallet --loan-id <id>
```

---

## 7. Transaction History

### View Your Balance

```bash
qv-wallet balance --wallet my-wallet
```

Shows spendable and pending amounts.

### Check UTXOs

QuantumVault uses **UTXOs** (Unspent Transaction Outputs) instead of account balances. View your UTXOs:

```bash
qv-wallet utxos --wallet my-wallet
```

Each UTXO is a "coin" you own. When you send, the wallet combines UTXOs to create new ones. This is transparent to you—the wallet handles it automatically.

### Transaction History

```bash
qv-wallet history --wallet my-wallet --limit 20
```

Shows sent and received transactions with dates, amounts, and confirmation status.

---

## 8. Security Best Practices

### Key Management
- **Passphrases**: Use a strong passphrase (16+ characters, mixed case, numbers, symbols).
- **Offline Backup**: Store wallet backups on USB drives or paper, disconnected from the internet.
- **Single Point of Failure**: Do not email wallet files or store them in cloud services (unless encrypted separately).

### Phishing Awareness
- Always verify addresses carefully; stealth addresses are long and unique. A small typo sends QV to the wrong encrypted recipient.
- Never share your spend key, wallet file, or passphrase via email or chat.
- When entering passphrases, ensure the console is not screen-recorded or monitored.

### Hardware Wallets (Future)

Hardware wallet support is planned for post-launch. Until then:
- Keep wallets on a dedicated, offline computer if holding large amounts.
- Use passphrases to encrypt wallet files at rest.

### Quantum Safety

QuantumVault uses post-quantum signatures (Dilithium Level 3) by default. Your keys are resistant to attacks by future quantum computers. This makes QuantumVault safer long-term than classical blockchains.

---

## 9. Frequently Asked Questions

**Q: How long until my transaction is final?**
A: Approximately 100 seconds (roughly 50 blocks). After that, it cannot be reversed.

**Q: Why are stealth addresses so long?**
A: They encode both a public view key and ephemeral data to ensure recipient privacy. Longer addresses = higher security.

**Q: What if I forget my passphrase?**
A: You can still access the wallet if you have the backup file. Recreate it with `qv-wallet restore --backup <file>` and set a new passphrase.

**Q: Is QuantumVault truly private?**
A: By default, yes—your address and transactions are hidden. Amounts are visible unless you use `--confidential`. Metadata (timestamps, IP addresses) may leak through your node; use Tor for full privacy.

**Q: How often do I earn staking rewards?**
A: Every 12 hours (one epoch), proportional to your delegated amount and the pool's performance.

**Q: Can I unstake immediately?**
A: Undelegation takes ~12 hours (one epoch). Your coins return to your wallet then.

**Q: What if my stake pool goes offline?**
A: Rewards pause while the pool is offline. Switch to a more reliable pool with `qv-wallet undelegate` and `qv-wallet delegate`.

**Q: Is Bulletproofs (confidential amounts) quantum-safe?**
A: No, Bulletproofs use classical elliptic curves. It's opt-in, not default. Use only if hiding amounts justifies the trade-off.

**Q: How do I verify a transaction on-chain?**
A: Use the block explorer at `explorer.quantumvault.io` and paste your transaction ID (txid).

---

## 10. Glossary

| Term | Definition |
|------|-----------|
| **UTXO** | Unspent Transaction Output. A "coin" you own that hasn't been spent. Multiple UTXOs make up your balance. |
| **Stealth Address** | A privacy-preserving address where only the recipient can see incoming transactions. Default on QuantumVault. |
| **Spend Key** | Your private signing key (Dilithium). Guards it like a password; only you should have it. |
| **View Key** | Your public key for viewing incoming transactions. Can be shared with auditors without risking funds. |
| **Epoch** | A period of 12 hours (~21,600 blocks). Staking rewards and delegations settle every epoch. |
| **Slot** | A 2-second opportunity for a leader to propose a block. A block happens in each slot (normally). |
| **VRF** | Verifiable Random Function. Used to fairly elect stake pool leaders without a central authority. |
| **KES** | Key Evolving Signature. A forward-secure signature scheme protecting against long-term key compromise. |
| **Finality** | The point after ~100 seconds when a transaction is irreversible. Until then, it could be reversed by a blockchain reorganization. |
| **AMM** | Automated Market Maker. A smart contract that lets anyone swap tokens at prices determined by a formula (x·y=k). |
| **LP Token** | Liquidity Provider token. Proves your share of a liquidity pool and entitles you to swap fees. |
| **MEV** | Maximal Extractable Value. The profit miners/validators extract by reordering transactions. QuantumVault protects against this via encrypted mempool. |
| **Dilithium** | A post-quantum digital signature algorithm resistant to quantum computer attacks. QuantumVault's default. |
| **Kyber** | A post-quantum key encapsulation mechanism (KEM) used for encryption. QuantumVault's default for stealth addresses. |

---

## Getting Help

- **Docs**: Full technical documentation at `docs/ARCHITECTURE_V2.md`
- **GitHub Issues**: Report bugs or ask questions on the project repository
- **Community**: Join the QuantumVault Discord for peer support

Good luck, and welcome to the post-quantum future of blockchain.
