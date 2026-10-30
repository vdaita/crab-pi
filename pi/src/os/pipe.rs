//! Kernel pipes: a fixed pool of ring buffers shared by every program. Each
//! end belongs to one open file description (which fork/dup share), so a
//! reader sees EOF once the write end is released, and a writer gets EPIPE
//! once the read end is.

use core::cell::SyncUnsafeCell;
use crate::os::errno::*;

const PIPE_BUF_SIZE: usize = 4096;
const NUM_PIPES: usize = 16;

struct Pipe {
    buf: [u8; PIPE_BUF_SIZE],
    head: usize,
    len: usize,
    readers: u32,
    writers: u32,
}

impl Pipe {
    const EMPTY: Pipe = Pipe { buf: [0; PIPE_BUF_SIZE], head: 0, len: 0, readers: 0, writers: 0 };

    fn in_use(&self) -> bool {
        self.readers > 0 || self.writers > 0
    }
}

static PIPES: SyncUnsafeCell<[Pipe; NUM_PIPES]> = SyncUnsafeCell::new([Pipe::EMPTY; NUM_PIPES]);

fn pipe(id: usize) -> &'static mut Pipe {
    unsafe { &mut (*PIPES.get())[id] }
}

/// Allocate a pipe with one reader and one writer.
pub fn create() -> Result<usize, Errno> {
    let pipes = unsafe { &mut *PIPES.get() };
    let id = pipes.iter().position(|p| !p.in_use()).ok_or(ENFILE)?;
    pipes[id] = Pipe { readers: 1, writers: 1, ..Pipe::EMPTY };
    Ok(id)
}

pub fn drop_ref(id: usize, write_end: bool) {
    let p = pipe(id);
    if write_end {
        p.writers = p.writers.saturating_sub(1)
    } else {
        p.readers = p.readers.saturating_sub(1)
    }
}

pub fn read(id: usize, out: &mut [u8]) -> SysResult {
    let p = pipe(id);
    if p.len == 0 {
        return if p.writers == 0 { Ok(0) } else { Err(ERESTART) };
    }
    let n = out.len().min(p.len);
    for byte in out[..n].iter_mut() {
        *byte = p.buf[p.head];
        p.head = (p.head + 1) % PIPE_BUF_SIZE;
    }
    p.len -= n;
    Ok(n as u32)
}

pub fn write(id: usize, data: &[u8]) -> SysResult {
    let p = pipe(id);
    if p.readers == 0 {
        return Err(EPIPE);
    }
    let n = data.len().min(PIPE_BUF_SIZE - p.len);
    if n == 0 {
        return Err(ERESTART);
    }
    for &byte in &data[..n] {
        p.buf[(p.head + p.len) % PIPE_BUF_SIZE] = byte;
        p.len += 1;
    }
    Ok(n as u32)
}

/// Bytes waiting to be read (for poll).
pub fn available(id: usize) -> usize {
    pipe(id).len
}
