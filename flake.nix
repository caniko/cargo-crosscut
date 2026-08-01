{
  description = "Analyze Rust workspace layout by decomposing large workspaces into bounded analysis units";

  inputs = {
    rs-harbor.url = "github:caniko/rs-harbor/e2778ff3beca1bd4c1f5183313251d1fb5b46dd6";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    plinth = {
      url = "git+https://codeberg.org/caniko/plinth.git?ref=refs/heads/trunk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    rs-harbor,
    nixpkgs,
    rust-overlay,
    plinth,
    ...
  }: let
    systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forAllSystems = nixpkgs.lib.genAttrs systems;
  in {
    packages = forAllSystems (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      toolchain = rs-harbor.lib.mkToolchain { inherit pkgs; toolchainProfile = "stable"; };
      rustPlatform = pkgs.makeRustPlatform { rustc = toolchain.rustToolchain; cargo = toolchain.rustToolchain; };
      buildCache = rs-harbor.lib.mkBuildCachePolicy {
        inherit pkgs;
        sccachePackage = rs-harbor.packages.${system}.sccache;
        cacheRoot = null;
        namespaceScope = "canix-rust";
        namespaceGeneration = 5;
      };
      website = plinth.lib.${system}.mkProjectSite {
        pname = "cargo-crosscut-website";
        domain = "cargo-crosscut.tartanoglu.com";
        configPath = ./website/plinth-project.toml;
      };
      cargo-crosscut = buildCache.withRustCache { package = rustPlatform.buildRustPackage {
        pname = "cargo-crosscut";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        meta.mainProgram = "cargo-crosscut";
      }; };
    in {
      default = cargo-crosscut;
      inherit cargo-crosscut website;
      site = website;
    });

    apps = forAllSystems (system: let
      pkgs = import nixpkgs {inherit system;};
      atticAdapter = rs-harbor.lib.mkAdapter {
        attic = {
          endpoint = "https://attic.candee.baby";
          cache = "canix";
        };
      };
    in {
      push-flake-inputs = rs-harbor.lib.mkAtticPush {
        inherit pkgs;
        adapter = atticAdapter;
        flake = ".";
      };
      deploy-pages = plinth.lib.${system}.mkDeployPagesApp {
        domain = "cargo-crosscut.tartanoglu.com";
      };
    });
  };
}
