{
  description = "Starbound Outfit Studio: Rust core, local WASM UI and asset verification";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/fe13242281a67641c0102cdb1087f0ebe586c7df";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      eachSystem = nixpkgs.lib.genAttrs systems;
    in {
      devShells = eachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
          rust = pkgs.rust-bin.stable."1.98.0".minimal.override {
            extensions = [ "clippy" "rustfmt" ];
            targets = [ "wasm32-unknown-unknown" ];
          };
        in {
          default = pkgs.mkShell {
            packages = [ rust pkgs.wasm-bindgen-cli ];
          };
        });
    };
}
