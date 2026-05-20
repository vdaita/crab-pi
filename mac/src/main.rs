use crc32fast::Hasher;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    style::{Color, Modifier, Style},
    layout::{Constraint, Direction, Layout},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
    Terminal,
};
use serialport::SerialPort;
use std::fs;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::process::Command;
use std::thread;
use std::time::Duration;
use indicatif::{ProgressBar, ProgressStyle};
use regex;

const ARMBASE: u32 = 0x0000_8000;
const GET_PROG_INFO: u32 = 0x11112222;
const PUT_PROG_INFO: u32 = 0x33334444;
const GET_CODE: u32 = 0x55556666;
const PUT_CODE: u32 = 0x77778888;
const BOOT_SUCCESS: u32 = 0x9999aaaa;
const BOOT_ERROR: u32 = 0xbbbbcccc;
const PRINT_STRING: u32 = 0xddddeeee;

#[derive(Clone)]
struct App {
    left: String,
    right: String,
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

fn draw(frame: &mut Frame, app: &App) {
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(frame.size());
    
    let left_h = panes[0].height.saturating_sub(2) as usize;
    let left_text: Vec<Line> = app.left.lines()
        .collect::<Vec<_>>()
        .iter()
        .rev().take(left_h).rev()
        .map(|l| Line::from(*l))
        .collect();
 
    frame.render_widget(
        Paragraph::new(left_text).block(Block::default().title(" output ").borders(Borders::ALL)),
        panes[0],
    );

    let right_h = panes[1].height.saturating_sub(2) as usize;
    let right_text: Vec<Line> = app.right.lines()
        .collect::<Vec<_>>()
        .iter()
        .rev().take(right_h).rev()
        .map(|l| Line::from(*l))
        .collect();
 
    frame.render_widget(
        Paragraph::new(right_text).block(Block::default().title(" program ").borders(Borders::ALL)),
        panes[1],
    );
}

fn print_left(text: &str, app_arc: &Arc<Mutex<App>>, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>)
    -> io::Result<()>
{
    {
        let mut a = app_arc.lock().unwrap();
        a.left.push_str(text);
    }

    // snapshot for drawing to avoid holding the lock while rendering
    let snapshot = {
        let a = app_arc.lock().unwrap();
        a.clone()
    };
    terminal.draw(|f| draw(f, &snapshot))?;
    Ok(())
}

fn print_right(text: &str, app_arc: &Arc<Mutex<App>>, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>)
    -> io::Result<()>
{
    {
        let mut a = app_arc.lock().unwrap();
        a.right.push_str(text);
    }

    let snapshot = {
        let a = app_arc.lock().unwrap();
        a.clone()
    };
    terminal.draw(|f| draw(f, &snapshot))?;
    Ok(())
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
        eprintln!("Found {status:08x} instead of GET_PROG_INFO({GET_PROG_INFO:08x}");
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
    let mut app = Arc::new(Mutex::new(App {
        left: "".to_string(),
        right: "".to_string()
    }));
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    
    let mut input_port = port.try_clone().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let app_user_inputs = Arc::clone(&app);
    thread::spawn(move || -> io::Result<()> {
        let mut buf = [0u8; 256];
        loop {
            let n = io::stdin().read(&mut buf)?;
            if n == 0 { return Ok(()); }

            // append stdin text to right buffer (no terminal redraw from this thread)
            let s = std::str::from_utf8(&buf[..n]).unwrap_or("<non-utf8>");
            {
                let mut a = app_user_inputs.lock().unwrap();
                a.right.push_str("[stdin] ");
                a.right.push_str(s);
            }

            for b in &mut buf[..n] {
                if *b == b'\r' { *b = b'\n'; }
            }
            write_exact(&mut *input_port, &buf[..n], false)?;
        }
    });

    let mut out = io::stdout();
    let mut received: Vec<u8> = Vec::new();
    let mut last_prog_end: usize = 0;

    let re = regex::Regex::new(r"(?s)<prog>(.*?)</prog>").unwrap();
    let app_pi_inputs = Arc::clone(&app);
    loop {
        let mut buf = [0u8; 256];
        match port.read(&mut buf) {
            Ok(0) => continue,
            Err(ref e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => continue,
            Err(e) => { disable_raw_mode().ok(); return Err(e); }
            Ok(n) => {
                for &b in &buf[..n] {
                    if b == b'\n' { out.write_all(b"\r\n")?; }
                    else { out.write_all(&[b])?; }
                }
                out.flush()?;
                received.extend_from_slice(&buf[..n]);

                // print all the content out on the left (so &buf[..n] goes on the left)

                // split read: route non-<prog> text to left, inner <prog> content to right
                let text = std::str::from_utf8(&received).unwrap_or("");
                // iterate matches and route spans
                let mut last = 0usize;
                for m in re.find_iter(text) {
                    let start = m.start();
                    let end = m.end();
                    if start > last {
                        let span = &text[last..start];
                        print_left(span, &app_pi_inputs, &mut terminal)?;
                    }
                    if let Some(caps) = re.captures(&text[start..end]) {
                        if let Some(inner) = caps.get(1) {
                            print_right(inner.as_str(), &app_pi_inputs, &mut terminal)?;
                        }
                    }
                    last = end;
                }
                if last < text.len() {
                    let tail = &text[last..];
                    print_left(tail, &app_pi_inputs, &mut terminal)?;
                }
                last_prog_end = received.len();

                

                if received.windows(7).any(|w| w == b"DONE!!!") {
                    disable_raw_mode().ok();
                    return Ok(());
                }
            }
        }
    }
}