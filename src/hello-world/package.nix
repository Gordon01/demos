{ ... }:
{
  perSystem =
    { pkgs, ... }:
    let
      hello-world = pkgs.rustPlatform.buildRustPackage {
        pname = "hello-world";
        version = (fromTOML (builtins.readFile ../../Cargo.toml)).workspace.package.version;
        src = ../..;
        cargoLock.lockFile = ../../Cargo.lock;
        cargoBuildFlags = [
          "-p"
          "hello-world"
        ];
        cargoTestFlags = [
          "-p"
          "hello-world"
        ];

        meta = {
          description = "A Rust hello-world demo";
          mainProgram = "hello-world";
          platforms = pkgs.lib.platforms.unix;
        };
      };
    in
    {
      packages = {
        default = hello-world;
        inherit hello-world;
      };
      checks = { inherit hello-world; };
    };
}
