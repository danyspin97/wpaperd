self: {
  config,
  lib,
  pkgs,
  ...
}: let
  inherit (pkgs.stdenv.hostPlatform) system;
  package = self.packages.${system}.default;
in {
  config = {
    programs.wpaperd.package = lib.mkDefault package;
  };
}
