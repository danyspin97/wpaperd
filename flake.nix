{
  description = "Wallpaper daemon for Wayland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    inputs @ { self
    , nixpkgs
    , ...
    }:
    let
      inherit (nixpkgs) lib;
      genSystems = lib.genAttrs [
        "x86_64-linux"
      ];

      pkgsFor = genSystems (system:
        import nixpkgs {
          inherit system;
        });

      mkDate = longDate: (lib.concatStringsSep "-" [
        (builtins.substring 0 4 longDate)
        (builtins.substring 4 2 longDate)
        (builtins.substring 6 2 longDate)
      ]);
    in
    {
      overlays.default = _: prev: rec {
        wpaperd = prev.callPackage ./nix/default.nix {
          version = "0.2.0" + "+date=" + (mkDate (self.lastModifiedDate or "19700101")) + "_" + (self.shortRev or "dirty");
        };
      };
      packages = genSystems
        (system:
          (self.overlays.default null pkgsFor.${system})
          // {
            default = self.packages.${system}.wpaperd;
          });
    };
}
