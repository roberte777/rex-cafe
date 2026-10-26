//! `--demo`: made-up Agents fed through the same pipeline as Rex, so the
//! café can be built, screenshotted, and tested without real agents.

use std::time::Duration;

use serde_json::json;
use tokio::sync::mpsc;

use crate::agents::{Fact, Record};
use crate::feed::{Command, FeedMsg};

/// How long one pass through the script takes before it repeats.
pub const LOOP_SECS: f64 = 64.0;

const SESSION: &str = "demo";

fn record(
    state: &str,
    kind: Option<&str>,
    title: &str,
    msg: Option<&str>,
    at: f64,
) -> Option<Record> {
    let mut r = Record::from_json(&json!({"state": state, "kind": kind, "app": "claude-code"}))?;
    r.title = Some(title.to_string());
    r.msg = msg.map(str::to_string);
    r.updated_at = Some(at);
    Some(r)
}

fn status(
    block: &str,
    state: &str,
    kind: Option<&str>,
    title: &str,
    msg: Option<&str>,
    at: f64,
) -> Fact {
    Fact::Status {
        session: SESSION.into(),
        block: format!("demo:{block}"),
        record: record(state, kind, title, msg, at),
    }
}

fn gone(block: &str) -> Fact {
    Fact::Status {
        session: SESSION.into(),
        block: format!("demo:{block}"),
        record: None,
    }
}

/// The opening scene and the timed events after it. `wall` is the Unix time
/// the script starts at, used for "N min" ages.
pub fn script(wall: f64) -> (Vec<Fact>, Vec<(f64, Fact)>) {
    const AUTH: &str = "Refactor auth middleware";
    const CI: &str = "Fix flaky CI tests";
    const IMG: &str = "Optimize image pipeline";
    const ONB: &str = "Onboarding flow";
    const IDX: &str = "Search indexing";
    let opening = vec![
        status("1", "idle", None, AUTH, None, wall - 130.0),
        status("2", "idle", None, CI, None, wall - 125.0),
        status("3", "idle", None, IMG, None, wall - 140.0),
        status("4", "idle", None, ONB, None, wall - 70.0),
    ];
    let at = |t: f64| wall + t;
    let events = vec![
        (2.0, status("5", "idle", None, IDX, None, at(2.0))),
        (7.0, status("5", "working", None, IDX, None, at(7.0))),
        (
            10.0,
            status(
                "5",
                "working",
                None,
                IDX,
                Some("Listing files in current directory"),
                at(10.0),
            ),
        ),
        (
            12.0,
            status(
                "2",
                "working",
                None,
                CI,
                Some("Running cargo test"),
                at(12.0),
            ),
        ),
        (
            19.0,
            status(
                "5",
                "blocked",
                Some("permission"),
                IDX,
                Some("approve Write: src/index.rs"),
                at(19.0),
            ),
        ),
        (
            25.0,
            status(
                "5",
                "working",
                None,
                IDX,
                Some("Writing src/index.rs"),
                at(25.0),
            ),
        ),
        (
            27.0,
            status(
                "3",
                "working",
                None,
                IMG,
                Some("Profiling resize()"),
                at(27.0),
            ),
        ),
        (30.0, status("2", "done", None, CI, None, at(30.0))),
        (
            35.0,
            status(
                "3",
                "error",
                None,
                IMG,
                Some("cargo build failed"),
                at(35.0),
            ),
        ),
        (38.0, status("5", "done", None, IDX, None, at(38.0))),
        (
            41.0,
            status(
                "4",
                "blocked",
                Some("question"),
                ONB,
                Some("Which locale should be the default?"),
                at(41.0),
            ),
        ),
        (47.0, status("4", "working", None, ONB, None, at(47.0))),
        (50.0, status("3", "idle", None, IMG, None, at(50.0))),
        (53.0, gone("5")),
        (57.0, status("2", "idle", None, CI, None, at(57.0))),
        (60.0, status("4", "idle", None, ONB, None, at(60.0))),
    ];
    (opening, events)
}

fn now_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Plays the script forever. Each pass after the first carries on from the
/// last; `r` starts over from the opening scene.
pub async fn run(tx: mpsc::UnboundedSender<FeedMsg>, mut cmds: mpsc::UnboundedReceiver<Command>) {
    let mut fresh = true;
    loop {
        let start = tokio::time::Instant::now();
        let (opening, events) = script(now_unix());
        if fresh {
            let _ = tx.send(FeedMsg::Reset(opening));
            let _ = tx.send(FeedMsg::Open);
        } else {
            for f in opening {
                let _ = tx.send(FeedMsg::Fact(f));
            }
        }
        let mut events = events.into_iter().peekable();
        fresh = loop {
            let due = events.peek().map(|(t, _)| *t).unwrap_or(LOOP_SECS);
            let wait = tokio::time::sleep_until(start + Duration::from_secs_f64(due));
            tokio::select! {
                _ = wait => match events.next() {
                    Some((_, f)) => { let _ = tx.send(FeedMsg::Fact(f)); }
                    None => break false,
                },
                cmd = cmds.recv() => match cmd {
                    None => return,
                    Some(Command::Resync) => break true,
                    Some(Command::Jump { .. }) => { let _ = tx.send(FeedMsg::Notice("demo mode: no pane to jump to".into())); }
                    Some(Command::Spawn) => { let _ = tx.send(FeedMsg::Notice("demo mode: agents can't be opened".into())); }
                },
            }
        };
    }
}
