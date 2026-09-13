//! Cross-session zjai summary for the bottom bar.
//!
//! This plugin never renames a tab, never writes a status record, and never
//! parses its own output back in to recover state. Zellij runs one copy of it
//! per tab; each copy keeps local seen-state for the active tab only. The one
//! state change it does make is switching sessions on click, which goes
//! through Zellij's own `switch_session` host call rather than anything this
//! plugin persists itself.
//!
//! Per-tab status is drawn by the sibling tab-bar plugin.

use std::collections::{BTreeMap, HashMap};

use zellij_tile::prelude::*;
use zjai_core::{self as status, session_status, Status};

/// Drives the spinner, and by extension every other timed behaviour here:
/// `Event::Timer` carries no identity, so two timers in flight cannot be told
/// apart. One timer, and a tick count for everything else.
const ANIMATION_INTERVAL_SECS: f64 = 0.1;
const STATUS_POLL_EVERY_TICKS: u64 = 3;
/// Zellij only rescans sibling sessions when a plugin calls
/// `get_session_list()`, and nothing does that on its own, so poll it here.
const SESSION_POLL_EVERY_TICKS: u64 = 20;

/// A session with no records at all. Absence of a record and explicit `Idle`
/// records both render as idle.
const IDLE_GLYPH: &str = "○";

#[derive(Default)]
struct State {
    sessions: Vec<SessionInfo>,
    /// Session name -> most urgent status in that session.
    statuses: BTreeMap<String, Status>,
    permissions_granted: bool,
    tabs: Vec<TabInfo>,
    pane_status: HashMap<u32, status::Record>,
    pane_tabs: HashMap<u32, usize>,
    animation_frame: usize,
    tick_count: u64,
    /// Column ranges (start, end) of each rendered session name, in the same
    /// order as the last `summary()` call, so a click column can be resolved
    /// back to a session to switch to.
    session_ranges: Vec<(usize, usize, String)>,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        // ReadCliPipes went away with the move to status-on-disk.
        // ChangeApplicationState is requested for one thing: switching
        // sessions when a rendered session name is clicked.
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        subscribe(&[EventType::PermissionRequestResult, EventType::Timer]);
        // Bootstraps the heartbeat, which re-arms itself from its own handler.
        set_timeout(ANIMATION_INTERVAL_SECS);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                set_selectable(false);
                self.permissions_granted = true;
                subscribe(&[
                    EventType::SessionUpdate,
                    EventType::TabUpdate,
                    EventType::PaneUpdate,
                    EventType::Timer,
                    EventType::Mouse,
                ]);
                self.poll_sessions();
                true
            }
            Event::PermissionRequestResult(_) => {
                set_selectable(false);
                true
            }
            Event::SessionUpdate(sessions, _) => {
                self.sessions = sessions;
                self.refresh_statuses();
                true
            }
            Event::TabUpdate(tabs) => {
                self.tabs = tabs;
                self.refresh_statuses();
                true
            }
            Event::PaneUpdate(manifest) => {
                self.pane_tabs.clear();
                for (tab_position, panes) in manifest.panes {
                    for pane in panes {
                        if !pane.is_plugin {
                            self.pane_tabs.insert(pane.id, tab_position);
                        }
                    }
                }
                self.refresh_statuses();
                true
            }
            Event::Timer(_) => self.tick(),
            Event::Mouse(Mouse::LeftClick(_, col)) => self.click(col),
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        if self.permissions_granted {
            print!("{}", self.summary(cols));
        } else {
            print!("zjai: waiting for permission");
        }
    }
}

impl State {
    fn tick(&mut self) -> bool {
        self.tick_count = self.tick_count.wrapping_add(1);
        self.animation_frame = self.animation_frame.wrapping_add(1);
        let mut should_render = false;

        if self.tick_count % STATUS_POLL_EVERY_TICKS == 0 && self.refresh_statuses() {
            should_render = true;
        }
        if self.tick_count % SESSION_POLL_EVERY_TICKS == 0 {
            self.poll_sessions();
            should_render = true;
        }
        // Only keep repainting while something is spinning; an idle bar costs
        // one directory read every few ticks.
        if self.statuses.values().any(|status| status.is_animated()) {
            should_render = true;
        }

        set_timeout(ANIMATION_INTERVAL_SECS);
        should_render
    }

    fn poll_sessions(&mut self) {
        if let Ok(snapshot) = get_session_list() {
            self.sessions = snapshot.live_sessions;
        }
    }

    /// Returns whether anything changed, so a settled bar does not repaint.
    fn refresh_statuses(&mut self) -> bool {
        let current_session_name = self
            .sessions
            .iter()
            .find(|session| session.is_current_session)
            .map(|session| session.name.clone());
        let current_status = current_session_name.as_ref().and_then(|session_name| {
            let active_tab_position = self
                .tabs
                .iter()
                .find(|tab| tab.active)
                .map(|tab| tab.position);
            let status::TabStatuses { records, tabs } =
                status::read_tab_statuses(session_name, active_tab_position, &self.pane_tabs);
            self.pane_status = records;
            tabs.into_values().reduce(status::merge)
        });

        let next: BTreeMap<String, Status> = self
            .sessions
            .iter()
            .filter_map(|session| {
                if Some(&session.name) == current_session_name.as_ref() {
                    current_status.map(|status| (session.name.clone(), status))
                } else {
                    session_status(&session.name).map(|status| (session.name.clone(), status))
                }
            })
            .collect();

        let changed = next != self.statuses;
        self.statuses = next;
        changed
    }

    /// Resolves a click column against the ranges recorded by the last
    /// `summary()` call and switches to the session under it, mirroring how
    /// the sibling tab-bar plugin maps a click column to a tab.
    fn click(&mut self, col: usize) -> bool {
        if let Some((_, _, name)) = self
            .session_ranges
            .iter()
            .find(|(start, end, _)| col >= *start && col < *end)
        {
            switch_session(Some(name));
        }
        false
    }

    fn summary(&mut self, cols: usize) -> String {
        // Sorted by name only, so clicking a session to switch to it doesn't
        // reorder the list out from under a repeated click.
        let mut sessions = self.sessions.clone();
        sessions.sort_by(|left, right| left.name.cmp(&right.name));

        self.session_ranges.clear();
        let mut summary = String::from("sessions:");
        for session in &sessions {
            let glyph = self
                .statuses
                .get(&session.name)
                .map(|status| status.glyph(self.animation_frame))
                .unwrap_or(IDLE_GLYPH);

            let item = format!(" {} {}", glyph, session.name);
            if summary.chars().count() + item.chars().count() > cols {
                break;
            }
            let start = summary.chars().count();
            summary.push_str(&item);
            let end = summary.chars().count();
            self.session_ranges.push((start, end, session.name.clone()));
        }
        summary
    }
}
