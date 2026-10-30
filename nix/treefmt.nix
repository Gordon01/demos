{ inputs, ... }:
{
  imports = [ inputs.treefmt-nix.flakeModule ];

  perSystem = { ... }: {
    treefmt = {
      projectRootFile = "flake.nix";
      programs.mdformat = {
        enable = true;
        settings.number = true;
      };
      programs.nixfmt.enable = true;
      programs.rustfmt.enable = true;
      programs.taplo.enable = true;
    };
  };
}
