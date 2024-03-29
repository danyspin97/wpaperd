{
  self,
  lib,
}: let
  inherit ((builtins.fromTOML (builtins.readFile ../daemon/Cargo.toml)).package) version;

  mkDate = longDate: (lib.concatStringsSep "-" [
    (builtins.substring 0 4 longDate)
    (builtins.substring 4 2 longDate)
    (builtins.substring 6 2 longDate)
  ]);
in {
  default = lib.composeManyExtensions [
    (final: _prev: let
      date = mkDate self.lastModifiedDate or "19700101";
    in {
      wpaperd = final.callPackage ./default.nix {
        version = "${version}+date=${date}_${self.shortRev or "dirty"}";
      };
    })
  ];
}
