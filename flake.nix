{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    systems.url = "github:nix-systems/default";
  };
  outputs =
    {
      self,
      systems,
      nixpkgs,
      ...
    }@inputs:
    let
      lib = nixpkgs.lib;
      eachSystem =
        f:
        lib.genAttrs (import systems) (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ inputs.rust-overlay.overlays.default ];
            }
          )
        );

      mkAsterveil =
        pkgs:
        pkgs.rustPlatform.buildRustPackage {
          pname = "asterveil";
          version = "0.1.0";
          src = builtins.path {
            path = ./.;
            name = "asterveil-src";
            filter =
              path: type:
              let
                base = builtins.baseNameOf path;
              in
              !(
                base == ".git"
                || base == ".direnv"
                || base == "target"
                || base == ".cache"
              );
          };
          cargoHash = "sha256-s1Gospnpe4LtrZzkBcL1J6+M0x0nK2kecW2WGsQJPe8=";

          meta = with lib; {
            description = "A ratatui-based NVIDIA GPU control TUI";
            homepage = "https://example.invalid/asterveil";
            license = licenses.mit;
            mainProgram = "asterveil";
            platforms = platforms.linux;
          };
        };
    in
    {
      overlays.default = final: prev: {
        asterveil = mkAsterveil prev;
      };

      packages = eachSystem (
        pkgs: {
          asterveil = mkAsterveil pkgs;
          default = mkAsterveil pkgs;
        }
      );

      devShells = eachSystem (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            # Complete Rust toolchain with cargo, rustc, etc.
            (rust-bin.stable.latest.default.override {
              extensions = [
                "rust-analyzer"
                "clippy"
                "rustfmt"
                "rust-src"
                "rust-docs"
              ];
              targets = [ "x86_64-unknown-linux-musl" ];
            })
            # Or alternatively, you can use the complete toolchain:
            # (rust-bin.stable.latest.complete)
            # (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
            # clang
            # Use mold when we are running in Linux
            # (pkgs.lib.optionals pkgs.stdenv.isLinux pkgs.mold)
          ];
          RUST_SRC_PATH = "${pkgs.rust-bin.stable.latest.rust-src}/lib/rustlib/src/rust/library";
        };
      });
    };
}
