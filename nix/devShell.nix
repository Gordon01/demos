{ ... }:
{
  perSystem =
    { config, pkgs, ... }:
    {
      devShells.default = pkgs.mkShell {
        packages =
          with pkgs;
          [
            clippy
            rustfmt
          ]
          ++ lib.optionals stdenv.hostPlatform.isLinux [ python3 ];

        inputsFrom = [
          config.packages.hello-world
          config.packages.visualiser
        ]
        ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [ config.packages.memfd ];

        RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux (
          pkgs.lib.makeLibraryPath config.packages.visualiser.buildInputs
        );
      };
    };
}
