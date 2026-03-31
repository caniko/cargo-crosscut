use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── Project layout (compartmentalization) ───────────────────────────────────

/// A logical subunit of a project for compartmentalized analysis.
///
/// Large projects are split into compartments so that each analysis pass
/// operates on a bounded slice of the source tree, keeping within model
/// context-window limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Compartment {
    /// Human-readable label (e.g. `"yh-core"`, `"yh-cli + yh-mcp"`).
    pub label: String,
    /// Relative paths from project root to the directories this compartment covers.
    pub paths: Vec<PathBuf>,
    /// Estimated lines of source code.
    pub estimated_loc: usize,
}

/// Cross-boundary analysis tile spanning two compartments.
///
/// Overlap tiles are read-only analysis units — they produce findings
/// but never modify code directly. Findings are routed to the owning
/// compartment for remediation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlapTile {
    /// Human-readable label (e.g. `"yh-core <> yh-cli"`).
    pub label: String,
    /// Index of the left compartment in the `compartments` vec.
    pub left_idx: usize,
    /// Index of the right compartment in the `compartments` vec.
    pub right_idx: usize,
    /// Union of both compartments' relative paths.
    pub paths: Vec<PathBuf>,
    /// Combined estimated LOC.
    pub estimated_loc: usize,
    /// Number of direct Cargo dependency edges crossing this boundary.
    pub dependency_edges: usize,
    /// Scheduling priority derived from coupling strength.
    pub priority: OverlapPriority,
}

/// Priority classification for overlap tile scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum OverlapPriority {
    /// No dependency edges and no indirect coupling — skip by default.
    None,
    /// Indirect coupling only (shared transitive workspace deps).
    Low,
    /// 1–2 direct dependency edges between compartments.
    Medium,
    /// 3+ direct dependency edges — strong coupling, analyze first.
    High,
}

/// A directed dependency edge between two crates: `from` depends on `to`.
/// Names are crate directory names (e.g. `"yh-core"`, `"yh-cli"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectomeEdge {
    pub from: String,
    pub to: String,
}

/// Result of analyzing a project's structure for compartmentalization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectLayout {
    /// Project is small enough to handle as a single unit.
    Whole,
    /// Project should be split into compartments for analysis.
    Compartmentalized {
        compartments: Vec<Compartment>,
        /// Overlap tiles for cross-boundary analysis (empty if ≤1 compartment).
        overlap_tiles: Vec<OverlapTile>,
        /// Workspace dependency graph (crate-level directed edges).
        connectome: Vec<ConnectomeEdge>,
        /// Total estimated LOC across all compartments.
        total_loc: usize,
    },
}
