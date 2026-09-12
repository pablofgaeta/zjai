{self}: {
  config,
  pkgs,
  ...
}: let
  plugins = self.packages.${pkgs.system};
  pluginDir = "${config.home.homeDirectory}/.config/zellij/plugins";
in {
  # Both aliases are overridden so the vendored tab-bar renders the status
  # glyph next to the tab name while the status-bar renders the
  # cross-session summary. Zellij instantiates each plugin once per tab; both
  # are read-only renderers, so the copies need no coordination.
  programs.zellij.extraConfig = ''
    plugins {
        tab-bar location="file:${pluginDir}/zjai_tab_bar.wasm"
        status-bar location="file:${pluginDir}/zjai_status.wasm"
    }
  '';

  home.file = {
    ".config/zellij/plugins/zjai_status.wasm".source = "${plugins.status-bar}/bin/zjai-status.wasm";
    ".config/zellij/plugins/zjai_tab_bar.wasm".source = "${plugins.tab-bar}/bin/zjai-tab-bar.wasm";

    ".pi/agent/extensions/zjai.ts".source = ./integrations/pi/zjai.ts;
    ".config/opencode/plugins/zjai.js".source = ./integrations/opencode/zjai.js;
    ".gemini/extensions/zjai/gemini-extension.json".source = ./integrations/gemini/gemini-extension.json;
    ".gemini/extensions/zjai/hooks/hooks.json".source = ./integrations/gemini/hooks/hooks.json;

    # Shared producer for hook-based agents (e.g. Claude Code, Gemini CLI)
    # that register shell-command hooks in their own settings rather than
    # loading a JS/TS extension module.
    ".local/libexec/zjai-notify" = {
      source = ./integrations/notify/zjai-notify.sh;
      executable = true;
    };
  };
}
