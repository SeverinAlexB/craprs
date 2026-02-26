use std::collections::HashMap;
use std::path::Path;

/// Per-file line coverage: line_number -> hit_count
pub type LineCoverage = HashMap<usize, u64>;

/// Parse LCOV content into file -> line coverage map.
pub fn parse_lcov(content: &str) -> HashMap<String, LineCoverage> {
    let mut result: HashMap<String, LineCoverage> = HashMap::new();
    let mut current_file = String::new();
    let mut current_lines = LineCoverage::new();

    for line in content.lines() {
        let line = line.trim();
        if let Some(path) = line.strip_prefix("SF:") {
            current_file = path.to_string();
            current_lines = LineCoverage::new();
        } else if let Some(rest) = line.strip_prefix("DA:") {
            // DA:line_number,hit_count
            let mut parts = rest.splitn(2, ',');
            if let (Some(ln_str), Some(hits_str)) = (parts.next(), parts.next()) {
                if let (Ok(ln), Ok(hits)) = (ln_str.parse::<usize>(), hits_str.parse::<u64>()) {
                    current_lines.insert(ln, hits);
                }
            }
        } else if line == "end_of_record" {
            if !current_file.is_empty() {
                result.insert(current_file.clone(), std::mem::take(&mut current_lines));
            }
        }
    }
    result
}

/// Compute coverage percentage (0.0-100.0) for a line range.
pub fn coverage_for_range(line_cov: &LineCoverage, start: usize, end: usize) -> f64 {
    let mut instrumented = 0u64;
    let mut hit = 0u64;
    for ln in start..=end {
        if let Some(&count) = line_cov.get(&ln) {
            instrumented += 1;
            if count > 0 {
                hit += 1;
            }
        }
    }
    if instrumented == 0 {
        0.0
    } else {
        100.0 * (hit as f64) / (instrumented as f64)
    }
}

/// Convert a source path to a module path.
/// e.g. "src/foo/bar.rs" -> "foo::bar", "src/foo/mod.rs" -> "foo"
pub fn source_to_module_path(path: &Path, src_dir: &Path) -> String {
    let relative = path.strip_prefix(src_dir).unwrap_or(path);
    let s = relative.to_string_lossy();
    let s = s.strip_suffix(".rs").unwrap_or(&s);
    let s = s.replace('/', "::");
    let s = s.replace('\\', "::");
    // Strip trailing ::mod
    if s == "mod" {
        String::new()
    } else if let Some(prefix) = s.strip_suffix("::mod") {
        prefix.to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_lcov_basic() {
        let lcov = "\
SF:src/main.rs
DA:1,1
DA:2,0
DA:3,5
end_of_record
SF:src/lib.rs
DA:1,2
end_of_record
";
        let result = parse_lcov(lcov);
        assert_eq!(result.len(), 2);
        let main_cov = &result["src/main.rs"];
        assert_eq!(main_cov[&1], 1);
        assert_eq!(main_cov[&2], 0);
        assert_eq!(main_cov[&3], 5);
        let lib_cov = &result["src/lib.rs"];
        assert_eq!(lib_cov[&1], 2);
    }

    #[test]
    fn coverage_for_range_basic() {
        let mut cov = LineCoverage::new();
        cov.insert(3, 1);
        cov.insert(4, 1);
        cov.insert(5, 0);
        // 2 hit out of 3 instrumented = 66.67%
        let pct = coverage_for_range(&cov, 3, 5);
        assert!((pct - 66.666).abs() < 0.01);
    }

    #[test]
    fn coverage_for_range_empty() {
        let cov = LineCoverage::new();
        assert_eq!(coverage_for_range(&cov, 1, 5), 0.0);
    }

    #[test]
    fn coverage_for_range_full() {
        let mut cov = LineCoverage::new();
        cov.insert(1, 1);
        cov.insert(2, 3);
        assert_eq!(coverage_for_range(&cov, 1, 2), 100.0);
    }

    #[test]
    fn source_to_module_basic() {
        let src = PathBuf::from("src");
        assert_eq!(
            source_to_module_path(Path::new("src/foo/bar.rs"), &src),
            "foo::bar"
        );
    }

    #[test]
    fn source_to_module_mod_rs() {
        let src = PathBuf::from("src");
        assert_eq!(
            source_to_module_path(Path::new("src/foo/mod.rs"), &src),
            "foo"
        );
    }

    #[test]
    fn source_to_module_top_level() {
        let src = PathBuf::from("src");
        assert_eq!(
            source_to_module_path(Path::new("src/main.rs"), &src),
            "main"
        );
    }
}
