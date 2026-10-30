//! Open files and the per-program descriptor table.
//!
//! As in Unix, descriptors point at shared open file descriptions: fork and
//! dup make a second descriptor for the same `KernelFile` (same offset, same
//! buffered contents), which lives in a global table and is released when
//! the last descriptor referring to it closes.
//!
//! Disk files are read whole into a kernel buffer on open and written back
//! on close/fsync if they were modified. Directories hold their entries as
//! pre-serialized `linux_dirent64` records. The console is the UART.

use core::cell::SyncUnsafeCell;
use crate::fat32::{self, pi_dirent_t, Statx};
use crate::kmalloc;
use crate::os::errno::*;
use crate::os::{pipe, vfs};
use crate::os::vfs::Path;
use crate::uart;

pub const NUM_FILE_DESCRIPTORS: usize = 16;
/// open file descriptions, across all programs
const MAX_OPEN_FILES: usize = 64;

// open(2) flags, ARM values
pub const O_ACCMODE: u32 = 0o3;
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DIRECTORY: u32 = 0o40000;
pub const O_CLOEXEC: u32 = 0o2000000;

pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

const S_IFIFO: u16 = 0o010000;
const S_IFCHR: u16 = 0o020000;

const EMPTY_DIRENT: pi_dirent_t = pi_dirent_t { name: [0; 16], raw_name: [0; 16], cluster_id: 0, is_dir_p: 0, nbytes: 0 };

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FileKind {
    Free,
    Console,
    Null,
    Disk,
    Dir,
    PipeRead,
    PipeWrite,
}

#[derive(Copy, Clone)]
pub struct KernelFile {
    pub kind: FileKind,
    /// open(2) flags: access mode, O_APPEND, O_NONBLOCK
    pub flags: u32,
    pub pos: usize,
    /// descriptors referring to this description
    refs: u32,
    /// Disk: file contents; Dir: dirent64 records. Lives on the kernel heap.
    data: *mut u8,
    len: usize,
    cap: usize,
    dirty: bool,
    dirent: pi_dirent_t,
    parent: pi_dirent_t,
    /// where a Disk/Dir file was opened from (for openat and fchdir)
    pub path: Path,
    pipe: usize,
}

impl KernelFile {
    pub const FREE: KernelFile = KernelFile {
        kind: FileKind::Free,
        flags: 0,
        pos: 0,
        refs: 0,
        data: core::ptr::null_mut(),
        len: 0,
        cap: 0,
        dirty: false,
        dirent: EMPTY_DIRENT,
        parent: EMPTY_DIRENT,
        path: Path::ROOT,
        pipe: 0,
    };

    pub fn new(kind: FileKind, flags: u32) -> KernelFile {
        KernelFile { kind, flags: flags & !O_CLOEXEC, ..KernelFile::FREE }
    }

    pub fn open_entry(entry: &vfs::Entry, path: &Path, flags: u32) -> KernelFile {
        let mut f = KernelFile::new(if entry.is_dir() { FileKind::Dir } else { FileKind::Disk }, flags);
        f.dirent = entry.dirent;
        f.parent = entry.parent;
        f.path = *path;
        let contents = if entry.is_dir() { vfs::list_dir(&entry.dirent) } else { vfs::read_file(&entry.dirent) };
        f.data = contents.data;
        f.len = contents.n_data;
        f.cap = contents.n_alloc;
        if f.kind == FileKind::Disk && flags & O_TRUNC != 0 && f.writable() {
            f.len = 0;
            f.dirty = true;
        }
        f
    }

    /// The (read, write) ends of a new pipe.
    pub fn pipe_pair(flags: u32) -> Result<(KernelFile, KernelFile), Errno> {
        let id = pipe::create()?;
        let mut r = KernelFile::new(FileKind::PipeRead, flags | O_RDONLY);
        let mut w = KernelFile::new(FileKind::PipeWrite, flags | O_WRONLY);
        r.pipe = id;
        w.pipe = id;
        Ok((r, w))
    }

    pub fn is_open(&self) -> bool {
        self.kind != FileKind::Free
    }

    pub fn readable(&self) -> bool {
        self.flags & O_ACCMODE != O_WRONLY
    }

    pub fn writable(&self) -> bool {
        self.flags & O_ACCMODE != O_RDONLY
    }

    pub fn is_tty(&self) -> bool {
        self.kind == FileKind::Console
    }

    /// Bytes `read` could return right now without blocking (for poll).
    pub fn ready_to_read(&self) -> bool {
        match self.kind {
            FileKind::PipeRead => pipe::available(self.pipe) > 0,
            _ => true,
        }
    }

    pub fn read(&mut self, buf: &mut [u8]) -> SysResult {
        if !self.readable() {
            return Err(EBADF);
        }
        let n = match self.kind {
            FileKind::Free => return Err(EBADF),
            FileKind::Null => 0,
            FileKind::Dir => return Err(EISDIR),
            FileKind::PipeWrite => return Err(EBADF),
            FileKind::PipeRead => return self.nonblock(pipe::read(self.pipe, buf)),
            FileKind::Console => return self.nonblock(read_console(buf)),
            FileKind::Disk => {
                let n = buf.len().min(self.len.saturating_sub(self.pos));
                if n > 0 {
                    unsafe { core::ptr::copy_nonoverlapping(self.data.add(self.pos), buf.as_mut_ptr(), n) };
                }
                n
            }
        };
        self.pos += n;
        Ok(n as u32)
    }

    pub fn write(&mut self, buf: &[u8]) -> SysResult {
        if !self.writable() {
            return Err(EBADF);
        }
        match self.kind {
            FileKind::Free | FileKind::PipeRead => Err(EBADF),
            FileKind::Dir => Err(EISDIR),
            FileKind::Null => Ok(buf.len() as u32),
            FileKind::PipeWrite => self.nonblock(pipe::write(self.pipe, buf)),
            FileKind::Console => {
                // \x02..\x03 framing lets the mac side split program output from kernel logs
                uart::write_bytes(b"\x02");
                uart::write_bytes(buf);
                uart::write_bytes(b"\x03");
                Ok(buf.len() as u32)
            }
            FileKind::Disk => {
                if self.flags & O_APPEND != 0 {
                    self.pos = self.len;
                }
                let end = self.pos + buf.len();
                self.set_len(end.max(self.len));
                unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), self.data.add(self.pos), buf.len()) };
                self.pos = end;
                Ok(buf.len() as u32)
            }
        }
    }

    pub fn seek(&mut self, offset: i64, whence: u32) -> Result<u64, Errno> {
        let base = match (self.kind, whence) {
            (FileKind::Null, _) => return Ok(0),
            (FileKind::Disk | FileKind::Dir, SEEK_SET) => 0,
            (FileKind::Disk | FileKind::Dir, SEEK_CUR) => self.pos as i64,
            (FileKind::Disk | FileKind::Dir, SEEK_END) => self.len as i64,
            (FileKind::Disk | FileKind::Dir, _) => return Err(EINVAL),
            (FileKind::Free, _) => return Err(EBADF),
            _ => return Err(ESPIPE),
        };
        let pos = base.checked_add(offset).filter(|&p| p >= 0).ok_or(EINVAL)?;
        self.pos = pos as usize;
        Ok(pos as u64)
    }

    pub fn truncate(&mut self, len: usize) -> Result<(), Errno> {
        if self.kind != FileKind::Disk || !self.writable() {
            return Err(EINVAL);
        }
        self.set_len(len);
        Ok(())
    }

    /// Next whole dirent64 records that fit in `buf`, advancing past them.
    pub fn read_dir(&mut self, buf: &mut [u8]) -> SysResult {
        if self.kind != FileKind::Dir {
            return Err(ENOTDIR);
        }
        let mut n = 0;
        while self.pos + n + 19 <= self.len {
            let reclen = unsafe { (self.data.add(self.pos + n + 16) as *const u16).read_unaligned() } as usize;
            if n + reclen > buf.len() {
                break;
            }
            n += reclen;
        }
        if n == 0 && self.pos < self.len {
            return Err(EINVAL); // buffer too small for even one record
        }
        unsafe { core::ptr::copy_nonoverlapping(self.data.add(self.pos), buf.as_mut_ptr(), n) };
        self.pos += n;
        Ok(n as u32)
    }

    /// The file's contents, for mmap.
    pub fn contents(&self) -> Result<&[u8], Errno> {
        if self.kind != FileKind::Disk || self.data.is_null() {
            return if self.kind == FileKind::Disk { Ok(&[]) } else { Err(EINVAL) };
        }
        Ok(unsafe { core::slice::from_raw_parts(self.data, self.len) })
    }

    pub fn stat(&self) -> Statx {
        let mut st = fat32::get_file_stat(&self.dirent);
        match self.kind {
            FileKind::Disk => {
                st.stx_size = self.len as u64;
                st.stx_blocks = (self.len as u64 + 511) / 512;
            }
            FileKind::Console => {
                st.stx_mode = S_IFCHR | 0o620;
                (st.stx_rdev_major, st.stx_rdev_minor) = (5, 1);
            }
            FileKind::Null => {
                st.stx_mode = S_IFCHR | 0o666;
                (st.stx_rdev_major, st.stx_rdev_minor) = (1, 3);
            }
            FileKind::PipeRead | FileKind::PipeWrite => st.stx_mode = S_IFIFO | 0o600,
            FileKind::Dir | FileKind::Free => {}
        }
        st
    }

    /// Write a modified disk file back to the SD card.
    pub fn flush(&mut self) -> Result<(), Errno> {
        if self.kind == FileKind::Disk && self.dirty {
            vfs::write_file(&self.parent, &self.dirent, self.data, self.len)?;
            self.dirty = false;
        }
        Ok(())
    }

    /// Drop a file that never made it into a descriptor table.
    pub fn discard(mut self) {
        self.release();
    }

    fn release(&mut self) {
        let _ = self.flush();
        match self.kind {
            FileKind::PipeRead => pipe::drop_ref(self.pipe, false),
            FileKind::PipeWrite => pipe::drop_ref(self.pipe, true),
            _ => {}
        }
        *self = KernelFile::FREE;
    }

    /// Turn "would block" into EAGAIN for O_NONBLOCK descriptors.
    fn nonblock(&self, r: SysResult) -> SysResult {
        match r {
            Err(ERESTART) if self.flags & O_NONBLOCK != 0 => Err(EAGAIN),
            r => r,
        }
    }

    /// Resize the in-memory contents, zero-filling any growth. The kernel heap
    /// can't free, so an outgrown buffer is simply abandoned.
    fn set_len(&mut self, len: usize) {
        if len > self.cap {
            let cap = len.max(self.cap * 2).max(4096);
            let data = unsafe { kmalloc::kmalloc(cap) };
            if self.len > 0 {
                unsafe { core::ptr::copy_nonoverlapping(self.data, data, self.len) };
            }
            self.data = data;
            self.cap = cap;
        }
        if len > self.len {
            unsafe { core::ptr::write_bytes(self.data.add(self.len), 0, len - self.len) };
        }
        self.len = len;
        self.dirty = true;
    }
}

/// Yield until the user starts typing, then take the rest of the line in one
/// go: the mini UART's FIFO is only 8 bytes deep, so we can't leave and come
/// back mid-line without dropping input.
fn read_console(buf: &mut [u8]) -> SysResult {
    if buf.is_empty() {
        return Ok(0);
    }
    if uart::read_bytes_nonblocking(&mut buf[..1]) == 0 {
        return Err(ERESTART);
    }
    if buf[0] == 0x04 {
        return Ok(0); // ^D
    }
    let mut n = 1;
    while n < buf.len() && buf[n - 1] != b'\n' {
        buf[n] = uart::read_byte();
        n += 1;
    }
    Ok(n as u32)
}

// single core, and only touched from syscalls (which run with IRQs off)
unsafe impl Sync for KernelFile {}

static OPEN_FILES: SyncUnsafeCell<[KernelFile; MAX_OPEN_FILES]> =
    SyncUnsafeCell::new([KernelFile::FREE; MAX_OPEN_FILES]);

fn open_file(id: usize) -> &'static mut KernelFile {
    unsafe { &mut (*OPEN_FILES.get())[id] }
}

fn retain(id: usize) {
    open_file(id).refs += 1;
}

fn put(id: usize) {
    let file = open_file(id);
    file.refs -= 1;
    if file.refs == 0 {
        file.release();
    }
}

/// One slot in a descriptor table. All-zero (as in a freshly zeroed Program) is closed.
#[derive(Copy, Clone)]
struct Fd {
    /// index into OPEN_FILES, plus one; 0 = closed
    file: u8,
    cloexec: bool,
}

impl Fd {
    const CLOSED: Fd = Fd { file: 0, cloexec: false };

    fn id(&self) -> Option<usize> {
        (self.file != 0).then(|| self.file as usize - 1)
    }
}

#[derive(Copy, Clone)]
pub struct FdTable {
    fds: [Fd; NUM_FILE_DESCRIPTORS],
}

impl FdTable {
    /// 0, 1 and 2 on one shared console description, like a terminal.
    pub fn init_stdio(&mut self) {
        self.close_all();
        let _ = self.install(KernelFile::new(FileKind::Console, O_RDWR), false);
        let _ = self.dup(0, 1, false);
        let _ = self.dup(0, 2, false);
    }

    fn slot(&mut self, fd: u32) -> Result<&mut Fd, Errno> {
        match self.fds.get_mut(fd as usize) {
            Some(slot) if slot.file != 0 => Ok(slot),
            _ => Err(EBADF),
        }
    }

    fn lowest_free(&self, min_fd: usize) -> Result<usize, Errno> {
        (min_fd..NUM_FILE_DESCRIPTORS).find(|&fd| self.fds[fd].file == 0).ok_or(EMFILE)
    }

    pub fn get(&mut self, fd: u32) -> Result<&'static mut KernelFile, Errno> {
        let id = self.slot(fd)?.id().unwrap();
        Ok(open_file(id))
    }

    pub fn cloexec(&mut self, fd: u32) -> Result<bool, Errno> {
        Ok(self.slot(fd)?.cloexec)
    }

    pub fn set_cloexec(&mut self, fd: u32, on: bool) -> Result<(), Errno> {
        self.slot(fd)?.cloexec = on;
        Ok(())
    }

    /// Make `file` a new open file description, at the lowest free descriptor.
    pub fn install(&mut self, mut file: KernelFile, cloexec: bool) -> SysResult {
        let table = unsafe { &mut *OPEN_FILES.get() };
        let slot = self.lowest_free(0).and_then(|fd| {
            let id = table.iter().position(|f| !f.is_open()).ok_or(ENFILE)?;
            Ok((fd, id))
        });
        let (fd, id) = match slot {
            Ok(slot) => slot,
            Err(e) => {
                file.release();
                return Err(e);
            }
        };
        table[id] = KernelFile { refs: 1, ..file };
        self.fds[fd] = Fd { file: id as u8 + 1, cloexec };
        Ok(fd as u32)
    }

    /// A new descriptor, at or above `min_fd`, for the same description as `old`.
    pub fn dup(&mut self, old: u32, min_fd: usize, cloexec: bool) -> SysResult {
        let file = self.slot(old)?.file;
        let fd = self.lowest_free(min_fd)?;
        retain(file as usize - 1);
        self.fds[fd] = Fd { file, cloexec };
        Ok(fd as u32)
    }

    /// Make `new` refer to `old`'s description, closing whatever `new` was (dup2).
    pub fn dup_to(&mut self, old: u32, new: u32, cloexec: bool) -> SysResult {
        let file = self.slot(old)?.file;
        if new as usize >= NUM_FILE_DESCRIPTORS {
            return Err(EBADF);
        }
        retain(file as usize - 1);
        let _ = self.close(new);
        self.fds[new as usize] = Fd { file, cloexec };
        Ok(new)
    }

    pub fn close(&mut self, fd: u32) -> SysResult {
        let slot = self.slot(fd)?;
        let id = slot.id().unwrap();
        *slot = Fd::CLOSED;
        put(id);
        Ok(0)
    }

    pub fn close_all(&mut self) {
        for fd in 0..NUM_FILE_DESCRIPTORS as u32 {
            let _ = self.close(fd);
        }
    }

    pub fn close_on_exec(&mut self) {
        for fd in 0..NUM_FILE_DESCRIPTORS {
            if self.fds[fd].cloexec {
                let _ = self.close(fd as u32);
            }
        }
    }

    /// After fork copied this table, count the copies as references.
    pub fn retain_all(&self) {
        self.fds.iter().filter_map(Fd::id).for_each(retain);
    }
}
