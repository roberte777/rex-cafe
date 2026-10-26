//! The Café's picture of which Agents exist and what they've reported.
//!
//! Everything here is derived from facts read from Rex (ADR 0002). Nothing in
//! this module knows about drawing or about the connection.

use std::collections::BTreeMap;

/// Program Status `app` values that count as Agents.
pub const AGENT_APPS: &[&str] = &["claude-code", "codex", "opencode", "pi"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockedOn {
    Permission,
    Question,
    Auth,
    Unknown,
}

impl BlockedOn {
    pub fn label(self) -> &'static str {
        match self {
            BlockedOn::Permission => "permission",
            BlockedOn::Question => "question",
            BlockedOn::Auth => "login",
            BlockedOn::Unknown => "input",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Idle,
    Working,
    Blocked(BlockedOn),
    Done,
    Error,
}

impl Status {
    /// Statuses that interrupt whatever a Clawd is doing.
    pub fn is_urgent(self) -> bool {
        matches!(self, Status::Blocked(_) | Status::Error)
    }
}

/// One Program Status record, as Rex reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub status: Status,
    pub app: Option<String>,
    pub title: Option<String>,
    pub msg: Option<String>,
    /// Unix seconds of the last report.
    pub updated_at: Option<f64>,
}

impl Record {
    /// Parses a record object from Rex. Unknown states yield `None`.
    pub fn from_json(v: &serde_json::Value) -> Option<Record> {
        let s = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .filter(|x| !x.is_empty())
                .map(str::to_string)
        };
        let status = match v.get("state")?.as_str()? {
            "idle" => Status::Idle,
            "working" => Status::Working,
            "done" => Status::Done,
            "error" => Status::Error,
            "blocked" => Status::Blocked(match v.get("kind").and_then(|k| k.as_str()) {
                Some("permission") => BlockedOn::Permission,
                Some("question") => BlockedOn::Question,
                Some("auth") => BlockedOn::Auth,
                _ => BlockedOn::Unknown,
            }),
            _ => return None,
        };
        Some(Record {
            status,
            app: s("effective_app").or_else(|| s("app")),
            title: s("title"),
            msg: s("msg"),
            updated_at: v
                .get("updated_at")
                .and_then(|t| t.as_str())
                .and_then(parse_rfc3339),
        })
    }
}

/// Something Rex told us about a terminal.
#[derive(Clone, Debug, PartialEq)]
pub enum Fact {
    /// The terminal's root status record, or `None` once it is removed.
    Status {
        session: String,
        block: String,
        record: Option<Record>,
    },
    Title {
        session: String,
        block: String,
        title: String,
    },
    Pwd {
        session: String,
        block: String,
        pwd: String,
    },
    /// The working directory of the terminal's foreground process.
    Cwd {
        session: String,
        block: String,
        cwd: String,
    },
    BlockClosed {
        block: String,
    },
    SessionGone {
        session: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Terminal {
    session: String,
    record: Option<Record>,
    title: Option<String>,
    pwd: Option<String>,
    cwd: Option<String>,
}

/// An Agent as the Café sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Agent {
    /// The Rex block the Agent runs in; an Agent's identity.
    pub block: String,
    pub session: String,
    pub app: String,
    pub name: String,
    pub status: Status,
    pub msg: Option<String>,
    /// Unix seconds when the Agent last reported.
    pub since: Option<f64>,
}

/// Every terminal's latest facts, from which the set of Agents is derived.
#[derive(Clone, Debug, Default)]
pub struct AgentBook {
    terminals: BTreeMap<String, Terminal>,
}

impl AgentBook {
    pub fn apply(&mut self, fact: Fact) {
        match fact {
            Fact::Status {
                session,
                block,
                record,
            } => {
                let t = self.terminal(session, block);
                t.record = record;
            }
            Fact::Title {
                session,
                block,
                title,
            } => {
                self.terminal(session, block).title = Some(title);
            }
            Fact::Pwd {
                session,
                block,
                pwd,
            } => {
                self.terminal(session, block).pwd = Some(pwd);
            }
            Fact::Cwd {
                session,
                block,
                cwd,
            } => {
                self.terminal(session, block).cwd = Some(cwd);
            }
            Fact::BlockClosed { block } => {
                self.terminals.remove(&block);
            }
            Fact::SessionGone { session } => {
                self.terminals.retain(|_, t| t.session != session);
            }
        }
    }

    fn terminal(&mut self, session: String, block: String) -> &mut Terminal {
        let t = self.terminals.entry(block).or_default();
        t.session = session;
        t
    }

    /// The Agents, ordered by block id so the order is stable.
    pub fn agents(&self) -> Vec<Agent> {
        self.terminals
            .iter()
            .filter_map(|(block, t)| {
                let r = t.record.as_ref()?;
                let app = r.app.clone().filter(|a| AGENT_APPS.contains(&a.as_str()))?;
                Some(Agent {
                    block: block.clone(),
                    session: t.session.clone(),
                    name: agent_name(
                        r.title.as_deref(),
                        t.title.as_deref(),
                        t.pwd.as_deref().or(t.cwd.as_deref()),
                        &app,
                    ),
                    app,
                    status: r.status,
                    msg: r.msg.clone(),
                    since: r.updated_at,
                })
            })
            .collect()
    }
}

/// What a terminal title says once spinner and status glyphs are stripped.
/// A title that only names the app says nothing about the task.
fn task_from_title(title: &str, app: &str) -> Option<String> {
    let t = title
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim();
    let generic = t.is_empty()
        || t.eq_ignore_ascii_case(app_display_name(app))
        || t.eq_ignore_ascii_case(app);
    (!generic).then(|| t.to_string())
}

pub fn app_display_name(app: &str) -> &str {
    match app {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        "pi" => "Pi",
        other => other,
    }
}

/// The Legend name: the record's title, else the terminal title, else the
/// working directory's name, else the app's name.
pub fn agent_name(
    record_title: Option<&str>,
    term_title: Option<&str>,
    pwd: Option<&str>,
    app: &str,
) -> String {
    record_title
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| term_title.and_then(|t| task_from_title(t, app)))
        .or_else(|| {
            let pwd = pwd?.trim_start_matches("file://");
            let pwd = pwd.trim_end_matches('/');
            pwd.rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| app_display_name(app).to_string())
}

/// Parses an RFC 3339 timestamp such as `2026-10-26T10:52:11.652426-05:00`
/// into Unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<f64> {
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let rest = s.get(19..)?;
    let (frac, zone) = match rest.strip_prefix('.') {
        Some(r) => {
            let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
            let digits = &r[..end];
            (format!("0.{digits}").parse::<f64>().ok()?, &r[end..])
        }
        None => (0.0, rest),
    };
    let offset = match zone {
        "Z" | "z" => 0,
        z if z.len() == 6 => {
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let oh: i64 = z.get(1..3)?.parse().ok()?;
            let om: i64 = z.get(4..6)?.parse().ok()?;
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };
    // Days from the civil calendar, after Howard Hinnant's algorithm.
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some((days * 86400 + h * 3600 + mi * 60 + sec - offset) as f64 + frac)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status(block: &str, state: &str, app: &str) -> Fact {
        Fact::Status {
            session: "s".into(),
            block: block.into(),
            record: Record::from_json(&json!({"state": state, "app": app, "effective_app": app})),
        }
    }

    #[test]
    fn only_agent_apps_are_agents() {
        let mut book = AgentBook::default();
        book.apply(status("b1", "working", "claude-code"));
        book.apply(status("b2", "working", "cargo"));
        let agents = book.agents();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].block, "b1");
        assert_eq!(agents[0].status, Status::Working);
    }

    #[test]
    fn removing_the_record_or_closing_the_block_removes_the_agent() {
        let mut book = AgentBook::default();
        book.apply(status("b1", "idle", "claude-code"));
        book.apply(Fact::Status {
            session: "s".into(),
            block: "b1".into(),
            record: None,
        });
        assert!(book.agents().is_empty());
        book.apply(status("b1", "idle", "claude-code"));
        book.apply(Fact::BlockClosed { block: "b1".into() });
        assert!(book.agents().is_empty());
    }

    #[test]
    fn parses_blocked_kind_and_msg() {
        let r = Record::from_json(&json!({
            "state": "blocked", "kind": "permission", "app": "claude-code",
            "msg": "approve Write: probe.txt", "updated_at": "2026-10-26T10:53:01.244848-05:00"
        }))
        .unwrap();
        assert_eq!(r.status, Status::Blocked(BlockedOn::Permission));
        assert_eq!(r.msg.as_deref(), Some("approve Write: probe.txt"));
        assert!(Record::from_json(&json!({"state": "sleeping"})).is_none());
    }

    #[test]
    fn names_prefer_record_title_then_task_title_then_pwd() {
        assert_eq!(
            agent_name(
                Some("Indexer"),
                Some("◑ Fix CI"),
                Some("/a/b"),
                "claude-code"
            ),
            "Indexer"
        );
        assert_eq!(
            agent_name(
                None,
                Some("◑ Fix flaky CI tests"),
                Some("/a/b"),
                "claude-code"
            ),
            "Fix flaky CI tests"
        );
        assert_eq!(
            agent_name(
                None,
                Some("✳ Claude Code"),
                Some("/a/rex-cafe"),
                "claude-code"
            ),
            "rex-cafe"
        );
        assert_eq!(
            agent_name(None, None, Some("file://host/a/web/"), "codex"),
            "web"
        );
        assert_eq!(agent_name(None, Some(""), None, "codex"), "Codex");
    }

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0.0));
        let t = parse_rfc3339("2026-10-26T10:52:11.5-05:00").unwrap();
        assert_eq!(t, parse_rfc3339("2026-10-26T15:52:11.5Z").unwrap());
        assert_eq!(parse_rfc3339("2000-03-01T00:00:00Z"), Some(951_868_800.0));
    }
}
