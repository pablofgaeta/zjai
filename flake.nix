{
  description = "Cross-zjai: zellij wasm plugins + AI CLI integrations.";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    ...
  }: let
    systems = ["aarch64-darwin" "x86_64-linux"];
    forEachSystem = nixpkgs.lib.genAttrs systems;

    mkWasmEnv = system: let
      pkgs = nixpkgs.legacyPackages.${system};
      wasmPkgs = pkgs.pkgsCross.wasm32-wasip1;
    in {
      inherit pkgs wasmPkgs;

      # crane and dev shells need one toolchain derivation exposing both cargo
      # and rustc; nixpkgs keeps them separate on the cross package set.
      toolchain = pkgs.symlinkJoin {
        name = "wasm32-wasip1-rust-toolchain";
        paths = [wasmPkgs.rustPlatform.rust.rustc wasmPkgs.rustPlatform.rust.cargo];
      };
    };

    mkPlugins = system: let
      env = mkWasmEnv system;
      inherit (env) pkgs wasmPkgs toolchain;

      craneLib = (crane.mkLib pkgs).overrideToolchain (_: toolchain);

      commonArgs = {
        src = craneLib.cleanCargoSource ./.;
        strictDeps = true;
        doCheck = false;
        CARGO_BUILD_TARGET = "wasm32-wasip1";
        nativeBuildInputs = [wasmPkgs.lld];
        env.RUSTFLAGS = "-C linker=wasm-ld";
      };

      # Built once per system; reused by both plugin builds below and
      # unaffected by edits to status-bar/tab-bar source.
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      # crane installs whatever binaries the build produces (scanned from
      # cargo's build log), named after the crate; `-p` scopes each package
      # to its own plugin so it doesn't also pull in the other's binary.
      buildPlugin = pname:
        craneLib.buildPackage (commonArgs
          // {
            inherit pname cargoArtifacts;
            cargoExtraArgs = "-p ${pname}";
          });
    in {
      status-bar = buildPlugin "zjai-status";
      tab-bar = buildPlugin "zjai-tab-bar";
    };
    mkDevShell = system: let
      env = mkWasmEnv system;
      inherit (env) pkgs wasmPkgs toolchain;
    in
      pkgs.mkShell {
        packages = [toolchain wasmPkgs.lld];
        CARGO_BUILD_TARGET = "wasm32-wasip1";
        RUSTFLAGS = "-C linker=wasm-ld";
      };
  in {
    packages = forEachSystem mkPlugins;
    checks = forEachSystem (system: self.packages.${system});
    devShells = forEachSystem (system: {default = mkDevShell system;});

    homeManagerModules.default = import ./module.nix {inherit self;};
  };
}
