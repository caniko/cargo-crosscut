use std::cmp::Reverse;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::{CargoToml, CrateCargoToml, CrateInfo};

/// Expand workspace member patterns and gather per-crate info.
pub fn resolve_crate_infos(project_dir: &Path, patterns: &[String]) -> Vec<CrateInfo> {
    let mut crates = Vec::new();

    for pattern in patterns {
        let expanded = expand_member_pattern(project_dir, pattern);
        for path in expanded {
            let abs = project_dir.join(&path);
            if !abs.join("Cargo.toml").exists() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let loc = count_rs_lines(&abs);
            crates.push(CrateInfo { path, name, loc });
        }
    }

    // Sort by LOC descending so large crates come first.
    crates.sort_by_key(|c| Reverse(c.loc));
    crates
}

/// Expand a single workspace member pattern.
///
/// Supports two forms:
/// - Literal path: `crates/yh-core` → `[crates/yh-core]`
/// - Glob suffix: `crates/*` → enumerate subdirectories of `crates/`
pub fn expand_member_pattern(project_dir: &Path, pattern: &str) -> Vec<PathBuf> {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        // Glob: enumerate subdirectories.
        let dir = project_dir.join(prefix);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| {
                let e = e.ok()?;
                if e.file_type().ok()?.is_dir() {
                    Some(PathBuf::from(prefix).join(e.file_name()))
                } else {
                    None
                }
            })
            .collect();
        paths.sort();
        paths
    } else {
        vec![PathBuf::from(pattern)]
    }
}

/// Count lines in all `.rs` files under `dir` (recursive).
pub fn count_rs_lines(dir: &Path) -> usize {
    let mut total = 0;
    count_rs_lines_recursive(dir, &mut total);
    total
}

fn count_rs_lines_recursive(dir: &Path, total: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip target directories.
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            count_rs_lines_recursive(&path, total);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            *total += content.lines().count();
        }
    }
}

/// Parse workspace-internal dependencies for all crates.
///
/// Returns a list of directed edges `(from_idx, to_idx)` where indices refer
/// to the `crates` slice. Only dependencies pointing to other workspace
/// members are included.
pub fn parse_workspace_deps(project_dir: &Path, crates: &[CrateInfo]) -> Vec<(usize, usize)> {
    let name_to_idx: HashMap<&str, usize> = crates
        .iter()
        .enumerate()
        .map(|(i, c)| (c.name.as_str(), i))
        .collect();

    let mut edges = Vec::new();

    for (i, cr) in crates.iter().enumerate() {
        let cargo_path = project_dir.join(&cr.path).join("Cargo.toml");
        let content = match std::fs::read_to_string(&cargo_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let parsed: CrateCargoToml = match toml::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Check both [dependencies] and [dev-dependencies].
        for dep_name in parsed
            .dependencies
            .keys()
            .chain(parsed.dev_dependencies.keys())
        {
            if let Some(&target_idx) = name_to_idx.get(dep_name.as_str())
                && target_idx != i
            {
                edges.push((i, target_idx));
            }
        }
    }

    // Deduplicate (a crate might list the same dep in both sections).
    edges.sort();
    edges.dedup();
    edges
}

/// Parse and return workspace members from a workspace Cargo.toml.
pub fn parse_workspace_members(project_dir: &Path) -> Option<Vec<String>> {
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let content = std::fs::read_to_string(&cargo_toml_path).ok()?;
    let cargo: CargoToml = toml::from_str(&content).ok()?;
    match cargo.workspace {
        Some(ws) if !ws.members.is_empty() => Some(ws.members),
        _ => None,
    }
}
