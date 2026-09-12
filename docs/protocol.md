# Zellij Agent State Protocol v1

Zellij Agent State Protocol, or `zjai`, is a file-per-pane protocol for sharing current agent state with Zellij UI plugins.

## Goals

- Let agent integrations publish current state from shell hooks or extension code.
- Let any Zellij UI plugin read that state without depending on the reference status bar or tab bar.
- Keep the writer protocol small enough to implement without SQLite, IPC, or a long-running daemon.

## State location

Writers on the host write records under Zellij's temp directory:

```text
${TMPDIR:-/tmp}/zellij-<uid>/zjai/<session>/<pane_id>
```

Wasm plugins read the same records through Zellij's plugin sandbox:

```text
/tmp/zjai/<session>/<pane_id>
```

Zellij preopens `/host`, `/data`, `/cache`, and `/tmp` for wasm plugins. `$HOME` is not available inside the guest, so the protocol uses the temp directory that Zellij maps into guest `/tmp`.

## Pane identity

Writers MUST use the current Zellij pane and session environment:

```text
ZELLIJ_SESSION_NAME
ZELLIJ_PANE_ID
```

Writers MUST remove the `terminal_` prefix from `ZELLIJ_PANE_ID` when present. For example, `terminal_12` becomes `12`.

## Record format

Each pane record is a single UTF-8 line:

```text
<status> <epoch_seconds> <source> [metadata...]
```

Example:

```text
blocked 1736459512 pi reason=permission
```

Fields:

- `status`: one of `working`, `blocked`, `done`, `idle`, `unknown`, or `error`.
- `epoch_seconds`: Unix timestamp in seconds.
- `source`: short producer name, such as `pi`, `opencode`, or `gemini`.
- `metadata`: optional space-separated `key=value` fields. Readers MAY ignore metadata.

## Status semantics

- `working`: the agent is running or using tools.
- `blocked`: the agent needs user input, permission, or another external unblock.
- `done`: the agent completed work and has not yet been acknowledged by the UI.
- `idle`: no active agent state is present.
- `unknown`: the writer cannot classify the state.
- `error`: the last observed agent action failed.

A missing file means `idle`. Writers SHOULD remove the pane file instead of writing an `idle` record.

## Write rules

Writers MUST write atomically:

1. Create the session directory if needed.
2. Write the new record to a temporary file in the same directory.
3. Rename the temporary file over the target pane file.

Writers MUST remove the target pane file for `idle`.

Writers SHOULD write only when state changes.

## Read rules

Readers MUST tolerate missing, malformed, unreadable, and stale records.

Readers SHOULD treat missing records as `idle`.

Readers SHOULD treat records older than their stale threshold as `idle` or `unknown`. The reference reader uses 30 minutes.

Readers SHOULD poll no faster than 250 ms unless they have a stronger external signal.

## Why file-per-pane instead of SQLite

The protocol stores current state, not history. One pane owns one small record, so file-per-pane matches the data model directly.

File-per-pane keeps shell-hook writers simple: a writer only needs `mkdir`, `printf`, `mv`, and `rm`. SQLite would require a database binary or library, schema setup, lock timeout choices, and recovery behavior.

File-per-pane also avoids a shared database write lock. Two agents in different panes write different files, and readers do not coordinate with writers beyond normal filesystem semantics.

## Reference implementation

This repository includes:

- `zjai-core`: Rust protocol reader library for Zellij wasm plugins.
- `integrations/notify/zjai-notify.sh`: shell writer for hook-based agents.
- `status-bar`: reference cross-session UI plugin.
- `tab-bar`: reference per-tab UI plugin.
