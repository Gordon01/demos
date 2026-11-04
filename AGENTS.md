# Project Guidelines

## Build and Test

- Use `nix build` from the repository root to build the default package.
- Enter the Nix development shell with `nix develop` before running Cargo
  commands. Or via `nix develop -c cargo ...`.
- Run `nix flake check` to build the package and run the configured checks.
- Run `nix fmt` to format files and run lints the project.

## Rust

- Follow the [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html);
- Design public APIs according to the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html); keep interfaces minimal, predictable, and documented where needed.

## Project Structure

- Keep each demo's documentation in its project directory; link from the root README.
- Use concise, preferably one-line doc comments for key types and non-obvious methods.
  Skip trivial getters and test functions.
