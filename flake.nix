{
  description = "Wallpaper daemon for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    systems.url = "github:nix-systems/default-linux";
  };

  outputs = {
    self,
    nixpkgs,
    systems,
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
    overlays = import ./nix/overlays.nix {inherit self lib;};

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
            egl-wayland
            glew-egl
            mesa
          ];

          shellHook = ''
            # Set LD_LIBRARY_PATH to include paths to Mesa libraries
            export LD_LIBRARY_PATH="${lib.makeLibraryPath [ wayland glew-egl egl-wayland mesa ]}:$LD_LIBRARY_PATH"
          '';
        };
      });

    formatter = eachSystem (system: pkgsFor.${system}.alejandra);
    homeManagerModules.default = import ./nix/hm-module.nix self;
  };


  
}
