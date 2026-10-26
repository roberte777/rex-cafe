use std::io::{self, Write};

use crossterm::{
    cursor,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

/// Puts the terminal into full-screen raw mode with mouse reporting, and
/// restores it when dropped or when the program panics.
pub struct Terminal {
    _private: (),
}

impl Terminal {
    pub fn enter() -> io::Result<Terminal> {
        terminal::enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            cursor::Hide,
            EnableMouseCapture
        )?;
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        Ok(Terminal { _private: () })
    }

    pub fn size() -> (u16, u16) {
        terminal::size().unwrap_or((80, 24))
    }

    pub fn write(&self, frame: &str) -> io::Result<()> {
        let mut out = io::stdout().lock();
        out.write_all(frame.as_bytes())?;
        out.flush()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        restore();
    }
}

fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        cursor::Show,
        LeaveAlternateScreen
    );
    let _ = terminal::disable_raw_mode();
}
