{
  lib,
  rustPlatform,
  pkg-config,
  wayland,
  glew-egl,
  version ? "git",
}:
rustPlatform.buildRustPackage rec {
  pname = "wpaperd";
  inherit version;

  src = lib.cleanSourceWith {
    filter = name: _type: let
      baseName = baseNameOf (toString name);
    in
      !(lib.hasSuffix ".nix" baseName);
    src = lib.cleanSource ../.;
  };

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs =
    [
      wayland
      glew-egl
    ];

  cargoLock.lockFile = ../Cargo.lock;

  meta = with lib; {
    description = "Wallpaper daemon for Wayland";
    longDescription = ''
      It allows the user to choose a different image for each output (aka for each monitor)
      just as swaybg. Moreover, a directory can be chosen and wpaperd will randomly choose
      an image from it. Optionally, the user can set a duration, after which the image
      displayed will be changed with another random one.
    '';
    homepage = "https://github.com/danyspin97/wpaperd";
    license = licenses.gpl3Plus;
    platforms = platforms.linux;
    maintainers = with maintainers; [yunfachi];
    mainProgram = "wpaperd";
  };
}
