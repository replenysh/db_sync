{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  # outputs = { nixpkgs, utils, naersk, ... }:
  #   utils.lib.eachDefaultSystem (system:
  #     let
  #       pkgs = import nixpkgs { inherit system; };
  #       naersk-lib = pkgs.callPackage naersk { };
  #       isLinux = pkgs.stdenv.isLinux;
  #     in {
  #       defaultPackage = naersk-lib.buildPackage {
  #         src = ./.;
  #         nativeBuildInputs = [ pkgs.cmake pkgs.llvmPackages_latest.llvm ];
  #         buildInputs = if isLinux then [ ] else [ pkgs.libiconv pkgs.darwin.apple_sdk.frameworks.SystemConfiguration ];
  #         LIBCLANG_PATH =
  #           pkgs.lib.makeLibraryPath [ pkgs.llvmPackages_latest.libclang.lib ];
  #         BINDGEN_EXTRA_CLANG_ARGS =
  #           # Includes with normal include path
  #           (builtins.map (a: ''-I"${a}/include"'')
  #             (if isLinux then [ pkgs.glibc.dev ] else [ ]))
  #           # Includes with special directory paths
  #           ++ [
  #             ''
  #               -I"${pkgs.llvmPackages_latest.libclang.lib}/lib/clang/${pkgs.llvmPackages_latest.libclang.version}/include"''
  #             ''-I"${pkgs.glib.dev}/include/glib-2.0"''
  #             "-I${pkgs.glib.out}/lib/glib-2.0/include/"
  #           ];
  #       };
  #     });

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
          LIBCLANG_PATH =
            pkgs.lib.makeLibraryPath [ pkgs.llvmPackages_latest.libclang.lib ];
          BINDGEN_EXTRA_CLANG_ARGS =
            (builtins.map (a: ''-I"${a}/include"'') (if pkgs.stdenv.isLinux then [ pkgs.glibc.dev ] else [ ]))
            ++ [
              ''-I"${pkgs.llvmPackages_latest.libclang.lib}/lib/clang/${pkgs.llvmPackages_latest.libclang.version}/include"''
              ''-I"${pkgs.glib.dev}/include/glib-2.0"''
              "-I${pkgs.glib.out}/lib/glib-2.0/include/"
            ];
          # preBuild = ''
          #   export BINDGEN_EXTRA_CLANG_ARGS="$(< ${stdenv.cc}/nix-support/libc-crt1-cflags) \
          #     $(< ${stdenv.cc}/nix-support/libc-cflags) \
          #     $(< ${stdenv.cc}/nix-support/cc-cflags) \
          #     $(< ${stdenv.cc}/nix-support/libcxx-cxxflags) \
          #     ${lib.optionalString stdenv.cc.isClang "-idirafter ${stdenv.cc.cc}/lib/clang/${lib.getVersion stdenv.cc.cc}/include"} \
          #     ${lib.optionalString stdenv.cc.isGNU "-isystem ${stdenv.cc.cc}/include/c++/${lib.getVersion stdenv.cc.cc} -isystem ${stdenv.cc.cc}/include/c++/${lib.getVersion stdenv.cc.cc}/${stdenv.hostPlatform.config} -idirafter ${stdenv.cc.cc}/lib/gcc/${stdenv.hostPlatform.config}/${lib.getVersion stdenv.cc.cc}/include"} \
          #   "
          # '';
          nativeBuildInputs = [ cmake llvmPackages_latest.llvm ];
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
