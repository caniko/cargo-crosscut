use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use cargo_crosscut::{
    LayoutConfig, ProjectLayout, analyze_rust_layout_with, compute_dry_tiles_with,
};

#[derive(Parser)]
#[command(
    name = "cargo-crosscut",
    about = "Analyze Rust workspace layout — compartments, overlap tiles, and connectivity"
)]
struct Cli {
    /// Path to workspace root.
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Output JSON instead of human-readable text.
    #[arg(long)]
    json: bool,

    /// Compute DRY connectivity tiles instead of compartments.
    #[arg(long)]
    dry_tiles: bool,

    /// Override total LOC threshold for compartmentalization.
    #[arg(long)]
    loc_threshold: Option<usize>,

    /// Override workspace member count threshold.
    #[arg(long)]
    member_threshold: Option<usize>,

    /// Override small-crate LOC cutoff.
    #[arg(long)]
    small_crate_loc: Option<usize>,

    /// Override grouped small-crate LOC ceiling.
    #[arg(long)]
    group_ceiling: Option<usize>,

    /// Override DRY tile target LOC.
    #[arg(long)]
    dry_tile_target: Option<usize>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let mut config = LayoutConfig::default();
    if let Some(v) = cli.loc_threshold {
        config.loc_threshold = v;
    }
    if let Some(v) = cli.member_threshold {
        config.member_count_threshold = v;
    }
    if let Some(v) = cli.small_crate_loc {
        config.small_crate_loc = v;
    }
    if let Some(v) = cli.group_ceiling {
        config.group_loc_ceiling = v;
    }
    if let Some(v) = cli.dry_tile_target {
        config.dry_tile_target_loc = v;
    }

    if cli.dry_tiles {
        let tiles = compute_dry_tiles_with(&cli.path, &config);
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&tiles).expect("serialize")
            );
        } else {
            print_dry_tiles(&tiles);
        }
    } else {
        let layout = analyze_rust_layout_with(&cli.path, &config);
        if cli.json {
            println!(
                "{}",
                serde_json::to_string_pretty(&layout).expect("serialize")
            );
        } else {
            print_layout(&layout);
        }
    }

    ExitCode::SUCCESS
}

fn print_layout(layout: &ProjectLayout) {
    match layout {
        ProjectLayout::Whole => {
            println!("Layout: Whole (below compartmentalization threshold)");
        }
        ProjectLayout::Compartmentalized {
            compartments,
            overlap_tiles,
            connectome,
            total_loc,
        } => {
            println!("Layout: Compartmentalized ({total_loc} LOC total)\n");

            println!("Compartments ({}):", compartments.len());
            println!("{:<30} {:>8}  Paths", "Label", "LOC");
            println!("{}", "-".repeat(70));
            for c in compartments {
                let paths: Vec<_> = c.paths.iter().map(|p| p.display().to_string()).collect();
                println!("{:<30} {:>8}  {}", c.label, c.estimated_loc, paths.join(", "));
            }

            if !overlap_tiles.is_empty() {
                println!("\nOverlap tiles ({}):", overlap_tiles.len());
                println!("{:<30} {:>8} {:>6}  Priority", "Label", "LOC", "Edges");
                println!("{}", "-".repeat(70));
                for t in overlap_tiles {
                    println!(
                        "{:<30} {:>8} {:>6}  {:?}",
                        t.label, t.estimated_loc, t.dependency_edges, t.priority
                    );
                }
            }

            if !connectome.is_empty() {
                println!("\nConnectome ({} edges):", connectome.len());
                for e in connectome {
                    println!("  {} -> {}", e.from, e.to);
                }
            }
        }
    }
}

fn print_dry_tiles(tiles: &[cargo_crosscut::Compartment]) {
    if tiles.is_empty() {
        println!("No DRY tiles (workspace too small or single crate).");
        return;
    }

    println!("DRY connectivity tiles ({}):", tiles.len());
    println!("{:<30} {:>8}  Paths", "Label", "LOC");
    println!("{}", "-".repeat(70));
    for t in tiles {
        let paths: Vec<_> = t.paths.iter().map(|p| p.display().to_string()).collect();
        println!("{:<30} {:>8}  {}", t.label, t.estimated_loc, paths.join(", "));
    }
}
