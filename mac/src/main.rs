use crc32fast::Hasher;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use serialport::SerialPort;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;
use indicatif::{ProgressBar, ProgressStyle};

const ARMBASE: u32 = 0x0000_8000;
const GET_PROG_INFO: u32 = 0x11112222;
const PUT_PROG_INFO: u32 = 0x33334444;
const GET_CODE: u32 = 0x55556666;
const PUT_CODE: u32 = 0x77778888;
const BOOT_SUCCESS: u32 = 0x9999aaaa;
const BOOT_ERROR: u32 = 0xbbbbcccc;
const PRINT_STRING: u32 = 0xddddeeee;

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
    let mut port = serialport::new(dev.to_str().unwrap(), 115_200)
        .timeout(Duration::from_millis(1000))
        .open()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    while get_u32(&mut *port)? != GET_PROG_INFO {}
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

    let mut out = io::stdout();
    let mut received: Vec<u8> = Vec::new();
    loop {
        let mut buf = [0u8; 256];
        match port.read(&mut buf) {
            Ok(0) => continue,
            Err(ref e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => continue,
            Err(e) => return Err(e),
            Ok(n) => {
                out.write_all(&buf[..n])?;
                out.flush()?;
                received.extend_from_slice(&buf[..n]);
                if received.windows(7).any(|w| w == b"DONE!!!") {
                    println!("[boot] read program done");
                    return Ok(());
                }
            }
        }
    }
}