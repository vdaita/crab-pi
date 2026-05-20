use crc32fast::Hasher;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use indicatif::{ProgressBar, ProgressStyle};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use serialport::SerialPort;
use std::fs;
use std::io::{self, Read, Write};
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

const ARMBASE: u32 = 0x0000_8000;
const GET_PROG_INFO: u32 = 0x11112222;
const PUT_PROG_INFO: u32 = 0x33334444;
const GET_CODE: u32 = 0x55556666;
const PUT_CODE: u32 = 0x77778888;
const BOOT_SUCCESS: u32 = 0x9999aaaa;
const BOOT_ERROR: u32 = 0xbbbbcccc;

const PROG_OPEN: &str = "[prog]";
const PROG_CLOSE: &str = "[/prog]";

enum UiEvent {
    Serial(String),
    Stdin(String),
    SerialClosed,
}

#[derive(Default)]
struct SerialSplitState {
    in_prog: bool,
    pending: String,
}

#[derive(Default)]
struct App {
    left: String,
    right: String,
    split: SerialSplitState,
    done: bool,
}

impl SerialSplitState {
    fn consume(&mut self, chunk: &str, left: &mut String, right: &mut String) {
        for ch in chunk.chars() {
            self.feed_char(ch, left, right);
        }
    }

    fn feed_char(&mut self, ch: char, left: &mut String, right: &mut String) {
        if self.pending.is_empty() {
            if ch == '[' {
                self.pending.push(ch);
            } else {
                Self::push_current(self.in_prog, left, right, ch);
            }
            return;
        }

        self.pending.push(ch);

        loop {
            if self.pending == PROG_OPEN {
                self.pending.clear();
                self.in_prog = true;
                return;
            }

            if self.pending == PROG_CLOSE {
                self.pending.clear();
                self.in_prog = false;
                return;
            }

            if PROG_OPEN.starts_with(&self.pending) || PROG_CLOSE.starts_with(&self.pending) {
                return;
            }

            let first = self.pending.remove(0);
            Self::push_current(self.in_prog, left, right, first);

            if self.pending.is_empty() {
                return;
            }
        }
    }

    fn flush_pending(&mut self, left: &mut String, right: &mut String) {
        for ch in self.pending.drain(..) {
            Self::push_current(self.in_prog, left, right, ch);
        }
    }

    fn push_current(in_prog: bool, left: &mut String, right: &mut String, ch: char) {
        if in_prog {
            right.push(ch);
        } else {
            left.push(ch);
        }
    }
}

impl App {
    fn push_serial(&mut self, chunk: &str) {
        self.split.consume(chunk, &mut self.left, &mut self.right);
        self.done = self.left.contains("DONE!!!");
    }

    fn push_stdin(&mut self, input: &str) {
        // self.right.push_str("[stdin] ");
        self.right.push_str(input);

        self.left.push_str("[stdin] ");
        self.left.push_str(input);
        if !input.ends_with('\n') {
            self.left.push('\n');
        }
    }

    fn finish(&mut self) {
        self.split.flush_pending(&mut self.left, &mut self.right);
    }
}

fn tail_lines(text: &str, max_lines: usize) -> String {
    if max_lines == 0 {
        return String::new();
    }

    let mut lines: Vec<&str> = text.lines().collect();
    if lines.len() > max_lines {
        lines = lines.split_off(lines.len() - max_lines);
    }
    lines.join("\n")
}

fn draw_ui(frame: &mut ratatui::Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(frame.size());

    let left_inner_height = chunks[0].height.saturating_sub(2) as usize;
    let right_inner_height = chunks[1].height.saturating_sub(2) as usize;
    let left_text = tail_lines(&app.left, left_inner_height);
    let right_text = tail_lines(&app.right, right_inner_height);

    let left = Paragraph::new(left_text).block(Block::default().borders(Borders::ALL).title("stdout"));
    let right = Paragraph::new(right_text).block(Block::default().borders(Borders::ALL).title("stdin + prog"));

    frame.render_widget(left, chunks[0]);
    frame.render_widget(right, chunks[1]);
}

fn spawn_stdin_thread(mut port: Box<dyn SerialPort>, tx: Sender<UiEvent>) {
    thread::spawn(move || {
        let mut buf = [0u8; 256];

        loop {
            let n = match io::stdin().read(&mut buf) {
                Ok(0) => return,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("[stdin] read error: {e}");
                    return;
                }
            };

            let mut outbound = buf[..n].to_vec();
            for byte in &mut outbound {
                if *byte == b'\r' {
                    *byte = b'\n';
                }
            }

            if let Err(e) = write_exact(&mut *port, &outbound, false) {
                eprintln!("[stdin] write error: {e}");
                return;
            }

            let text = String::from_utf8_lossy(&outbound).into_owned();
            if tx.send(UiEvent::Stdin(text)).is_err() {
                return;
            }
        }
    });
}

fn spawn_serial_thread(mut port: Box<dyn SerialPort>, tx: Sender<UiEvent>) {
    thread::spawn(move || {
        let mut buf = [0u8; 1024];

        loop {
            match port.read(&mut buf) {
                Ok(0) => continue,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if tx.send(UiEvent::Serial(text)).is_err() {
                        return;
                    }
                }
                Err(ref e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => continue,
                Err(e) => {
                    eprintln!("[serial] read error: {e}");
                    let _ = tx.send(UiEvent::SerialClosed);
                    return;
                }
            }
        }
    });
}

fn write_exact(port: &mut dyn SerialPort, mut buf: &[u8], show_progress: bool) -> io::Result<()> {
    let bar = show_progress.then(|| {
        ProgressBar::new(buf.len() as u64)
            .with_style(ProgressStyle::default_bar()
                .template("{bar:40} {bytes}/{total_bytes} ({eta})")
                .unwrap())
    });

    while !buf.is_empty() {
        match port.write(buf) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "write returned 0")),
            Ok(n) => {
                if let Some(ref bar) = bar { bar.inc(n as u64); }
                buf = &buf[n..];
            }
            Err(ref e) if matches!(e.kind(), io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e),
        }
    }

    if let Some(bar) = bar { bar.finish(); }
    Ok(())
}

fn get_u8(port: &mut dyn SerialPort) -> io::Result<u8> {
    let mut b = [0u8; 1];
    loop {
        match port.read(&mut b) {
            Ok(1) => return Ok(b[0]),
            Ok(_) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "0 bytes read")),
            Err(ref e) if matches!(e.kind(), io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e),
        }
    }
}

fn get_u32(port: &mut dyn SerialPort) -> io::Result<u32> {
    let mut b = [0u8; 4];
    for byte in &mut b { *byte = get_u8(port)?; }
    Ok(u32::from_le_bytes(b))
}

fn put_u32(port: &mut dyn SerialPort, v: u32) -> io::Result<()> {
    write_exact(port, &v.to_le_bytes(), false)
}

fn main() -> io::Result<()> {
    let status = Command::new("cargo").current_dir("../pi").args(["build", "--release"]).status()?;
    if !status.success() { return Err(io::Error::new(io::ErrorKind::Other, "cargo build failed")); }
    let elf = "../pi/target/armv6zk-none-eabihf/release/crab-pi";
    let bin = format!("{elf}.bin");
    let status = Command::new("arm-none-eabi-objcopy").args([elf, "-O", "binary", &bin]).status()?;
    if !status.success() { return Err(io::Error::new(io::ErrorKind::Other, "objcopy failed")); }

    let prog = fs::read(&bin)?;
    let mut hasher = Hasher::new();
    hasher.update(&prog);
    let crc = hasher.finalize();
    eprintln!("[boot] {} bytes, crc={crc:08x}", prog.len());

    let dev = fs::read_dir("/dev")?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|n| n.to_str())
            .map(|n| ["ttyUSB","ttyACM","cu.SLAB_USB","cu.usbserial"].iter().any(|p| n.starts_with(p)))
            .unwrap_or(false))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no tty device found"))?;

    eprintln!("[boot] found device {}", dev.display());

    let mut port = serialport::new(dev.to_str().unwrap(), 115_200)
        .timeout(Duration::from_millis(1000))
        .open()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let mut status = get_u32(&mut *port)?;
    while status != GET_PROG_INFO { 
        eprintln!("Found {status:08x} instead of GET_PROG_INFO({GET_PROG_INFO:08x})");
        status = get_u32(&mut *port)?;
    }

    put_u32(&mut *port, PUT_PROG_INFO)?;
    put_u32(&mut *port, ARMBASE)?;
    put_u32(&mut *port, prog.len() as u32)?;
    put_u32(&mut *port, crc)?;
    while get_u32(&mut *port)? != GET_CODE {}
    if get_u32(&mut *port)? != crc { return Err(io::Error::new(io::ErrorKind::InvalidData, "crc mismatch")); }
    put_u32(&mut *port, PUT_CODE)?;
    write_exact(&mut *port, &prog, true)?;
    match get_u32(&mut *port)? {
        BOOT_SUCCESS => eprintln!("[boot] success"),
        BOOT_ERROR => return Err(io::Error::new(io::ErrorKind::Other, "BOOT_ERROR")),
        v => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unexpected: {v:08x}"))),
    }

    enable_raw_mode().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    terminal.clear().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let result = (|| -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        let input_port = port.try_clone().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        spawn_stdin_thread(input_port, tx.clone());
        spawn_serial_thread(port, tx);

        let mut app = App::default();
        terminal
            .draw(|frame| draw_ui(frame, &app))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        loop {
            match rx.recv() {
                Ok(UiEvent::Serial(chunk)) => app.push_serial(&chunk),
                Ok(UiEvent::Stdin(chunk)) => app.push_stdin(&chunk),
                Ok(UiEvent::SerialClosed) | Err(_) => break,
            }

            terminal
                .draw(|frame| draw_ui(frame, &app))
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            if app.done {
                break;
            }
        }

        app.finish();
        terminal
            .draw(|frame| draw_ui(frame, &app))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    })();

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}