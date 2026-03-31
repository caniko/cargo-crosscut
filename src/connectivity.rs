use std::collections::HashMap;
use std::path::Path;

use crate::config::LayoutConfig;
use crate::types::Compartment;
use crate::workspace::{parse_workspace_deps, parse_workspace_members, resolve_crate_infos};
use crate::CrateInfo;

/// Compute connectivity-based tiles for DRY deduplication analysis.
///
/// Clusters workspace crates by dependency connectivity: crates with more
/// inter-edges are grouped into the same tile. Prefers fewer, larger tiles
/// (too few > too many) so each tile captures cross-crate duplication.
///
/// Returns an empty `Vec` if the workspace has fewer than 2 crates.
pub fn compute_dry_tiles(project_dir: &Path) -> Vec<Compartment> {
    compute_dry_tiles_with(project_dir, &LayoutConfig::default())
}

/// Compute connectivity-based tiles with custom thresholds.
pub fn compute_dry_tiles_with(project_dir: &Path, config: &LayoutConfig) -> Vec<Compartment> {
    let members = match parse_workspace_members(project_dir) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let crates = resolve_crate_infos(project_dir, &members);
    if crates.len() < 2 {
        return Vec::new();
    }

    let dep_edges = parse_workspace_deps(project_dir, &crates);
    connectivity_tiles(crates, &dep_edges, config.dry_tile_target_loc)
}

/// Cluster crates into connectivity-based tiles using agglomerative merging.
///
/// Repeatedly merges the two clusters with the most inter-edges until the
/// cluster count reaches `ceil(total_loc / target_loc_per_tile)` or no
/// cross-edges remain.
pub fn connectivity_tiles(
    crates: Vec<CrateInfo>,
    dep_edges: &[(usize, usize)],
    target_loc_per_tile: usize,
) -> Vec<Compartment> {
    if crates.is_empty() {
        return Vec::new();
    }

    let n = crates.len();
    let total_loc: usize = crates.iter().map(|c| c.loc).sum();
    let target_count = total_loc.div_ceil(target_loc_per_tile).max(1);

    // Union-Find with path compression and union by rank.
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<usize> = vec![0; n];

    let find = |parent: &mut Vec<usize>, mut x: usize| -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]]; // path halving
            x = parent[x];
        }
        x
    };

    let mut cluster_count = n;

    while cluster_count > target_count {
        // Count cross-cluster edges.
        let mut pair_edges: HashMap<(usize, usize), usize> = HashMap::new();
        for &(from, to) in dep_edges {
            let cf = find(&mut parent, from);
            let ct = find(&mut parent, to);
            if cf != ct {
                let key = if cf < ct { (cf, ct) } else { (ct, cf) };
                *pair_edges.entry(key).or_default() += 1;
            }
        }

        if pair_edges.is_empty() {
            break;
        }

        // Merge the pair with the most cross-edges.
        let &(a, b) = pair_edges
            .iter()
            .max_by_key(|&(_, &count)| count)
            .unwrap()
            .0;

        // Union by rank.
        let ra = find(&mut parent, a);
        let rb = find(&mut parent, b);
        if ra != rb {
            if rank[ra] < rank[rb] {
                parent[ra] = rb;
            } else {
                parent[rb] = ra;
                if rank[ra] == rank[rb] {
                    rank[ra] += 1;
                }
            }
            cluster_count -= 1;
        }
    }

    // Collect clusters into Compartments.
    let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        clusters.entry(find(&mut parent, i)).or_default().push(i);
    }

    let mut tiles: Vec<Compartment> = clusters
        .values()
        .map(|members| {
            let label = members
                .iter()
                .map(|&i| crates[i].name.as_str())
                .collect::<Vec<_>>()
                .join(" + ");
            let paths = members.iter().map(|&i| crates[i].path.clone()).collect();
            let estimated_loc = members.iter().map(|&i| crates[i].loc).sum();
            Compartment {
                label,
                paths,
                estimated_loc,
            }
        })
        .collect();

    // Sort by LOC descending for deterministic ordering.
    tiles.sort_by_key(|t| std::cmp::Reverse(t.estimated_loc));
    tiles
}
