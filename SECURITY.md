# QuantumVault L1 — Security Policy

**Version**: 1.0  
**Status**: Active (AŞAMA 14)  
**Last Updated**: 2026-04-27  
**Contact**: alimert930@gmail.com

---

## Vulnerability Disclosure Policy

QuantumVault follows a **90-day coordinated disclosure** model aligned with industry best practices. We value security researchers and welcome responsible bug reports.

### Reporting a Vulnerability

**DO NOT** open a public GitHub issue for security vulnerabilities.

**Instead**, email the security team:
- **Primary**: alimert930@gmail.com
- **Subject line**: `[SECURITY] <brief description>`
- **Include**:
  - Type of vulnerability (code, design, cryptography)
  - Crate affected (`qv-*`)
  - Proof-of-concept code (optional, but helpful)
  - Reproduction steps
  - Suggested fix (if known)
  - Your name + affiliation (for credits)

**PGP Encryption** (optional, highly recommended):
```
-----BEGIN PGP PUBLIC KEY BLOCK-----
mQINBGQxABMBEADK...  [full key below]
-----END PGP PUBLIC KEY BLOCK-----
```

[Download QuantumVault security PGP key](https://quantumvault.com/security.asc)

---

## Vulnerability Assessment & Triage

### Severity Classification

| Severity | CVSS Range | Timeline | Examples |
|----------|-----------|----------|----------|
| **Critical** | 9.0–10.0 | Patch within 24h | Undetectable double-spend, signature forgery, consensus halt |
| **High** | 7.0–8.9 | Patch within 7 days | Large-scale DoS, unconfirmed reorg, wallet key leak |
| **Medium** | 4.0–6.9 | Patch within 30 days | Mempool censorship, gas metering escape, timing leak |
| **Low** | 0.1–3.9 | Patch when convenient | Minor DoS, usability issue, theoretical vulnerability |

### Triage Process

1. **Acknowledgment** (within 24h):
   - Confirm receipt of report
   - Provide ticket number for reference
   - Estimate triage completion date

2. **Investigation** (within 48h):
   - Reproduce vulnerability
   - Determine scope (which crates, networks affected)
   - Assess true severity (may differ from reporter estimate)

3. **Remediation** (severity-dependent):
   - Critical: Hotfix deployed within 24–48h
   - High: Regular patch cycle (weekly)
   - Medium: Next scheduled release (monthly)
   - Low: Backlog for future release

4. **Disclosure** (90-day standard):
   - Day 1: Researcher submits report (confidential)
   - Day 60: QuantumVault notifies ecosystem partners (exchanges, pool operators)
   - Day 90: Public disclosure + patch release simultaneously
   - Day 0–90: Researcher can request extension for critical coordinated fix

---

## Disclosure Timeline

### Standard 90-Day Process

```
Timeline          Action
─────────────────────────────────────────────────────
Day 0             Vulnerability reported (confidential)
                  ├─ Triage & reproduction
                  └─ Severity assessment

Day 1–7           Investigation & root cause analysis
                  ├─ Internal code review
                  └─ Patch development

Day 8–30          Patch testing & validation
                  ├─ Unit tests for fix
                  ├─ Integration tests
                  ├─ Fuzz testing (if applicable)
                  └─ Regression testing

Day 31–60         Ecosystem notification (private)
                  ├─ Notify exchange partners
                  ├─ Notify mining pool operators
                  ├─ Notify major node operators
                  └─ Request coordinated upgrade

Day 61–89         Final preparation
                  ├─ Public advisory draft
                  ├─ Release notes + upgrade instructions
                  └─ Legal review (if needed)

Day 90            Simultaneous public disclosure + patch release
                  ├─ GitHub release + announcement
                  ├─ Security advisory published
                  ├─ Twitter / official channels announcement
                  └─ Ecosystem upgrade period begins
```

### Early Disclosure Requests

Researchers may request earlier disclosure in these cases:

- **Active in-the-wild exploitation**: If evidence of active attacks
- **Other vendors affected**: If vulnerability affects multiple platforms
- **Regulatory requirement**: If disclosure mandated by law

**Request process**: Email `alimert930@gmail.com` with evidence. Core team votes on amendment.

---

## Responsible Disclosure Guidelines

Researchers MUST:

✅ **DO**:
- Report only to QuantumVault security team (not public, not to 3rd parties without permission)
- Allow reasonable time for patch (7–90 days depending on severity)
- Avoid exploiting vulnerability beyond proof-of-concept
- Avoid accessing other users' data or systems
- Maintain confidentiality until coordinated disclosure

❌ **DO NOT**:
- Publish exploits before patch is released
- Demand payment or publicity for disclosure
- Share vulnerability with other projects without permission
- Test on mainnet (use testnet only)
- Disclose to exchanges or regulatory bodies without approval

**Violations**: Researchers who violate responsible disclosure may be:
- Excluded from bug bounty rewards
- Banned from future security programs
- Reported to law enforcement (if criminal activity)

---

## Bug Bounty Program

### Scope & Rewards

**In Scope**: Confirmed vulnerabilities in:
- `crates/qv-*` public APIs (all Rust code)
- Protocol consensus logic
- Cryptographic implementations (wrapper layer over liboqs)
- Network message parsing
- UTXO state machine

**Out of Scope**:
- Vulnerabilities in upstream dependencies (liboqs, libp2p, RocksDB, etc.) — report directly to those projects
- Operational security (key management, infrastructure) — report privately but no bounty
- Social engineering or phishing
- Theoretical vulnerabilities without proof-of-concept
- Vulnerabilities already disclosed publicly

### Bounty Amounts (USD)

| Severity | Base Reward | Bonus (Exploit) | Bonus (Writeup) | Max Total |
|----------|-------------|-----------------|-----------------|-----------|
| Critical | $25,000 | +$10,000 | +$5,000 | $40,000 |
| High | $5,000 | +$2,500 | +$1,000 | $8,500 |
| Medium | $1,000 | +$500 | +$250 | $1,750 |
| Low | $100 | — | — | $100 |

**Exploit Bonus**: +50% if you provide working proof-of-concept (not required, but appreciated).

**Writeup Bonus**: +25% if you submit a detailed technical writeup for educational purposes (can be published with attribution).

### Payment Terms

- **Eligibility**: Researcher must comply with responsible disclosure + legal terms
- **Currency**: USD paid via wire transfer or stablecoin (USDC/USDT)
- **Timeline**: Payment issued within 30 days of patch release
- **Tax**: Researcher is responsible for any tax implications

### Leaderboard & Hall of Fame

Top 10 security researchers (by lifetime bounties) are listed on:
- quantumvault.com/security#hall-of-fame
- Annual recognition at QuantumVault security summit
- First priority for future bug bounty program enhancements

---

## Security Audits

### External Audit Schedule

**Phase**: AŞAMA 14 (Pre-mainnet)
- **Target**: Q3 2026 (pre-mainnet launch)
- **Scope**: Tier 1 + Tier 2 modules (see audit-prep.md)
- **Duration**: 8–12 weeks
- **Estimated Cost**: $150k–$300k (tier-1 firm)

**Audit Firm Requirements**:
- NIST/ISO 27001 certified
- Experience with blockchain protocols
- Deep knowledge of PQC (Dilithium, Kyber)
- Post-mortem publication agreement (30–60 day delay)

### Historical Audits

- **v1 (C++)**: None completed (project ended pre-mainnet)
- **v2 (Rust)**: Initial audit scheduled Q3 2026

### Continuous Security

- **Fuzzing**: Weekly automated fuzzing (24h campaigns)
- **Dependency scanning**: Monthly `cargo audit` + `cargo deny` checks
- **Static analysis**: Per-commit clippy linting + code review
- **Peer review**: All PRs reviewed by ≥2 security engineers

---

## Security Advisory Format

### Sample Advisory

```markdown
# Security Advisory: QuantumVault-2026-04-27-001

**Vulnerability**: Unvalidated Script VM Opcode
**Severity**: Critical (CVSS 9.5)
**Affected Versions**: v1.0.0–v1.1.2
**Fixed Versions**: v1.2.0, v1.1.3 (backport)
**Disclosure Date**: 2026-04-27
**Discovered By**: Alice Eve (alice@research.org)

## Summary

A flaw in the script VM opcode CHECKSIG_PQC allows arbitrary transactions to be spent without a valid signature. This breaks consensus and enables coin theft.

## Technical Details

The opcode implementation has an early-return bug that causes it to succeed even when the signature is invalid. See CVE-2026-1234 for full details.

## Impact

- Consensus is broken; attackers can create arbitrary blocks
- Coins can be stolen without authorization
- Recommendation: STOP all operations immediately

## Workarounds

None. Upgrade immediately.

## Resolution

Update to v1.2.0 or apply backport patch v1.1.3.

Affected validators must coordinate a network-wide upgrade within 24 hours.

## Credits

Thanks to Alice Eve for responsible disclosure.
```

---

## Response Contacts

### Severity Escalation

| Level | Contact | Response Time | Authority |
|-------|---------|----------------|-----------|
| **P0 (Critical)** | alimert930@gmail.com | <1h | CEO + Security Lead |
| **P1 (High)** | security@quantumvault.com | <4h | Security Lead |
| **P2 (Medium)** | security@quantumvault.com | <24h | Security Lead |
| **P3 (Low)** | GitHub Issues | ≤7 days | Engineering Team |

### Public Channels

- **Security Home**: quantumvault.com/security
- **Advisory List**: security-advisories@quantumvault.com (subscribe)
- **Twitter**: @QuantumVaultL1 (announcements only)
- **Slack**: #security-advisories (private; invite-only)

---

## Build & Deployment Security

### Supply Chain Security

**Build System**:
- Nix flake provides reproducible environment (no dependency surprises)
- All dependencies pinned + hashed in `flake.lock`
- Weekly `cargo audit` checks for known CVEs

**Deployment**:
- Only release binaries signed with GPG key (see below)
- Checksum verification required before running
- Release notes include audit summary + new threat models

### Release Signing

**GPG Public Key** (for verifying releases):
```
-----BEGIN PGP PUBLIC KEY BLOCK-----
Comment: QuantumVault Release Signing Key
mQINBGQxABMBEADK... [see quantumvault.com/releases.asc]
-----END PGP PUBLIC KEY BLOCK-----
```

**Verification**:
```bash
# Download release + signature
curl -O https://github.com/quantumvault/l1/releases/download/v1.2.0/qv-node-v1.2.0.tar.gz
curl -O https://github.com/quantumvault/l1/releases/download/v1.2.0/qv-node-v1.2.0.tar.gz.sig

# Verify signature
gpg --verify qv-node-v1.2.0.tar.gz.sig qv-node-v1.2.0.tar.gz

# Expected: "Good signature from "QuantumVault Release <releases@quantumvault.com>""
```

---

## Incident Response & Communication

See [`docs/security/runbook-incident.md`](docs/security/runbook-incident.md) for:
- P0–P3 incident classifications
- Triage & containment procedures
- Communication templates
- Escalation chain

---

## Privacy & Data Protection

### What We Collect

When you report a vulnerability:
- Email address (for acknowledgment + payment)
- Vulnerability details (technical)
- Researcher affiliation (optional, for credits)
- PGP fingerprint (optional, for encrypted comms)

### What We Do NOT Collect

- No tracking cookies
- No IP logging beyond email headers
- No submission metadata (unless you provide it)

### Retention

- Reports retained for 5 years (for audit trail)
- Personal information deleted after bounty payment (unless researcher consents to Hall of Fame)

---

## References

- [`docs/threat-model/README.md`](docs/threat-model/README.md) — Threat models & STRIDE analysis
- [`docs/security/audit-prep.md`](docs/security/audit-prep.md) — External audit scope
- [`docs/security/runbook-incident.md`](docs/security/runbook-incident.md) — Incident response
- [`docs/security/key-management.md`](docs/security/key-management.md) — Key handling guidance
- [`CLAUDE.md`](CLAUDE.md) — Architecture decisions
- [`PROJECT_STATUS.md`](PROJECT_STATUS.md) — Phase completion status

---

## FAQ

**Q: What if I discover a vulnerability in a dependency (e.g., liboqs)?**

A: Report it directly to the upstream project. If it affects QuantumVault, report both to us and the upstream maintainers. We'll coordinate disclosure.

**Q: How long will it take to fix my bug?**

A: Critical: 1–7 days. High: 7–30 days. Medium: 30–90 days. This is our target; actual time depends on complexity.

**Q: Can I publish my findings before the 90-day deadline?**

A: No, without explicit written permission from the core team. Early disclosure may forfeit bounty eligibility.

**Q: What if I disagree with the severity rating?**

A: Contact the security lead. We'll re-evaluate and explain our assessment. You can appeal to the CEO if unresolved.

**Q: Do you offer a bug bounty for mainnet only, or testnet too?**

A: Both testnet and mainnet. Testnet vulnerabilities are eligible but may have lower bounty amounts (contact us for specifics).

---

## License

This security policy is licensed under CC0 (public domain). Community members may adapt it for their projects.

---

**Questions?** Contact alimert930@gmail.com
