//! The interactive loop: feed messages in, input in, frames out.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::agents::{AgentBook, Fact};
use std::path::PathBuf;

use crate::audio::{Audio, Channel, Cue, LEVEL_STEP};
use crate::cafe::{Cafe, Moment};
use crate::engine::Renderer;
use crate::engine::term::Terminal;
use crate::feed::{Command, FeedMsg};
use crate::settings::Settings;
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
    audio: Option<Audio>,
    settings_path: Option<PathBuf>,
    /// What's saved. Its mute changes only with `m`, never with `--quiet`.
    settings: Settings,
}

#[derive(PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

impl App {
    /// `audio` is `Err` with the reason when there's no sound.
    pub fn new(
        cmds: mpsc::UnboundedSender<Command>,
        demo: bool,
        audio: Result<Audio>,
        settings: Settings,
        settings_path: Option<PathBuf>,
        muted: bool,
    ) -> App {
        let mut app = App {
            model: Model::default(),
            ui: Ui {
                demo,
                sound: audio.is_ok().then_some(!muted),
                levels: settings.levels,
                ..Ui::default()
            },
            notice_until: 0.0,
            cmds,
            frame: None,
            audio: None,
            settings_path,
            settings,
        };
        let (min, max) = settings.lightning.gap();
        app.model.cafe.ambient.storm.set_gap(min, max);
        match audio {
            Ok(a) => app.audio = Some(a),
            Err(e) => app.notify(format!("no sound: {e}")),
        }
        app
    }

    fn notify(&mut self, notice: String) {
        self.ui.notice = Some(notice);
        self.notice_until = self.model.cafe.time + NOTICE_SECS;
    }

    fn save_settings(&mut self) {
        let Some(path) = &self.settings_path else {
            return;
        };
        // Save everything that was loaded, so hand-tuned settings like lightning survive.
        self.settings.levels = self.ui.levels;
        if let Err(e) = self.settings.save(path) {
            self.notify(format!("couldn't save settings: {e}"));
        }
    }

    fn set_level(&mut self, ch: Channel, level: f32) {
        self.ui.levels.set(ch, level);
        self.cue(Cue::Level(ch, self.ui.levels.get(ch)));
        self.save_settings();
    }

    fn nudge(&mut self, ch: Channel, steps: i32) {
        self.set_level(ch, self.ui.levels.get(ch) + steps as f32 * LEVEL_STEP);
    }

    fn toggle_mute(&mut self) {
        if let Some(on) = self.ui.sound {
            self.ui.sound = Some(!on);
            self.settings.muted = on;
            self.cue(Cue::Muted(on));
            self.save_settings();
        }
    }

    fn toggle_volume_menu(&mut self) {
        if self.ui.sound.is_none() {
            self.notify("there's no sound to adjust".into());
            return;
        }
        self.ui.volume_menu = match self.ui.volume_menu {
            Some(_) => None,
            None => Some(Channel::Master),
        };
    }

    /// Keys while the volume menu is open; it takes everything but quitting.
    fn handle_volume_key(&mut self, ch: Channel, code: KeyCode) -> Flow {
        let i = Channel::ALL.iter().position(|c| *c == ch).unwrap_or(0);
        let n = Channel::ALL.len();
        match code {
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Up | KeyCode::BackTab => {
                self.ui.volume_menu = Some(Channel::ALL[(i + n - 1) % n])
            }
            KeyCode::Down | KeyCode::Tab => self.ui.volume_menu = Some(Channel::ALL[(i + 1) % n]),
            KeyCode::Left | KeyCode::Char('-') => self.nudge(ch, -1),
            KeyCode::Right | KeyCode::Char('+') | KeyCode::Char('=') => self.nudge(ch, 1),
            KeyCode::Char('m') => self.toggle_mute(),
            KeyCode::Char('v') | KeyCode::Esc | KeyCode::Enter => self.ui.volume_menu = None,
            _ => {}
        }
        Flow::Continue
    }

    fn cue(&self, cue: Cue) {
        if let Some(a) = &self.audio {
            a.cue(cue);
        }
    }

    fn apply(&mut self, msg: FeedMsg) {
        match msg {
            FeedMsg::Open => self.cue(Cue::Open(true)),
            FeedMsg::Closed => self.cue(Cue::Open(false)),
            _ => {}
        }
        if let Some(n) = self.model.apply(msg) {
            self.notify(n);
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
                code: KeyCode::Char('c'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => return Flow::Quit,
            Event::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) if self.ui.volume_menu.is_some() => {
                let ch = self.ui.volume_menu.unwrap_or(Channel::Master);
                return self.handle_volume_key(ch, code);
            }
            Event::Key(KeyEvent {
                code,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            }) => match code {
                KeyCode::Char('q') | KeyCode::Esc => return Flow::Quit,
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
                KeyCode::Char('m') => self.toggle_mute(),
                KeyCode::Char('v') => self.toggle_volume_menu(),
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
                    // With the volume menu open, a click anywhere else just closes it.
                    MouseEventKind::Down(MouseButton::Left)
                        if self.ui.volume_menu.is_some()
                            && !matches!(
                                target,
                                Some(
                                    Target::VolumePanel | Target::VolumeRow(_) | Target::Volume(..)
                                )
                            ) =>
                    {
                        self.ui.volume_menu = None;
                    }
                    MouseEventKind::Down(MouseButton::Left) => match target {
                        Some(Target::Agent(b)) => {
                            self.ui.selected = Some(b.clone());
                            self.jump(&b);
                        }
                        Some(Target::Door) => {
                            let _ = self.cmds.send(Command::Spawn);
                        }
                        Some(Target::VolumeRow(ch)) => self.ui.volume_menu = Some(ch),
                        Some(Target::Volume(ch, tenths)) => {
                            self.ui.volume_menu = Some(ch);
                            self.set_level(ch, tenths as f32 * LEVEL_STEP);
                        }
                        Some(Target::VolumePanel) | None => {}
                    },
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        if let Some(Target::VolumeRow(ch) | Target::Volume(ch, _)) = target {
                            self.ui.volume_menu = Some(ch);
                            let up = m.kind == MouseEventKind::ScrollUp;
                            self.nudge(ch, if up { 1 } else { -1 });
                        }
                    }
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
                    for moment in self.model.cafe.take_moments() {
                        self.cue(match moment {
                            Moment::DrinkStarted(drink) => Cue::Brew { steam: drink.steam_secs() },
                            Moment::DrinkServed => Cue::Serve,
                            Moment::Stirred(pan) => Cue::Stir { pan },
                            // Light arrives first; the farther the strike, the longer the thunder takes.
                            Moment::Lightning(strength) => Cue::Thunder {
                                strength,
                                delay: 0.3 + (1.0 - strength) * 3.2,
                            },
                        });
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, MouseEvent};

    fn app() -> App {
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            tx,
            true,
            Err(anyhow::anyhow!("no device in tests")),
            Settings::default(),
            None,
            false,
        );
        app.ui.sound = Some(true);
        app
    }

    fn key(app: &mut App, code: KeyCode) -> Flow {
        app.handle(Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }))
    }

    #[test]
    fn the_volume_menu_takes_the_keys_while_open() {
        let mut app = app();
        key(&mut app, KeyCode::Char('v'));
        assert_eq!(app.ui.volume_menu, Some(Channel::Master));
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Left);
        key(&mut app, KeyCode::Left);
        assert!((app.ui.levels.music - 0.8).abs() < 1e-6);
        assert_eq!(app.ui.levels.master, 1.0);
        key(&mut app, KeyCode::Char('m'));
        assert_eq!(app.ui.sound, Some(false));
        assert!(key(&mut app, KeyCode::Esc) == Flow::Continue);
        assert_eq!(app.ui.volume_menu, None);
        assert!(key(&mut app, KeyCode::Esc) == Flow::Quit);
    }

    #[test]
    fn clicking_outside_the_menu_closes_it() {
        let mut app = app();
        key(&mut app, KeyCode::Char('v'));
        app.frame = Some(view::compose(&app.model.cafe, &app.ui, 140, 48, 0.0));
        let click = |column, row| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        app.handle(click(118 + 2, 4));
        assert!((app.ui.levels.rain - 0.3).abs() < 1e-6);
        assert_eq!(app.ui.volume_menu, Some(Channel::Rain));
        app.handle(click(10, 30));
        assert_eq!(app.ui.volume_menu, None);
    }

    #[test]
    fn quiet_runs_dont_save_the_mute() {
        let dir = std::env::temp_dir().join(format!("rex-cafe-quiet-{}", std::process::id()));
        let path = dir.join("settings.json");
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            tx,
            true,
            Err(anyhow::anyhow!("none")),
            Settings::default(),
            Some(path.clone()),
            true,
        );
        app.ui.sound = Some(false);
        key(&mut app, KeyCode::Char('v'));
        key(&mut app, KeyCode::Left);
        assert!(!Settings::load(&path).muted);
        key(&mut app, KeyCode::Char('m'));
        key(&mut app, KeyCode::Char('m'));
        assert!(Settings::load(&path).muted);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn volume_changes_keep_the_lightning_setting() {
        let dir = std::env::temp_dir().join(format!("rex-cafe-lightning-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut settings = Settings::default();
        settings.lightning = crate::settings::Lightning {
            min_secs: 5.0,
            max_secs: 8.0,
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            tx,
            true,
            Err(anyhow::anyhow!("none")),
            settings,
            Some(path.clone()),
            false,
        );
        app.ui.sound = Some(true);
        key(&mut app, KeyCode::Char('v'));
        key(&mut app, KeyCode::Left);
        let saved = Settings::load(&path);
        assert_eq!(saved.lightning.gap(), (5.0, 8.0));
        assert!((saved.levels.master - 0.9).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_configured_lightning_gap_is_used() {
        let mut settings = Settings::default();
        settings.lightning = crate::settings::Lightning {
            min_secs: 5.0,
            max_secs: 8.0,
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let mut app = App::new(
            tx,
            true,
            Err(anyhow::anyhow!("none")),
            settings,
            None,
            false,
        );
        let mut strikes = 0;
        for _ in 0..(20 * 30) {
            app.model.cafe.update(1.0 / 30.0);
            strikes += app
                .model
                .cafe
                .take_moments()
                .iter()
                .filter(|m| matches!(m, Moment::Lightning(_)))
                .count();
        }
        assert!(
            (2..=4).contains(&strikes),
            "{strikes} strikes in 20s with a 5-8s gap"
        );
    }

    #[test]
    fn without_sound_there_is_no_menu() {
        let mut app = app();
        app.ui.sound = None;
        key(&mut app, KeyCode::Char('v'));
        assert_eq!(app.ui.volume_menu, None);
        assert!(app.ui.notice.is_some());
    }
}
