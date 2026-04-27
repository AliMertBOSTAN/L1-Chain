# QuantumVault Testing Documentation Index

**Last Updated**: 2026-04-10

This index provides quick access to all testing-related documentation for the QuantumVault L1 blockchain project.

---

## Core Documentation

### Primary Documents

1. **[docs/TESTING_STRATEGY.md](TESTING_STRATEGY.md)** ⭐ START HERE
   - Comprehensive testing architecture
   - Test pyramid structure (80% unit, 15% integration, 5% property/fuzz)
   - Detailed test cases for every module
   - Coverage targets and performance benchmarks
   - Known-Answer Tests strategy
   - **Read Time**: 30-45 minutes
   - **Best For**: Understanding the overall testing approach

2. **[docs/ADR/001-testing-framework.md](ADR/001-testing-framework.md)**
   - Architecture decision record for framework selection
   - Why Google Test + Google Benchmark
   - Framework comparison (gtest vs Catch2 vs doctest)
   - CI/CD integration strategy
   - Implementation examples
   - **Read Time**: 20-30 minutes
   - **Best For**: Understanding framework decisions and rationale

3. **[docs/TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md)** ⭐ FOR DEVELOPERS
   - Quick start commands
   - Common test operations
   - Troubleshooting guide
   - Performance targets
   - Test implementation examples
   - **Read Time**: 10-15 minutes
   - **Best For**: Day-to-day development work

---

## Module-Specific Plans

### Crypto Module (Security-Critical)

**[tests/crypto/TEST_PLAN.md](../tests/crypto/TEST_PLAN.md)** ⭐ CRITICAL
- Complete test plan for cryptographic operations
- Dilithium signature tests (keygen, sign, verify, KATs)
- Kyber KEM tests (encapsulation, decapsulation, KATs)
- Hybrid KEM tests (X25519 + Kyber)
- Hash function tests (SHA3-256/512, BLAKE3, Argon2id)
- PRNG tests
- Performance benchmarks for each primitive
- Coverage target: 95%
- **Read Time**: 45-60 minutes
- **Best For**: Implementing crypto module tests

### Core Module Tests
*Coming soon*: tests/core/TEST_PLAN.md
- UTXO model tests
- Transaction validation tests
- Block construction and validation tests
- Coverage target: 90%

### Consensus Module Tests
*Coming soon*: tests/consensus/TEST_PLAN.md
- PoW difficulty adjustment tests
- PoS committee selection tests
- Finality threshold tests
- Coverage target: 85%

---

## Developer Resources

### Test Template

**[tests/TEST_TEMPLATE.cpp](../tests/TEST_TEMPLATE.cpp)** ⭐ FOR NEW TESTS
- Complete template for writing unit tests
- Examples of:
  - Standalone tests (TEST macro)
  - Test fixtures (TEST_F macro)
  - Parameterized tests (TEST_P macro)
  - Custom matchers
  - Death tests
  - Best practices
- Assertion reference
- Running test commands
- **Read Time**: 20 minutes
- **Best For**: Creating new test files

### CMake Configuration

**[tests/CMakeLists.txt](../tests/CMakeLists.txt)**
- Comprehensive test infrastructure
- Auto-discovery of test files
- Test and benchmark helper functions
- Aggregate test targets
- Coverage integration
- **Read Time**: 15 minutes
- **Best For**: Understanding build system integration

**[CMakeLists.txt](../CMakeLists.txt)** (root)
- Testing configuration options
- Coverage support (gcov + LCOV)
- Sanitizer integration
- Build system status messages
- **Read Time**: 10 minutes

---

## Summary Documents

### Setup Summary

**[TESTING_SETUP_SUMMARY.md](../TESTING_SETUP_SUMMARY.md)** ⭐ OVERVIEW
- Complete summary of testing infrastructure
- What was created (files, features)
- Quick start guide
- Test organization
- CI/CD integration
- Development workflow
- Future enhancements
- **Read Time**: 15-20 minutes
- **Best For**: High-level overview and next steps

---

## Quick Links by Task

### "I want to run tests"
1. Read: [docs/TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md) → "Quick Start" section
2. Execute: `cmake --preset dev && ninja -C build && ctest --test-dir build`

### "I want to write new tests"
1. Read: [tests/TEST_TEMPLATE.cpp](../tests/TEST_TEMPLATE.cpp)
2. Copy template to `tests/<module>/test_<feature>.cpp`
3. Implement test cases
4. Run: `ctest --test-dir build -R <feature>`

### "I want to implement crypto tests"
1. Read: [tests/crypto/TEST_PLAN.md](../tests/crypto/TEST_PLAN.md)
2. Create `tests/crypto/test_dilithium.cpp`, `test_kyber.cpp`, etc.
3. Follow the test case specifications
4. Integrate Known-Answer Tests from official specs

### "I want to understand framework decisions"
1. Read: [docs/ADR/001-testing-framework.md](ADR/001-testing-framework.md)
2. Review framework comparison table
3. Check implementation examples (Appendix A, B)

### "I want to check code coverage"
1. Read: [docs/TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md) → "Coverage Analysis" section
2. Execute: `cmake --preset dev -DENABLE_COVERAGE=ON && ninja -C build run_tests_with_coverage`
3. Open: `build/coverage/index.html`

### "I want to run benchmarks"
1. Read: [docs/TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md) → "Benchmarks" section
2. Execute: `ninja -C build run_benchmarks`
3. View: `build/results/*.json`

### "I want to set up CI/CD"
1. Read: [docs/ADR/001-testing-framework.md](ADR/001-testing-framework.md) → "CI Integration" section
2. Read: [TESTING_SETUP_SUMMARY.md](../TESTING_SETUP_SUMMARY.md) → "CI/CD Integration"
3. Implement GitHub Actions workflow

### "I have a failing test"
1. Read: [docs/TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md) → "Troubleshooting" section
2. Check console output for error message
3. Reproduce with: `ctest --test-dir build -R <test_name> --verbose`

---

## File Organization

```
QuantumVault Project Root/
├── docs/
│   ├── TESTING_INDEX.md                    ← You are here
│   ├── TESTING_STRATEGY.md                 ⭐ Comprehensive strategy
│   ├── TESTING_QUICK_REFERENCE.md          ⭐ Developer quick guide
│   └── ADR/
│       └── 001-testing-framework.md        ⭐ Framework decision
├── tests/
│   ├── CMakeLists.txt                      ⭐ Test infrastructure
│   ├── TEST_TEMPLATE.cpp                   ⭐ Template for new tests
│   ├── crypto/
│   │   ├── test_*.cpp                      (TBD: Implementation)
│   │   ├── bench_*.cpp                     (TBD: Implementation)
│   │   ├── TEST_PLAN.md                    ⭐ Crypto test plan
│   │   └── kat/                            (TBD: KAT vectors)
│   ├── core/                               (Test structure TBD)
│   ├── consensus/                          (Test structure TBD)
│   ├── privacy/                            (Test structure TBD)
│   ├── vm/                                 (Test structure TBD)
│   ├── storage/                            (Test structure TBD)
│   ├── da/                                 (Test structure TBD)
│   ├── net/                                (Test structure TBD)
│   ├── rpc/                                (Test structure TBD)
│   └── integration/                        (Test structure TBD)
├── CMakeLists.txt                          ⭐ Root build config
├── TESTING_SETUP_SUMMARY.md                ⭐ Complete summary
└── CLAUDE.md                               (Project instructions)
```

⭐ = Essential documents

---

## Document Relationships

```
TESTING_INDEX.md (this file)
    ├─→ TESTING_STRATEGY.md (comprehensive overview)
    │   ├─→ ADR/001-testing-framework.md (why these tools)
    │   ├─→ tests/crypto/TEST_PLAN.md (detailed crypto tests)
    │   └─→ TESTING_QUICK_REFERENCE.md (how to use)
    │
    ├─→ TESTING_SETUP_SUMMARY.md (what was created)
    │   └─→ TESTING_QUICK_REFERENCE.md (how to use)
    │
    ├─→ tests/TEST_TEMPLATE.cpp (write new tests)
    │
    ├─→ tests/CMakeLists.txt (build system)
    │
    └─→ CMakeLists.txt (root configuration)
```

---

## Coverage Targets by Module

| Module | Target | Plan | Notes |
|--------|--------|------|-------|
| **crypto/** | 95% | [TEST_PLAN.md](../tests/crypto/TEST_PLAN.md) | Security-critical |
| **core/** | 90% | Coming soon | Data structure integrity |
| **consensus/** | 85% | Coming soon | Algorithm correctness |
| **privacy/** | 85% | Coming soon | Privacy logic |
| **vm/** | 90% | Coming soon | Execution correctness |
| **storage/** | 80% | Coming soon | Persistence |
| **da/** | 85% | Coming soon | Coding theory |
| **net/** | 75% | Coming soon | Network protocols |
| **rpc/** | 70% | Coming soon | API layer |

---

## Testing Timeline

### Phase 1: Foundation (Weeks 1-2) ✅ COMPLETE
- [x] Testing strategy documentation
- [x] Framework selection and ADR
- [x] CMake test infrastructure
- [x] Developer templates and guides

### Phase 2: Crypto Module (Weeks 3-4) 🔄 IN PROGRESS
- [ ] test_dilithium.cpp (KATs, roundtrips)
- [ ] test_kyber.cpp (KATs, roundtrips)
- [ ] test_hybrid_kem.cpp
- [ ] test_hash.cpp (NIST KATs)
- [ ] test_rng.cpp
- [ ] Benchmarks (Dilithium, Kyber, Hash)
- [ ] Achieve 95% coverage

### Phase 3: Core & Consensus (Weeks 5-6) 🔜 PENDING
- [ ] test_transaction.cpp
- [ ] test_block.cpp
- [ ] test_utxo.cpp
- [ ] test_pow.cpp
- [ ] test_pos.cpp
- [ ] test_validation.cpp
- [ ] Achieve 85-90% coverage

### Phase 4: Integration & CI (Weeks 7-8) 🔜 PENDING
- [ ] Integration tests
- [ ] GitHub Actions setup
- [ ] Coverage reporting
- [ ] Performance regression detection

### Phase 5: Advanced (Future) 🔜 FUTURE
- [ ] libFuzzer integration
- [ ] RapidCheck property tests
- [ ] Formal verification (TLA+)

---

## References

### Testing Frameworks
- [Google Test (gtest)](https://google.github.io/googletest/)
- [Google Benchmark](https://github.com/google/benchmark/wiki)
- [LCOV Code Coverage](https://github.com/linux-test-project/lcov)

### Cryptographic Standards
- [NIST FIPS 203 (ML-KEM)](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.203.pdf)
- [NIST FIPS 204 (ML-DSA)](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.204.pdf)
- [Open Quantum Safe](https://liboqs.org/)
- [NIST CAVP](https://csrc.nist.gov/projects/cryptographic-algorithm-validation-program/)

### Best Practices
- [Test Pyramid Pattern](https://martinfowler.com/bliki/TestPyramid.html)
- [Test-Driven Development](https://en.wikipedia.org/wiki/Test-driven_development)
- [Known-Answer Tests](https://nvlpubs.nist.gov/nistpubs/Legacy/SP/nistspecialpublication800-135r1.pdf)

---

## Document Statistics

| Document | Lines | Purpose | Audience |
|----------|-------|---------|----------|
| TESTING_STRATEGY.md | 900+ | Comprehensive strategy | Architects, Leads |
| ADR/001-testing-framework.md | 500+ | Framework decision | Architects, Decision makers |
| TESTING_QUICK_REFERENCE.md | 400+ | Developer guide | All developers |
| tests/crypto/TEST_PLAN.md | 650+ | Crypto test details | Crypto developers |
| TESTING_SETUP_SUMMARY.md | 400+ | Setup overview | All team members |
| TEST_TEMPLATE.cpp | 400+ | Template for tests | Test implementers |
| CMakeLists.txt (tests) | 300+ | Test infrastructure | Build engineers |

**Total**: ~3,550 lines of testing documentation

---

## Support and Questions

### Common Questions

**Q: Which document should I read first?**  
A: Start with [TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md) for a quick overview, then [TESTING_STRATEGY.md](TESTING_STRATEGY.md) for comprehensive details.

**Q: How do I write tests?**  
A: See [tests/TEST_TEMPLATE.cpp](../tests/TEST_TEMPLATE.cpp) for examples, then read [tests/crypto/TEST_PLAN.md](../tests/crypto/TEST_PLAN.md) for module-specific guidance.

**Q: What are Known-Answer Tests?**  
A: See "Known-Answer Tests (KATs) Strategy" in [TESTING_STRATEGY.md](TESTING_STRATEGY.md) and "Dilithium KATs" in [tests/crypto/TEST_PLAN.md](../tests/crypto/TEST_PLAN.md).

**Q: How do I check code coverage?**  
A: See "Coverage Analysis" in [TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md).

**Q: How do I run benchmarks?**  
A: See "Benchmarks" in [TESTING_QUICK_REFERENCE.md](TESTING_QUICK_REFERENCE.md).

---

## Document Status

| Document | Status | Completeness | Review |
|----------|--------|--------------|--------|
| TESTING_INDEX.md | ✅ COMPLETE | 100% | 2026-04-10 |
| TESTING_STRATEGY.md | ✅ APPROVED | 100% | 2026-04-10 |
| ADR/001-testing-framework.md | ✅ APPROVED | 100% | 2026-04-10 |
| TESTING_QUICK_REFERENCE.md | ✅ APPROVED | 100% | 2026-04-10 |
| tests/crypto/TEST_PLAN.md | ✅ APPROVED | 100% | 2026-04-10 |
| TESTING_SETUP_SUMMARY.md | ✅ APPROVED | 100% | 2026-04-10 |
| TEST_TEMPLATE.cpp | ✅ APPROVED | 100% | 2026-04-10 |
| CMakeLists.txt (tests) | ✅ COMPLETE | 100% | 2026-04-10 |

---

## Next Steps

1. **Implement crypto tests** (Week 3-4)
   - Use [tests/crypto/TEST_PLAN.md](../tests/crypto/TEST_PLAN.md)
   - Reference [tests/TEST_TEMPLATE.cpp](../tests/TEST_TEMPLATE.cpp)
   - Target: 95% coverage

2. **Implement core & consensus tests** (Week 5-6)
   - Follow same pattern as crypto
   - Create separate test plans
   - Target: 85-90% coverage

3. **Setup CI/CD** (Week 7-8)
   - Implement GitHub Actions
   - Configure coverage reporting
   - Setup performance benchmarking

4. **Maintain and expand** (Ongoing)
   - Add tests as code is developed
   - Monitor coverage metrics
   - Track performance regressions

---

**Created**: 2026-04-10  
**Last Updated**: 2026-04-10  
**Next Review**: 2026-05-10  
**Status**: ACTIVE

---

## Navigation

- **Up**: [docs/](../docs/) directory
- **Projects**: [L1 Blockchain](../)
- **Index**: You are here
