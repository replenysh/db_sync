{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { nixpkgs, utils, rust-overlay, ... } @ inputs: 
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          overlays = [ (import rust-overlay) ];
          inherit system;
        };
      in
      {
        defaultPackage = with pkgs; rustPlatform.buildRustPackage {
          pname = "db_sync";
          version = "1.0.3";
          src = ./.;
          cargoHash = "sha256-l1vL2ZdtDRxSGvP0X/l3nMw8+6WF67KPutJEzUROjg8=";
          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };
          nativeBuildInputs = [ cmake llvmPackages_latest.llvm rustPlatform.bindgenHook ];
          buildInputs = if stdenv.isLinux then [ ] else [ libiconv darwin.apple_sdk.frameworks.SystemConfiguration ];
        };
        devShell = with pkgs;
          mkShell {
            buildInputs = [
              cargo
              postgresql_15
              python311Packages.sqlparse
              rust-bin.beta.latest.default
              darwin.apple_sdk.frameworks.SystemConfiguration
              cmake
              rustc
              rustfmt
              pre-commit
              rustPackages.clippy
              libiconv
              llvmPackages_latest.llvm
            ];
            LOCAL_DB_URL = "postgresql://postgres:postgres@localhost:54322/postgres";
            PGHOST = "localhost";
            PGPORT = "54322";
            PGUSER = "postgres";
            PGPASSWORD = "postgres";
            PGDATABASE = "postgres";
          };
      }
    );
}
