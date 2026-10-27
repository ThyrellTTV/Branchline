//! An integrated terminal: the user's shell in a pseudo-terminal (ConPTY on Windows), its output
//! interpreted by `vt100` and painted with egui.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use egui::{
    Color32, Event, FontId, Key, Modifiers, Rect, Sense, Stroke, Ui, pos2,
    text::{LayoutJob, TextFormat},
    vec2,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::theme::{self, Tokens};

const SCROLLBACK: usize = 5000;
const FONT_SIZE: f32 = 12.5;

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    /// Shared with the reader thread, which answers the shell's cursor-position queries.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    /// (rows, cols) the pty was last sized to.
    size: (u16, u16),
    /// Lines scrolled back into history (0 = following the output).
    scroll: usize,
    /// Fractional wheel scrolling not yet turned into lines.
    scroll_rest: f32,
    pub shell: String,
}

impl Terminal {
    /// Starts the user's shell in `cwd`.
    pub fn spawn(cwd: &Path, ctx: &egui::Context) -> Result<Self, String> {
        let (rows, cols) = (24, 100);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("could not open a terminal: {e}"))?;
        let (program, args) = shell();
        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&args);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("could not start {program}: {e}"))?;
        // The child holds its own handle; ours would keep the pty open after it exits.
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("terminal: {e}"))?;
        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|e| format!("terminal: {e}"))?,
        ));

        let parser = Arc::new(Mutex::new(vt100::Parser::new(
            nz(rows),
            nz(cols),
            SCROLLBACK,
        )));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let parser = Arc::clone(&parser);
            let exited = Arc::clone(&exited);
            let writer = Arc::clone(&writer);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let bytes = buf.get(..n).unwrap_or_default();
                            let mut parser = lock(&parser);
                            parser.process(bytes);
                            // ConPTY (and some programs) ask where the cursor is and wait for
                            // the answer before printing anything (DSR 6 → CPR).
                            if contains(bytes, b"[6n") {
                                let (row, col) = parser.screen().cursor_position();
                                let reply = format!("[{};{}R", row + 1, col + 1);
                                let mut w = lock(&writer);
                                let _ = w.write_all(reply.as_bytes());
                                let _ = w.flush();
                            }
                            drop(parser);
                            ctx.request_repaint();
                        }
                    }
                }
                exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            });
        }

        let shell = Path::new(&program)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(program);
        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            exited,
            size: (rows, cols),
            scroll: 0,
            scroll_rest: 0.0,
            shell,
        })
    }

    pub fn has_exited(&mut self) -> bool {
        if !self.exited.load(Ordering::Relaxed)
            && let Ok(Some(_)) = self.child.try_wait()
        {
            // ConPTY doesn't always end the read when the shell exits.
            self.exited.store(true, Ordering::Relaxed);
        }
        self.exited.load(Ordering::Relaxed)
    }

    fn send(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        {
            let mut w = lock(&self.writer);
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
        // Typing follows the output again.
        if self.scroll != 0 {
            self.scroll = 0;
            lock(&self.parser).screen_mut().set_scrollback(0);
        }
    }

    /// Paints the screen into all of `ui`'s remaining space and handles input.
    pub fn ui(&mut self, ui: &mut Ui) {
        let t = Tokens::get(ui.ctx());
        let font = theme::mono(FONT_SIZE);
        let glyph = ui
            .painter()
            .layout_no_wrap("M".to_owned(), font.clone(), t.text);
        let cell = vec2(glyph.size().x.max(1.0), glyph.size().y.max(1.0));

        let rect = ui.available_rect_before_wrap();
        let id = ui.id().with("terminal");
        let resp = ui.interact(rect, id, Sense::click());
        ui.allocate_rect(rect, Sense::hover());
        if resp.clicked() || resp.secondary_clicked() {
            resp.request_focus();
        } else if resp.has_focus()
            && ui.input(|i| i.pointer.any_pressed())
            && !ui.rect_contains_pointer(rect)
        {
            // Like a text field: a click anywhere else gives the keyboard back to the app.
            resp.surrender_focus();
        }
        let focused = resp.has_focus();
        if focused {
            // Keep Tab, arrows and Escape for the shell.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }

        // Size the pty to the panel.
        let inner = rect.shrink2(vec2(8.0, 6.0));
        let cols = ((inner.width() / cell.x).floor() as u16).clamp(2, 1000);
        let rows = ((inner.height() / cell.y).floor() as u16).clamp(1, 500);
        if (rows, cols) != self.size {
            self.size = (rows, cols);
            let _ = self.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
            lock(&self.parser).set_size(nz(rows), nz(cols));
        }

        // Input.
        if focused {
            let (events, app_cursor, bracketed) = {
                let parser = lock(&self.parser);
                let screen = parser.screen();
                (
                    ui.input(|i| i.events.clone()),
                    screen.application_cursor(),
                    screen.bracketed_paste(),
                )
            };
            for event in events {
                match event {
                    Event::Text(text) => self.send(text.as_bytes()),
                    Event::Paste(text) => {
                        let text = text.replace("\r\n", "\r").replace('\n', "\r");
                        if bracketed {
                            self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                        } else {
                            self.send(text.as_bytes());
                        }
                    }
                    // egui turns Ctrl+C / Ctrl+X into clipboard events; the shell wants them.
                    Event::Copy => self.send(b"\x03"),
                    Event::Cut => self.send(b"\x18"),
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                            self.send(&bytes);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Wheel scrolls through history.
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                self.scroll_rest += dy / cell.y;
                let lines = self.scroll_rest.trunc();
                self.scroll_rest -= lines;
                let mut parser = lock(&self.parser);
                let wanted = (self.scroll as f32 + lines).max(0.0) as usize;
                parser.screen_mut().set_scrollback(wanted);
                self.scroll = parser.screen().scrollback();
            }
        }

        // Paint.
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, t.field);
        let parser = lock(&self.parser);
        let screen = parser.screen();
        let (rows, cols) = screen.size();
        let (rows, cols) = (rows.get(), cols.get());
        let palette = Palette::new(&t);
        for row in 0..rows {
            let y = inner.top() + row as f32 * cell.y;
            let mut job = LayoutJob::default();
            let mut col = 0;
            while col < cols {
                let Some(c) = screen.cell(row, col) else {
                    break;
                };
                let (fg, bg) = palette.colors(c);
                if let Some(bg) = bg {
                    let x = inner.left() + col as f32 * cell.x;
                    let w = if c.is_wide() { 2.0 } else { 1.0 };
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, y), vec2(cell.x * w, cell.y)),
                        0.0,
                        bg,
                    );
                }
                if !c.is_wide_continuation() {
                    let text = if c.has_contents() { c.contents() } else { " " };
                    job.append(
                        text,
                        0.0,
                        TextFormat {
                            font_id: font.clone(),
                            color: fg,
                            underline: if c.underline() {
                                Stroke::new(1.0, fg)
                            } else {
                                Stroke::NONE
                            },
                            italics: c.italic(),
                            ..Default::default()
                        },
                    );
                }
                col += 1;
            }
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            painter.galley(pos2(inner.left(), y), galley, t.text);
        }

        // Cursor (only when following the output).
        if self.scroll == 0 && !screen.hide_cursor() && !self.exited.load(Ordering::Relaxed) {
            let (cr, cc) = screen.cursor_position();
            let r = Rect::from_min_size(
                pos2(
                    inner.left() + cc as f32 * cell.x,
                    inner.top() + cr as f32 * cell.y,
                ),
                cell,
            );
            if focused {
                painter.rect_filled(r, 0.0, t.text.gamma_multiply(0.7));
            } else {
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, t.text_faint),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if self.scroll > 0 {
            painter.text(
                rect.right_top() + vec2(-10.0, 8.0),
                egui::Align2::RIGHT_TOP,
                format!("↑ {} lines", self.scroll),
                FontId::proportional(11.0),
                t.text_faint,
            );
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// The shell to run: PowerShell (7 if installed) on Windows, `$SHELL` elsewhere.
fn shell() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        let program = if on_path("pwsh.exe") {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };
        (program.to_owned(), vec!["-NoLogo".to_owned()])
    }
    #[cfg(not(windows))]
    {
        let program = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned());
        (program, Vec::new())
    }
}

#[cfg(windows)]
fn on_path(exe: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(exe).is_file()))
        .unwrap_or(false)
}

fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A poisoned lock still holds usable data.
fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// What a key sends to the shell (xterm sequences).
fn key_bytes(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let csi_or_ss3 = |c: char| {
        if app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
    };
    if mods.ctrl && !mods.alt {
        // Ctrl+letter → control character (Ctrl+C and Ctrl+X arrive as Copy / Cut).
        let name = key.name();
        if name.len() == 1
            && let Some(c) = name.chars().next()
            && c.is_ascii_alphabetic()
        {
            return Some(vec![(c.to_ascii_uppercase() as u8) - b'A' + 1]);
        }
    }
    let s: String = match key {
        Key::Enter => "\r".into(),
        Key::Tab if mods.shift => "\x1b[Z".into(),
        Key::Tab => "\t".into(),
        Key::Backspace => "\x7f".into(),
        Key::Escape => "\x1b".into(),
        Key::ArrowUp => csi_or_ss3('A'),
        Key::ArrowDown => csi_or_ss3('B'),
        Key::ArrowRight => csi_or_ss3('C'),
        Key::ArrowLeft => csi_or_ss3('D'),
        Key::Home => csi_or_ss3('H'),
        Key::End => csi_or_ss3('F'),
        Key::PageUp => "\x1b[5~".into(),
        Key::PageDown => "\x1b[6~".into(),
        Key::Insert => "\x1b[2~".into(),
        Key::Delete => "\x1b[3~".into(),
        Key::F1 => "\x1bOP".into(),
        Key::F2 => "\x1bOQ".into(),
        Key::F3 => "\x1bOR".into(),
        Key::F4 => "\x1bOS".into(),
        _ => return None,
    };
    Some(s.into_bytes())
}

/// Terminal colours tuned to the app theme.
struct Palette {
    fg: Color32,
    ansi: [Color32; 16],
}

impl Palette {
    fn new(t: &Tokens) -> Self {
        let ansi = if t.dark() {
            [
                Color32::from_rgb(40, 40, 40),
                Color32::from_rgb(236, 91, 98),
                Color32::from_rgb(92, 196, 122),
                Color32::from_rgb(232, 176, 70),
                Color32::from_rgb(64, 156, 255),
                Color32::from_rgb(200, 110, 230),
                Color32::from_rgb(60, 205, 210),
                Color32::from_rgb(200, 200, 200),
                Color32::from_rgb(110, 110, 110),
                Color32::from_rgb(255, 120, 125),
                Color32::from_rgb(130, 225, 150),
                Color32::from_rgb(250, 205, 100),
                Color32::from_rgb(110, 180, 255),
                Color32::from_rgb(220, 150, 245),
                Color32::from_rgb(110, 225, 230),
                Color32::from_rgb(245, 245, 245),
            ]
        } else {
            [
                Color32::from_rgb(20, 20, 20),
                Color32::from_rgb(200, 40, 50),
                Color32::from_rgb(30, 140, 70),
                Color32::from_rgb(170, 110, 0),
                Color32::from_rgb(20, 100, 210),
                Color32::from_rgb(150, 60, 190),
                Color32::from_rgb(0, 140, 150),
                Color32::from_rgb(110, 110, 110),
                Color32::from_rgb(90, 90, 90),
                Color32::from_rgb(230, 60, 70),
                Color32::from_rgb(40, 170, 90),
                Color32::from_rgb(200, 140, 0),
                Color32::from_rgb(40, 120, 240),
                Color32::from_rgb(180, 80, 220),
                Color32::from_rgb(0, 170, 180),
                Color32::from_rgb(40, 40, 40),
            ]
        };
        Self { fg: t.text, ansi }
    }

    fn color(&self, c: vt100::Color) -> Option<Color32> {
        match c {
            vt100::Color::Default => None,
            vt100::Color::Idx(i) if i < 16 => self.ansi.get(i as usize).copied(),
            vt100::Color::Idx(i) if i < 232 => {
                let i = i - 16;
                let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Some(Color32::from_rgb(
                    level(i / 36),
                    level((i / 6) % 6),
                    level(i % 6),
                ))
            }
            vt100::Color::Idx(i) => {
                let v = 8 + (i - 232).saturating_mul(10);
                Some(Color32::from_gray(v))
            }
            vt100::Color::Rgb(r, g, b) => Some(Color32::from_rgb(r, g, b)),
        }
    }

    /// (foreground, background-if-not-default) of a cell.
    fn colors(&self, cell: &vt100::Cell) -> (Color32, Option<Color32>) {
        let mut fg = self.color(cell.fgcolor()).unwrap_or(self.fg);
        let mut bg = self.color(cell.bgcolor());
        if cell.bold()
            && let vt100::Color::Idx(i) = cell.fgcolor()
            && i < 8
        {
            fg = self.ansi.get(i as usize + 8).copied().unwrap_or(fg);
        }
        if cell.dim() {
            fg = fg.gamma_multiply(0.6);
        }
        if cell.inverse() {
            let old_fg = fg;
            fg = bg.unwrap_or(Color32::from_rgb(30, 30, 30));
            bg = Some(old_fg);
        }
        (fg, bg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_to_bytes() {
        assert_eq!(
            key_bytes(Key::Enter, Modifiers::NONE, false),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            key_bytes(Key::ArrowUp, Modifiers::NONE, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(Key::ArrowUp, Modifiers::NONE, true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(key_bytes(Key::D, Modifiers::CTRL, false), Some(vec![4]));
        assert_eq!(key_bytes(Key::A, Modifiers::NONE, false), None);
    }

    #[test]
    fn indexed_colours() {
        let p = Palette::new(&Tokens::for_kind(crate::theme::ThemeKind::Dark));
        assert_eq!(
            p.color(vt100::Color::Idx(16)),
            Some(Color32::from_rgb(0, 0, 0))
        );
        assert_eq!(
            p.color(vt100::Color::Idx(231)),
            Some(Color32::from_rgb(255, 255, 255))
        );
        assert_eq!(
            p.color(vt100::Color::Idx(255)),
            Some(Color32::from_gray(238))
        );
    }
}
