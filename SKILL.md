---
name: craprs
description: Use when the user asks for a CRAP report, cyclomatic complexity analysis, or code quality metrics on a Rust project
---

# craprs — CRAP Metric for Rust

Computes the **CRAP** (Change Risk Anti-Pattern) score for every function in a Rust project. CRAP combines cyclomatic complexity with test coverage to identify functions that are both complex and under-tested.

## Setup

Requires `cargo-tarpaulin` (default) or `cargo-llvm-cov` for coverage:

```bash
cargo install cargo-tarpaulin
# or
cargo install cargo-llvm-cov
```

## Usage

```bash
# Analyze all source files under src/ (runs tarpaulin first)
cargo run

# Filter to specific modules
cargo run -- complexity coverage

# Use llvm-cov instead of tarpaulin
cargo run -- --coverage-tool llvm-cov

# Skip coverage generation, reuse existing lcov.info
cargo run -- --skip-coverage

# Custom source directory
cargo run -- --src lib
```

### Output

A table sorted by CRAP score (worst first):

```
CRAP Report
===========
Function                       Module                               CC   Cov%     CRAP
---------------------------------------------------------------------------------------
complex_fn                     my_crate::module                     12   45.0%    130.2
simple_fn                      my_crate::module                      1  100.0%      1.0
```

## Interpreting Scores

| CRAP Score | Meaning |
|-----------|---------|
| 1-5       | Clean — low complexity, well tested |
| 5-30      | Moderate — consider refactoring or adding tests |
| 30+       | Crappy — high complexity with poor coverage |

## How It Works

1. Deletes stale `lcov.info` and runs coverage tool (`cargo tarpaulin --out lcov` or `cargo llvm-cov --lcov`)
2. Finds all `.rs` files under `--src` directory (default `src/`)
3. Parses Rust AST with `syn` to extract functions (top-level fns, impl methods, trait default methods)
4. Computes cyclomatic complexity (if/while/for/loop/match arms/&&/||/? operator)
5. Parses LCOV output for per-line hit counts
6. Applies CRAP formula: `CC² × (1 - cov)³ + CC`
7. Sorts by CRAP score descending and prints report

Skips `#[test]` functions and `#[cfg(test)]` modules automatically.
