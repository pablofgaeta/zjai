{self}: {
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.zjai;
  plugins = self.packages.${pkgs.system};
  pluginDir = "${config.home.homeDirectory}/.config/zellij/plugins";

  pluginConfig = lib.concatStringsSep "\n" (lib.filter (line: line != "") [
    (lib.optionalString cfg.plugins.tabBar ''        tab-bar location="file:${pluginDir}/zjai_tab_bar.wasm"'')
    (lib.optionalString cfg.plugins.statusBar ''        status-bar location="file:${pluginDir}/zjai_status.wasm"'')
  ]);

  needsNotify = cfg.integrations.pi || cfg.integrations.opencode || cfg.integrations.gemini;
in {
  options.programs.zjai = {
    enable = lib.mkEnableOption "Zellij Agent State Protocol integrations" // {default = true;};

    plugins = {
      statusBar = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Install and register the reference cross-session zjai status-bar plugin.";
      };

      tabBar = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Install and register the reference zjai-aware tab-bar plugin.";
      };
    };

    integrations = {
      pi = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Install the Pi zjai extension.";
      };

      opencode = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Install the OpenCode zjai plugin.";
      };

      gemini = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Install the Gemini CLI zjai extension and hooks.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    # Both aliases can be overridden independently. The vendored tab-bar
    # renders the status glyph next to the tab name, and the status-bar renders
    # the cross-session summary. Zellij instantiates each plugin once per tab;
    # both are read-only renderers, so the copies need no coordination.
    programs.zellij.extraConfig = lib.mkIf (pluginConfig != "") ''
      plugins {
${pluginConfig}
      }
    '';

    home.file = lib.mkMerge [
      (lib.mkIf cfg.plugins.statusBar {
        ".config/zellij/plugins/zjai_status.wasm".source = "${plugins.status-bar}/bin/zjai-status.wasm";
      })

      (lib.mkIf cfg.plugins.tabBar {
        ".config/zellij/plugins/zjai_tab_bar.wasm".source = "${plugins.tab-bar}/bin/zjai-tab-bar.wasm";
      })

      (lib.mkIf cfg.integrations.pi {
        ".pi/agent/extensions/zjai.ts".source = ./integrations/pi/zjai.ts;
      })

      (lib.mkIf cfg.integrations.opencode {
        ".config/opencode/plugins/zjai.js".source = ./integrations/opencode/zjai.js;
      })

      (lib.mkIf cfg.integrations.gemini {
        ".gemini/extensions/zjai/gemini-extension.json".source = ./integrations/gemini/gemini-extension.json;
        ".gemini/extensions/zjai/hooks/hooks.json".source = ./integrations/gemini/hooks/hooks.json;
      })

      # Shared producer for hook-based agents (e.g. Claude Code, Gemini CLI)
      # that register shell-command hooks in their own settings rather than
      # loading a JS/TS extension module.
      (lib.mkIf needsNotify {
        ".local/libexec/zjai-notify" = {
          source = ./integrations/notify/zjai-notify.sh;
          executable = true;
        };
      })
    ];
  };
}
