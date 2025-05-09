{
  description = "Wallpaper daemon for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    systems.url = "github:nix-systems/default-linux";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    systems,
    rust-overlay,
    ...
  }: let
    inherit (nixpkgs) lib;
    eachSystem = lib.genAttrs (import systems);
    pkgsFor = eachSystem (system:
      import nixpkgs {
        inherit system;
        overlays = [self.overlays.default (import rust-overlay)];
      });
  in {
    overlays = import ./nix/overlays.nix {inherit self lib pkgsFor;};

    packages = eachSystem (system: {
      default = self.packages.${system}.wpaperd;

      inherit
        (pkgsFor.${system})
        wpaperd
        ;
    });

    devShells = eachSystem (system:
      with pkgsFor.${system}; {
        default = mkShell {
          buildInputs = [
            rust-bin.stable.latest.default
          ];
          packages = [
            pkg-config
            wayland
            glew
          ];
        };
      });

    formatter = eachSystem (system: pkgsFor.${system}.alejandra);
    homeManagerModules.default = import ./nix/hm-module.nix self;
  };
}
