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
        postInstall = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isDarwin ''
          app="$out/Applications/Frequency Visualiser.app"
          mkdir -p "$app/Contents/MacOS"
          cp "$out/bin/visualiser" "$app/Contents/MacOS/visualiser"
          cat > "$app/Contents/Info.plist" <<'PLIST'
          <?xml version="1.0" encoding="UTF-8"?>
          <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
          <plist version="1.0">
          <dict>
            <key>CFBundleExecutable</key>
            <string>visualiser</string>
            <key>CFBundleIdentifier</key>
            <string>org.gordon01.demos.visualiser</string>
            <key>CFBundleName</key>
            <string>Frequency Visualiser</string>
            <key>CFBundlePackageType</key>
            <string>APPL</string>
            <key>CFBundleShortVersionString</key>
            <string>0.1.0</string>
            <key>LSMinimumSystemVersion</key>
            <string>14.2</string>
            <key>NSAudioCaptureUsageDescription</key>
            <string>Frequency Visualiser captures system output to display its frequency bands.</string>
          </dict>
          </plist>
          PLIST
          cat > "$out/bin/visualiser-app" <<EOF
          #!/bin/sh
          exec /usr/bin/open -a "$app" --args "\$@"
          EOF
          chmod +x "$out/bin/visualiser-app"
        '';
        postFixup = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
          wrapProgram "$out/bin/visualiser" \
            --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath visualiser.buildInputs}"
        '';

        meta = {
          description = "A frequency-band audio visualiser";
          mainProgram = if pkgs.stdenv.hostPlatform.isDarwin then "visualiser-app" else "visualiser";
          platforms = pkgs.lib.platforms.unix;
        };
      };
    in
    {
      packages = { inherit visualiser; };
      checks = { inherit visualiser; };
    };
}
