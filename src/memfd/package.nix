{ ... }:
{
  perSystem =
    {
      lib,
      pkgs,
      system,
      ...
    }:
    lib.mkIf (lib.hasSuffix "-linux" system) (
      let
        memfd = pkgs.rustPlatform.buildRustPackage {
          pname = "memfd";
          version = (fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
          src = ../..;
          cargoLock.lockFile = ../../Cargo.lock;
          cargoBuildFlags = [
            "-p"
            "memfd"
          ];
          cargoTestFlags = [
            "-p"
            "memfd"
          ];
          postBuild = ''
            "$CXX" -std=c++17 -O2 -Wall -Wextra -Wpedantic -Werror \
              src/memfd/cpp/client.cpp -o memfd-client
          '';
          postInstall = ''
            install -Dm755 memfd-client "$out/bin/memfd-client"
          '';

          meta = {
            description = "Zero-copy shared-memory exchange between Rust and C++";
            mainProgram = "memfd-server";
            platforms = pkgs.lib.platforms.linux;
          };
        };
      in
      {
        packages = { inherit memfd; };
        checks = {
          inherit memfd;
          memfd-roundtrip =
            pkgs.runCommand "memfd-roundtrip"
              {
                nativeBuildInputs = [ pkgs.python3 ];
              }
              ''
                python3 ${./tests/smoke.py} \
                  ${memfd}/bin/memfd-server ${memfd}/bin/memfd-client
                touch "$out"
              '';
        };
      }
    );
}
