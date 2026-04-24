use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::Parser;

use craprs::complexity;
use craprs::coverage::{self, LineCoverage};
use craprs::crap::{self, CrapEntry};
use craprs::workspace;

#[derive(Parser)]
#[command(name = "craprs", version, about = "CRAP metric for Rust")]
struct Cli {
    /// Coverage tool to use
    #[arg(long, default_value = "tarpaulin")]
    coverage_tool: CoverageTool,

    /// Skip coverage generation, use existing lcov.info
    #[arg(long)]
    skip_coverage: bool,

    /// Project directory (where Cargo.toml lives)
    #[arg(short = 'C', long)]
    project_dir: Option<PathBuf>,

    /// Source directory (relative to project dir)
    #[arg(long, default_value = "src")]
    src: PathBuf,

    /// Analyze only specific workspace members (by package name)
    #[arg(short = 'p', long = "package")]
    packages: Vec<String>,

    /// Hide entries with CRAP below this threshold.
    /// Entries with no coverage data (uninstrumented files) are unaffected.
    #[arg(long, default_value_t = 0.0)]
    min_crap: f64,

    /// Show only the top N entries after sorting and filtering.
    #[arg(long)]
    top: Option<usize>,

    /// Include entries for source files not present in lcov.info (shown with `—`).
    /// By default these are suppressed and summarized in a trailing note.
    #[arg(long)]
    include_uninstrumented: bool,

    /// Module name fragments to filter by
    module_filters: Vec<String>,
}

#[derive(Clone, clap::ValueEnum)]
enum CoverageTool {
    Tarpaulin,
    LlvmCov,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(ref dir) = cli.project_dir {
        std::env::set_current_dir(dir)
            .with_context(|| format!("failed to cd into {}", dir.display()))?;
        if !Path::new("Cargo.toml").exists() {
            if Path::new("../Cargo.toml").exists() {
                if let Some(parent) = dir.parent() {
                    bail!(
                        "no Cargo.toml in {} — did you mean {}?",
                        dir.display(),
                        parent.display()
                    );
                }
            }
            bail!("no Cargo.toml found in {}", dir.display());
        }
    }

    let resolved = workspace::resolve_targets(Path::new("."), &cli.src, &cli.packages)?;

    if !cli.skip_coverage {
        delete_stale_coverage();
        run_coverage(&cli.coverage_tool, resolved.is_workspace, &cli.packages)?;
    }

    let lcov_content = std::fs::read_to_string("lcov.info")
        .context("failed to read lcov.info — did coverage run succeed?")?;
    let file_coverage = coverage::parse_lcov(&lcov_content);

    let mut all_entries = Vec::new();
    let mut uninstrumented_files: u64 = 0;
    for target in &resolved.targets {
        let sources = find_rust_sources(&target.src_dir)?;
        let sources = filter_sources(sources, &cli.module_filters);

        for source_path in &sources {
            let source = std::fs::read_to_string(source_path)
                .with_context(|| format!("failed to read {}", source_path.display()))?;
            let fns = complexity::extract_functions(&source);
            if fns.is_empty() {
                continue;
            }
            let module_path = coverage::source_to_module_path(source_path, &target.src_dir);
            let module_path = match &target.crate_name {
                Some(name) if !module_path.is_empty() => format!("{name}::{module_path}"),
                Some(name) => name.clone(),
                None => module_path,
            };
            let line_cov = find_coverage_for_file(source_path, &file_coverage);

            if line_cov.is_none() {
                uninstrumented_files += 1;
                if !cli.include_uninstrumented {
                    continue;
                }
            }

            for f in &fns {
                let (cov, score) = match &line_cov {
                    Some(lc) => {
                        let c = coverage::coverage_for_range(lc, f.start_line, f.end_line);
                        (Some(c), crap::crap_score(f.complexity, Some(c)))
                    }
                    None => (None, None),
                };
                all_entries.push(CrapEntry {
                    name: f.name.clone(),
                    module_path: module_path.clone(),
                    complexity: f.complexity,
                    coverage: cov,
                    crap: score,
                });
            }
        }
    }

    crap::sort_entries(&mut all_entries);
    let filtered = apply_filters(all_entries, cli.min_crap, cli.top);
    print!("{}", crap::format_report(&filtered));

    if uninstrumented_files > 0 && !cli.include_uninstrumented {
        println!(
            "note: {uninstrumented_files} source file(s) had no coverage data (not reached by the \
             executed test set). Pass --include-uninstrumented to list them."
        );
    }

    Ok(())
}

fn delete_stale_coverage() {
    let _ = std::fs::remove_file("lcov.info");
}

fn run_coverage(tool: &CoverageTool, is_workspace: bool, packages: &[String]) -> Result<()> {
    let (program, mut args): (&str, Vec<String>) = match tool {
        CoverageTool::Tarpaulin => (
            "cargo",
            vec![
                "tarpaulin".into(),
                "--out".into(),
                "lcov".into(),
                "--output-dir".into(),
                ".".into(),
            ],
        ),
        CoverageTool::LlvmCov => (
            "cargo",
            vec![
                "llvm-cov".into(),
                "--lcov".into(),
                "--output-path".into(),
                "lcov.info".into(),
            ],
        ),
    };

    // Scope coverage to match analysis scope.
    // When the user picked specific packages, pass them through.
    // Otherwise, if we're in a workspace, run all member tests.
    if !packages.is_empty() {
        for pkg in packages {
            args.push("-p".into());
            args.push(pkg.clone());
        }
    } else if is_workspace {
        args.push("--workspace".into());
    }

    let status = Command::new(program)
        .args(&args)
        .status()
        .with_context(|| format!("failed to run {program} {}", args.join(" ")))?;

    if !status.success() {
        bail!(
            "coverage command failed (exit {})",
            status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn find_rust_sources(src_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_rs_files(src_dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

pub fn filter_sources(files: Vec<PathBuf>, filters: &[String]) -> Vec<PathBuf> {
    if filters.is_empty() {
        return files;
    }
    files
        .into_iter()
        .filter(|f| {
            let s = f.to_string_lossy();
            filters.iter().any(|filt| s.contains(filt))
        })
        .collect()
}

/// Find coverage data for a source file. Tries canonical match first, then
/// literal match, then suffix match against a normalized form that strips any
/// leading `./`. Returns `None` when the file has no entry in lcov.info —
/// distinct from an entry that exists but has zero hits.
pub fn find_coverage_for_file(
    source_path: &Path,
    file_coverage: &HashMap<String, LineCoverage>,
) -> Option<LineCoverage> {
    // Best signal: canonical absolute path (tarpaulin emits absolutes).
    let canonical = source_path
        .canonicalize()
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    if let Some(ref c) = canonical {
        if let Some(cov) = file_coverage.get(c) {
            return Some(cov.clone());
        }
    }

    // Fall back to the literal string as we were given it.
    let source_str = source_path.to_string_lossy();
    if let Some(cov) = file_coverage.get(source_str.as_ref()) {
        return Some(cov.clone());
    }

    // Suffix match using a normalized form that strips leading `./`. Without
    // this, `./src/foo.rs` never matches `/abs/path/src/foo.rs` in lcov.
    let normalized = source_str.strip_prefix("./").unwrap_or(&source_str);
    for (lcov_path, cov) in file_coverage {
        if lcov_path.ends_with(normalized) || normalized.ends_with(lcov_path.as_str()) {
            return Some(cov.clone());
        }
    }

    None
}

/// Apply `--min-crap` and `--top` to a sorted entry list.
/// Entries with no CRAP score (uninstrumented) pass through the min-crap filter
/// untouched so they can still be displayed; they're already sunk to the bottom
/// by `sort_entries`.
pub fn apply_filters(
    entries: Vec<CrapEntry>,
    min_crap: f64,
    top: Option<usize>,
) -> Vec<CrapEntry> {
    let mut kept: Vec<CrapEntry> = entries
        .into_iter()
        .filter(|e| match e.crap {
            Some(s) => s >= min_crap,
            None => true,
        })
        .collect();
    if let Some(n) = top {
        kept.truncate(n);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_sources_no_filter() {
        let files = vec![PathBuf::from("src/foo.rs"), PathBuf::from("src/bar.rs")];
        let result = filter_sources(files.clone(), &[]);
        assert_eq!(result, files);
    }

    #[test]
    fn filter_sources_with_filter() {
        let files = vec![
            PathBuf::from("src/complexity.rs"),
            PathBuf::from("src/coverage.rs"),
            PathBuf::from("src/crap.rs"),
        ];
        let result = filter_sources(files, &["complexity".to_string()]);
        assert_eq!(result, vec![PathBuf::from("src/complexity.rs")]);
    }

    #[test]
    fn filter_sources_nested_match() {
        let files = vec![
            PathBuf::from("src/foo/bar.rs"),
            PathBuf::from("src/baz.rs"),
        ];
        let result = filter_sources(files, &["foo/bar".to_string()]);
        assert_eq!(result, vec![PathBuf::from("src/foo/bar.rs")]);
    }

    #[test]
    fn find_coverage_exact_match() {
        let mut file_cov = HashMap::new();
        let mut line_cov = LineCoverage::new();
        line_cov.insert(1, 5);
        file_cov.insert("src/main.rs".to_string(), line_cov);

        let result = find_coverage_for_file(Path::new("src/main.rs"), &file_cov);
        let cov = result.expect("expected Some");
        assert_eq!(cov.get(&1), Some(&5));
    }

    #[test]
    fn find_coverage_suffix_match() {
        let mut file_cov = HashMap::new();
        let mut line_cov = LineCoverage::new();
        line_cov.insert(1, 3);
        file_cov.insert("/home/user/project/src/main.rs".to_string(), line_cov);

        let result = find_coverage_for_file(Path::new("src/main.rs"), &file_cov);
        let cov = result.expect("expected Some via suffix match");
        assert_eq!(cov.get(&1), Some(&3));
    }

    #[test]
    fn find_coverage_no_match_returns_none() {
        let file_cov = HashMap::new();
        let result = find_coverage_for_file(Path::new("src/main.rs"), &file_cov);
        assert!(result.is_none(), "absent file must be None, not Some(empty)");
    }

    #[test]
    fn find_coverage_present_but_empty_returns_some_empty() {
        // Regression: tarpaulin emits SF: for files with no executable lines.
        // That is instrumented-but-empty — distinct from "not in the build at all".
        let mut file_cov = HashMap::new();
        file_cov.insert("src/empty.rs".to_string(), LineCoverage::new());

        let result = find_coverage_for_file(Path::new("src/empty.rs"), &file_cov);
        let cov = result.expect("entry exists, must be Some");
        assert!(cov.is_empty());
    }

    #[test]
    fn find_coverage_dot_slash_prefix_matches_absolute() {
        // Regression: `resolve_targets(Path::new("."), ...)` produces source paths
        // like `./src/foo.rs`, while tarpaulin writes absolute paths into lcov.info.
        // The suffix-match fallback must strip the leading `./` so the two forms align.
        let mut file_cov = HashMap::new();
        let mut line_cov = LineCoverage::new();
        line_cov.insert(1, 7);
        file_cov.insert(
            "/Users/dev/project/src/foo.rs".to_string(),
            line_cov,
        );

        let result = find_coverage_for_file(Path::new("./src/foo.rs"), &file_cov);
        let cov = result.expect("dot-slash prefix must still suffix-match");
        assert_eq!(cov.get(&1), Some(&7));
    }

    fn entry(name: &str, crap: Option<f64>) -> CrapEntry {
        CrapEntry {
            name: name.into(),
            module_path: String::new(),
            complexity: 1,
            coverage: crap.map(|_| 0.0),
            crap,
        }
    }

    #[test]
    fn filter_min_crap_drops_low_scores() {
        let entries = vec![
            entry("a", Some(50.0)),
            entry("b", Some(20.0)),
            entry("c", Some(5.0)),
        ];
        let kept = apply_filters(entries, 10.0, None);
        let names: Vec<&str> = kept.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn filter_top_truncates() {
        let entries = vec![
            entry("a", Some(50.0)),
            entry("b", Some(20.0)),
            entry("c", Some(5.0)),
        ];
        let kept = apply_filters(entries, 0.0, Some(1));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "a");
    }

    #[test]
    fn filter_combined() {
        let entries = vec![
            entry("a", Some(50.0)),
            entry("b", Some(20.0)),
            entry("c", Some(5.0)),
        ];
        let kept = apply_filters(entries, 10.0, Some(10));
        let names: Vec<&str> = kept.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn filter_min_crap_zero_is_noop() {
        let entries = vec![
            entry("a", Some(50.0)),
            entry("b", Some(20.0)),
            entry("c", Some(5.0)),
        ];
        let kept = apply_filters(entries, 0.0, None);
        assert_eq!(kept.len(), 3);
    }

    #[test]
    fn filter_top_zero_empties_body() {
        let entries = vec![entry("a", Some(50.0)), entry("b", Some(20.0))];
        let kept = apply_filters(entries, 0.0, Some(0));
        assert!(kept.is_empty());
    }

    #[test]
    fn filter_top_larger_than_entries_returns_all() {
        let entries = vec![entry("a", Some(50.0)), entry("b", Some(20.0))];
        let kept = apply_filters(entries, 0.0, Some(100));
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn filter_preserves_uninstrumented_regardless_of_min_crap() {
        // Entries with no CRAP score bypass --min-crap — we don't want to silently
        // drop files we have no data on, only to then print "0 files uninstrumented".
        let entries = vec![
            entry("covered", Some(50.0)),
            entry("uninstrumented", None),
            entry("low", Some(2.0)),
        ];
        let kept = apply_filters(entries, 10.0, None);
        let names: Vec<&str> = kept.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["covered", "uninstrumented"]);
    }

    #[test]
    fn filter_top_counts_uninstrumented_rows() {
        // --top truncates after sort/filter, so uninstrumented rows at the tail can be dropped.
        let entries = vec![
            entry("a", Some(50.0)),
            entry("b", Some(20.0)),
            entry("u", None),
        ];
        let kept = apply_filters(entries, 0.0, Some(2));
        let names: Vec<&str> = kept.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }
}
