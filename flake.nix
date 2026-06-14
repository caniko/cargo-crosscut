{
  description = "Analyze Rust workspace layout by decomposing large workspaces into bounded analysis units";

  inputs = {
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
      rustToolchain = pkgs.rust-bin.stable.latest.default;
      website = plinth.lib.${system}.mkProjectSite {
        pname = "cargo-crosscut-website";
        domain = "cargo-crosscut.tartanoglu.com";
        configPath = ./website/plinth-project.toml;
      };
      cargo-crosscut = pkgs.rustPlatform.buildRustPackage {
        pname = "cargo-crosscut";
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [rustToolchain];
        meta.mainProgram = "cargo-crosscut";
      };
    in {
      default = cargo-crosscut;
      inherit cargo-crosscut website;
      site = website;
    });

    apps = forAllSystems (system: {
      deploy-pages = plinth.lib.${system}.mkDeployPagesApp {
        domain = "cargo-crosscut.tartanoglu.com";
      };
    });
  };
}
