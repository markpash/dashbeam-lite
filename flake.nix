{
  description = "DashBeam development shell (Rust + gpui-kit, macOS)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f (
            import nixpkgs {
              inherit system;
              overlays = [ (import rust-overlay) ];
            }
          )
        );
    in
    {
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          name = "dashbeam";
          nativeBuildInputs = [
            (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
              extensions = [
                "rust-src"
                "rust-analyzer"
                "clippy"
                "rustfmt"
              ];
            }
          ];
          shellHook = ''
            echo "DashBeam dev shell: rustc $(rustc --version | cut -d' ' -f2)"
            echo "  cargo run                    run the app"
            echo "  cargo xtask bundle           build DashBeam.app"
          '';
        };
      });
    };
}
