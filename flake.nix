{
  description = "Cross-agent status: zellij wasm plugins + AI CLI integrations.";

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

    mkPlugins = system: let
      pkgs = nixpkgs.legacyPackages.${system};
      wasmPkgs = pkgs.pkgsCross.wasm32-wasip1;

      # crane needs one toolchain derivation exposing both cargo and rustc;
      # nixpkgs keeps them separate on the cross package set.
      toolchain = pkgs.symlinkJoin {
        name = "wasm32-wasip1-rust-toolchain";
        paths = [wasmPkgs.rustPlatform.rust.rustc wasmPkgs.rustPlatform.rust.cargo];
      };

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
      status-bar = buildPlugin "zellij-agent-status";
      tab-bar = buildPlugin "zellij-agent-tab-bar";
    };
  in {
    packages = forEachSystem mkPlugins;
  };
}
