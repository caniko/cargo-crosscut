{
  description = "Analyze Rust workspace layout by decomposing large workspaces into bounded analysis units";

  inputs = {
    harbor-rs.url = "git+https://github.com/caniko/harbor-rs.git?ref=trunk&rev=0d53ee174d64cdebf22666691a7e8a99b2eb8cc6";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    plinth = {
      # Pinned: plinth trunk now asserts dioxus-cli 0.7.10, which this repo's
      # nixpkgs (2026-07-29) does not provide. Bump together with nixpkgs.
      url = "git+https://github.com/caniko/plinth.git?ref=refs/heads/trunk&rev=bfa970d7f38f51515e01092cd7f9fde81eef26ae";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    harbor-rs,
    nixpkgs,
    rust-overlay,
    plinth,
    ...
  }: let
    systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forAllSystems = nixpkgs.lib.genAttrs systems;
    mkRelease = {pkgs, system}:
      harbor-rs.lib.mkBinstallRelease {
        inherit pkgs system;
        pname = "cargo-crosscut";
        version = "0.1.2";
        commonArgs = {
          src = pkgs.lib.cleanSource ./.;
          cargoLock = ./Cargo.lock;
          strictDeps = true;
        };
        crates.cargo-crosscut.cargoToml = ./Cargo.toml;
      };
  in {
    packages = forAllSystems (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      toolchain = harbor-rs.lib.mkToolchain {
        inherit pkgs;
        toolchainProfile = "stable";
      };
      rustPlatform = pkgs.makeRustPlatform {
        rustc = toolchain.rustToolchain;
        cargo = toolchain.rustToolchain;
      };
      buildCache = harbor-rs.lib.mkBuildCachePolicy {
        inherit pkgs;
        sccachePackage = harbor-rs.packages.${system}.sccache;
        cacheRoot = null;
        namespaceScope = "canix-rust";
        namespaceGeneration = 5;
      };
      website = plinth.lib.${system}.mkProjectSite {
        pname = "cargo-crosscut-website";
        domain = "cargo-crosscut.tartanoglu.com";
        configPath = ./website/plinth-project.toml;
      };
      cargo-crosscut = buildCache.withRustCache {
        package = rustPlatform.buildRustPackage {
          pname = "cargo-crosscut";
          version = "0.1.2";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          meta.mainProgram = "cargo-crosscut";
        };
      };
      release = mkRelease {inherit pkgs system;};
    in
      {
        default = cargo-crosscut;
        inherit cargo-crosscut website;
        site = website;
      }
      // nixpkgs.lib.optionalAttrs (release.releaseBundle != null) {
        # Per-system prebuilt archives for `cargo binstall cargo-crosscut`;
        # built by the Simit release workflow's `system_runners` matrix.
        release-bundle = release.releaseBundle;
      });

    # `cargo binstall` reads [package.metadata.binstall] from Cargo.toml, so the
    # check keeps that block equal to the archives the bundle publishes.
    checks = forAllSystems (system:
      (mkRelease {
        pkgs = import nixpkgs {
          inherit system;
          overlays = [rust-overlay.overlays.default];
        };
        inherit system;
      })
      .checks);

    devShells = forAllSystems (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      toolchain = harbor-rs.lib.mkToolchain {
        inherit pkgs;
        toolchainProfile = "stable";
      };
    in {
      default = pkgs.mkShell {
        packages = [toolchain.rustToolchain pkgs.treefmt];
      };
    });

    apps = forAllSystems (system: {
      deploy-pages = plinth.lib.${system}.mkDeployPagesApp {
        domain = "cargo-crosscut.tartanoglu.com";
      };
    });
  };
}
