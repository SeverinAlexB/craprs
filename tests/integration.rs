use std::path::Path;

use craprs::complexity;
use craprs::coverage;
use craprs::crap;

#[test]
fn full_pipeline_synthetic() {
    // Synthetic Rust source
    let source = r#"
fn simple() -> i32 {
    42
}

fn branchy(x: bool, y: Option<i32>) -> i32 {
    if x {
        if let Some(v) = y {
            v
        } else {
            0
        }
    } else {
        -1
    }
}
"#;

    // Synthetic LCOV data
    let lcov = "\
SF:src/example.rs
DA:2,1
DA:3,1
DA:7,5
DA:8,3
DA:9,3
DA:10,3
DA:12,2
DA:15,2
end_of_record
";

    // Step 1: Extract functions
    let fns = complexity::extract_functions(source);
    assert_eq!(fns.len(), 2);
    assert_eq!(fns[0].name, "simple");
    assert_eq!(fns[0].complexity, 1);
    assert_eq!(fns[1].name, "branchy");
    assert_eq!(fns[1].complexity, 3); // if + if let = 2 + base 1

    // Step 2: Parse LCOV
    let file_cov = coverage::parse_lcov(lcov);
    assert!(file_cov.contains_key("src/example.rs"));
    let line_cov = &file_cov["src/example.rs"];

    // Step 3: Compute coverage per function
    let simple_cov = coverage::coverage_for_range(line_cov, fns[0].start_line, fns[0].end_line);
    let branchy_cov = coverage::coverage_for_range(line_cov, fns[1].start_line, fns[1].end_line);
    assert!(simple_cov > 0.0);
    assert!(branchy_cov > 0.0);

    // Step 4: Compute CRAP scores
    let module_path = coverage::source_to_module_path(Path::new("src/example.rs"), Path::new("src"));
    assert_eq!(module_path, "example");

    let mut entries: Vec<crap::CrapEntry> = fns
        .iter()
        .map(|f| {
            let cov = coverage::coverage_for_range(line_cov, f.start_line, f.end_line);
            let score = crap::crap_score(f.complexity, cov);
            crap::CrapEntry {
                name: f.name.clone(),
                module_path: module_path.clone(),
                complexity: f.complexity,
                coverage: cov,
                crap: score,
            }
        })
        .collect();

    // Step 5: Sort and format
    crap::sort_entries(&mut entries);
    let report = crap::format_report(&entries);

    assert!(report.contains("CRAP Report"));
    assert!(report.contains("simple"));
    assert!(report.contains("branchy"));
    assert!(report.contains("example"));

    // Verify sorted descending — branchy should be first (higher CC, same or lower coverage)
    assert_eq!(entries[0].name, "branchy");
    assert_eq!(entries[1].name, "simple");

    // Verify CRAP formula: simple is CC=1, 100% covered => CRAP = 1.0
    assert_eq!(entries[1].complexity, 1);
    assert_eq!(entries[1].coverage, 100.0);
    assert!((entries[1].crap - 1.0).abs() < 0.001);
}

#[test]
fn module_path_variations() {
    let src = Path::new("src");
    assert_eq!(
        coverage::source_to_module_path(Path::new("src/lib.rs"), src),
        "lib"
    );
    assert_eq!(
        coverage::source_to_module_path(Path::new("src/foo/bar.rs"), src),
        "foo::bar"
    );
    assert_eq!(
        coverage::source_to_module_path(Path::new("src/foo/mod.rs"), src),
        "foo"
    );
}

#[test]
fn empty_source_produces_no_entries() {
    let fns = complexity::extract_functions("// empty file\n");
    assert!(fns.is_empty());
}

#[test]
fn lcov_with_no_matching_file() {
    let lcov = "\
SF:src/other.rs
DA:1,1
end_of_record
";
    let file_cov = coverage::parse_lcov(lcov);
    let line_cov = file_cov.get("src/example.rs");
    assert!(line_cov.is_none());

    // No coverage data means 0% coverage
    let empty_cov = coverage::LineCoverage::new();
    let cov = coverage::coverage_for_range(&empty_cov, 1, 10);
    assert_eq!(cov, 0.0);
}
