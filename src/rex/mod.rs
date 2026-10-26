//! Rex as the Café's only source of truth (ADR 0002), over one WebSocket (ADR 0001).

pub mod client;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures_util::future::join_all;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::agents::{Fact, Record};
use crate::feed::{Command, FeedMsg};
use client::{Connection, Event};

const TERMINAL: &str = "com.superlogical.terminal";

/// What the café knows about where it is running.
#[derive(Clone, Debug)]
pub struct Context {
    pub socket: PathBuf,
    /// The café's own session and pane, from `REX_SESSION` / `REX_BLOCK`.
    pub session: Option<String>,
    pub block: Option<String>,
    pub cwd: String,
    pub spawn_cmd: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Status,
    Title,
    Pwd,
    Cwd,
}

/// Which snapshot field a fact updates, for skipping events a snapshot already includes.
fn fact_key(f: &Fact) -> Option<(String, Kind)> {
    match f {
        Fact::Status { block, .. } => Some((block.clone(), Kind::Status)),
        Fact::Title { block, .. } => Some((block.clone(), Kind::Title)),
        Fact::Pwd { block, .. } => Some((block.clone(), Kind::Pwd)),
        Fact::Cwd { block, .. } => Some((block.clone(), Kind::Cwd)),
        Fact::BlockClosed { .. } | Fact::SessionGone { .. } => None,
    }
}

/// Reads the facts an event carries, if it carries any the Café cares about.
pub fn fact_from_event(ev: &Value) -> Option<Fact> {
    match ev["type"].as_str()? {
        "session_destroyed" => Some(Fact::SessionGone {
            session: ev["payload"]["session_id"].as_str()?.to_string(),
        }),
        "session_event" => {
            let session = ev["payload"]["session_id"].as_str()?.to_string();
            let inner = &ev["payload"]["event"];
            match inner["type"].as_str()? {
                "block_closed" => Some(Fact::BlockClosed {
                    block: inner["payload"]["block_id"].as_str()?.to_string(),
                }),
                "block_event" => {
                    let p = &inner["payload"];
                    let block = p["block_id"].as_str()?.to_string();
                    let body = &p["payload"];
                    match p["name"].as_str()? {
                        "program_status_changed"
                            if body["id"].as_str().unwrap_or("").is_empty() =>
                        {
                            Some(Fact::Status {
                                session,
                                block,
                                record: Record::from_json(&body["record"]),
                            })
                        }
                        "program_status_removed" => {
                            let ids = body["ids"].as_array()?;
                            ids.iter()
                                .any(|id| id.as_str() == Some(""))
                                .then_some(Fact::Status {
                                    session,
                                    block,
                                    record: None,
                                })
                        }
                        "title_changed" => Some(Fact::Title {
                            session,
                            block,
                            title: body["title"].as_str().unwrap_or("").to_string(),
                        }),
                        "pwd_changed" => Some(Fact::Pwd {
                            session,
                            block,
                            pwd: body["pwd"].as_str()?.to_string(),
                        }),
                        "process_changed" => Some(Fact::Cwd {
                            session,
                            block,
                            cwd: body["foreground"]["cwd"].as_str()?.to_string(),
                        }),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Reads every terminal's status, title, and directory, then folds in events
/// that arrived meanwhile. Returns the snapshot and the events to apply after it.
async fn snapshot(
    conn: &Connection,
    events: &mut mpsc::UnboundedReceiver<Event>,
) -> Result<(Vec<Fact>, Vec<Fact>)> {
    let (_, v) = conn
        .call("session.attach_all", json!({"event_scope": "all"}))
        .await?;
    let sessions: Vec<String> = v["sessions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["session_id"].as_str().map(str::to_string))
        .collect();

    let lists = join_all(
        sessions
            .iter()
            .map(|s| conn.call("session.list_blocks", json!({"session_id": s}))),
    )
    .await;
    let mut blocks = Vec::new();
    for (session, list) in sessions.iter().zip(lists) {
        // A session can end while we look; skip it.
        let Ok((_, v)) = list else { continue };
        for b in v["blocks"].as_array().into_iter().flatten() {
            if b["creator_name"] == TERMINAL
                && let Some(id) = b["block_id"].as_str()
            {
                blocks.push((session.clone(), id.to_string()));
            }
        }
    }

    let reads = join_all(blocks.iter().map(|(s, b)| async move {
        tokio::join!(
            conn.block_call(s, b, "program_status"),
            conn.block_call(s, b, "title"),
            conn.block_call(s, b, "pwd"),
            conn.block_call(s, b, "process"),
        )
    }))
    .await;

    let mut facts = Vec::new();
    let mut marks: HashMap<(String, Kind), u64> = HashMap::new();
    for ((session, block), (status, title, pwd, process)) in blocks.into_iter().zip(reads) {
        if let Ok((seq, v)) = status {
            let root = v["records"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r.get("id").and_then(Value::as_str).unwrap_or("").is_empty());
            if let Some(record) = root.and_then(Record::from_json) {
                facts.push(Fact::Status {
                    session: session.clone(),
                    block: block.clone(),
                    record: Some(record),
                });
            }
            marks.insert((block.clone(), Kind::Status), seq);
        }
        if let Ok((seq, v)) = title {
            if let Some(t) = v["title"].as_str() {
                facts.push(Fact::Title {
                    session: session.clone(),
                    block: block.clone(),
                    title: t.to_string(),
                });
            }
            marks.insert((block.clone(), Kind::Title), seq);
        }
        if let Ok((seq, v)) = pwd {
            if let Some(p) = v["pwd"].as_str() {
                facts.push(Fact::Pwd {
                    session: session.clone(),
                    block: block.clone(),
                    pwd: p.to_string(),
                });
            }
            marks.insert((block.clone(), Kind::Pwd), seq);
        }
        if let Ok((seq, v)) = process {
            if let Some(cwd) = v["foreground"]["cwd"].as_str() {
                facts.push(Fact::Cwd {
                    session: session.clone(),
                    block: block.clone(),
                    cwd: cwd.to_string(),
                });
            }
            marks.insert((block, Kind::Cwd), seq);
        }
    }

    let mut later = Vec::new();
    while let Ok((seq, ev)) = events.try_recv() {
        let Some(f) = fact_from_event(&ev) else {
            continue;
        };
        let included = fact_key(&f)
            .and_then(|k| marks.get(&k))
            .is_some_and(|m| seq <= *m);
        if !included {
            later.push(f);
        }
    }
    Ok((facts, later))
}

/// Shows an Agent's pane: focuses it in its session, then has the Rex app
/// that's showing the café switch to that session.
async fn jump(
    conn: &Connection,
    cafe_session: Option<&str>,
    session: &str,
    block: &str,
) -> Result<()> {
    conn.call(
        "session.focus_block",
        json!({"session_id": session, "block_id": block}),
    )
    .await?;
    let (_, v) = conn.call("client.list", json!({})).await?;
    let apps: Vec<&Value> = v["clients"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["info"]["kind"] == "app")
        .collect();
    let showing_cafe = |c: &&&Value| {
        cafe_session.is_some_and(|s| {
            c["session_ids"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == s))
        })
    };
    let app = apps
        .iter()
        .find(showing_cafe)
        .or_else(|| {
            apps.iter()
                .max_by_key(|c| c["connected_at"].as_str().unwrap_or("").to_string())
        })
        .ok_or_else(|| anyhow!("no Rex app is connected"))?;
    conn.call(
        "client.perform",
        json!({
            "client_id": app["client_id"],
            "action": "session.select",
            "args": {"session_id": session},
            "session_id": session,
        }),
    )
    .await?;
    Ok(())
}

/// Opens a new Agent beside the café: the first one to its right, later ones
/// stacked under the last, so the café keeps its width. Returns the new block.
async fn spawn(conn: &Connection, ctx: &Context, previous: Option<&str>) -> Result<String> {
    let session = ctx
        .session
        .as_deref()
        .ok_or_else(|| anyhow!("run the café inside Rex to open agents"))?;
    let layout = json!({"block": {
        "flavor": "com.superlogical.terminal.shell",
        "label": "agent",
        "options": {"command": ctx.spawn_cmd, "cwd": ctx.cwd},
    }});
    let new_block = |v: Value| {
        v["block_ids"][0]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("Rex made no block"))
    };
    if let Some(prev) = previous {
        let below = json!({
            "session_id": session, "anchor_id": prev, "direction": "vertical", "side": "after",
            "ratio": 0.5, "layout": layout, "focus": true,
        });
        // The last one may have closed; then split the café instead.
        if let Ok((_, v)) = conn.call("session.new_split", below).await {
            return new_block(v);
        }
    }
    let anchor = ctx
        .block
        .as_deref()
        .ok_or_else(|| anyhow!("run the café inside Rex to open agents"))?;
    let right = json!({
        "session_id": session, "anchor_id": anchor, "direction": "horizontal", "side": "after",
        "ratio": 0.65, "layout": layout, "focus": true,
    });
    let (_, v) = conn.call("session.new_split", right).await?;
    new_block(v)
}

/// Keeps the Café fed from Rex, reconnecting whenever the connection drops.
pub async fn run(
    ctx: Context,
    tx: mpsc::UnboundedSender<FeedMsg>,
    mut cmds: mpsc::UnboundedReceiver<Command>,
) {
    let mut backoff = Duration::from_millis(500);
    let mut spawned: Option<String> = None;
    loop {
        if let Ok((conn, mut events)) = Connection::open(&ctx.socket).await {
            backoff = Duration::from_millis(500);
            match snapshot(&conn, &mut events).await {
                Ok((facts, later)) => {
                    let _ = tx.send(FeedMsg::Reset(facts));
                    let _ = tx.send(FeedMsg::Open);
                    for f in later {
                        let _ = tx.send(FeedMsg::Fact(f));
                    }
                }
                Err(e) => {
                    let _ = tx.send(FeedMsg::Notice(format!("couldn't read Rex: {e}")));
                }
            }
            loop {
                tokio::select! {
                    ev = events.recv() => match ev {
                        Some((_, ev)) => {
                            if let Some(f) = fact_from_event(&ev) {
                                let _ = tx.send(FeedMsg::Fact(f));
                            }
                        }
                        None => break,
                    },
                    cmd = cmds.recv() => match cmd {
                        None => return,
                        Some(Command::Resync) => match snapshot(&conn, &mut events).await {
                            Ok((facts, later)) => {
                                let _ = tx.send(FeedMsg::Reset(facts));
                                for f in later {
                                    let _ = tx.send(FeedMsg::Fact(f));
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(FeedMsg::Notice(format!("couldn't re-sync: {e}")));
                            }
                        },
                        Some(Command::Jump { session, block }) => {
                            let (conn, tx, cafe) = (conn.clone(), tx.clone(), ctx.session.clone());
                            tokio::spawn(async move {
                                if let Err(e) = jump(&conn, cafe.as_deref(), &session, &block).await {
                                    let _ = tx.send(FeedMsg::Notice(format!("couldn't jump: {e}")));
                                }
                            });
                        }
                        Some(Command::Spawn) => match spawn(&conn, &ctx, spawned.as_deref()).await {
                            Ok(b) => spawned = Some(b),
                            Err(e) => {
                                let _ = tx.send(FeedMsg::Notice(format!("couldn't open an agent: {e}")));
                            }
                        },
                    },
                }
            }
        }
        let _ = tx.send(FeedMsg::Closed);
        let wait = tokio::time::sleep(backoff);
        tokio::pin!(wait);
        backoff = (backoff * 2).min(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = &mut wait => break,
                cmd = cmds.recv() => match cmd {
                    None => return,
                    Some(_) => {
                        let _ = tx.send(FeedMsg::Notice("Rex isn't reachable right now".into()));
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Status;

    fn block_event(name: &str, payload: Value) -> Value {
        json!({"type": "session_event", "payload": {"session_id": "s1", "event": {
            "type": "block_event",
            "payload": {"block_id": "b1", "creator_name": TERMINAL, "name": name, "payload": payload}
        }}})
    }

    #[test]
    fn reads_root_status_changes() {
        let ev = block_event(
            "program_status_changed",
            json!({"id": "", "reason": "report", "record": {"state": "working", "app": "claude-code"}}),
        );
        let Some(Fact::Status {
            session,
            block,
            record: Some(r),
        }) = fact_from_event(&ev)
        else {
            panic!()
        };
        assert_eq!(
            (session.as_str(), block.as_str(), r.status),
            ("s1", "b1", Status::Working)
        );
    }

    #[test]
    fn ignores_child_records() {
        let ev = block_event(
            "program_status_changed",
            json!({"id": "build/test", "record": {"state": "working"}}),
        );
        assert_eq!(fact_from_event(&ev), None);
        let ev = block_event(
            "program_status_removed",
            json!({"ids": ["build/test"], "reason": "clear"}),
        );
        assert_eq!(fact_from_event(&ev), None);
    }

    #[test]
    fn reads_removals_titles_and_closes() {
        let ev = block_event(
            "program_status_removed",
            json!({"ids": [""], "reason": "clear"}),
        );
        assert!(matches!(
            fact_from_event(&ev),
            Some(Fact::Status { record: None, .. })
        ));
        let ev = block_event(
            "process_changed",
            json!({"generation": 3, "foreground": {"name": "claude", "cwd": "/a/web"}}),
        );
        assert!(matches!(fact_from_event(&ev), Some(Fact::Cwd { cwd, .. }) if cwd == "/a/web"));
        let ev = block_event("title_changed", json!({"title": "◑ Fix CI"}));
        assert!(
            matches!(fact_from_event(&ev), Some(Fact::Title { title, .. }) if title == "◑ Fix CI")
        );
        let ev = json!({"type": "session_event", "payload": {"session_id": "s1", "event": {"type": "block_closed", "payload": {"block_id": "b1"}}}});
        assert_eq!(
            fact_from_event(&ev),
            Some(Fact::BlockClosed { block: "b1".into() })
        );
        let ev = json!({"type": "session_destroyed", "payload": {"session_id": "s1"}});
        assert_eq!(
            fact_from_event(&ev),
            Some(Fact::SessionGone {
                session: "s1".into()
            })
        );
    }
}
