{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{
      self,
      flake-parts,
      rust-overlay,
      ...
    }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.treefmt-nix.flakeModule
      ];

      systems = inputs.nixpkgs.lib.systems.flakeExposed;

      perSystem =
        {
          pkgs,
          ...
        }:
        let
          rustPkgs = pkgs.appendOverlays [ (import rust-overlay) ];
          toolchain = rustPkgs.rust-bin.stable.latest.complete;
        in
        {
          treefmt.config = {
            package = pkgs.treefmt;
            programs.rustfmt.enable = true;
            programs.nixfmt.enable = true;
            programs.prettier.enable = true;
            programs.taplo.enable = true;
            programs.beautysh = {
              enable = true;
              indent_size = 4;
            };
          };

          packages = rec {
            default = joshuto;
            joshuto = pkgs.callPackage ./utils/nix {
              inherit toolchain;
              version = self.shortRev or self.dirtyShortRev or "unknown";
            };
          };

          devShells.default = pkgs.mkShell {
            packages = [
              toolchain
            ];
            shellHook = ''
              echo $'\e[1;32mWelcome to joshuto project\e[0m'
            '';
          };
        };
    };
}
