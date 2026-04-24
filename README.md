# craprs

**CRAP** (Change Risk Anti-Pattern) metric for Rust projects.

Combines cyclomatic complexity with test coverage to identify functions that are both complex and under-tested — the riskiest code to change.

## Quick Start

Install a coverage tool:

```bash
cargo install cargo-tarpaulin    # default
# or
cargo install cargo-llvm-cov
```

Run from your project root:

```bash
cargo install --path .           # install craprs
craprs                           # deletes old lcov.info, runs tarpaulin, analyzes
```

Or run directly from the craprs source:

```bash
cargo run                        # same thing, via cargo
```

## Output

```
CRAP Report
===========
Function                       Module                               CC   Cov%     CRAP
---------------------------------------------------------------------------------------
complex_fn                     my_crate::module                     12   45.0%    130.2
simple_fn                      my_crate::module                      1  100.0%      1.0
```

## Filtering

Pass module name fragments as arguments to filter:

```bash
craprs complexity coverage       # only files matching "complexity" or "coverage"
```

## Options

```
craprs [OPTIONS] [MODULE_FILTERS...]

Options:
  --coverage-tool <tarpaulin|llvm-cov>   Coverage tool [default: tarpaulin]
  --skip-coverage                        Reuse existing lcov.info
  -C, --project-dir <DIR>                Project / workspace root [default: .]
  --src <DIR>                            Source directory per crate [default: src]
  -p, --package <NAME>                   Limit analysis (and coverage) to workspace members
  --min-crap <N>                         Hide entries with CRAP below N [default: 0]
  --top <N>                              Show only the top N entries
  --include-uninstrumented               List files missing from lcov.info (rendered with `—`)
  -V, --version                          Print version
```

### Workspace behavior

When the project root is a Cargo workspace, craprs scopes coverage to match analysis:

- No `-p`: runs `cargo tarpaulin --workspace` so every member's tests execute.
- One or more `-p <name>`: runs `cargo tarpaulin -p <name> [-p <name>...]`.

Files absent from `lcov.info` are suppressed from the report by default and summarized in a single trailing note. Use `--include-uninstrumented` to list them explicitly with `—` in the Cov% / CRAP columns.

## CRAP Formula

```
CRAP(fn) = CC² × (1 - coverage)³ + CC
```

- **CC** = cyclomatic complexity (decision points + 1)
- **coverage** = fraction of lines covered by tests (from LCOV)

| Score | Risk |
|-------|------|
| 1-5   | Low — clean code |
| 5-30  | Moderate — refactor or add tests |
| 30+   | High — complex and under-tested |

## What It Counts

Decision points that increase cyclomatic complexity:

- `if` / `if let`
- `while` / `while let`
- `for`
- `loop`
- Each `match` arm
- `&&`, `||`
- `?` (try operator)

Closures contribute to their parent function's CC. Nested `fn` items are extracted separately. `#[test]` functions and `#[cfg(test)]` modules are skipped.

## Development

```bash
cargo test                       # run all tests
cargo run                        # run on own source
cargo run -- --skip-coverage     # reuse existing coverage data
```

---

Inspired by https://github.com/unclebob/crap4clj