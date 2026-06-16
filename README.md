# cargo-crosscut

<!-- simit:badges:start -->
[![CI](https://img.shields.io/badge/CI-drift-2088ff)](.forgejo/workflows/ci.yaml) [![Nix](https://img.shields.io/badge/Nix-managed-5277c3)](flake.nix) [![crates.io](https://img.shields.io/badge/crates.io-ready-f46623)](https://crates.io/crates/cargo-crosscut)
<!-- simit:badges:end -->

Analyze Rust workspace layout by decomposing large workspaces into bounded analysis units. Useful for partitioning codebases into chunks that fit within LLM context windows or for understanding workspace structure.

## Features

- **Compartments** -- groups crates into LOC-bounded partitions, separating large crates from clusters of smaller ones
- **Overlap tiles** -- cross-boundary analysis units for studying inter-compartment dependencies
- **Connectivity tiles** -- agglomerative clustering for DRY/duplication analysis across crate boundaries
- **Connectome** -- full workspace dependency graph as named directed edges
- **JSON output** -- machine-readable output for integration with other tools

## Installation

```sh
cargo install cargo-crosscut
```

## CLI usage

Analyze the workspace in the current directory:

```sh
cargo crosscut
```

Analyze a specific workspace with JSON output:

```sh
cargo crosscut /path/to/workspace --json
```

Compute DRY connectivity tiles:

```sh
cargo crosscut --dry-tiles
```

Override thresholds:

```sh
cargo crosscut --loc-threshold 3000 --group-ceiling 8000
```

## Library usage

```rust
use cargo_crosscut::{analyze_rust_layout, ProjectLayout};
use std::path::Path;

let layout = analyze_rust_layout(Path::new("."));

match layout {
    ProjectLayout::Whole => println!("Small workspace, no decomposition needed"),
    ProjectLayout::Compartmentalized { compartments, overlap_tiles, connectome, total_loc } => {
        println!("{total_loc} LOC across {} compartments", compartments.len());
    }
}
```

## CI

Woodpecker CI on Codeberg runs `cargo build`, `cargo test`, `cargo clippy`, and `cargo fmt --check` on every push and pull request.

## License

MIT
