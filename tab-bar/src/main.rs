mod line;
mod tab;

use std::cmp::{max, min};
use std::collections::{BTreeMap, HashMap};
use std::convert::TryInto;

use tab::get_tab_to_focus;
use zellij_tile::prelude::*;

use crate::line::tab_line;
use crate::tab::tab_style;
use zjai_core as status;
use zjai_core::Status;

#[derive(Debug, Default)]
pub struct LinePart {
    part: String,
    len: usize,
    tab_index: Option<usize>,
}

impl LinePart {
    pub fn append(&mut self, to_append: &LinePart) {
        self.part.push_str(&to_append.part);
        self.len += to_append.len;
    }
}

/// Drives the spinner. Also the resolution of every other timed behaviour
/// here, because zellij's `Event::Timer` carries no identity: two timers in
/// flight cannot be told apart, so this plugin arms exactly one and derives
/// the rest from a tick count.
const ANIMATION_INTERVAL_SECS: f64 = 0.1;
const STATUS_POLL_EVERY_TICKS: u64 = 3;
/// Formerly a `set_timeout(5.0)` of its own, now expressed against the
/// heartbeat so it cannot be consumed by an animation tick.
const HINT_TIMEOUT_TICKS: u64 = 50;

#[derive(Default, Debug)]
struct State {
    tabs: Vec<TabInfo>,
    active_tab_idx: usize,
    mode_info: ModeInfo,
    tab_line: Vec<LinePart>,
    hide_swap_layout_indication: bool,
    cached_keybinds: KeybindsVec,
    active_pane_scroll: Option<(usize, usize)>,
    new_tab_button_range: Option<(usize, usize)>,
    breadcrumb_range: Option<(usize, usize)>,
    hovered_tab_idx: Option<usize>,
    hovered_new_tab_button: bool,
    hint_text: Option<BTreeMap<usize, StyledText>>,
    /// Zjai per tab position, folded from the per-pane records on
    /// disk after locally seen `Done` records are treated as `Idle`.
    zjai_status: HashMap<usize, Status>,
    /// Raw zjai per terminal pane id, read from disk.
    pane_status: HashMap<u32, status::Record>,
    /// Terminal pane id -> tab position, needed to attribute a record to a tab.
    pane_tabs: HashMap<u32, usize>,
    animation_frame: usize,
    tick_count: u64,
    hint_expires_at_tick: Option<u64>,
}

static ARROW_SEPARATOR: &str = "";

register_plugin!(State);

impl State {
    /// Refreshes protocol records and folds them into a status per tab.
    ///
    /// Returns whether the result changed, so a settled bar does not repaint
    /// on every poll.
    fn refresh_zjai_status(&mut self) -> bool {
        let Some(session_name) = self.mode_info.session_name.as_deref() else {
            return false;
        };

        let previous_status = self.zjai_status.clone();
        let active_tab_position = self
            .tabs
            .iter()
            .find(|tab| tab.active)
            .map(|tab| tab.position);
        let status::TabStatuses { records, tabs } =
            status::read_tab_statuses(session_name, active_tab_position, &self.pane_tabs);
        self.pane_status = records;
        self.zjai_status = tabs;

        previous_status != self.zjai_status
    }

    fn has_animated_status(&self) -> bool {
        self.zjai_status.values().any(|status| status.is_animated())
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        // Built-in plugins get permissions implicitly; external wasm plugins
        // must request them. ReadApplicationState is required for all the
        // layout/tab/pane events below; ChangeApplicationState is required
        // for mouse-click actions (switch_tab_to, new_tab).
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        self.hide_swap_layout_indication = configuration
            .get("hide_swap_layout_indication")
            .map(|s| s == "true")
            .unwrap_or(false);
        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::ModeUpdate,
            EventType::Mouse,
            EventType::InitialKeybinds,
            EventType::ActivePaneScroll,
            EventType::HintText,
            EventType::Timer,
            EventType::InputReceived,
        ]);
        // Bootstraps the heartbeat, which re-arms itself from its own handler.
        set_timeout(ANIMATION_INTERVAL_SECS);
    }

    fn update(&mut self, event: Event) -> bool {
        let mut should_render = false;
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                set_selectable(false);
                should_render = true;
            }
            Event::PermissionRequestResult(_) => {
                set_selectable(false);
                should_render = true;
            }
            Event::InitialKeybinds(keybinds) => {
                self.cached_keybinds = keybinds;
                if !self.cached_keybinds.is_empty() {
                    self.mode_info.keybinds = self.cached_keybinds.clone();
                }
                should_render = true;
            }
            Event::ModeUpdate(mut mode_info) => {
                if mode_info.keybinds.is_empty() && !self.cached_keybinds.is_empty() {
                    mode_info.keybinds = self.cached_keybinds.clone();
                } else if !mode_info.keybinds.is_empty() {
                    self.cached_keybinds = mode_info.keybinds.clone();
                }
                if self.mode_info != mode_info {
                    should_render = true;
                }
                self.mode_info = mode_info;
            }
            Event::TabUpdate(tabs) => {
                if let Some(active_tab_index) = tabs.iter().position(|t| t.active) {
                    // tabs are indexed starting from 1 so we need to add 1
                    let active_tab_idx = active_tab_index + 1;

                    if self.active_tab_idx != active_tab_idx || self.tabs != tabs {
                        should_render = true;
                    }
                    self.active_tab_idx = active_tab_idx;
                    self.tabs = tabs;
                    if self.refresh_zjai_status() {
                        should_render = true;
                    }
                } else {
                    eprintln!("Could not find active tab.");
                }
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
                if self.refresh_zjai_status() {
                    should_render = true;
                }
            }
            Event::ActivePaneScroll(scroll) => {
                if self.active_pane_scroll != scroll {
                    should_render = true;
                }
                self.active_pane_scroll = scroll;
            }
            Event::HintText(hint_variants) => {
                if hint_variants.is_empty() {
                    if self.hint_text.is_some() {
                        self.hint_text = None;
                        self.hint_expires_at_tick = None;
                        should_render = true;
                    }
                } else {
                    self.hint_text = Some(hint_variants);
                    self.hint_expires_at_tick = Some(self.tick_count + HINT_TIMEOUT_TICKS);
                    should_render = true;
                }
            }
            Event::Timer(_) => {
                self.tick_count = self.tick_count.wrapping_add(1);
                self.animation_frame = self.animation_frame.wrapping_add(1);

                if let Some(expires_at) = self.hint_expires_at_tick {
                    if self.tick_count >= expires_at {
                        self.hint_expires_at_tick = None;
                        if self.hint_text.is_some() {
                            self.hint_text = None;
                            should_render = true;
                        }
                    }
                }

                if self.tick_count % STATUS_POLL_EVERY_TICKS == 0 && self.refresh_zjai_status() {
                    should_render = true;
                }

                // Only keep repainting while something is actually spinning;
                // a settled bar costs one directory read every few ticks.
                if self.has_animated_status() {
                    should_render = true;
                }

                set_timeout(ANIMATION_INTERVAL_SECS);
            }
            Event::InputReceived => {
                if self.hint_text.is_some() {
                    self.hint_text = None;
                    should_render = true;
                }
            }
            Event::Mouse(me) => match me {
                Mouse::LeftClick(_, col) => {
                    if let Some((start, end)) = self.breadcrumb_range {
                        if col >= start && col < end {
                            focus_host_session();
                            return should_render;
                        }
                    }
                    if let Some((start, end)) = self.new_tab_button_range {
                        if col >= start && col < end {
                            new_tab::<&str>(None, None);
                            return should_render;
                        }
                    }
                    let tab_to_focus = get_tab_to_focus(&self.tab_line, self.active_tab_idx, col);
                    if let Some(idx) = tab_to_focus {
                        switch_tab_to(idx.try_into().unwrap());
                    }
                }
                Mouse::Hover(_, col) => {
                    let simplified_ui = self.mode_info.capabilities.arrow_fonts;
                    let mut new_hovered_new_tab_button = false;
                    let mut new_hovered_tab_idx = None;
                    if !simplified_ui {
                        if let Some((start, end)) = self.new_tab_button_range {
                            if col >= start && col < end {
                                new_hovered_new_tab_button = true;
                            }
                        }
                        if !new_hovered_new_tab_button {
                            new_hovered_tab_idx =
                                get_tab_to_focus(&self.tab_line, self.active_tab_idx, col);
                        }
                    }
                    if self.hovered_new_tab_button != new_hovered_new_tab_button
                        || self.hovered_tab_idx != new_hovered_tab_idx
                    {
                        self.hovered_new_tab_button = new_hovered_new_tab_button;
                        self.hovered_tab_idx = new_hovered_tab_idx;
                        should_render = true;
                    }
                }
                Mouse::ScrollUp(_) => {
                    switch_tab_to(min(self.active_tab_idx + 1, self.tabs.len()) as u32);
                }
                Mouse::ScrollDown(_) => {
                    switch_tab_to(max(self.active_tab_idx.saturating_sub(1), 1) as u32);
                }
                _ => {}
            },
            _ => {
                eprintln!("Got unrecognized event: {:?}", event);
            }
        }
        if self.tabs.is_empty() {
            // no need to render if we have no tabs, this can sometimes happen on startup before we
            // get the tab update and then we definitely don't want to render
            should_render = false;
        }
        should_render
    }

    fn render(&mut self, _rows: usize, cols: usize) {
        if self.tabs.is_empty() {
            return;
        }
        let dimmed = self.mode_info.session_ascended == Some(true)
            || self.mode_info.session_dimmed == Some(true);
        let mut all_tabs: Vec<LinePart> = vec![];
        let mut active_tab_index = 0;
        let mut is_alternate_tab = false;
        for t in &self.tabs {
            let mut tabname = t.name.clone();
            let mut is_renaming = false;
            if t.active && self.mode_info.mode == InputMode::RenameTab {
                if tabname.is_empty() {
                    tabname = String::from("Enter name...");
                }
                active_tab_index = t.position;
                is_renaming = true;
            } else if t.active {
                active_tab_index = t.position;
            }
            // The glyph is applied to this local copy only. zellij's stored
            // tab name is never touched, so nothing has to be written back,
            // reconciled against a manual rename, or cleaned up on exit.
            // Prepending it here also means the fitting logic in line.rs
            // measures the real width without needing to know about it.
            if !is_renaming {
                if let Some(status) = self.zjai_status.get(&t.position) {
                    tabname = format!("{} {}", status.glyph(self.animation_frame), tabname);
                }
            }
            let is_hovered = self.hovered_tab_idx == Some(t.position + 1);
            let tab = tab_style(
                tabname,
                t,
                is_alternate_tab,
                is_hovered,
                self.mode_info.style.colors,
                self.mode_info.capabilities,
                dimmed,
            );
            is_alternate_tab = !is_alternate_tab;
            all_tabs.push(tab);
        }

        let background = self.mode_info.style.colors.text_unselected.background;

        let full_pane_frames = self.mode_info.pane_frame_style == Some(PaneFrameStyle::Full);
        let hint_text = if full_pane_frames {
            None
        } else {
            self.hint_text.as_ref()
        };
        let breadcrumb_ancestry: Vec<String> = if self.mode_info.host_fullscreen == Some(true) {
            self.mode_info.session_ancestry.clone()
        } else {
            vec![]
        };
        let (line, new_tab_button_range, breadcrumb_range) = tab_line(
            self.mode_info.session_name.as_deref(),
            all_tabs,
            active_tab_index,
            cols.saturating_sub(1),
            self.mode_info.style.colors,
            self.mode_info.capabilities,
            self.mode_info.style.hide_session_name,
            self.tabs.iter().find(|t| t.active),
            &self.mode_info,
            self.hide_swap_layout_indication,
            &background,
            self.active_pane_scroll,
            hint_text,
            is_alternate_tab,
            self.hovered_new_tab_button,
            dimmed,
            &breadcrumb_ancestry,
        );
        self.tab_line = line;
        self.new_tab_button_range = new_tab_button_range;
        self.breadcrumb_range = breadcrumb_range;

        let output = self
            .tab_line
            .iter()
            .fold(String::new(), |output, part| output + &part.part);

        match background {
            PaletteColor::Rgb((r, g, b)) => {
                print!("{}\u{1b}[48;2;{};{};{}m\u{1b}[0K", output, r, g, b);
            }
            PaletteColor::EightBit(color) => {
                print!("{}\u{1b}[48;5;{}m\u{1b}[0K", output, color);
            }
        }
    }
}
