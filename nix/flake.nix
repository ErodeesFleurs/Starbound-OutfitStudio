{
  description = "Starbound Outfit Studio: Rust core, local WASM UI and asset verification";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/d4f25a3e562008648d45bcf2d70bcc7422cad84e";
    rust-overlay.url = "github:oxalica/rust-overlay/8bbfc858c7b85f5e6ed6d6cc201ffcee1bb08a9b";
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
          rust = pkgs.rust-bin.stable."1.100.0".minimal.override {
            extensions = [ "clippy" "rustfmt" ];
            targets = [ "wasm32-unknown-unknown" ];
          };
        in {
          default = pkgs.mkShell {
            packages = [ rust pkgs.wasm-bindgen-cli pkgs.typescript ];
          };
        });
    };
}
