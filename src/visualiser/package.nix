{ ... }:
{
  perSystem =
    { pkgs, ... }:
    let
      visualiser = pkgs.rustPlatform.buildRustPackage {
        pname = "visualiser";
        version = (fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
        src = ../..;
        cargoLock.lockFile = ../../Cargo.lock;
        cargoBuildFlags = [
          "-p"
          "visualiser"
        ];
        cargoTestFlags = [
          "-p"
          "visualiser"
        ];
        nativeBuildInputs = [
          pkgs.llvmPackages.libclang
        ]
        ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          pkgs.makeWrapper
          pkgs.pkg-config
        ];
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          pkgs.alsa-lib
          pkgs.libGL
          pkgs.libxkbcommon
          pkgs.wayland
          pkgs.xorg.libX11
          pkgs.xorg.libXcursor
          pkgs.xorg.libXext
          pkgs.xorg.libXi
          pkgs.xorg.libXinerama
          pkgs.xorg.libXrandr
        ];
        postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
          wrapProgram "$out/bin/visualiser" \
            --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath visualiser.buildInputs}"
        '';

        meta = {
          description = "A frequency-band audio visualiser";
          mainProgram = "visualiser";
          platforms = pkgs.lib.platforms.unix;
        };
      };
    in
    {
      packages = { inherit visualiser; };
      checks = { inherit visualiser; };
    };
}
