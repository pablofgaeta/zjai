# zjai

`zjai` is a Zellij Agent State Protocol implementation.

It lets agent integrations publish current state for a Zellij pane, then lets any Zellij UI plugin render that state. The bundled status bar and tab bar are reference UI plugins, not required parts of the protocol.

## What is included

- `docs/protocol.md`: Zellij Agent State Protocol v1.
- `zjai-core`: Rust reader library for Zellij wasm UI plugins.
- `integrations/notify/zjai-notify.sh`: shell writer for hook-based agents.
- `integrations/pi`: Pi extension writer.
- `integrations/opencode`: OpenCode plugin writer.
- `integrations/gemini`: Gemini CLI hook writer.
- `status-bar`: optional cross-session summary UI plugin.
- `tab-bar`: optional tab bar UI plugin.

## Protocol shape

Writers publish one file per Zellij pane:

```text
${TMPDIR:-/tmp}/zellij-<uid>/zjai/<session>/<pane_id>
```

Zellij wasm plugins read the same state as:

```text
/tmp/zjai/<session>/<pane_id>
```

Each record is one line:

```text
<status> <epoch_seconds> <source> [metadata...]
```

Missing files mean `idle`. Writers update records with a temp file and atomic rename.

See `docs/protocol.md` for the full protocol.

## Using the Home Manager module

Import `homeManagerModules.default`, then configure the pieces you want:

```nix
programs.zjai = {
  enable = true;

  plugins = {
    statusBar = true;
    tabBar = true;
  };

  integrations = {
    pi = true;
    opencode = true;
    gemini = true;
  };
};
```

All options default to `true` when `programs.zjai.enable` is enabled.

To use only the protocol writers without the reference UI plugins:

```nix
programs.zjai = {
  enable = true;
  plugins.statusBar = false;
  plugins.tabBar = false;
};
```

To use only one reference plugin:

```nix
programs.zjai = {
  enable = true;
  plugins.statusBar = false;
  plugins.tabBar = true;
};
```

## Building

Build both reference wasm plugins:

```sh
nix build .#status-bar .#tab-bar
```

Run checks:

```sh
nix flake check
```

Enter the wasm development shell and check Rust code:

```sh
nix develop
cargo check
```

## Integrating your own UI plugin

Depend on `zjai-core` and read protocol records directly:

```rust
let tab_statuses = zjai_core::read_tab_statuses(
    session_name,
    active_tab_position,
    &pane_tabs,
);
```

See `docs/ui-plugin-integration.md` for the reader interface.
