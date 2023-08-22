{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, utils, naersk, ... }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };
        isLinux = pkgs.stdenv.isLinux;
      in {
        defaultPackage = naersk-lib.buildPackage {
          src = ./.;
          nativeBuildInputs = [ pkgs.cmake pkgs.llvmPackages_latest.llvm ];
          buildInputs = if isLinux then [ ] else [ pkgs.libiconv ];

          LIBCLANG_PATH =
            pkgs.lib.makeLibraryPath [ pkgs.llvmPackages_latest.libclang.lib ];

          BINDGEN_EXTRA_CLANG_ARGS =
            # Includes with normal include path
            (builtins.map (a: ''-I"${a}/include"'')
              (if isLinux then [ pkgs.glibc.dev ] else [ ]))
            # Includes with special directory paths
            ++ [
              ''
                -I"${pkgs.llvmPackages_latest.libclang.lib}/lib/clang/${pkgs.llvmPackages_latest.libclang.version}/include"''
              ''-I"${pkgs.glib.dev}/include/glib-2.0"''
              "-I${pkgs.glib.out}/lib/glib-2.0/include/"
            ];
        };
        devShell = with pkgs;
          mkShell {
            buildInputs = [
              cargo
              rustc
              rustfmt
              pre-commit
              rustPackages.clippy
              postgresql_15
              python311Packages.sqlparse

              libiconv
              cmake
              pkgs.llvmPackages_latest.llvm
            ];
            RUST_SRC_PATH = rustPlatform.rustLibSrc;

            LOCAL_DB_URL =
              "postgresql://postgres:postgres@localhost:54322/postgres";

            PGHOST = "localhost";
            PGPORT = "54322";
            PGUSER = "postgres";
            PGPASSWORD = "postgres";
            PGDATABASE = "postgres";
          };
      });
}
