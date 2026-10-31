{
  description = "LEZ auction engine: auction_ffi, the C library behind the Auctions app core module";

  # Prebuilt lez_core wallet-ffi comes from the Logos cache (the module build pulls it in).
  nixConfig = {
    extra-substituters = [ "https://logos-co.cachix.org" ];
    extra-trusted-public-keys = [
      "logos-co.cachix.org-1:12K8609ho1pCt0erUQrOrs/KmOuhWq/EhVwhzQRb35E="
    ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        # The repo's pinned toolchain (rust-toolchain.toml).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # The whole workspace without build outputs; crane still sees Cargo.lock and every path dependency.
        src = craneLib.cleanCargoSource ./.;

        # auction_ffi reaches the rapidsnark prover through `lee`. Its build script downloads the static
        # libraries unless RAPIDSNARK_LIB_DIR names them, and a Nix build has no network: fetch the
        # release the build script would (version from the logos-blockchain-rust-rapidsnark checkout).
        rapidsnarkVersion = "v0.0.8";
        rapidsnarkAssets = {
          x86_64-linux = {
            slug = "linux-x86_64-pic";
            hash = "sha256-Wb3XCe7ZYjXeBh81KJP0ZQySO1S1kQUhGFkwErsc2DE=";
          };
          aarch64-linux = {
            slug = "linux-aarch64-pic";
            hash = "sha256-X2Wa+CmRgTxX52HyrGsjM1WRyG7pEcL1hBdyx4rwmZ8=";
          };
        };
        rapidsnarkAsset =
          rapidsnarkAssets.${system}
            or (throw "auction_ffi: no rapidsnark asset pinned for ${system} (linux only)");
        rapidsnarkLibs = pkgs.stdenvNoCC.mkDerivation {
          pname = "rapidsnark-pic";
          version = rapidsnarkVersion;
          src = pkgs.fetchurl {
            url = "https://github.com/logos-blockchain/logos-blockchain-rust-rapidsnark/releases/download/rapidsnark-pic-${rapidsnarkVersion}/rapidsnark-${rapidsnarkAsset.slug}-${rapidsnarkVersion}.zip";
            inherit (rapidsnarkAsset) hash;
          };
          nativeBuildInputs = [ pkgs.unzip ];
          sourceRoot = ".";
          installPhase = ''
            mkdir -p $out
            cp -r rapidsnark-${rapidsnarkAsset.slug}-${rapidsnarkVersion}/lib $out/lib
          '';
        };

        # The proof circuits are another prebuilt download of a build script (LBC_ROOT_DIR names a local copy).
        circuitsVersion = "0.5.3";
        circuitsAssets = {
          x86_64-linux = {
            slug = "linux-x86_64";
            hash = "sha256-25129Q81NqwRIGfyCD2K7z+rgCTR7y96JZs3SQvDXqw=";
          };
          aarch64-linux = {
            slug = "linux-aarch64";
            hash = "sha256-Sy/zH4d45c0orABTEC/GUdpTgf7NditgYaqPITQx+ao=";
          };
        };
        circuitsAsset =
          circuitsAssets.${system}
            or (throw "auction_ffi: no circuits asset pinned for ${system} (linux only)");
        circuits = pkgs.stdenvNoCC.mkDerivation {
          pname = "logos-blockchain-circuits";
          version = circuitsVersion;
          src = pkgs.fetchurl {
            url = "https://github.com/logos-blockchain/logos-blockchain-circuits/releases/download/v${circuitsVersion}/logos-blockchain-circuits-v${circuitsVersion}-${circuitsAsset.slug}.tar.gz";
            inherit (circuitsAsset) hash;
          };
          installPhase = ''
            mkdir -p $out
            cp -r . $out
          '';
          dontFixup = true;
        };

        # `lee`'s build script reads the prebuilt privacy-circuit guest from `../artifacts` of its checkout (the
        # execution-zone repository root), and crane vendors crates, not their repository. Add the directory.
        cargoVendorDir = craneLib.vendorCargoDeps {
          inherit src;
          overrideVendorGitCheckout = packages: checkout:
            if pkgs.lib.any (p: pkgs.lib.hasInfix "logos-execution-zone.git" (p.source or "")) packages then
              checkout.overrideAttrs (_: {
                postInstall = ''
                  cp -r artifacts $out/artifacts
                '';
              })
            else
              checkout;
        };

        # One host FFI package per build. The workspace also has methods crates whose build scripts compile
        # RISC Zero guests; `-p` keeps those out, and RISC0_SKIP_BUILD stops risc0-build from trying.
        mkHostFfi = { package, sourceDir, header }:
          let
            commonArgs = {
              inherit src cargoVendorDir;
              strictDeps = true;
              pname = package;
              version = "0.1.0";
              cargoExtraArgs = "-p ${package}";
              doCheck = false;
              RISC0_SKIP_BUILD = "1";
              RAPIDSNARK_LIB_DIR = "${rapidsnarkLibs}/lib";
              LBC_ROOT_DIR = "${circuits}";
              # cbindgen runs as a build-dependency from build.rs.
            };
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          in
          craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            postInstall = ''
              mkdir -p $out/include
              cp ${sourceDir}/include/${header} $out/include/
            '' + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
              # The module builder does not stage external dylibs beside the plugin: use an absolute store name.
              if [ -f $out/lib/lib${package}.dylib ]; then
                install_name_tool -id "$out/lib/lib${package}.dylib" $out/lib/lib${package}.dylib
              fi
            '';
          });

        auctionFfi = mkHostFfi {
          package = "auction_ffi";
          sourceDir = "crates/auction_ffi";
          header = "auction_ffi.h";
        };
      in
      {
        packages = {
          default = auctionFfi;
          auction_ffi = auctionFfi;
        };
      });
}
