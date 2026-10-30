# Project Guidelines

## Build and Test

- Use `nix build` from the repository root to build the default package.
- Enter the Nix development shell with `nix develop` before running Cargo
  commands.
- Run `nix flake check` to build the package and run the configured checks.
- Run `nix fmt` to format the project.
