use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::Parser;

use craprs::complexity;
use craprs::coverage::{self, LineCoverage};
use craprs::crap::{self, CrapEntry};

#[derive(Parser)]
#[command(name = "craprs", about = "CRAP metric for Rust")]
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

    if !cli.skip_coverage {
        delete_stale_coverage();
        run_coverage(&cli.coverage_tool)?;
    }

    let lcov_content = std::fs::read_to_string("lcov.info")
        .context("failed to read lcov.info — did coverage run succeed?")?;
    let file_coverage = coverage::parse_lcov(&lcov_content);

    let mut sources = find_rust_sources(&cli.src)?;
    sources = filter_sources(sources, &cli.module_filters);

    let mut all_entries = Vec::new();
    for source_path in &sources {
        let source = std::fs::read_to_string(source_path)
            .with_context(|| format!("failed to read {}", source_path.display()))?;
        let fns = complexity::extract_functions(&source);
        let module_path = coverage::source_to_module_path(source_path, &cli.src);
        let line_cov = find_coverage_for_file(source_path, &file_coverage);

        for f in &fns {
            let cov = coverage::coverage_for_range(&line_cov, f.start_line, f.end_line);
            let score = crap::crap_score(f.complexity, cov);
            all_entries.push(CrapEntry {
                name: f.name.clone(),
                module_path: module_path.clone(),
                complexity: f.complexity,
                coverage: cov,
                crap: score,
            });
        }
    }

    crap::sort_entries(&mut all_entries);
    print!("{}", crap::format_report(&all_entries));

    Ok(())
}

fn delete_stale_coverage() {
    let _ = std::fs::remove_file("lcov.info");
}

fn run_coverage(tool: &CoverageTool) -> Result<()> {
    let (program, args): (&str, Vec<&str>) = match tool {
        CoverageTool::Tarpaulin => ("cargo", vec!["tarpaulin", "--out", "lcov"]),
        CoverageTool::LlvmCov => (
            "cargo",
            vec!["llvm-cov", "--lcov", "--output-path", "lcov.info"],
        ),
    };

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

/// Find coverage data for a source file. Try exact match, then suffix match.
pub fn find_coverage_for_file(
    source_path: &Path,
    file_coverage: &HashMap<String, LineCoverage>,
) -> LineCoverage {
    let source_str = source_path.to_string_lossy();

    // Try exact match
    if let Some(cov) = file_coverage.get(source_str.as_ref()) {
        return cov.clone();
    }

    // Try suffix match (handles absolute vs relative paths)
    for (lcov_path, cov) in file_coverage {
        if lcov_path.ends_with(source_str.as_ref()) || source_str.ends_with(lcov_path.as_str()) {
            return cov.clone();
        }
    }

    LineCoverage::new()
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
        assert_eq!(result.get(&1), Some(&5));
    }

    #[test]
    fn find_coverage_suffix_match() {
        let mut file_cov = HashMap::new();
        let mut line_cov = LineCoverage::new();
        line_cov.insert(1, 3);
        file_cov.insert(
            "/home/user/project/src/main.rs".to_string(),
            line_cov,
        );

        let result = find_coverage_for_file(Path::new("src/main.rs"), &file_cov);
        assert_eq!(result.get(&1), Some(&3));
    }

    #[test]
    fn find_coverage_no_match() {
        let file_cov = HashMap::new();
        let result = find_coverage_for_file(Path::new("src/main.rs"), &file_cov);
        assert!(result.is_empty());
    }
}
