{
  description = "Wallpaper daemon for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    systems.url = "github:nix-systems/default-linux";
    rust-overlay.url = "github:oxalica/rust-overlay";
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
    overlays = import ./nix/overlays.nix {inherit self lib;};

    packages = eachSystem (system: {
      default = self.packages.${system}.wpaperd;

      inherit
        (pkgsFor.${system})
        wpaperd
        overlays
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
            glew-egl
          ];
        };
      });

    formatter = eachSystem (system: pkgsFor.${system}.alejandra);
    homeManagerModules.default = import ./nix/hm-module.nix self;
  };
}
