use crc32fast::Hasher;
use indicatif::{ProgressBar, ProgressStyle};
use serialport::SerialPort;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

const ARMBASE: u32 = 0x1000_8000;
const GET_PROG_INFO: u32 = 0x11112222;
const PUT_PROG_INFO: u32 = 0x33334444;
const GET_CODE: u32 = 0x55556666;
const PUT_CODE: u32 = 0x77778888;
const BOOT_SUCCESS: u32 = 0x9999aaaa;
const BOOT_ERROR: u32 = 0xbbbbcccc;
const PRINT_STRING: u32 = 0xddddeeee;

const PROG_OPEN: char = '\x02';  // STX - Start of Text
const PROG_CLOSE: char = '\x03'; // ETX - End of Text

enum UiEvent {
    Serial(String),
    Stdin(String),
    SerialClosed,
}

#[derive(Default)]
struct SerialSplitState {
    in_prog: bool,
}

impl SerialSplitState {
    fn consume(&mut self, chunk: &str) -> (String, String) {
        let mut stdout_buf = String::new();
        let mut log_buf = String::new();
        
        for ch in chunk.chars() {
            if ch == PROG_OPEN {
                self.in_prog = true;
                log_buf.push(ch);
            } else if ch == PROG_CLOSE {
                self.in_prog = false;
                log_buf.push(ch);
            } else {
                if self.in_prog {
                    stdout_buf.push(ch);
                }
                log_buf.push(ch);
            }
        }
        
        (stdout_buf, log_buf)
    }
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

fn handle_print_string(port: &mut dyn SerialPort) -> io::Result<()> {
    let nbytes = get_u32(port)? as usize;
    if nbytes == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "PRINT_STRING with zero bytes"));
    }
    if nbytes > 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "PRINT_STRING too long"));
    }

    let mut buf = vec![0u8; nbytes];
    for byte in &mut buf {
        *byte = get_u8(port)?;
    }

    if buf.last() == Some(&b'\n') {
        buf.pop();
    }

    let mut out = io::stdout();
    out.write_all(&buf)?;
    out.flush()?;
    Ok(())
}

fn read_boot_op(port: &mut dyn SerialPort) -> io::Result<u32> {
    loop {
        let op = get_u32(port)?;
        if op != PRINT_STRING {
            return Ok(op);
        }

        handle_print_string(port)?;
    }
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

    eprintln!("[boot] opened port");

    let mut status = read_boot_op(&mut *port)?;
    
    while status != GET_PROG_INFO { 
        eprintln!("[boot] found {status:08x} instead of GET_PROG_INFO({GET_PROG_INFO:08x})");
        status = read_boot_op(&mut *port)?;
    }

    eprintln!("[boot] writing program");

    put_u32(&mut *port, PUT_PROG_INFO)?;
    put_u32(&mut *port, ARMBASE)?;
    put_u32(&mut *port, prog.len() as u32)?;
    put_u32(&mut *port, crc)?;

    status = read_boot_op(&mut *port)?;
    while status != GET_CODE {
        eprintln!("[boot] found {status:08x} instead of GET_CODE({GET_CODE:08x})");
        status = read_boot_op(&mut *port)?;
    }
    if read_boot_op(&mut *port)? != crc { return Err(io::Error::new(io::ErrorKind::InvalidData, "crc mismatch")); }
    put_u32(&mut *port, PUT_CODE)?;
    write_exact(&mut *port, &prog, true)?;

    match read_boot_op(&mut *port)? {
        BOOT_SUCCESS => eprintln!("[boot] success"),
        BOOT_ERROR => return Err(io::Error::new(io::ErrorKind::Other, "BOOT_ERROR")),
        v => return Err(io::Error::new(io::ErrorKind::InvalidData, format!("unexpected: {v:08x}"))),
    }

    let (tx, rx) = mpsc::channel();
    let input_port = port.try_clone().map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    spawn_stdin_thread(input_port, tx.clone());
    spawn_serial_thread(port, tx);

    let mut log_file = File::create("pi.log")?;
    let mut split = SerialSplitState::default();

    loop {
        match rx.recv() {
            Ok(UiEvent::Serial(chunk)) => {
                let (stdout_text, log_text) = split.consume(&chunk);
                print!("{}", stdout_text);
                io::stdout().flush().ok();
                write!(log_file, "{}", log_text).ok();
                log_file.flush().ok();
            }
            Ok(UiEvent::Stdin(chunk)) => {
                writeln!(log_file, "[stdin] {}", chunk.trim_end()).ok();
                log_file.flush().ok();
            }
            Ok(UiEvent::SerialClosed) | Err(_) => break,
        }
    }

    log_file.flush().ok();

    Ok(())
}