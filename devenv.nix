{ pkgs, lib, config, inputs, ... }:

{
  languages.rust = {
    enable = true;
    channel = "stable";
  };

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
  };

  # Identifies this project to metadev's per-project config (e.g. SSH)
  env.METADEV_PROJECT = "cdj-media";

  # Single crate, not a polylith workspace
  metadev.polylith.enable = false;

  claude.code.enable = true;
}
