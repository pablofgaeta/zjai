//! Cross-session zjai summary for the bottom bar.
//!
//! This plugin only reads. It renders a line into its own pane and does
//! nothing else: it never renames a tab, never writes a status record, and
//! never parses its own output back in to recover state. Zellij runs one copy
//! of it per tab; each copy keeps local seen-state for the active tab only.
//!
//! Per-tab status is drawn by the sibling tab-bar plugin.

use std::collections::{BTreeMap, HashMap};

use zjai_core::{self as status, session_status, Record, Status};
use zellij_tile::prelude::*;

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
    pane_status: HashMap<u32, Record>,
    pane_tabs: HashMap<u32, usize>,
    animation_frame: usize,
    tick_count: u64,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        // Reading is all this plugin does. It no longer asks for
        // ChangeApplicationState (it renamed tabs) or ReadCliPipes (status
        // arrived over a pipe); both went away with the move to disk.
        request_permission(&[PermissionType::ReadApplicationState]);
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
        if let Some(session_name) = &current_session_name {
            self.pane_status = status::read_session_records(session_name);
            status::cleanup_seen(session_name, &self.pane_status);
            self.mark_active_tab_done_seen(session_name);
        }

        let next: BTreeMap<String, Status> = self
            .sessions
            .iter()
            .filter_map(|session| {
                if Some(&session.name) == current_session_name.as_ref() {
                    self.current_session_status(&session.name)
                        .map(|status| (session.name.clone(), status))
                } else {
                    session_status(&session.name).map(|status| (session.name.clone(), status))
                }
            })
            .collect();

        let changed = next != self.statuses;
        self.statuses = next;
        changed
    }

    fn mark_active_tab_done_seen(&self, session_name: &str) {
        let Some(active_tab_position) = self
            .tabs
            .iter()
            .find(|tab| tab.active)
            .map(|tab| tab.position)
        else {
            return;
        };

        for (&pane_id, &record) in &self.pane_status {
            if record.status == Status::Done
                && self.pane_tabs.get(&pane_id) == Some(&active_tab_position)
            {
                status::mark_done_seen(session_name, pane_id, record.written_at);
            }
        }
    }

    fn current_session_status(&self, session_name: &str) -> Option<Status> {
        self.pane_status
            .iter()
            .filter_map(|(&pane_id, &record)| {
                if !self.pane_tabs.contains_key(&pane_id) {
                    return None;
                }
                if status::is_done_seen(session_name, pane_id, record) {
                    Some(Status::Idle)
                } else {
                    Some(record.status)
                }
            })
            .reduce(status::merge)
    }

    fn summary(&self, cols: usize) -> String {
        let mut sessions = self.sessions.clone();
        sessions.sort_by(|left, right| {
            right
                .is_current_session
                .cmp(&left.is_current_session)
                .then_with(|| left.name.cmp(&right.name))
        });

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
            summary.push_str(&item);
        }
        summary
    }
}
