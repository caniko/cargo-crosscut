use std::fs;
use std::path::Path;

use crate::compartments::group_into_compartments;
use crate::config::LayoutConfig;
use crate::connectivity::connectivity_tiles;
use crate::overlap::{
    compute_priority, crate_compartment_idx, exceeds_overlap_loc_ceiling, generate_overlap_tiles,
};
use crate::types::{Compartment, OverlapPriority, OverlapTile, ProjectLayout};
use crate::workspace::{count_rs_lines, expand_member_pattern, parse_workspace_deps};
use crate::{CrateInfo, analyze_rust_layout, batch_by_disjointness};

fn write_rs_file(dir: &Path, name: &str, lines: usize) {
    let content: String = (0..lines).map(|i| format!("// line {i}\n")).collect();
    fs::write(dir.join(name), content).unwrap();
}

fn make_crate(project_dir: &Path, rel_path: &str, rs_lines: usize) {
    let crate_dir = project_dir.join(rel_path);
    let src = crate_dir.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        crate_dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{}\"\nversion = \"0.1.0\"\n",
            rel_path.replace('/', "-")
        ),
    )
    .unwrap();
    write_rs_file(&src, "lib.rs", rs_lines);
}

#[test]
fn count_rs_lines_basic() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let src = dir.join("src");
    fs::create_dir_all(&src).unwrap();
    write_rs_file(&src, "main.rs", 100);
    write_rs_file(&src, "lib.rs", 50);

    assert_eq!(count_rs_lines(dir), 150);
}

#[test]
fn count_rs_lines_skips_target() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let src = dir.join("src");
    let target = dir.join("target").join("debug");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&target).unwrap();
    write_rs_file(&src, "main.rs", 100);
    write_rs_file(&target, "generated.rs", 9999);

    assert_eq!(count_rs_lines(dir), 100);
}

#[test]
fn below_threshold_returns_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    // 4 crates but total LOC < 15,000
    let members: Vec<String> = (0..4).map(|i| format!("crates/crate-{i}")).collect();
    for m in &members {
        make_crate(project, m, 1000); // 4 * 1000 = 4000 LOC
    }

    let members_toml: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
    fs::write(
        project.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers = [{}]\nresolver = \"2\"\n",
            members_toml.join(", ")
        ),
    )
    .unwrap();

    assert!(matches!(analyze_rust_layout(project), ProjectLayout::Whole));
}

#[test]
fn few_members_returns_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    // 3 crates with high LOC — still Whole because <= 3 members
    for i in 0..3 {
        make_crate(project, &format!("crates/crate-{i}"), 10000);
    }
    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
    )
    .unwrap();

    assert!(matches!(analyze_rust_layout(project), ProjectLayout::Whole));
}

#[test]
fn large_workspace_compartmentalizes() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    // 5 crates: 2 large, 3 small
    make_crate(project, "crates/big-a", 10000);
    make_crate(project, "crates/big-b", 5000);
    make_crate(project, "crates/small-c", 500);
    make_crate(project, "crates/small-d", 400);
    make_crate(project, "crates/small-e", 300);

    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
    )
    .unwrap();

    let layout = analyze_rust_layout(project);
    match layout {
        ProjectLayout::Compartmentalized {
            compartments,
            total_loc,
            ..
        } => {
            assert_eq!(total_loc, 16200);
            // big-a and big-b each get their own compartment,
            // small-c/d/e get grouped together
            assert_eq!(compartments.len(), 3);

            // First compartment should be biggest (sorted by LOC desc)
            assert_eq!(compartments[0].label, "big-a");
            assert_eq!(compartments[0].estimated_loc, 10000);

            assert_eq!(compartments[1].label, "big-b");
            assert_eq!(compartments[1].estimated_loc, 5000);

            // Grouped small crates
            assert!(compartments[2].label.contains("small-"));
            assert_eq!(compartments[2].paths.len(), 3);
        }
        ProjectLayout::Whole => panic!("expected Compartmentalized"),
    }
}

#[test]
fn expand_glob_pattern() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();
    let crates_dir = project.join("crates");
    fs::create_dir_all(crates_dir.join("alpha")).unwrap();
    fs::create_dir_all(crates_dir.join("beta")).unwrap();
    // regular file should be ignored
    fs::write(crates_dir.join("README.md"), "hello").unwrap();

    let mut result = expand_member_pattern(project, "crates/*");
    result.sort();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], std::path::PathBuf::from("crates/alpha"));
    assert_eq!(result[1], std::path::PathBuf::from("crates/beta"));
}

#[test]
fn expand_literal_pattern() {
    let tmp = tempfile::tempdir().unwrap();
    let result = expand_member_pattern(tmp.path(), "crates/my-crate");
    assert_eq!(result, vec![std::path::PathBuf::from("crates/my-crate")]);
}

#[test]
fn grouping_respects_ceiling() {
    let config = LayoutConfig::default();
    // Build crates that individually are small but exceed group_loc_ceiling together
    let crates: Vec<CrateInfo> = (0..10)
        .map(|i| CrateInfo {
            path: std::path::PathBuf::from(format!("crates/tiny-{i}")),
            name: format!("tiny-{i}"),
            loc: 800,
        })
        .collect();

    let compartments = group_into_compartments(crates, &config);
    // 10 * 800 = 8000 LOC, ceiling = 5000
    // Groups: [0..5] = 4000, [6..9] = 4000 — but actual grouping depends on
    // iteration order, should be at least 2 groups.
    assert!(compartments.len() >= 2);
    for c in &compartments {
        assert!(c.estimated_loc <= config.group_loc_ceiling + config.small_crate_loc);
    }
}

// ── Overlap tile tests ──────────────────────────────────────────────────

fn make_compartment(label: &str, paths: &[&str], loc: usize) -> Compartment {
    Compartment {
        label: label.to_string(),
        paths: paths.iter().map(std::path::PathBuf::from).collect(),
        estimated_loc: loc,
    }
}

#[test]
fn no_overlap_tiles_for_single_compartment() {
    let config = LayoutConfig::default();
    let compartments = vec![make_compartment("core", &["crates/core"], 10000)];
    let tiles = generate_overlap_tiles(&compartments, &[], &[], &config);
    assert!(tiles.is_empty());
}

#[test]
fn adjacent_tiles_generated_for_two_compartments() {
    let config = LayoutConfig::default();
    let compartments = vec![
        make_compartment("core", &["crates/core"], 8000),
        make_compartment("cli", &["crates/cli"], 3000),
    ];
    let crate_names = vec!["core".to_string(), "cli".to_string()];
    // cli depends on core
    let dep_edges = vec![(1, 0)];

    let tiles = generate_overlap_tiles(&compartments, &crate_names, &dep_edges, &config);
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].label, "core <> cli");
    assert_eq!(tiles[0].left_idx, 0);
    assert_eq!(tiles[0].right_idx, 1);
    assert_eq!(tiles[0].dependency_edges, 1);
    assert_eq!(tiles[0].priority, OverlapPriority::Medium);
    assert_eq!(tiles[0].estimated_loc, 11000);
    assert_eq!(tiles[0].paths.len(), 2);
}

#[test]
fn priority_scoring_by_edge_count() {
    let config = LayoutConfig::default();
    let compartments = vec![
        make_compartment("a", &["crates/a"], 5000),
        make_compartment("b", &["crates/b"], 5000),
    ];
    let crate_names = vec!["a".to_string(), "b".to_string()];

    // 0 edges → None
    let tiles = generate_overlap_tiles(&compartments, &crate_names, &[], &config);
    assert_eq!(tiles[0].priority, OverlapPriority::None);

    // 1 edge → Medium
    let tiles = generate_overlap_tiles(&compartments, &crate_names, &[(0, 1)], &config);
    assert_eq!(tiles[0].priority, OverlapPriority::Medium);

    // 2 edges → Medium
    let tiles = generate_overlap_tiles(&compartments, &crate_names, &[(0, 1), (1, 0)], &config);
    assert_eq!(tiles[0].priority, OverlapPriority::Medium);

    // 3 edges → High (need a multi-crate compartment to get 3 edges)
    let compartments = vec![
        make_compartment("left", &["crates/a1", "crates/a2"], 5000),
        make_compartment("right", &["crates/b1", "crates/b2"], 5000),
    ];
    let crate_names = vec![
        "a1".to_string(),
        "a2".to_string(),
        "b1".to_string(),
        "b2".to_string(),
    ];
    let dep_edges = vec![(2, 0), (2, 1), (3, 0)]; // 3 cross-boundary edges
    let tiles = generate_overlap_tiles(&compartments, &crate_names, &dep_edges, &config);
    assert_eq!(tiles[0].priority, OverlapPriority::High);
    assert_eq!(tiles[0].dependency_edges, 3);
}

#[test]
fn three_compartments_produce_two_adjacent_tiles() {
    let config = LayoutConfig::default();
    let compartments = vec![
        make_compartment("a", &["crates/a"], 6000),
        make_compartment("b", &["crates/b"], 5000),
        make_compartment("c", &["crates/c"], 5000),
    ];
    let crate_names = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let dep_edges = vec![(1, 0), (2, 1)]; // b→a, c→b

    let tiles = generate_overlap_tiles(&compartments, &crate_names, &dep_edges, &config);
    assert_eq!(tiles.len(), 2);
    // Both are Medium (1 edge each), so sorted by edge count (equal),
    // order is stable: a<>b first, b<>c second
    assert_eq!(tiles[0].label, "a <> b");
    assert_eq!(tiles[1].label, "b <> c");
}

#[test]
fn non_adjacent_strongly_coupled_gets_tile() {
    let config = LayoutConfig::default();
    let compartments = vec![
        make_compartment("a", &["crates/a1", "crates/a2"], 5000),
        make_compartment("b", &["crates/b"], 5000),
        make_compartment("c", &["crates/c1", "crates/c2"], 5000),
    ];
    let crate_names = vec![
        "a1".to_string(),
        "a2".to_string(),
        "b".to_string(),
        "c1".to_string(),
        "c2".to_string(),
    ];
    // a1→c1, a2→c1, a1→c2 = 3 edges between compartments 0 and 2 (non-adjacent)
    let dep_edges = vec![(0, 3), (1, 3), (0, 4)];

    let tiles = generate_overlap_tiles(&compartments, &crate_names, &dep_edges, &config);
    // Should have: a<>b (adjacent, 0 edges), b<>c (adjacent, 0 edges),
    // plus a<>c (non-adjacent, 3 edges, High priority)
    assert_eq!(tiles.len(), 3);
    // High priority tile should be first
    assert_eq!(tiles[0].priority, OverlapPriority::High);
    assert_eq!(tiles[0].label, "a <> c");
    assert_eq!(tiles[0].dependency_edges, 3);
}

#[test]
fn overlap_tile_loc_ceiling() {
    let tile = OverlapTile {
        label: "test".into(),
        left_idx: 0,
        right_idx: 1,
        paths: vec![],
        estimated_loc: 20_000,
        dependency_edges: 0,
        priority: OverlapPriority::None,
    };
    assert!(!exceeds_overlap_loc_ceiling(&tile));

    let large_tile = OverlapTile {
        estimated_loc: 20_001,
        ..tile
    };
    assert!(exceeds_overlap_loc_ceiling(&large_tile));
}

#[test]
fn workspace_deps_parsed_from_cargo_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    // Create crates with deps
    make_crate(project, "crates/core", 5000);
    make_crate(project, "crates/cli", 3000);

    // Add dep: cli depends on core
    fs::write(
        project.join("crates/cli/Cargo.toml"),
        "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n[dependencies]\ncore = { path = \"../core\" }\n",
    )
    .unwrap();

    let crates = vec![
        CrateInfo {
            path: std::path::PathBuf::from("crates/core"),
            name: "core".to_string(),
            loc: 5000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/cli"),
            name: "cli".to_string(),
            loc: 3000,
        },
    ];

    let edges = parse_workspace_deps(project, &crates);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0], (1, 0)); // cli → core
}

#[test]
fn large_workspace_includes_overlap_tiles() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    // 5 crates: 2 large, 3 small — with inter-crate deps
    make_crate(project, "crates/big-a", 10000);
    make_crate(project, "crates/big-b", 5000);
    make_crate(project, "crates/small-c", 500);
    make_crate(project, "crates/small-d", 400);
    make_crate(project, "crates/small-e", 300);

    // big-b depends on big-a
    fs::write(
        project.join("crates/big-b/Cargo.toml"),
        "[package]\nname = \"big-b\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\nbig-a = { path = \"../big-a\" }\n",
    )
    .unwrap();

    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
    )
    .unwrap();

    let layout = analyze_rust_layout(project);
    match layout {
        ProjectLayout::Compartmentalized {
            compartments,
            overlap_tiles,
            ..
        } => {
            assert_eq!(compartments.len(), 3);
            // Should have at least 2 adjacent overlap tiles
            assert!(overlap_tiles.len() >= 2);
            // big-a <> big-b should be Medium priority (1 dep edge)
            let ab_tile = overlap_tiles
                .iter()
                .find(|t| t.label.contains("big-a") && t.label.contains("big-b"))
                .expect("should have big-a <> big-b tile");
            assert_eq!(ab_tile.priority, OverlapPriority::Medium);
            assert_eq!(ab_tile.dependency_edges, 1);
        }
        ProjectLayout::Whole => panic!("expected Compartmentalized"),
    }
}

// ── parse_workspace_deps ────────────────────────────────────────────

#[test]
fn parse_workspace_deps_no_deps() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    make_crate(project, "crates/alpha", 2000);
    make_crate(project, "crates/beta", 2000);

    let crates = vec![
        CrateInfo {
            path: std::path::PathBuf::from("crates/alpha"),
            name: "alpha".to_string(),
            loc: 2000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/beta"),
            name: "beta".to_string(),
            loc: 2000,
        },
    ];

    let edges = parse_workspace_deps(project, &crates);
    assert!(edges.is_empty());
}

#[test]
fn parse_workspace_deps_mutual_deps() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    make_crate(project, "crates/alpha", 2000);
    make_crate(project, "crates/beta", 2000);

    // alpha depends on beta
    fs::write(
        project.join("crates/alpha/Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\nbeta = { path = \"../beta\" }\n",
    )
    .unwrap();

    // beta depends on alpha (circular, unusual but valid for test)
    fs::write(
        project.join("crates/beta/Cargo.toml"),
        "[package]\nname = \"beta\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\nalpha = { path = \"../alpha\" }\n",
    )
    .unwrap();

    let crates = vec![
        CrateInfo {
            path: std::path::PathBuf::from("crates/alpha"),
            name: "alpha".to_string(),
            loc: 2000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/beta"),
            name: "beta".to_string(),
            loc: 2000,
        },
    ];

    let edges = parse_workspace_deps(project, &crates);
    assert_eq!(edges.len(), 2);
    assert!(edges.contains(&(0, 1))); // alpha → beta
    assert!(edges.contains(&(1, 0))); // beta → alpha
}

#[test]
fn parse_workspace_deps_dev_dependencies() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    make_crate(project, "crates/lib", 3000);
    make_crate(project, "crates/test-utils", 1000);

    // lib has test-utils as dev-dependency only
    fs::write(
        project.join("crates/lib/Cargo.toml"),
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\n\n\
         [dev-dependencies]\ntest-utils = { path = \"../test-utils\" }\n",
    )
    .unwrap();

    let crates = vec![
        CrateInfo {
            path: std::path::PathBuf::from("crates/lib"),
            name: "lib".to_string(),
            loc: 3000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/test-utils"),
            name: "test-utils".to_string(),
            loc: 1000,
        },
    ];

    let edges = parse_workspace_deps(project, &crates);
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0], (0, 1)); // lib → test-utils
}

#[test]
fn parse_workspace_deps_deduplicates() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    make_crate(project, "crates/app", 2000);
    make_crate(project, "crates/core", 5000);

    // app lists core in both [dependencies] and [dev-dependencies]
    fs::write(
        project.join("crates/app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\ncore = { path = \"../core\" }\n\n\
         [dev-dependencies]\ncore = { path = \"../core\" }\n",
    )
    .unwrap();

    let crates = vec![
        CrateInfo {
            path: std::path::PathBuf::from("crates/app"),
            name: "app".to_string(),
            loc: 2000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/core"),
            name: "core".to_string(),
            loc: 5000,
        },
    ];

    let edges = parse_workspace_deps(project, &crates);
    assert_eq!(edges.len(), 1, "duplicate dep edge should be deduped");
    assert_eq!(edges[0], (0, 1));
}

// ── crate_compartment_idx ───────────────────────────────────────────

#[test]
fn crate_compartment_idx_finds_by_name() {
    let compartments = vec![
        make_compartment("core", &["crates/core"], 8000),
        make_compartment("cli + mcp", &["crates/cli", "crates/mcp"], 3000),
    ];

    assert_eq!(crate_compartment_idx("core", &compartments), Some(0));
    assert_eq!(crate_compartment_idx("cli", &compartments), Some(1));
    assert_eq!(crate_compartment_idx("mcp", &compartments), Some(1));
}

#[test]
fn crate_compartment_idx_not_found() {
    let compartments = vec![make_compartment("core", &["crates/core"], 8000)];
    assert_eq!(crate_compartment_idx("nonexistent", &compartments), None);
}

#[test]
fn crate_compartment_idx_empty_compartments() {
    assert_eq!(crate_compartment_idx("anything", &[]), None);
}

// ── generate_overlap_tiles edge cases ───────────────────────────────

#[test]
fn generate_overlap_tiles_empty_compartments() {
    let config = LayoutConfig::default();
    let tiles = generate_overlap_tiles(&[], &[], &[], &config);
    assert!(tiles.is_empty());
}

// ── compute_priority ────────────────────────────────────────────────

#[test]
fn compute_priority_zero_edges_no_indirect() {
    let comp_deps = vec![
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
    ];
    assert_eq!(compute_priority(0, 0, 1, &comp_deps), OverlapPriority::None);
}

#[test]
fn compute_priority_zero_edges_with_indirect() {
    // Both compartments 0 and 1 depend on compartment 2
    let mut deps0 = std::collections::HashSet::new();
    deps0.insert(2usize);
    let mut deps1 = std::collections::HashSet::new();
    deps1.insert(2usize);
    let comp_deps = vec![deps0, deps1, std::collections::HashSet::new()];
    assert_eq!(compute_priority(0, 0, 1, &comp_deps), OverlapPriority::Low);
}

#[test]
fn compute_priority_one_edge() {
    let comp_deps = vec![
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
    ];
    assert_eq!(
        compute_priority(1, 0, 1, &comp_deps),
        OverlapPriority::Medium
    );
}

#[test]
fn compute_priority_two_edges() {
    let comp_deps = vec![
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
    ];
    assert_eq!(
        compute_priority(2, 0, 1, &comp_deps),
        OverlapPriority::Medium
    );
}

#[test]
fn compute_priority_three_edges() {
    let comp_deps = vec![
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
    ];
    assert_eq!(compute_priority(3, 0, 1, &comp_deps), OverlapPriority::High);
}

#[test]
fn compute_priority_many_edges() {
    let comp_deps = vec![
        std::collections::HashSet::new(),
        std::collections::HashSet::new(),
    ];
    assert_eq!(
        compute_priority(10, 0, 1, &comp_deps),
        OverlapPriority::High
    );
}

// ── exceeds_overlap_loc_ceiling boundary ────────────────────────────

#[test]
fn exceeds_overlap_loc_ceiling_below() {
    let tile = OverlapTile {
        label: "test".into(),
        left_idx: 0,
        right_idx: 1,
        paths: vec![],
        estimated_loc: 19_999,
        dependency_edges: 0,
        priority: OverlapPriority::None,
    };
    assert!(!exceeds_overlap_loc_ceiling(&tile));
}

#[test]
fn exceeds_overlap_loc_ceiling_at_boundary() {
    let tile = OverlapTile {
        label: "test".into(),
        left_idx: 0,
        right_idx: 1,
        paths: vec![],
        estimated_loc: 20_000,
        dependency_edges: 0,
        priority: OverlapPriority::None,
    };
    assert!(!exceeds_overlap_loc_ceiling(&tile));
}

#[test]
fn exceeds_overlap_loc_ceiling_one_above() {
    let tile = OverlapTile {
        label: "test".into(),
        left_idx: 0,
        right_idx: 1,
        paths: vec![],
        estimated_loc: 20_001,
        dependency_edges: 0,
        priority: OverlapPriority::None,
    };
    assert!(exceeds_overlap_loc_ceiling(&tile));
}

// ── connectome in analyze_rust_layout ────────────────────────────────

#[test]
fn large_workspace_has_connectome_edges() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path();

    make_crate(project, "crates/core", 10000);
    make_crate(project, "crates/cli", 5000);
    make_crate(project, "crates/mcp", 3000);
    make_crate(project, "crates/extra", 2000);

    // cli depends on core
    fs::write(
        project.join("crates/cli/Cargo.toml"),
        "[package]\nname = \"cli\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\ncore = { path = \"../core\" }\n",
    )
    .unwrap();

    // mcp depends on core
    fs::write(
        project.join("crates/mcp/Cargo.toml"),
        "[package]\nname = \"mcp\"\nversion = \"0.1.0\"\n\n\
         [dependencies]\ncore = { path = \"../core\" }\n",
    )
    .unwrap();

    fs::write(
        project.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
    )
    .unwrap();

    let layout = analyze_rust_layout(project);
    match layout {
        ProjectLayout::Compartmentalized { connectome, .. } => {
            assert!(!connectome.is_empty());
            // cli→core and mcp→core edges should exist
            assert!(connectome.iter().any(|e| e.from == "cli" && e.to == "core"));
            assert!(connectome.iter().any(|e| e.from == "mcp" && e.to == "core"));
        }
        ProjectLayout::Whole => panic!("expected Compartmentalized"),
    }
}

// ── connectivity_tiles tests ────────────────────────────────────────

#[test]
fn connectivity_tiles_empty() {
    let tiles = connectivity_tiles(vec![], &[], 30_000);
    assert!(tiles.is_empty());
}

#[test]
fn connectivity_tiles_single_crate() {
    let crates = vec![CrateInfo {
        path: std::path::PathBuf::from("crates/core"),
        name: "core".to_string(),
        loc: 50_000,
    }];
    let tiles = connectivity_tiles(crates, &[], 30_000);
    // Single crate → single tile (can't split further).
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].label, "core");
}

#[test]
fn connectivity_tiles_merges_connected_crates() {
    // Three crates: core (40k), cli (3k), mcp (3k)
    // cli→core, mcp→core — all connected through core.
    // Target: 30k → ceil(46k/30k) = 2 tiles
    // But cli and mcp both connect to core, so the pair with most
    // edges gets merged first.
    let crates = vec![
        CrateInfo {
            path: std::path::PathBuf::from("crates/core"),
            name: "core".to_string(),
            loc: 40_000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/cli"),
            name: "cli".to_string(),
            loc: 3_000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/mcp"),
            name: "mcp".to_string(),
            loc: 3_000,
        },
    ];
    let dep_edges = vec![(1, 0), (2, 0)]; // cli→core, mcp→core

    let tiles = connectivity_tiles(crates, &dep_edges, 30_000);
    // Target count = ceil(46000/30000) = 2.
    // Both cli and mcp have 1 edge to core — one gets merged first,
    // then we're at 2 clusters and stop.
    assert_eq!(tiles.len(), 2);
    // Total LOC should be preserved.
    let total: usize = tiles.iter().map(|t| t.estimated_loc).sum();
    assert_eq!(total, 46_000);
}

#[test]
fn connectivity_tiles_disconnected_stay_separate() {
    // Two crates with no deps — can't merge.
    let crates = vec![
        CrateInfo {
            path: std::path::PathBuf::from("crates/a"),
            name: "a".to_string(),
            loc: 20_000,
        },
        CrateInfo {
            path: std::path::PathBuf::from("crates/b"),
            name: "b".to_string(),
            loc: 20_000,
        },
    ];
    // Target count = ceil(40k/30k) = 2, but even if target were 1,
    // no edges means no merging.
    let tiles = connectivity_tiles(crates, &[], 30_000);
    assert_eq!(tiles.len(), 2);
}

#[test]
fn connectivity_tiles_strongly_connected_merge_fully() {
    // Four crates all connected, target = 1 tile.
    let crates: Vec<CrateInfo> = (0..4)
        .map(|i| CrateInfo {
            path: std::path::PathBuf::from(format!("crates/c{i}")),
            name: format!("c{i}"),
            loc: 5_000,
        })
        .collect();
    // Fully connected: every pair has an edge.
    let dep_edges = vec![
        (0, 1),
        (1, 0),
        (0, 2),
        (2, 0),
        (0, 3),
        (1, 2),
        (2, 3),
        (3, 1),
    ];
    // 20k total, target 30k → ceil = 1 tile.
    let tiles = connectivity_tiles(crates, &dep_edges, 30_000);
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].paths.len(), 4);
    assert_eq!(tiles[0].estimated_loc, 20_000);
}

// ── batch_by_disjointness tests ─────────────────────────────────────

#[test]
fn batch_disjoint_tiles() {
    let tiles = [
        make_compartment("a", &["crates/a"], 10_000),
        make_compartment("b", &["crates/b"], 10_000),
        make_compartment("c", &["crates/c"], 10_000),
    ];
    let batches = batch_by_disjointness(&tiles);
    // All disjoint → single batch.
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 3);
}

#[test]
fn batch_overlapping_tiles() {
    let tiles = [
        make_compartment("a+b", &["crates/a", "crates/b"], 15_000),
        make_compartment("b+c", &["crates/b", "crates/c"], 15_000),
        make_compartment("d", &["crates/d"], 5_000),
    ];
    let batches = batch_by_disjointness(&tiles);
    // a+b and b+c overlap on crates/b → separate batches.
    // d is disjoint from a+b → same batch as a+b.
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 2); // a+b + d
    assert_eq!(batches[1].len(), 1); // b+c
}

#[test]
fn batch_empty_tiles() {
    let tiles: [Compartment; 0] = [];
    let batches = batch_by_disjointness(&tiles);
    assert!(batches.is_empty());
}
