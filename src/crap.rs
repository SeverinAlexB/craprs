pub struct CrapEntry {
    pub name: String,
    pub module_path: String,
    pub complexity: u32,
    /// `None` means the source file was not instrumented by the executed test set
    /// (no entry in lcov.info). `Some(pct)` is real observed coverage, 0.0–100.0.
    pub coverage: Option<f64>,
    /// `None` whenever `coverage` is `None` — we can't score without data.
    pub crap: Option<f64>,
}

/// CRAP = CC^2 * (1 - coverage)^3 + CC. Returns `None` when coverage is unknown.
pub fn crap_score(complexity: u32, coverage_pct: Option<f64>) -> Option<f64> {
    let pct = coverage_pct?;
    let cc = complexity as f64;
    let uncov = 1.0 - pct / 100.0;
    Some(cc * cc * uncov * uncov * uncov + cc)
}

/// Sort descending by CRAP. `None` CRAP entries sink to the bottom, preserving
/// their input order (stable — `sort_by` is Rust's stable sort).
pub fn sort_entries(entries: &mut [CrapEntry]) {
    entries.sort_by(|a, b| match (a.crap, b.crap) {
        // Both scored: higher CRAP first.
        (Some(ax), Some(bx)) => bx.partial_cmp(&ax).unwrap_or(std::cmp::Ordering::Equal),
        // `a` scored, `b` not: `a` goes first.
        (Some(_), None) => std::cmp::Ordering::Less,
        // `a` unscored, `b` scored: `a` goes last.
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
}

const DASH: &str = "—";

pub fn format_report(entries: &[CrapEntry]) -> String {
    let header = format!(
        "{:<30} {:<45} {:>4} {:>6} {:>8}",
        "Function", "Module", "CC", "Cov%", "CRAP"
    );
    let sep = "-".repeat(header.len());
    let mut lines = vec!["CRAP Report".to_string(), "===========".to_string(), header, sep];
    for e in entries {
        let cov_cell = match e.coverage {
            Some(pct) => format!("{pct:>5.1}%"),
            None => format!("{DASH:>6}"),
        };
        let crap_cell = match e.crap {
            Some(s) => format!("{s:>8.1}"),
            None => format!("{DASH:>8}"),
        };
        lines.push(format!(
            "{:<30} {:<45} {:>4} {cov_cell} {crap_cell}",
            e.name, e.module_path, e.complexity
        ));
    }
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_full_coverage() {
        assert_eq!(crap_score(5, Some(100.0)), Some(5.0));
    }

    #[test]
    fn score_zero_coverage() {
        assert_eq!(crap_score(5, Some(0.0)), Some(30.0));
    }

    #[test]
    fn score_partial_coverage() {
        // CC=8, cov=45 => 64 * (0.55)^3 + 8 = 64 * 0.166375 + 8 = 18.648
        let score = crap_score(8, Some(45.0)).unwrap();
        assert!((score - 18.648).abs() < 0.01);
    }

    #[test]
    fn score_trivial_full_coverage() {
        assert_eq!(crap_score(1, Some(100.0)), Some(1.0));
    }

    #[test]
    fn score_missing_coverage_is_none() {
        assert_eq!(crap_score(5, None), None);
        assert_eq!(crap_score(1, None), None);
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
    fn sort_descending_by_crap() {
        let mut entries = vec![
            entry("a", Some(10.0)),
            entry("b", Some(50.0)),
            entry("c", Some(1.0)),
        ];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["b", "a", "c"]);
    }

    #[test]
    fn sort_sinks_none_crap_to_bottom_preserving_order() {
        let mut entries = vec![
            entry("a", None),
            entry("b", Some(50.0)),
            entry("c", None),
            entry("d", Some(10.0)),
        ];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        // Some-entries sorted desc, then None-entries in original order (a, c).
        assert_eq!(names, vec!["b", "d", "a", "c"]);
    }

    #[test]
    fn sort_all_none_is_stable() {
        let mut entries = vec![entry("a", None), entry("b", None), entry("c", None)];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn format_report_contains_data() {
        let entries = vec![CrapEntry {
            name: "foo".into(),
            module_path: "test::bar".into(),
            complexity: 3,
            coverage: Some(85.0),
            crap: Some(4.5),
        }];
        let report = format_report(&entries);
        assert!(report.contains("foo"));
        assert!(report.contains("test::bar"));
        assert!(report.contains("CRAP"));
        assert!(report.contains("85.0%"));
    }

    #[test]
    fn format_report_renders_none_as_dash() {
        let entries = vec![CrapEntry {
            name: "uncovered".into(),
            module_path: "mod::x".into(),
            complexity: 4,
            coverage: None,
            crap: None,
        }];
        let report = format_report(&entries);
        assert!(report.contains("uncovered"));
        assert!(report.contains(DASH));
        // Make sure we didn't print a spurious 0.0%.
        assert!(!report.contains("0.0%"));
    }
}
