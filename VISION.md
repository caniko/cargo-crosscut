# cargo-crosscut vision

A `cargo` subcommand and library that decomposes a Rust workspace into bounded
analysis units: compartments of crates that fit inside a model context window,
overlap tiles for studying cross-boundary dependencies, connectivity tiles for
DRY/duplication analysis, and a connectome of named dependency edges. Its users
are people - and agents - who need to read, review or refactor a workspace too
large to hold in one pass.

## Problem

A big workspace is a bad input for analysis. Asking an LLM to look at forty
crates at once either overflows the window or produces shallow guesses, and
splitting by directory ignores how crates actually depend on each other. The
manual alternative - reading `Cargo.toml` files and `wc -l` by hand - is slow and
gives no repeatable boundary. A decomposition that is derived from the manifest
and dependency graph is reproducible, and can be re-run as the workspace changes.

## Principles

- The manifest is the input. Members come from workspace globs, LOC is counted
  from `.rs` files, and dependencies come from `Cargo.toml`, so the result
  tracks the repository rather than a hard-coded map.
- Boundaries are explicit and tunable: total LOC threshold, member-count
  threshold, small-crate size and group ceiling, non-adjacent edge threshold and
  DRY tile target all live in one `LayoutConfig` with documented defaults.
- Separate the two questions. Compartments answer "what fits together"; overlap
  tiles answer "what crosses a boundary"; connectivity tiles answer "where might
  duplication live across crates". They are different outputs, not one score.
- Machine-readable as a first-class mode. `--json` emits the layout for other
  tools instead of requiring them to parse text.
- Small workspaces are not decomposed. Below the thresholds the answer is
  `ProjectLayout::Whole`; splitting a five-crate project adds noise, not insight.
- A reusable library, not only a binary: `analyze_rust_layout` returns the same
  typed `ProjectLayout` the CLI prints.

## Scope

- Workspace parsing: member globs, per-crate `.rs` LOC, workspace dependency
  parsing, crate resolution.
- Compartment partitioning with priority-ordered overlap tiles.
- Agglomerative connectivity clustering for DRY tile generation.
- Connectome output as named directed edges.
- Text and JSON output, threshold overrides through CLI flags.

## Non-goals

- Running the analysis for you: it partitions a workspace, it does not review,
  refactor or deduplicate code.
- Semantic or symbol-level analysis. Boundaries come from crate-level LOC and
  direct dependency edges, not from a compiler.
- A general project manager; it does not build, test or publish the workspace.

## Direction

Not yet documented beyond the released feature set. Recent history is
infrastructure: migration to `harbor-rs`, GitHub canonical inputs and Simit 0.18
workflows. The one visible feature thread is that the `cargo-crosscut` flags
advertise crosscut capabilities (compartments, overlap, DRY tiles, connectome)
rather than auto-detected ones.

## Place in the fleet

Canix pins it in `root/hosts/atlas/simit-updater.nix` for automated dependency
updates; beyond that it is a standalone cargo plugin.
