/// Tunable thresholds for layout analysis.
#[derive(Debug, Clone)]
pub struct LayoutConfig {
    /// Total LOC threshold below which the project is treated as a single unit.
    pub loc_threshold: usize,
    /// Workspace member count at or below which the project is treated as a single unit.
    pub member_count_threshold: usize,
    /// Crates below this LOC are candidates for grouping with other small crates.
    pub small_crate_loc: usize,
    /// Target ceiling for grouped small crates.
    pub group_loc_ceiling: usize,
    /// Minimum direct dependency edges between non-adjacent compartments to
    /// generate an overlap tile for them.
    pub non_adjacent_edge_threshold: usize,
    /// Target LOC per connectivity tile for DRY analysis.
    pub dry_tile_target_loc: usize,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            loc_threshold: 15_000,
            member_count_threshold: 3,
            small_crate_loc: 1_000,
            group_loc_ceiling: 5_000,
            non_adjacent_edge_threshold: 3,
            dry_tile_target_loc: 30_000,
        }
    }
}
