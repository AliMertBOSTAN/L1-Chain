"""Reference model for the Genesis maxvalid-bg chain-selection rule (ADR-008).

maxvalid-bg decides whether a bootstrapping / long-offline node should adopt
a candidate chain over its local chain:
  * shallow fork (local diverges <= k blocks from the fork point):
        longest chain wins (deterministic hash tie-break on equal length);
  * deep fork (local diverges > k blocks):
        compare block density in the s-slot window right after the fork
        point -- the denser chain wins (else keep local).
Mirrors the Rust maxvalid_bg in chain_state.rs. A chain is a list of
(hash, slot) ordered genesis -> tip; both chains share genesis.
"""
import random

KEEP_LOCAL = "KeepLocal"
ADOPT_CANDIDATE = "AdoptCandidate"


def common_prefix_len(local, candidate):
    n = 0
    for a, b in zip(local, candidate):
        if a[0] != b[0]:
            break
        n += 1
    return n


def density_in_window(chain, common, fork_slot, window_end):
    count = 0
    for (_, slot) in chain[common:]:
        if fork_slot < slot <= window_end:
            count += 1
    return count


def longer_chain(local, candidate):
    if len(candidate) > len(local):
        return ADOPT_CANDIDATE
    if len(candidate) < len(local):
        return KEEP_LOCAL
    if candidate and local and candidate[-1][0] < local[-1][0]:
        return ADOPT_CANDIDATE
    return KEEP_LOCAL


def maxvalid_bg(local, candidate, k, s):
    common = common_prefix_len(local, candidate)
    if common == 0:
        return KEEP_LOCAL
    fork_slot = local[common - 1][1]
    local_after = len(local) - common
    if local_after <= k:
        return longer_chain(local, candidate)
    window_end = fork_slot + s
    local_density = density_in_window(local, common, fork_slot, window_end)
    cand_density = density_in_window(candidate, common, fork_slot, window_end)
    return ADOPT_CANDIDATE if cand_density > local_density else KEEP_LOCAL


def fail(msg):
    raise SystemExit("FAIL: " + msg)


def chain(prefix, suffix_slots, tag):
    out = list(prefix)
    for i, sl in enumerate(suffix_slots):
        out.append(("%s%d" % (tag, i), sl))
    return out


GEN = [("G", 0)] + [("S%d" % i, (i + 1) * 10) for i in range(5)]
K = 50


def test_scenarios():
    local = chain(GEN, [60, 70], "A")
    cand = chain(GEN, [62, 72, 82], "B")
    assert maxvalid_bg(local, cand, K, 1000) == ADOPT_CANDIDATE

    local = chain(GEN, [60, 70, 80], "A")
    cand = chain(GEN, [62, 72], "B")
    assert maxvalid_bg(local, cand, K, 1000) == KEEP_LOCAL

    local = chain(GEN, [60, 70], "A")
    cand = local + [("A2", 80)]
    assert maxvalid_bg(local, cand, K, 1000) == ADOPT_CANDIDATE

    local = chain(GEN, [60, 70], "A")
    assert maxvalid_bg(local, list(local), K, 1000) == KEEP_LOCAL

    local = chain(GEN, [60], "A")
    alien = [("X", 0), ("X1", 10)]
    assert maxvalid_bg(local, alien, K, 1000) == KEEP_LOCAL

    base = [("G", 0)]
    s = 2160
    local = chain(base, list(range(20, 4001, 20)), "L")
    cand = chain(base, list(range(8, 4001, 8)), "C")
    assert maxvalid_bg(local, cand, K, s) == ADOPT_CANDIDATE
    assert maxvalid_bg(cand, local, K, s) == KEEP_LOCAL

    local = chain(base, list(range(10, 10 + 60 * 20, 20)), "L")
    cand = chain(base, list(range(10, 10 + 60 * 20, 20)), "C")
    assert maxvalid_bg(local, cand, K, s) == KEEP_LOCAL
    print("ok  scenario tests")


def grow_branch(rng, base, tag, slot_prob, n_slots):
    out = list(base)
    idx = 0
    for sl in range(1, n_slots + 1):
        if rng.random() < slot_prob:
            out.append(("%s%d" % (tag, idx), sl))
            idx += 1
    return out


def test_random_honest_wins():
    rng = random.Random(20260522)
    base = [("G", 0)]
    f = 0.05
    s = 2160
    wins, valid = 0, 0
    for _ in range(400):
        honest = grow_branch(rng, base, "H", f * 0.70, 4000)
        adv = grow_branch(rng, base, "A", f * 0.30, 4000)
        if len(honest) - 1 <= K or len(adv) - 1 <= K:
            continue
        valid += 1
        if maxvalid_bg(adv, honest, K, s) == ADOPT_CANDIDATE:
            wins += 1
        if maxvalid_bg(honest, adv, K, s) != KEEP_LOCAL:
            fail("adversarial sparse chain wrongly adopted over honest")
    if wins != valid:
        fail("honest adopted only %d/%d valid deep-fork runs" % (wins, valid))
    print("ok  random honest-vs-adversary: honest adopted %d/%d deep-fork runs" % (wins, valid))


def explore_s():
    rng = random.Random(7)
    base = [("G", 0)]
    f = 0.05
    print("s-window vs honest-adoption rate (70/30 stake split):")
    for s in (200, 500, 1000, 2160, 4000):
        wins, runs = 0, 300
        for _ in range(runs):
            honest = grow_branch(rng, base, "H", f * 0.70, 4000)
            adv = grow_branch(rng, base, "A", f * 0.30, 4000)
            if len(honest) - 1 <= K or len(adv) - 1 <= K:
                runs -= 1
                continue
            if maxvalid_bg(adv, honest, K, s) == ADOPT_CANDIDATE:
                wins += 1
        rate = wins / max(runs, 1)
        print("  s=%5d slots:  honest adopted %5.1f%%" % (s, rate * 100))


if __name__ == "__main__":
    test_scenarios()
    test_random_honest_wins()
    explore_s()
    print("ALL CHECKS PASSED")
