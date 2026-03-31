//! Workspace layout analysis for Rust projects.
//!
//! Parses `Cargo.toml` workspace members, counts lines of Rust code per crate,
//! and groups crates into compartments that fit within model context limits.
//! Useful for partitioning large workspaces into bounded analysis units.

pub mod config;
mod compartments;
pub mod connectivity;
pub mod overlap;
pub mod types;
pub mod workspace;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use config::LayoutConfig;
pub use connectivity::{compute_dry_tiles, compute_dry_tiles_with, connectivity_tiles};
pub use overlap::{compute_priority, crate_compartment_idx, generate_overlap_tiles};
pub use types::{Compartment, ConnectomeEdge, OverlapPriority, OverlapTile, ProjectLayout};
pub use workspace::{
    count_rs_lines, expand_member_pattern, parse_workspace_deps, parse_workspace_members,
    resolve_crate_infos,
};

/// A workspace member with its estimated LOC.
pub struct CrateInfo {
    /// Relative path from project root (e.g. `crates/yh-core`).
    pub path: PathBuf,
    /// Crate name derived from directory name.
    pub name: String,
    /// Lines of `.rs` source code.
    pub loc: usize,
}

/// Minimal struct to extract dependency names from a crate's Cargo.toml.
#[derive(serde::Deserialize, Default)]
pub(crate) struct CrateCargoToml {
    #[serde(default)]
    pub(crate) dependencies: HashMap<String, toml::Value>,
    #[serde(default, rename = "dev-dependencies")]
    pub(crate) dev_dependencies: HashMap<String, toml::Value>,
}

/// Minimal struct to extract workspace members from Cargo.toml.
#[derive(serde::Deserialize)]
pub(crate) struct CargoToml {
    pub(crate) workspace: Option<WorkspaceSection>,
}

#[derive(serde::Deserialize)]
pub(crate) struct WorkspaceSection {
    #[serde(default)]
    pub(crate) members: Vec<String>,
}

/// Analyze a Rust workspace and return a [`ProjectLayout`].
pub fn analyze_rust_layout(project_dir: &Path) -> ProjectLayout {
    analyze_rust_layout_with(project_dir, &LayoutConfig::default())
}

/// Analyze a Rust workspace with custom thresholds.
pub fn analyze_rust_layout_with(project_dir: &Path, config: &LayoutConfig) -> ProjectLayout {
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let content = match std::fs::read_to_string(&cargo_toml_path) {
        Ok(c) => c,
        Err(_) => return ProjectLayout::Whole,
    };

    let cargo: CargoToml = match toml::from_str(&content) {
        Ok(v) => v,
        Err(_) => return ProjectLayout::Whole,
    };

    let members = match cargo.workspace {
        Some(ws) if !ws.members.is_empty() => ws.members,
        _ => return ProjectLayout::Whole,
    };

    let crates = workspace::resolve_crate_infos(project_dir, &members);

    if crates.len() <= config.member_count_threshold {
        return ProjectLayout::Whole;
    }

    let total_loc: usize = crates.iter().map(|c| c.loc).sum();
    if total_loc < config.loc_threshold {
        return ProjectLayout::Whole;
    }

    // Parse the workspace dependency graph before grouping consumes crate info.
    let dep_edges = workspace::parse_workspace_deps(project_dir, &crates);
    let crate_names: Vec<String> = crates.iter().map(|c| c.name.clone()).collect();

    // Build connectome (named edge list) for prompt injection.
    let connectome: Vec<ConnectomeEdge> = dep_edges
        .iter()
        .map(|&(from, to)| ConnectomeEdge {
            from: crate_names[from].clone(),
            to: crate_names[to].clone(),
        })
        .collect();

    let compartments = compartments::group_into_compartments(crates, config);
    let overlap_tiles =
        overlap::generate_overlap_tiles(&compartments, &crate_names, &dep_edges, config);

    ProjectLayout::Compartmentalized {
        compartments,
        overlap_tiles,
        connectome,
        total_loc,
    }
}

/// Partition tiles into batches where tiles within a batch have disjoint file
/// paths and can safely run in parallel.
///
/// Tiles with overlapping paths are placed in separate batches to avoid
/// concurrent modifications to the same files.
pub fn batch_by_disjointness(tiles: &[Compartment]) -> Vec<Vec<&Compartment>> {
    let mut batches: Vec<Vec<&Compartment>> = Vec::new();

    for tile in tiles {
        // Find the first batch where no existing tile shares a path.
        let slot = batches.iter().position(|batch| {
            !batch
                .iter()
                .any(|existing| existing.paths.iter().any(|p| tile.paths.contains(p)))
        });

        match slot {
            Some(idx) => batches[idx].push(tile),
            None => batches.push(vec![tile]),
        }
    }

    batches
}
