//! Full-screen terminal UI for `metadata`.
pub mod app;
pub mod theme;
pub mod ui;

use app::App;
use ratatui::crossterm::event::{self, Event};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

/// Restores the terminal on drop (normal exit, `?` early return, unwinding panic). `ratatui::init`
/// additionally installs a panic hook that restores before the panic message prints.
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn osc52(text: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = text.as_bytes();
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    format!("\x1b]52;c;{out}\x07")
}

/// Run the interactive UI until the user quits. `scan` starts a background scan of that path.
pub fn run(root: PathBuf, scan: bool, use_external: bool) -> std::io::Result<()> {
    let mut app = App::new(root.clone(), use_external);
    if scan {
        app.start_scan(root);
    }
    let mut terminal = ratatui::init();
    let _guard = TerminalGuard;
    while !app.should_quit {
        app.poll();
        terminal.draw(|f| ui::draw(f, &app))?;
        if let Some(text) = app.clipboard.take() {
            let mut out = std::io::stdout();
            out.write_all(osc52(&text).as_bytes())?;
            out.flush()?;
        }
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                app.on_key(k);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn osc52_base64() {
        assert_eq!(super::osc52("hello"), "\x1b]52;c;aGVsbG8=\x07");
        assert_eq!(super::osc52("ab"), "\x1b]52;c;YWI=\x07");
    }
}
