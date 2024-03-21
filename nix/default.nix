{ config
, lib
, pkgs
, rustPlatform
, version
, pkg-config
, libxkbcommon
, ...
}:

rustPlatform.buildRustPackage rec {
  inherit version;
  pname = "wpaperd";

  src = lib.cleanSourceWith {
    filter = name: type:
      let
        baseName = baseNameOf (toString name);
      in
        ! (
          lib.hasSuffix ".nix" baseName
        );
    src = lib.cleanSource ../.;
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "timer-0.2.0" = "sha256-yofy6Wszf6EwNGGdVDWNG0RcbpvBgv5/BdOjAFxghwc=";
    };
  };

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    libxkbcommon
  ];

  meta = with lib; {
    homepage = "https://github.com/Narice/wpaperd";
    description = "Wallpaper daemon for Wayland";
    license = licenses.gpl3;
    platforms = platforms.linux;
    mainProgram = "wpaperd";
  };
}
