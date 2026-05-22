"""
Reference model for a DETERMINISTIC Praos leader check (ADR-009).

The production code currently decides leadership with f64 transcendentals
(`exp`, `ln`) — not bit-identical across platforms, a consensus-determinism
risk. This model works out and validates a fixed-point integer replacement:

    leader  <=>  p < 1 - (1-f)^sigma
                 (1-f)^sigma = exp(sigma * ln(1-f))

`f` is a protocol constant (0.05), so `ln(1-f)` is a precomputed constant.
`exp` of a small negative number is evaluated with a bounded Taylor series
in 2^64-scaled unsigned integer arithmetic — fully deterministic.

This script: computes the constant, picks the Taylor term count, checks the
u128 overflow budget, validates against a high-precision oracle, and emits
test vectors for the Rust port.
"""
from decimal import Decimal, getcontext, ROUND_HALF_EVEN

getcontext().prec = 80

SCALE_BITS = 64
SCALE = 1 << SCALE_BITS          # fixed-point scale 2^64
F_NUM, F_DEN = 1, 20             # active slot coefficient f = 0.05
U128_MAX = (1 << 128) - 1

# --- precomputed constant: |ln(1 - f)| scaled by 2^64 -----------------------
one_minus_f = Decimal(F_DEN - F_NUM) / Decimal(F_DEN)   # 0.95
ln_1_minus_f = one_minus_f.ln()                          # negative
LN95_MAG = int((-ln_1_minus_f * SCALE).to_integral_value(rounding=ROUND_HALF_EVEN))


def exp_neg_fixed(m, k_terms):
    """exp(-m_real) scaled by 2^64, where m = m_real * 2^64, m >= 0.

    Mirrors the Rust u128 implementation exactly: every value is a
    non-negative integer, every division is truncating (same for u128 and
    Python // on non-negatives). Returns (result, max_intermediate)."""
    term = SCALE          # term_0 = x^0/0! = 1.0
    acc = SCALE           # accumulator
    max_inter = 0
    for k in range(1, k_terms + 1):
        prod = term * m                      # <-- the only place that can be large
        max_inter = max(max_inter, prod)
        term = prod // SCALE                 # fixed-point multiply by m
        term = term // k                     # divide by k  (term is now m^k/k!)
        if k % 2 == 1:
            acc -= term
        else:
            acc += term
    return acc, max_inter


def threshold_fixed(stake_num, stake_den, k_terms):
    """Leader threshold 1 - (1-f)^sigma, scaled by 2^64. sigma = num/den."""
    m = (stake_num * LN95_MAG) // stake_den          # |x| scaled by 2^64
    exp_val, max_inter = exp_neg_fixed(m, k_terms)
    max_inter = max(max_inter, stake_num * LN95_MAG)
    return SCALE - exp_val, max_inter


def is_leader_fixed(stake_num, stake_den, vrf64, k_terms):
    """Deterministic leader check. vrf64 = top 64 bits of the VRF output."""
    thr, _ = threshold_fixed(stake_num, stake_den, k_terms)
    return vrf64 < thr


def true_threshold(stake_num, stake_den):
    """High-precision oracle: 1 - (1-f)^sigma as a Decimal."""
    sigma = Decimal(stake_num) / Decimal(stake_den)
    return Decimal(1) - one_minus_f ** sigma


def fail(msg):
    raise SystemExit("FAIL: " + msg)


print(f"LN95_MAG (|ln(0.95)| * 2^64) = {LN95_MAG}")
print(f"  cross-check: {Decimal(LN95_MAG) / SCALE}  vs  {-ln_1_minus_f}")
print()

# --- 1. choose the Taylor term count ---------------------------------------
# sigma = 1 is the worst case (largest |x|). Compare scaled error vs oracle.
print("Taylor term count vs scaled error (sigma = 1, the worst case):")
chosen_k = None
for k in range(3, 12):
    thr, _ = threshold_fixed(1, 1, k)
    true_thr_scaled = true_threshold(1, 1) * SCALE
    err = abs(Decimal(thr) - true_thr_scaled)
    mark = ""
    if chosen_k is None and err < 1:
        chosen_k = k
        mark = "  <- first sub-ulp; CHOSEN"
    print(f"  K={k:2d}:  scaled error = {err:.4f}{mark}")
K = chosen_k
print(f"\nChosen Taylor term count K = {K}\n")

# --- 2. overflow budget (must stay within u128) ----------------------------
max_seen = 0
for (sn, sd) in [(1, 1), (1, 3), (7, 10), (999_999, 1_000_000),
                 (2**63, 2**63), (2**64 - 1, 2**64 - 1)]:
    _, mi = threshold_fixed(sn, sd, K)
    max_seen = max(max_seen, mi)
print(f"max intermediate value     = {max_seen}")
print(f"u128 max                   = {U128_MAX}")
if max_seen > U128_MAX:
    fail("intermediate overflows u128")
print(f"headroom                   = 2^{(U128_MAX // max(max_seen,1)).bit_length() - 1} margin")
print("overflow budget: OK (fits u128)\n")

# --- 3. validate against the high-precision oracle -------------------------
# For many (sigma, p) pairs, the fixed-point decision must match the ideal
# real-valued decision except within a negligible band around the boundary.
worst_band = Decimal(0)
disagreements = 0
checks = 0
for (sn, sd) in [(1, 1), (1, 2), (3, 10), (1, 100), (37, 1000),
                 (1, 1_000_000), (123_456, 1_000_000), (1, 10**9)]:
    true_thr = true_threshold(sn, sd)
    fx_thr, _ = threshold_fixed(sn, sd, K)
    # sample vrf values, including ones right next to the threshold
    samples = [0, 1, SCALE // 2, SCALE - 1,
               int(true_thr * SCALE) - 2, int(true_thr * SCALE) - 1,
               int(true_thr * SCALE), int(true_thr * SCALE) + 1,
               int(true_thr * SCALE) + 2]
    for vrf64 in samples:
        if not (0 <= vrf64 < SCALE):
            continue
        checks += 1
        fx_decision = vrf64 < fx_thr
        true_decision = (Decimal(vrf64) / SCALE) < true_thr
        if fx_decision != true_decision:
            disagreements += 1
            band = abs(Decimal(vrf64) / SCALE - true_thr)
            worst_band = max(worst_band, band)

print(f"oracle comparison: {checks} checks, {disagreements} disagreements")
print(f"  worst disagreement band  = {worst_band}  (~2^{worst_band.ln()/Decimal(2).ln():.1f})")
print("  (disagreements only inside the sub-ulp truncation band — and the")
print("   fixed-point result is itself bit-identical on every node, so this")
print("   is consistent across the network, not a fork risk.)\n")
if worst_band > Decimal(2) ** -60:
    fail("disagreement band larger than expected truncation error")

# --- 4. sanity: sigma = 1 threshold equals f -------------------------------
thr1, _ = threshold_fixed(1, 1, K)
f_scaled = Decimal(F_NUM) / Decimal(F_DEN) * SCALE
print(f"sigma=1 threshold = {Decimal(thr1)/SCALE}  (expected f = 0.05)")
if abs(Decimal(thr1) - f_scaled) > 2:
    fail("sigma=1 threshold should equal f")

# --- 5. emit test vectors for the Rust port --------------------------------
print("\n--- Rust test vectors (stake_num, stake_den, threshold_fixed) ---")
for (sn, sd) in [(1, 1), (1, 2), (1, 10), (7, 10), (1, 1000), (1, 1_000_000)]:
    thr, _ = threshold_fixed(sn, sd, K)
    print(f"  sigma={sn}/{sd:<9}  threshold_fixed = {thr}")

print("\nALL CHECKS PASSED")
