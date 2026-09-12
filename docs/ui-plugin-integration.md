# Integrating zjai into a Zellij UI plugin

`zjai-core` is the protocol reader library for Zellij wasm UI plugins. Use it when you want agent state in your own status bar, tab bar, or custom plugin without using the reference `zjai-status` or `zjai-tab-bar` plugins.

Read `protocol.md` if you want to implement a writer. Use this file if you want to implement a reader.

## Cargo dependency

Inside this workspace:

```toml
[dependencies]
zjai-core = { path = "../zjai-core" }
```

From another project, depend on the published crate or a pinned git revision once one exists.

## Current session records

Use this when your plugin wants raw pane-level state:

```rust
let records = zjai_core::read_session_records(session_name);

for (pane_id, record) in records {
    let glyph = record.status.glyph(animation_frame);
    // Render pane_id, glyph, or record.status however your UI wants.
}
```

`read_session_records` skips missing, malformed, unreadable, and stale records.

## Per-tab status

Use this when your plugin can map terminal panes to tab positions:

```rust
let active_tab_position = tabs.iter().find(|tab| tab.active).map(|tab| tab.position);
let tab_statuses = zjai_core::read_tab_statuses(
    session_name,
    active_tab_position,
    &pane_tabs,
);

for (tab_position, status) in tab_statuses.tabs {
    let glyph = status.glyph(animation_frame);
    // Render glyph beside tab_position.
}
```

`pane_tabs` is a `HashMap<u32, usize>` mapping terminal pane id to Zellij tab position. UI plugins usually build it from `Event::PaneUpdate`.

`read_tab_statuses` also handles the reference "done has been seen" behavior: `Done` renders until the tab containing that pane becomes active, then the same done record renders as `Idle`.

## Cross-session status

Use this when your plugin wants a summary for a sibling session:

```rust
if let Some(status) = zjai_core::session_status(session_name) {
    let glyph = status.glyph(animation_frame);
    // Render one summary item for session_name.
}
```

`session_status` returns the most urgent status in the session. It cannot apply per-tab seen-state for sibling sessions because Zellij does not expose those panes to the current plugin instance.

## Status priority

When multiple panes map to one UI item, `zjai-core` picks the most urgent state:

```text
blocked > error > done > working > unknown > idle
```

Use `zjai_core::merge(left, right)` if your UI has a different grouping model.

## Polling

A UI plugin SHOULD poll no faster than 250 ms. The reference plugins poll current-session status every few timer ticks and session lists less often.

## Minimal interface

Most UI plugins need only these functions and types:

```rust
zjai_core::Status
zjai_core::Record
zjai_core::TabStatuses
zjai_core::read_session_records
zjai_core::read_tab_statuses
zjai_core::session_status
zjai_core::merge
```
