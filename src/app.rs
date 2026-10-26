//! The interactive loop: feed messages in, input in, frames out.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::agents::{AgentBook, Fact};
use crate::cafe::Cafe;
use crate::engine::Renderer;
use crate::engine::term::Terminal;
use crate::feed::{Command, FeedMsg};
use crate::view::{self, Frame, Target, Ui};

const FPS: u64 = 30;
const NOTICE_SECS: f64 = 4.0;

/// The Agents as Rex reports them, and the Café that plays them out.
#[derive(Default)]
pub struct Model {
    pub book: AgentBook,
    pub cafe: Cafe,
}

impl Model {
    /// Applies a feed message; returns a notice to show, if any.
    pub fn apply(&mut self, msg: FeedMsg) -> Option<String> {
        match msg {
            FeedMsg::Open => self.cafe.set_open(true),
            FeedMsg::Closed => self.cafe.set_open(false),
            FeedMsg::Reset(facts) => {
                self.book = AgentBook::default();
                facts.into_iter().for_each(|f: Fact| self.book.apply(f));
                self.cafe.reset(&self.book.agents());
            }
            FeedMsg::Fact(f) => {
                self.book.apply(f);
                self.cafe.sync(&self.book.agents());
            }
            FeedMsg::Notice(n) => return Some(n),
        }
        None
    }
}

pub struct App {
    pub model: Model,
    pub ui: Ui,
    notice_until: f64,
    cmds: mpsc::UnboundedSender<Command>,
    frame: Option<Frame>,
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

impl App {
    pub fn new(cmds: mpsc::UnboundedSender<Command>, demo: bool) -> App {
        App {
            model: Model::default(),
            ui: Ui {
                demo,
                ..Ui::default()
            },
            notice_until: 0.0,
            cmds,
            frame: None,
        }
    }

    fn apply(&mut self, msg: FeedMsg) {
        if let Some(n) = self.model.apply(msg) {
            self.ui.notice = Some(n);
            self.notice_until = self.model.cafe.time + NOTICE_SECS;
        }
        // Forget selections for Agents that have gone.
        let cafe = &self.model.cafe;
        if self
            .ui
            .selected
            .as_deref()
            .is_some_and(|b| cafe.clawd(b).is_none())
        {
            self.ui.selected = None;
        }
        if self
            .ui
            .hover
            .as_deref()
            .is_some_and(|b| cafe.clawd(b).is_none())
        {
            self.ui.hover = None;
        }
    }

    fn step_selection(&mut self, forward: bool) {
        let legend = self.model.cafe.legend();
        if legend.is_empty() {
            return;
        }
        let current = self
            .ui
            .selected
            .as_deref()
            .and_then(|b| legend.iter().position(|c| c.agent.block == b));
        let n = legend.len();
        let next = match current {
            None if forward => 0,
            None => n - 1,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        };
        self.ui.selected = Some(legend[next].agent.block.clone());
    }

    fn jump(&mut self, block: &str) {
        if let Some(c) = self.model.cafe.clawd(block) {
            let cmd = Command::Jump {
                session: c.agent.session.clone(),
                block: c.agent.block.clone(),
            };
            let _ = self.cmds.send(cmd);
        }
    }

    fn handle(&mut self, ev: Event) -> Flow {
        match ev {
            Event::Key(KeyEvent {
                code,
                modifiers,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => match code {
                KeyCode::Char('q') | KeyCode::Esc => return Flow::Quit,
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    return Flow::Quit;
                }
                KeyCode::Tab | KeyCode::Right | KeyCode::Down => self.step_selection(true),
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up => self.step_selection(false),
                KeyCode::Enter => {
                    if let Some(b) = self.ui.selected.clone() {
                        self.jump(&b);
                    }
                }
                KeyCode::Char('n') => {
                    let _ = self.cmds.send(Command::Spawn);
                }
                KeyCode::Char('r') => {
                    let _ = self.cmds.send(Command::Resync);
                }
                _ => {}
            },
            Event::Mouse(m) => {
                let target = self
                    .frame
                    .as_ref()
                    .and_then(|f| f.hit(m.column, m.row))
                    .cloned();
                match m.kind {
                    MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                        self.ui.hover = match target {
                            Some(Target::Agent(b)) => Some(b),
                            _ => None,
                        };
                    }
                    MouseEventKind::Down(MouseButton::Left) => match target {
                        Some(Target::Agent(b)) => {
                            self.ui.selected = Some(b.clone());
                            self.jump(&b);
                        }
                        Some(Target::Door) => {
                            let _ = self.cmds.send(Command::Spawn);
                        }
                        None => {}
                    },
                    _ => {}
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    /// Runs full-screen until the user quits.
    pub async fn run(mut self, mut feed: mpsc::UnboundedReceiver<FeedMsg>) -> Result<()> {
        let term = Terminal::enter()?;
        let mut input = EventStream::new();
        let mut renderer = Renderer::default();
        let mut tick = tokio::time::interval(Duration::from_millis(1000 / FPS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last = Instant::now();
        let mut out = String::new();
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let now = Instant::now();
                    let dt = (now - last).as_secs_f32().min(0.1);
                    last = now;
                    self.model.cafe.update(dt);
                    if self.model.cafe.time > self.notice_until {
                        self.ui.notice = None;
                    }
                    let (cols, rows) = Terminal::size();
                    let frame = view::compose(&self.model.cafe, &self.ui, cols, rows, unix_now());
                    out.clear();
                    renderer.render(&frame.screen, &mut out);
                    term.write(&out)?;
                    self.frame = Some(frame);
                }
                ev = input.next() => match ev {
                    Some(Ok(Event::Resize(..))) => renderer.invalidate(),
                    Some(Ok(ev)) => if self.handle(ev) == Flow::Quit { break },
                    Some(Err(e)) => return Err(e.into()),
                    None => break,
                },
                Some(msg) = feed.recv() => self.apply(msg),
            }
        }
        drop(term);
        Ok(())
    }
}

pub fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
