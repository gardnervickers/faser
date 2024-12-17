{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    fenix.url = "github:nix-community/fenix";
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
      utils,
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ fenix.overlays.default ];
        pkgs = import nixpkgs {
          system = system;
          overlays = overlays;
        };
        fx = fenix.packages.${system};
        rust-toolchain = fx.combine [
          fx.latest.cargo
          fx.latest.rustc
          fx.latest.rust-analyzer
          fx.latest.clippy
          fx.latest.rustfmt
          fx.latest.rust-src
          fx.latest.miri
        ];
      in
      {
        devShell = pkgs.mkShell {
          nativeBuildInputs = [
            pkgs.cargo-udeps
            pkgs.cargo-outdated
            rust-toolchain
          ];
        };
      }
    );
}
