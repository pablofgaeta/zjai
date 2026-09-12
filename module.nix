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
        tab-bar location="file:${pluginDir}/zellij_agent_tab_bar.wasm"
        status-bar location="file:${pluginDir}/zellij_agent_status.wasm"
    }
  '';

  home.file = {
    ".config/zellij/plugins/zellij_agent_status.wasm".source = "${plugins.status-bar}/bin/zellij-agent-status.wasm";
    ".config/zellij/plugins/zellij_agent_tab_bar.wasm".source = "${plugins.tab-bar}/bin/zellij-agent-tab-bar.wasm";

    ".pi/agent/extensions/agent-status.ts".source = ./integrations/pi/agent-status.ts;
    ".config/opencode/plugins/agent-status.js".source = ./integrations/opencode/agent-status.js;
    ".gemini/extensions/agent-status/gemini-extension.json".source = ./integrations/gemini/gemini-extension.json;
    ".gemini/extensions/agent-status/hooks/hooks.json".source = ./integrations/gemini/hooks/hooks.json;

    # Shared producer for hook-based agents (e.g. Claude Code, Gemini CLI)
    # that register shell-command hooks in their own settings rather than
    # loading a JS/TS extension module.
    ".local/libexec/agent-status-notify" = {
      source = ./integrations/notify/agent-status-notify.sh;
      executable = true;
    };
  };
}
