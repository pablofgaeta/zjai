//! Reader library for Zellij Agent State Protocol v1.
//!
//! The protocol is documented in `docs/protocol.md`. `zjai-core` is the small
//! interface UI plugins depend on: it reads pane records, parses statuses,
//! applies stale-record handling, and exposes merge/glyph helpers.
//!
//! Records are written by protocol producers such as `zjai-notify`. Zellij
//! wasm plugins read them from `/tmp/zjai/<session>/<pane_id>`, which maps to
//! `${TMPDIR:-/tmp}/zellij-<uid>/zjai/<session>/<pane_id>` on the host.
//!
//! Third-party UI plugins usually start with [`read_session_records`],
//! [`read_tab_statuses`], or [`session_status`].

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WORKING_FRAMES: &[&str] = &["✢", "✢", "✻", "✻", "✽", "✽", "✶", "✶", "✽", "✽", "✻", "✻"];
pub const BLOCKED_FRAMES: &[&str] = &["■", "■", "■", "□", "□", "□"];

const STATUS_ROOT: &str = "/tmp/zjai";
const SEEN_DIR: &str = ".seen";

/// Backstop for an agent that died without clearing its record. Deliberately
/// generous: a single long tool call legitimately holds `Working` for a long
/// time without the producing hook firing again, so this exists to recover
/// from a kill, never to infer liveness.
const STALE_AFTER_SECS: u64 = 30 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Working,
    Blocked,
    Done,
    Idle,
    Unknown,
    Error,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record {
    pub status: Status,
    pub written_at: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TabStatuses {
    /// Raw protocol records keyed by terminal pane id.
    pub records: HashMap<u32, Record>,
    /// Renderable status keyed by Zellij tab position.
    pub tabs: HashMap<usize, Status>,
}

impl Status {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "working" => Some(Status::Working),
            "blocked" => Some(Status::Blocked),
            "done" => Some(Status::Done),
            "idle" => Some(Status::Idle),
            "unknown" => Some(Status::Unknown),
            "error" => Some(Status::Error),
            _ => Some(Status::Unknown),
        }
    }

    pub fn glyph(self, animation_frame: usize) -> &'static str {
        match self {
            Status::Working => WORKING_FRAMES[animation_frame % WORKING_FRAMES.len()],
            Status::Blocked => BLOCKED_FRAMES[animation_frame % BLOCKED_FRAMES.len()],
            Status::Done => "●",
            Status::Idle => "○",
            Status::Unknown => "?",
            Status::Error => "✗",
        }
    }

    pub fn is_animated(self) -> bool {
        matches!(self, Status::Working | Status::Blocked)
    }

    fn priority(self) -> u8 {
        match self {
            Status::Idle => 0,
            Status::Unknown => 1,
            Status::Working => 2,
            Status::Done => 3,
            Status::Error => 4,
            Status::Blocked => 5,
        }
    }
}

/// The most urgent of two statuses, for a tab holding several agent panes.
pub fn merge(left: Status, right: Status) -> Status {
    if right.priority() > left.priority() {
        right
    } else {
        left
    }
}

fn now_secs() -> Option<u64> {
    // Guarded rather than unwrapped: if the sandbox declines to provide a
    // clock, expiry switches off instead of taking the plugin down.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs())
}

fn parse_record(contents: &str, now: Option<u64>) -> Option<Record> {
    let mut fields = contents.split_whitespace();
    let status = Status::parse(fields.next()?)?;
    let written_at = fields.next().and_then(|field| field.parse::<u64>().ok());

    if let (Some(now), Some(written_at)) = (now, written_at) {
        if now.saturating_sub(written_at) > STALE_AFTER_SECS {
            return None;
        }
    }

    Some(Record { status, written_at })
}

fn session_dir(session_name: &str) -> PathBuf {
    [STATUS_ROOT, session_name].iter().collect()
}

fn seen_dir(session_name: &str) -> PathBuf {
    session_dir(session_name).join(SEEN_DIR)
}

fn seen_path(session_name: &str, pane_id: u32) -> PathBuf {
    seen_dir(session_name).join(pane_id.to_string())
}

/// Status records for one session, keyed by terminal pane id.
///
/// Unreadable records are skipped rather than surfaced: this runs on a timer,
/// and a transient read failure should read as "no status" rather than as an
/// error state. Malformed status values are parsed as `Unknown`.
pub fn read_session_records(session_name: &str) -> HashMap<u32, Record> {
    let mut records = HashMap::new();
    let now = now_secs();

    let Ok(entries) = fs::read_dir(session_dir(session_name)) else {
        return records;
    };

    for entry in entries.flatten() {
        let Some(pane_id) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue; // skips the writer's temp files and metadata dirs, e.g. "12.4567" or ".seen"
        };
        let Ok(contents) = fs::read_to_string(entry.path()) else {
            continue;
        };
        if let Some(record) = parse_record(&contents, now) {
            records.insert(pane_id, record);
        }
    }
    records
}

/// Statuses for one session, keyed by terminal pane id.
pub fn read_session(session_name: &str) -> HashMap<u32, Status> {
    read_session_records(session_name)
        .into_iter()
        .map(|(pane_id, record)| (pane_id, record.status))
        .collect()
}

/// Records that the current done status for a pane has been seen.
pub fn mark_done_seen(session_name: &str, pane_id: u32, written_at: Option<u64>) {
    let dir = seen_dir(session_name);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let target = seen_path(session_name, pane_id);
    let tmp = dir.join(format!("{}.tmp", pane_id));
    let contents = written_at.map(|t| t.to_string()).unwrap_or_default();
    if fs::write(&tmp, contents).is_ok() {
        let _ = fs::rename(tmp, target);
    }
}

/// Returns whether this exact done record has been seen.
pub fn is_done_seen(session_name: &str, pane_id: u32, record: Record) -> bool {
    if record.status != Status::Done {
        return false;
    }
    let Ok(contents) = fs::read_to_string(seen_path(session_name, pane_id)) else {
        return false;
    };
    match record.written_at {
        Some(written_at) => contents.trim().parse::<u64>().ok() == Some(written_at),
        None => true,
    }
}

/// Removes stale seen markers for panes that are no longer done.
pub fn cleanup_seen(session_name: &str, records: &HashMap<u32, Record>) {
    let Ok(entries) = fs::read_dir(seen_dir(session_name)) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(pane_id) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(record) = records.get(&pane_id) else {
            let _ = fs::remove_file(entry.path());
            continue;
        };
        if !is_done_seen(session_name, pane_id, *record) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Reads records and folds them into renderable tab statuses.
///
/// A `Done` record in the active tab is marked as seen before folding, so it
/// renders once and then drops back to `Idle`. Records for unknown panes are
/// ignored because only the UI plugin can map panes to tab positions.
pub fn read_tab_statuses(
    session_name: &str,
    active_tab_position: Option<usize>,
    pane_tabs: &HashMap<u32, usize>,
) -> TabStatuses {
    let records = read_session_records(session_name);
    cleanup_seen(session_name, &records);
    if let Some(active_tab_position) = active_tab_position {
        mark_tab_done_seen(session_name, active_tab_position, &records, pane_tabs);
    }
    let tabs = fold_tab_statuses(session_name, &records, pane_tabs);
    TabStatuses { records, tabs }
}

/// Records that all currently done panes in a tab have been seen.
pub fn mark_tab_done_seen(
    session_name: &str,
    tab_position: usize,
    records: &HashMap<u32, Record>,
    pane_tabs: &HashMap<u32, usize>,
) {
    for (&pane_id, &record) in records {
        if record.status == Status::Done && pane_tabs.get(&pane_id) == Some(&tab_position) {
            mark_done_seen(session_name, pane_id, record.written_at);
        }
    }
}

/// Folds per-pane records into the most urgent renderable status per tab.
pub fn fold_tab_statuses(
    session_name: &str,
    records: &HashMap<u32, Record>,
    pane_tabs: &HashMap<u32, usize>,
) -> HashMap<usize, Status> {
    let mut folded: HashMap<usize, Status> = HashMap::new();
    for (&pane_id, &record) in records {
        let Some(&tab_position) = pane_tabs.get(&pane_id) else {
            continue;
        };
        let rendered_status = if is_done_seen(session_name, pane_id, record) {
            Status::Idle
        } else {
            record.status
        };
        folded
            .entry(tab_position)
            .and_modify(|existing| *existing = merge(*existing, rendered_status))
            .or_insert(rendered_status);
    }
    folded
}

/// The most urgent renderable status recorded anywhere in a session, if any.
///
/// Used for the cross-session summary, where per-pane detail is not wanted
/// and sibling sessions' panes are not knowable from here anyway. Seen `Done`
/// records render as `Idle`, so a session does not jump back to done after the
/// user switches away from it.
pub fn session_status(session_name: &str) -> Option<Status> {
    read_session_records(session_name)
        .into_iter()
        .map(|(pane_id, record)| {
            if is_done_seen(session_name, pane_id, record) {
                Status::Idle
            } else {
                record.status
            }
        })
        .reduce(merge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_well_formed_record() {
        assert_eq!(
            parse_record("working 1000 jetski\n", Some(1000)),
            Some(Record {
                status: Status::Working,
                written_at: Some(1000)
            })
        );
    }

    #[test]
    fn parses_idle_unknown_and_unrecognized_statuses() {
        assert_eq!(
            parse_record("idle 1000 jetski", Some(1000)).map(|record| record.status),
            Some(Status::Idle)
        );
        assert_eq!(
            parse_record("unknown 1000 jetski", Some(1000)).map(|record| record.status),
            Some(Status::Unknown)
        );
        assert_eq!(
            parse_record("banana 1000 jetski", Some(1000)).map(|record| record.status),
            Some(Status::Unknown)
        );
        assert_eq!(parse_record("", Some(1000)), None);
    }

    #[test]
    fn expires_records_past_the_backstop() {
        let written = 1_000_000;
        let record = format!("working {} jetski", written);

        assert_eq!(
            parse_record(&record, Some(written + STALE_AFTER_SECS)).map(|record| record.status),
            Some(Status::Working)
        );
        assert_eq!(
            parse_record(&record, Some(written + STALE_AFTER_SECS + 1)),
            None
        );
    }

    #[test]
    fn keeps_records_when_no_clock_is_available() {
        assert_eq!(
            parse_record("working 1 jetski", None).map(|record| record.status),
            Some(Status::Working)
        );
    }

    #[test]
    fn tolerates_a_missing_timestamp() {
        assert_eq!(
            parse_record("done", Some(1000)).map(|record| record.status),
            Some(Status::Done)
        );
    }

    #[test]
    fn merge_prefers_the_most_urgent() {
        assert_eq!(merge(Status::Done, Status::Working), Status::Done);
        assert_eq!(merge(Status::Working, Status::Done), Status::Done);
        assert_eq!(merge(Status::Working, Status::Error), Status::Error);
        assert_eq!(merge(Status::Blocked, Status::Error), Status::Blocked);
        assert_eq!(merge(Status::Unknown, Status::Idle), Status::Unknown);
    }

    fn test_session(name: &str) -> String {
        format!("zjai-test-{}-{}", name, std::process::id())
    }

    fn write_test_record(session_name: &str, pane_id: u32, contents: &str) {
        let dir = session_dir(session_name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(pane_id.to_string()), contents).unwrap();
    }

    #[test]
    fn folds_pane_records_by_tab_and_urgency() {
        let session = test_session("folds");
        let mut records = HashMap::new();
        records.insert(
            1,
            Record {
                status: Status::Working,
                written_at: Some(1000),
            },
        );
        records.insert(
            2,
            Record {
                status: Status::Blocked,
                written_at: Some(1000),
            },
        );
        records.insert(
            3,
            Record {
                status: Status::Error,
                written_at: Some(1000),
            },
        );
        let pane_tabs = HashMap::from([(1, 0), (2, 0), (3, 1)]);

        assert_eq!(
            fold_tab_statuses(&session, &records, &pane_tabs),
            HashMap::from([(0, Status::Blocked), (1, Status::Error)])
        );
    }

    #[test]
    fn marks_active_tab_done_records_as_seen_before_folding() {
        let session = test_session("done-seen");
        let _ = fs::remove_dir_all(session_dir(&session));
        let records = HashMap::from([
            (
                1,
                Record {
                    status: Status::Done,
                    written_at: Some(1000),
                },
            ),
            (
                2,
                Record {
                    status: Status::Done,
                    written_at: Some(2000),
                },
            ),
        ]);
        let pane_tabs = HashMap::from([(1, 0), (2, 1)]);

        mark_tab_done_seen(&session, 0, &records, &pane_tabs);
        let tabs = fold_tab_statuses(&session, &records, &pane_tabs);

        assert_eq!(tabs.get(&0), Some(&Status::Idle));
        assert_eq!(tabs.get(&1), Some(&Status::Done));
        let _ = fs::remove_dir_all(session_dir(&session));
    }

    #[test]
    fn does_not_mark_done_seen_when_session_is_not_viewed() {
        // A session whose plugins are running but that no client is attached
        // to passes `None` for the active tab: nothing is being viewed, so a
        // `Done` record must survive as `Done` rather than being marked seen.
        let session = test_session("no-view-no-mark");
        let _ = fs::remove_dir_all(session_dir(&session));
        let written_at = now_secs().unwrap_or(1000);
        write_test_record(&session, 1, &format!("done {written_at} cloudcode"));
        let pane_tabs = HashMap::from([(1u32, 0usize)]);

        let TabStatuses { tabs, .. } = read_tab_statuses(&session, None, &pane_tabs);
        assert_eq!(tabs.get(&0), Some(&Status::Done));

        // Once the session is actually viewed (Some active tab), the same
        // record is marked seen and folds to Idle.
        let TabStatuses { tabs, .. } = read_tab_statuses(&session, Some(0), &pane_tabs);
        assert_eq!(tabs.get(&0), Some(&Status::Idle));
        let _ = fs::remove_dir_all(session_dir(&session));
    }

    #[test]
    fn session_status_renders_seen_done_records_as_idle() {
        let session = test_session("session-seen-done");
        let _ = fs::remove_dir_all(session_dir(&session));
        let written_at = now_secs().unwrap_or(1000);
        write_test_record(&session, 1, &format!("done {written_at} pi"));

        assert_eq!(session_status(&session), Some(Status::Done));

        mark_done_seen(&session, 1, Some(written_at));

        assert_eq!(session_status(&session), Some(Status::Idle));
        let _ = fs::remove_dir_all(session_dir(&session));
    }
}
