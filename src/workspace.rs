use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// One analysis target — a single crate (or the whole project for non-workspaces).
pub struct CrateTarget {
    /// Rust crate name (hyphens → underscores). `None` for single-crate projects.
    pub crate_name: Option<String>,
    /// Path to the crate's source directory, e.g. `pubky-common/src`.
    pub src_dir: PathBuf,
}

/// Result of workspace resolution: the analysis targets plus whether the root is a workspace.
pub struct ResolvedWorkspace {
    pub targets: Vec<CrateTarget>,
    /// True if the root `Cargo.toml` has a `[workspace]` table.
    pub is_workspace: bool,
}

/// Detect workspace vs single crate and return the list of analysis targets.
///
/// * `root` — project root directory (where the root `Cargo.toml` lives).
/// * `src_rel` — source directory relative to each crate root (default `src`).
/// * `packages` — if non-empty, only include members whose package name matches.
pub fn resolve_targets(
    root: &Path,
    src_rel: &Path,
    packages: &[String],
) -> Result<ResolvedWorkspace> {
    let cargo_path = root.join("Cargo.toml");
    let cargo_toml = std::fs::read_to_string(&cargo_path)
        .with_context(|| format!("failed to read {}", cargo_path.display()))?;
    let doc: toml::Value = cargo_toml
        .parse()
        .with_context(|| format!("failed to parse {}", cargo_path.display()))?;

    let workspace = match doc.get("workspace") {
        Some(ws) => ws,
        None => {
            // Single-crate project.
            return Ok(ResolvedWorkspace {
                targets: vec![CrateTarget {
                    crate_name: None,
                    src_dir: root.join(src_rel),
                }],
                is_workspace: false,
            });
        }
    };

    // --- workspace mode ---

    let members = workspace
        .get("members")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let excludes = workspace
        .get("exclude")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut member_dirs = expand_members(root, &members, &excludes)?;

    // If root also has [package], include it as an implicit member.
    if doc.get("package").is_some() && !member_dirs.contains(&root.to_path_buf()) {
        member_dirs.push(root.to_path_buf());
    }

    member_dirs.sort();

    let mut targets = Vec::new();
    for dir in &member_dirs {
        let pkg_name = read_package_name(dir)?;
        let rust_name = pkg_name.replace('-', "_");

        // Apply --package filter (match against either form).
        if !packages.is_empty()
            && !packages.iter().any(|p| p == &pkg_name || p == &rust_name)
        {
            continue;
        }

        targets.push(CrateTarget {
            crate_name: Some(rust_name),
            src_dir: dir.join(src_rel),
        });
    }

    if targets.is_empty() && !packages.is_empty() {
        bail!(
            "no workspace members matched --package {:?}",
            packages
        );
    }

    Ok(ResolvedWorkspace {
        targets,
        is_workspace: true,
    })
}

/// Expand glob patterns from `members`, then subtract `excludes`.
fn expand_members(root: &Path, members: &[String], excludes: &[String]) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();

    for pattern in members {
        let full_pattern = root.join(pattern);
        let full_str = full_pattern.to_string_lossy();
        for entry in glob::glob(&full_str)
            .with_context(|| format!("invalid glob pattern: {pattern}"))?
        {
            let path = entry.with_context(|| format!("glob error for {pattern}"))?;
            if path.is_dir() && path.join("Cargo.toml").exists() {
                dirs.push(path);
            }
        }
    }

    if !excludes.is_empty() {
        let exclude_set: Vec<PathBuf> = excludes.iter().map(|e| root.join(e)).collect();
        dirs.retain(|d| !exclude_set.contains(d));
    }

    Ok(dirs)
}

/// Read `package.name` from a crate's `Cargo.toml`.
fn read_package_name(crate_dir: &Path) -> Result<String> {
    let path = crate_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let doc: toml::Value = content
        .parse()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(String::from)
        .with_context(|| format!("no package.name in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn tempdir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "craprs_test_{}_{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_single_crate(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .unwrap();
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("lib.rs"), "").unwrap();
    }

    #[test]
    fn single_crate_returns_one_target() {
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"solo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();

        let resolved = resolve_targets(&tmp, Path::new("src"), &[]).unwrap();
        assert!(!resolved.is_workspace);
        assert_eq!(resolved.targets.len(), 1);
        assert!(resolved.targets[0].crate_name.is_none());
        assert_eq!(resolved.targets[0].src_dir, tmp.join("src"));
    }

    #[test]
    fn workspace_discovers_members() {
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crate-a\", \"crate-b\"]\n",
        )
        .unwrap();
        write_single_crate(&tmp.join("crate-a"), "crate-a");
        write_single_crate(&tmp.join("crate-b"), "crate-b");

        let resolved = resolve_targets(&tmp, Path::new("src"), &[]).unwrap();
        assert!(resolved.is_workspace);
        assert_eq!(resolved.targets.len(), 2);
        assert_eq!(resolved.targets[0].crate_name.as_deref(), Some("crate_a"));
        assert_eq!(resolved.targets[0].src_dir, tmp.join("crate-a/src"));
        assert_eq!(resolved.targets[1].crate_name.as_deref(), Some("crate_b"));
    }

    #[test]
    fn workspace_glob_expansion() {
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"my-*\"]\n",
        )
        .unwrap();
        write_single_crate(&tmp.join("my-alpha"), "my-alpha");
        write_single_crate(&tmp.join("my-beta"), "my-beta");
        // Directory without Cargo.toml should be ignored.
        fs::create_dir_all(tmp.join("my-empty")).unwrap();

        let resolved = resolve_targets(&tmp, Path::new("src"), &[]).unwrap();
        assert!(resolved.is_workspace);
        assert_eq!(resolved.targets.len(), 2);
    }

    #[test]
    fn workspace_exclude() {
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\nexclude = [\"b\"]\n",
        )
        .unwrap();
        write_single_crate(&tmp.join("a"), "a");
        write_single_crate(&tmp.join("b"), "b");
        write_single_crate(&tmp.join("c"), "c");

        let resolved = resolve_targets(&tmp, Path::new("src"), &[]).unwrap();
        let names: Vec<_> = resolved
            .targets
            .iter()
            .map(|t| t.crate_name.as_deref().unwrap())
            .collect();
        assert_eq!(names, vec!["a", "c"]);
    }

    #[test]
    fn package_filter() {
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"foo\", \"bar\"]\n",
        )
        .unwrap();
        write_single_crate(&tmp.join("foo"), "foo");
        write_single_crate(&tmp.join("bar"), "bar");

        let resolved =
            resolve_targets(&tmp, Path::new("src"), &["foo".to_string()]).unwrap();
        assert_eq!(resolved.targets.len(), 1);
        assert_eq!(resolved.targets[0].crate_name.as_deref(), Some("foo"));
    }

    #[test]
    fn package_filter_no_match() {
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"foo\"]\n",
        )
        .unwrap();
        write_single_crate(&tmp.join("foo"), "foo");

        let result =
            resolve_targets(&tmp, Path::new("src"), &["nonexistent".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn hyphen_to_underscore() {
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"my-crate\"]\n",
        )
        .unwrap();
        write_single_crate(&tmp.join("my-crate"), "my-crate");

        let resolved = resolve_targets(&tmp, Path::new("src"), &[]).unwrap();
        assert_eq!(resolved.targets[0].crate_name.as_deref(), Some("my_crate"));
    }

    #[test]
    fn hybrid_workspace_with_root_package() {
        // Root Cargo.toml has both [package] and [workspace] — the root is an
        // implicit member, and is_workspace must still be true so coverage is
        // scoped with --workspace/-p.
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[package]\nname = \"root-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             [workspace]\nmembers = [\"sub\"]\n",
        )
        .unwrap();
        // Root has its own src/
        fs::create_dir_all(tmp.join("src")).unwrap();
        fs::write(tmp.join("src").join("lib.rs"), "").unwrap();
        write_single_crate(&tmp.join("sub"), "sub");

        let resolved = resolve_targets(&tmp, Path::new("src"), &[]).unwrap();
        assert!(resolved.is_workspace);
        let names: Vec<_> = resolved
            .targets
            .iter()
            .filter_map(|t| t.crate_name.as_deref())
            .collect();
        assert!(names.contains(&"root_crate"));
        assert!(names.contains(&"sub"));
    }

    #[test]
    fn virtual_workspace_with_empty_glob_is_still_workspace() {
        // A workspace with a glob that matches nothing should still be tagged as a workspace.
        let tmp = tempdir();
        fs::write(
            tmp.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\"]\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.join("crates")).unwrap();

        let resolved = resolve_targets(&tmp, Path::new("src"), &[]).unwrap();
        assert!(resolved.is_workspace);
        assert!(resolved.targets.is_empty());
    }
}
