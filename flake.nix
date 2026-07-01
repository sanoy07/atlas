{
  description = "Atlas - Developer Knowledge Engine";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
     };   
  };

    outputs = { self, nixpkgs, flake-utils, rust-overlay }:
      flake-utils.lib.eachDefaultSystem (system:
            let
              overlays    = [ (import rust-overlay) ];
              pkgs        = import nixpkgs { inherit system overlays; };
              rustToolchain = pkgs.rust-bin.stable.latest.default.override {
                extensions = [ "rust-src" "clippy" "rustfmt" ];
              };
            in {
              devShells.default = pkgs.mkShell {
                buildInputs = with pkgs; [
                  rustToolchain
                  sqlite
                  git
                  gh
                  pkg-config
                  openssl
                  cargo-watch   # `cargo watch -x test` for inner loop
                ];

                RUST_LOG  = "debug";
                ATLAS_DB  = "./atlas.db";

                shellHook = ''
                  echo ""
                  echo "Atlas dev environment"
                  echo "Rust:   $(rustc --version)"
                  echo "SQLite: $(sqlite3 --version | cut -d' ' -f1)"
                  echo "Git:    $(git --version | cut -d' ' -f3)"
                  echo ""
                '';
              };
      });
}
