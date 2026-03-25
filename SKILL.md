---
name: craprs
description: Use when the user asks for a CRAP report, cyclomatic complexity analysis, code quality metrics, or wants to identify risky functions in a Rust project. Triggers include "run craprs", "CRAP score", "code quality", "complexity analysis", "which functions need tests", or "risky code".
---

# craprs — CRAP Metric for Rust

Run the `craprs` CLI to compute CRAP (Change Risk Anti-Pattern) scores for every function in a Rust project. CRAP combines cyclomatic complexity with test coverage to surface functions that are both complex and under-tested.

## Workflow

1. **Check prerequisites**: Ensure `craprs` is installed. If not, tell the user to install it with `cargo install craprs`. Also ensure a coverage tool is available (`cargo-tarpaulin` by default, or `cargo-llvm-cov`).

2. **Run the analysis**: Execute `craprs` in the project directory. Choose flags based on the user's request:

   ```bash
   # Default: analyze current project with tarpaulin
   craprs

   # Analyze a different project
   craprs -C /path/to/project

   # Filter to specific modules
   craprs <module_name_fragment> [...]

   # Use llvm-cov instead of tarpaulin
   craprs --coverage-tool llvm-cov

   # Reuse existing lcov.info (faster, skips coverage generation)
   craprs --skip-coverage

   # Custom source directory (default: src)
   craprs --src lib

   # Workspace: analyze all member crates (auto-detected)
   craprs -C /path/to/workspace

   # Workspace: analyze only specific members
   craprs -C /path/to/workspace -p my-crate -p other-crate
   ```

3. **Present the results**: Summarize the output for the user:
   - Highlight the **worst offenders** (highest CRAP scores) first
   - Group findings by severity using the score table below
   - For each problematic function, briefly explain *why* it scored high (high complexity, low coverage, or both)

4. **Suggest actionable follow-ups** based on the results:
   - **CRAP > 30**: Recommend refactoring to reduce complexity *and* adding tests
   - **CRAP 5–30**: Suggest adding test coverage or simplifying logic
   - **CRAP 1–5**: No action needed — these are clean

## Score Interpretation

| CRAP Score | Meaning |
|------------|---------|
| 1–5        | Clean — low complexity, well tested |
| 5–30       | Moderate — consider refactoring or adding tests |
| 30+        | Crappy — high complexity with poor coverage, prioritize fixing |

## CLI Reference

| Flag | Description |
|------|-------------|
| `-C, --project-dir <DIR>` | Project directory (default: current directory) |
| `--coverage-tool <TOOL>` | `tarpaulin` (default) or `llvm-cov` |
| `--skip-coverage` | Skip coverage generation, reuse existing `lcov.info` |
| `--src <DIR>` | Source directory relative to project dir (default: `src`) |
| `-p, --package <NAME>` | Analyze only specific workspace members (repeatable) |
| `[MODULE_FILTERS...]` | Module name fragments to filter results |

## Notes

- `craprs` automatically detects Cargo workspaces and analyzes all member crates
- Module paths in workspace mode are prefixed with the crate name (e.g. `pubky_common::keys::auth`)
- `craprs` automatically skips `#[test]` functions and `#[cfg(test)]` modules
- Coverage generation can be slow on large projects — use `--skip-coverage` if `lcov.info` is already up to date
- If the user doesn't specify a coverage tool, default to tarpaulin
