use std::collections::{HashMap, HashSet};

use crate::config::LayoutConfig;
use crate::types::{Compartment, OverlapPriority, OverlapTile};

/// Find which compartment a crate belongs to (by matching its name against
/// compartment labels or path directory names).
pub fn crate_compartment_idx(crate_name: &str, compartments: &[Compartment]) -> Option<usize> {
    for (i, comp) in compartments.iter().enumerate() {
        // Check if crate name matches any path's final component.
        for p in &comp.paths {
            if let Some(dir_name) = p.file_name().and_then(|n| n.to_str())
                && dir_name == crate_name
            {
                return Some(i);
            }
        }
    }
    None
}

/// Generate overlap tiles for cross-boundary analysis.
///
/// Produces tiles for:
/// 1. All adjacent compartment pairs (priority based on dep edge count).
/// 2. Non-adjacent pairs with ≥ the configured edge threshold direct dep edges.
pub fn generate_overlap_tiles(
    compartments: &[Compartment],
    crate_names: &[String],
    dep_edges: &[(usize, usize)],
    config: &LayoutConfig,
) -> Vec<OverlapTile> {
    if compartments.len() <= 1 {
        return Vec::new();
    }

    // Map each crate to its compartment index.
    let crate_to_comp: Vec<Option<usize>> = crate_names
        .iter()
        .map(|name| crate_compartment_idx(name, compartments))
        .collect();

    // Count cross-compartment edges for each compartment pair.
    let mut pair_edges: HashMap<(usize, usize), usize> = HashMap::new();
    for &(from, to) in dep_edges {
        let Some(comp_from) = crate_to_comp[from] else {
            continue;
        };
        let Some(comp_to) = crate_to_comp[to] else {
            continue;
        };
        if comp_from != comp_to {
            let key = if comp_from < comp_to {
                (comp_from, comp_to)
            } else {
                (comp_to, comp_from)
            };
            *pair_edges.entry(key).or_default() += 1;
        }
    }

    // Check for indirect coupling: two compartments that both depend on the
    // same third compartment (shared transitive workspace deps).
    let mut comp_deps: Vec<HashSet<usize>> = vec![HashSet::new(); compartments.len()];
    for &(from, to) in dep_edges {
        if let (Some(cf), Some(ct)) = (crate_to_comp[from], crate_to_comp[to])
            && cf != ct
        {
            comp_deps[cf].insert(ct);
        }
    }

    let mut tiles = Vec::new();

    // Generate tiles for adjacent pairs.
    for i in 0..compartments.len() - 1 {
        let j = i + 1;
        let edges = pair_edges.get(&(i, j)).copied().unwrap_or(0);
        let priority = compute_priority(edges, i, j, &comp_deps);
        tiles.push(build_overlap_tile(compartments, i, j, edges, priority));
    }

    // Generate tiles for non-adjacent strongly coupled pairs.
    for (&(i, j), &edges) in &pair_edges {
        if j != i + 1 && edges >= config.non_adjacent_edge_threshold {
            let priority = compute_priority(edges, i, j, &comp_deps);
            tiles.push(build_overlap_tile(compartments, i, j, edges, priority));
        }
    }

    // Sort by priority descending, then dep edges descending.
    tiles.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(b.dependency_edges.cmp(&a.dependency_edges))
    });

    tiles
}

/// Compute overlap priority from edge count and indirect coupling.
pub fn compute_priority(
    edges: usize,
    i: usize,
    j: usize,
    comp_deps: &[HashSet<usize>],
) -> OverlapPriority {
    match edges {
        0 => {
            // Check indirect coupling: do both compartments depend on a common third?
            let has_indirect = comp_deps[i].iter().any(|dep| comp_deps[j].contains(dep))
                || comp_deps[j].iter().any(|dep| comp_deps[i].contains(dep));
            if has_indirect {
                OverlapPriority::Low
            } else {
                OverlapPriority::None
            }
        }
        1..=2 => OverlapPriority::Medium,
        _ => OverlapPriority::High,
    }
}

/// Build an `OverlapTile` from two compartment indices.
fn build_overlap_tile(
    compartments: &[Compartment],
    left_idx: usize,
    right_idx: usize,
    dependency_edges: usize,
    priority: OverlapPriority,
) -> OverlapTile {
    let left = &compartments[left_idx];
    let right = &compartments[right_idx];
    let paths = left
        .paths
        .iter()
        .chain(right.paths.iter())
        .cloned()
        .collect();
    OverlapTile {
        label: format!("{} <> {}", left.label, right.label),
        left_idx,
        right_idx,
        paths,
        estimated_loc: left.estimated_loc + right.estimated_loc,
        dependency_edges,
        priority,
    }
}

/// Combined LOC ceiling for overlap tiles above which the prompt switches to
/// interface-only scanning.
#[cfg(test)]
const OVERLAP_LOC_CEILING: usize = 20_000;

/// Returns `true` if the overlap tile's combined LOC exceeds the ceiling
/// for full scanning.
#[cfg(test)]
pub(crate) fn exceeds_overlap_loc_ceiling(tile: &OverlapTile) -> bool {
    tile.estimated_loc > OVERLAP_LOC_CEILING
}
