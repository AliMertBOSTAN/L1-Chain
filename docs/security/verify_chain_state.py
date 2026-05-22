"""
Faithful re-implementation of the fixed qv-consensus ChainState fork-choice +
finality logic, used to verify the algorithm design (no Rust compiler in the
sandbox). Mirrors crates/qv-consensus/src/chain_state.rs after the 2026-05-22
audit fix.
"""
import random

GENESIS = 0


class ChainState:
    def __init__(self, k):
        self.k = k
        self.entries = {GENESIS: (GENESIS, 0)}  # hash -> (parent_hash, height)
        self.tip = GENESIS
        self.final_hash = GENESIS
        self.final_height = 0

    def is_ancestor(self, ancestor, descendant):
        if ancestor not in self.entries:
            return False
        target = self.entries[ancestor][1]
        cur = descendant
        while True:
            if cur == ancestor:
                return True
            if cur not in self.entries:
                return False
            par, h = self.entries[cur]
            if h <= target:
                return False
            if cur == par:
                return False
            cur = par

    def advance_finality(self):
        tip_h = self.entries[self.tip][1]
        if tip_h < self.k:
            return
        target = tip_h - self.k
        if target <= self.final_height:
            return
        cur = self.tip
        while True:
            if cur not in self.entries:
                return
            par, h = self.entries[cur]
            if h == target:
                self.final_hash = cur
                self.final_height = h
                return
            if h < target or cur == par:
                return
            cur = par

    def add_block(self, h, parent, height):
        if parent not in self.entries and h != GENESIS:
            return ("err", "unknown_parent")
        if h in self.entries:
            return ("err", "duplicate")
        if h != GENESIS and not self.is_ancestor(self.final_hash, parent):
            return ("err", "conflicts_finalized")
        self.entries[h] = (parent, height)
        tip_h = self.entries[self.tip][1]
        if height > tip_h:
            becomes = True
        elif height == tip_h:
            becomes = h < self.tip          # tie-break: lower hash wins
        else:
            becomes = False
        if becomes:
            self.tip = h
            self.advance_finality()
            return ("ok", True)
        return ("ok", False)

    def is_final(self, h):
        return self.is_ancestor(h, self.final_hash)


def fail(msg):
    raise SystemExit("FAIL: " + msg)


# ---------------------------------------------------------------------------
# 1. Scenario tests mirroring the Rust unit tests.
# ---------------------------------------------------------------------------
def test_finality_advances_monotonically():
    cs = ChainState(k=2)
    assert cs.final_height == 0
    cs.add_block(1, 0, 1); assert cs.final_height == 0
    cs.add_block(2, 1, 2); assert cs.final_height == 0
    cs.add_block(3, 2, 3); assert cs.final_height == 1
    cs.add_block(4, 3, 4); assert cs.final_height == 2
    print("ok  finality_advances_monotonically")


def test_conflict_rejected():
    cs = ChainState(k=2)
    for h, p, ht in [(1, 0, 1), (2, 1, 2), (3, 2, 3), (4, 3, 4)]:
        assert cs.add_block(h, p, ht)[0] == "ok"
    assert cs.final_height == 2
    # block forking at height 1 (below finalized height 2) -> rejected
    r = cs.add_block(99, 1, 2)
    if r != ("err", "conflicts_finalized"):
        fail(f"conflict block not rejected: {r}")
    assert 99 not in cs.entries
    print("ok  block_conflicting_with_finalized_history_is_rejected")


def test_equal_height_reorg_cannot_revert():
    cs = ChainState(k=2)
    for h, p, ht in [(0x10, 0, 1), (0x11, 0x10, 2), (0x12, 0x11, 3), (0x13, 0x12, 4)]:
        assert cs.add_block(h, p, ht)[0] == "ok"
    final_before = cs.final_hash
    assert cs.final_height == 2
    # rival forking at genesis with a LOWER hash -> must still be rejected
    r = cs.add_block(0x01, 0, 1)
    if r != ("err", "conflicts_finalized"):
        fail(f"equal-height deep reorg not rejected: {r}")
    assert cs.final_hash == final_before
    assert cs.is_final(0x11)
    print("ok  equal_height_reorg_cannot_revert_finalized_block")


def test_legal_reorg_above_finality():
    cs = ChainState(k=3)
    for h, p, ht in [(0xA1, 0, 1), (0xA2, 0xA1, 2), (0xA3, 0xA2, 3), (0xA4, 0xA3, 4)]:
        assert cs.add_block(h, p, ht)[0] == "ok"
    assert cs.final_height == 1
    assert cs.add_block(0xB3, 0xA2, 3)[0] == "ok"
    assert cs.add_block(0xB4, 0xB3, 4)[0] == "ok"
    switched = cs.add_block(0xB5, 0xB4, 5)
    if switched != ("ok", True):
        fail(f"legal reorg did not switch tip: {switched}")
    assert cs.tip == 0xB5
    assert cs.is_final(0xA1)
    print("ok  legal_reorg_above_finality_is_allowed")


# ---------------------------------------------------------------------------
# 2. Randomised property test: the core safety invariant.
#    No matter how blocks/forks arrive, a single node must never finalize
#    two conflicting blocks, and finality must only move forward.
# ---------------------------------------------------------------------------
def chain_to_genesis(entries, h):
    out = []
    cur = h
    while True:
        out.append(cur)
        par = entries[cur][0]
        if cur == par:
            break
        cur = par
    return out


def test_random_safety(seed, n_blocks, k):
    rng = random.Random(seed)
    cs = ChainState(k=k)
    next_hash = 1
    ever_final = []          # every distinct finalized block, in order
    prev_final_height = 0

    for _ in range(n_blocks):
        parent = rng.choice(list(cs.entries.keys()))
        h = next_hash
        next_hash += 1
        height = cs.entries[parent][1] + 1
        before_final = cs.final_hash
        cs.add_block(h, parent, height)

        # invariant A: finalized height never decreases
        if cs.final_height < prev_final_height:
            fail(f"finality went backwards: {prev_final_height} -> {cs.final_height}")
        prev_final_height = cs.final_height

        # invariant B: tip always descends from the finalized block
        if not cs.is_ancestor(cs.final_hash, cs.tip):
            fail("tip does not descend from finalized block")

        if cs.final_hash != before_final:
            ever_final.append(cs.final_hash)

    # invariant C: every block that was ever finalized lies on ONE chain
    # (each consecutive pair: the older is an ancestor of the newer). If two
    # conflicting blocks had both been finalized this would fail.
    for a, b in zip(ever_final, ever_final[1:]):
        if not cs.is_ancestor(a, b):
            fail(f"two finalized blocks conflict: {a} and {b}")

    # invariant D: once final, always final
    for fb in ever_final:
        if not cs.is_final(fb):
            fail(f"previously finalized block {fb} no longer final")

    return len(ever_final)


if __name__ == "__main__":
    test_finality_advances_monotonically()
    test_conflict_rejected()
    test_equal_height_reorg_cannot_revert()
    test_legal_reorg_above_finality()

    total_runs = 0
    total_final = 0
    for seed in range(400):
        k = (seed % 6) + 1
        nf = test_random_safety(seed, n_blocks=250, k=k)
        total_runs += 1
        total_final += nf
    print(f"ok  random_safety: {total_runs} runs, "
          f"{total_final} finalizations, all invariants held")
    print("ALL CHECKS PASSED")
