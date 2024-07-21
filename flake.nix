{
  description = "Wallpaper daemon for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    systems.url = "github:nix-systems/default-linux";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    systems,
    fenix,
    ...
  }: let
    inherit (nixpkgs) lib;
    eachSystem = lib.genAttrs (import systems);
    pkgsFor = eachSystem (system:
      import nixpkgs {
        inherit system;
        overlays = [self.overlays.default];
      });
  in {
    overlays = import ./nix/overlays.nix {inherit self lib fenix;};

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
          packages = [
            pkg-config
            wayland
            glew-egl
          ];
        };
      });

    formatter = eachSystem (system: pkgsFor.${system}.alejandra);
    homeManagerModules.default = import ./nix/hm-module.nix self;
  };
}
