{
  description = "usb-ser-mon implemented in rust";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs =
    { self
    , nixpkgs
    , rust-overlay
    , crane
    , flake-utils
    }:
    flake-utils.lib.eachDefaultSystem (system:
    let
      overlays = [ (import rust-overlay) ];
      pkgs = import nixpkgs { inherit system overlays; };
      toolchain = pkgs.rust-bin.stable.latest.default;
      craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
      packages = self.packages.${system};
    in
    {
      packages = {
        serial-monitor = craneLib.buildPackage {
          src = craneLib.cleanCargoSource (craneLib.path ./.);
          strictDeps = true;
          nativeBuildInputs = with pkgs; [
            pkg-config
            installShellFiles
          ];
          buildInputs = with pkgs; [
            systemdMinimal
          ];
          postInstall = ''
            installShellCompletion --cmd serial-monitor \
              --bash <($out/bin/serial-monitor completion bash) \
              --fish <($out/bin/serial-monitor completion fish) \
              --zsh <($out/bin/serial-monitor completion zsh)
          '';
        };
        default = packages.serial-monitor;
      };

      apps.default = flake-utils.lib.mkApp {
        drv = packages.default;
      };

      devShells.default = pkgs.mkShell {
        inputsFrom = [ packages.serial-monitor ];
        packages = with pkgs; [
          pre-commit
          nixpkgs-fmt
          rust-analyzer
        ];
      };
    });
}
