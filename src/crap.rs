pub struct CrapEntry {
    pub name: String,
    pub module_path: String,
    pub complexity: u32,
    pub coverage: f64,
    pub crap: f64,
}

/// CRAP = CC^2 * (1 - coverage)^3 + CC
pub fn crap_score(complexity: u32, coverage_pct: f64) -> f64 {
    let cc = complexity as f64;
    let uncov = 1.0 - coverage_pct / 100.0;
    cc * cc * uncov * uncov * uncov + cc
}

pub fn sort_entries(entries: &mut Vec<CrapEntry>) {
    entries.sort_by(|a, b| b.crap.partial_cmp(&a.crap).unwrap_or(std::cmp::Ordering::Equal));
}

pub fn format_report(entries: &[CrapEntry]) -> String {
    let header = format!(
        "{:<30} {:<35} {:>4} {:>6} {:>8}",
        "Function", "Module", "CC", "Cov%", "CRAP"
    );
    let sep = "-".repeat(header.len());
    let mut lines = vec!["CRAP Report".to_string(), "===========".to_string(), header, sep];
    for e in entries {
        lines.push(format!(
            "{:<30} {:<35} {:>4} {:>5.1}% {:>8.1}",
            e.name, e.module_path, e.complexity, e.coverage, e.crap
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
        assert_eq!(crap_score(5, 100.0), 5.0);
    }

    #[test]
    fn score_zero_coverage() {
        assert_eq!(crap_score(5, 0.0), 30.0);
    }

    #[test]
    fn score_partial_coverage() {
        // CC=8, cov=45 => 64 * (0.55)^3 + 8 = 64 * 0.166375 + 8 = 18.648
        let score = crap_score(8, 45.0);
        assert!((score - 18.648).abs() < 0.01);
    }

    #[test]
    fn score_trivial_full_coverage() {
        assert_eq!(crap_score(1, 100.0), 1.0);
    }

    #[test]
    fn sort_descending_by_crap() {
        let mut entries = vec![
            CrapEntry { name: "a".into(), module_path: String::new(), complexity: 1, coverage: 0.0, crap: 10.0 },
            CrapEntry { name: "b".into(), module_path: String::new(), complexity: 1, coverage: 0.0, crap: 50.0 },
            CrapEntry { name: "c".into(), module_path: String::new(), complexity: 1, coverage: 0.0, crap: 1.0 },
        ];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["b", "a", "c"]);
    }

    #[test]
    fn format_report_contains_data() {
        let entries = vec![CrapEntry {
            name: "foo".into(),
            module_path: "test::bar".into(),
            complexity: 3,
            coverage: 85.0,
            crap: 4.5,
        }];
        let report = format_report(&entries);
        assert!(report.contains("foo"));
        assert!(report.contains("test::bar"));
        assert!(report.contains("CRAP"));
    }
}
