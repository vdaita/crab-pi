//! File and directory syscalls.

use super::{time, uaccess};
use crate::fat32::{self, Statx};
use crate::os::errno::*;
use crate::os::file::*;
use crate::os::holder::OSHolder;
use crate::os::interrupts::InterruptFrame;
use crate::os::vfs::{self, Path};

type Frame = InterruptFrame;

const AT_FDCWD: u32 = -100i32 as u32;
const AT_REMOVEDIR: u32 = 0x200;
const AT_EMPTY_PATH: u32 = 0x1000;

fn fds(h: &mut OSHolder) -> &'static mut FdTable {
    &mut h.current().fds
}

fn file(h: &mut OSHolder, fd: u32) -> Result<&'static mut KernelFile, Errno> {
    fds(h).get(fd)
}

/// Resolve a path argument, relative to `dirfd` (or the cwd for AT_FDCWD).
fn path_at(h: &mut OSHolder, dirfd: u32, path: &str) -> Result<Path, Errno> {
    let base = if dirfd == AT_FDCWD || path.starts_with('/') {
        h.current().cwd
    } else {
        let dir = file(h, dirfd)?;
        if dir.kind != FileKind::Dir {
            return Err(ENOTDIR);
        }
        dir.path
    };
    Path::resolve(&base, path)
}

fn user_path_at(h: &mut OSHolder, dirfd: u32, va: u32) -> Result<Path, Errno> {
    path_at(h, dirfd, uaccess::cstr(va)?)
}

/// Device files that don't exist on the SD card.
fn device(path: &Path, flags: u32) -> Option<KernelFile> {
    match path.as_str() {
        "/dev/null" => Some(KernelFile::new(FileKind::Null, flags)),
        "/dev/tty" | "/dev/console" => Some(KernelFile::new(FileKind::Console, flags)),
        _ => None,
    }
}

// ---- open / close ----------------------------------------------------------

fn open_at(h: &mut OSHolder, dirfd: u32, path_va: u32, flags: u32) -> SysResult {
    let path = user_path_at(h, dirfd, path_va)?;
    let file = match device(&path, flags) {
        Some(dev) => dev,
        None => {
            let entry = match vfs::lookup(&path) {
                Ok(_) if flags & O_CREAT != 0 && flags & O_EXCL != 0 => return Err(EEXIST),
                Ok(entry) => entry,
                Err(ENOENT) if flags & O_CREAT != 0 => vfs::create(&path, false)?,
                Err(e) => return Err(e),
            };
            if entry.is_dir() && flags & O_ACCMODE != O_RDONLY {
                return Err(EISDIR);
            }
            if !entry.is_dir() && flags & O_DIRECTORY != 0 {
                return Err(ENOTDIR);
            }
            KernelFile::open_entry(&entry, &path, flags)
        }
    };
    fds(h).install(file, flags & O_CLOEXEC != 0)
}

pub fn open(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    open_at(h, AT_FDCWD, f.r0, f.r1)
}

pub fn openat(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    open_at(h, f.r0, f.r1, f.r2)
}

pub fn creat(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    open_at(h, AT_FDCWD, f.r0, O_CREAT | O_WRONLY | O_TRUNC)
}

pub fn close(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    fds(h).close(f.r0)
}

// ---- read / write ----------------------------------------------------------

pub fn read(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let buf = uaccess::slice_mut(f.r1, f.r2 as usize)?;
    file(h, f.r0)?.read(buf)
}

pub fn write(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let buf = uaccess::slice(f.r1, f.r2 as usize)?;
    file(h, f.r0)?.write(buf)
}

/// readv/writev: run `op` over each iovec until one comes up short.
fn vectored(h: &mut OSHolder, f: &Frame, op: fn(&mut KernelFile, u32, usize) -> SysResult) -> SysResult {
    let (fd, iov, count) = (f.r0, f.r1, f.r2);
    let mut total = 0;
    for i in 0..count {
        let base: u32 = uaccess::read(iov + i * 8)?;
        let len: u32 = uaccess::read(iov + i * 8 + 4)?;
        if len == 0 {
            continue;
        }
        match op(file(h, fd)?, base, len as usize) {
            Ok(n) => {
                total += n;
                if n < len {
                    break;
                }
            }
            Err(e) if total == 0 => return Err(e),
            Err(_) => break,
        }
    }
    Ok(total)
}

pub fn readv(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    vectored(h, f, |file, base, len| file.read(uaccess::slice_mut(base, len)?))
}

pub fn writev(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    vectored(h, f, |file, base, len| file.write(uaccess::slice(base, len)?))
}

/// Run `op` at `offset` without moving the file position (pread/pwrite).
fn at_offset(file: &mut KernelFile, offset: i64, op: impl FnOnce(&mut KernelFile) -> SysResult) -> SysResult {
    let saved = file.pos;
    file.seek(offset, SEEK_SET)?;
    let result = op(file);
    file.pos = saved;
    result
}

// pread64/pwrite64(fd, buf, count, pad, off_lo, off_hi): the 64-bit offset is register-pair aligned
pub fn pread64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let buf = uaccess::slice_mut(f.r1, f.r2 as usize)?;
    at_offset(file(h, f.r0)?, join64(f.r4, f.r5), |file| file.read(buf))
}

pub fn pwrite64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let buf = uaccess::slice(f.r1, f.r2 as usize)?;
    at_offset(file(h, f.r0)?, join64(f.r4, f.r5), |file| file.write(buf))
}

fn join64(lo: u32, hi: u32) -> i64 {
    ((hi as u64) << 32 | lo as u64) as i64
}

pub fn lseek(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let pos = file(h, f.r0)?.seek(f.r1 as i32 as i64, f.r2)?;
    Ok(pos as u32)
}

// _llseek(fd, offset_hi, offset_lo, result*, whence)
pub fn llseek(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let pos = file(h, f.r0)?.seek(join64(f.r2, f.r1), f.r4)?;
    uaccess::write(f.r3, pos)?;
    Ok(0)
}

pub fn getdents64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let buf = uaccess::slice_mut(f.r1, f.r2 as usize)?;
    file(h, f.r0)?.read_dir(buf)
}

pub fn fsync(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    file(h, f.r0)?.flush()?;
    Ok(0)
}

pub fn ftruncate(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    file(h, f.r0)?.truncate(f.r1 as usize)?;
    Ok(0)
}

// ftruncate64(fd, pad, len_lo, len_hi)
pub fn ftruncate64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    file(h, f.r0)?.truncate(join64(f.r2, f.r3) as usize)?;
    Ok(0)
}

// truncate64(path, pad, len_lo, len_hi)
pub fn truncate64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let path = user_path_at(h, AT_FDCWD, f.r0)?;
    let entry = vfs::lookup(&path)?;
    if entry.is_dir() {
        return Err(EISDIR);
    }
    let mut file = KernelFile::open_entry(&entry, &path, O_RDWR);
    file.truncate(join64(f.r2, f.r3) as usize)?;
    file.flush()?;
    Ok(0)
}

// ---- descriptors -----------------------------------------------------------

pub fn dup(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    fds(h).dup(f.r0, 0, false)
}

pub fn dup2(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    if f.r0 == f.r1 {
        file(h, f.r0)?;
        return Ok(f.r1);
    }
    fds(h).dup_to(f.r0, f.r1, false)
}

pub fn dup3(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    if f.r0 == f.r1 {
        return Err(EINVAL);
    }
    fds(h).dup_to(f.r0, f.r1, f.r2 & O_CLOEXEC != 0)
}

pub fn fcntl(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    const F_DUPFD: u32 = 0;
    const F_GETFD: u32 = 1;
    const F_SETFD: u32 = 2;
    const F_GETFL: u32 = 3;
    const F_SETFL: u32 = 4;
    const F_DUPFD_CLOEXEC: u32 = 1030;
    const SETTABLE: u32 = O_APPEND | O_NONBLOCK;

    let (fd, cmd, arg) = (f.r0, f.r1, f.r2);
    let file = file(h, fd)?;
    match cmd {
        F_DUPFD | F_DUPFD_CLOEXEC => fds(h).dup(fd, arg as usize, cmd == F_DUPFD_CLOEXEC),
        F_GETFD => Ok(fds(h).cloexec(fd)? as u32),
        F_SETFD => {
            fds(h).set_cloexec(fd, arg & 1 != 0)?;
            Ok(0)
        }
        F_GETFL => Ok(file.flags),
        F_SETFL => {
            file.flags = (file.flags & !SETTABLE) | (arg & SETTABLE);
            Ok(0)
        }
        5..=7 | 12..=14 => Ok(0), // record locks: nothing else can contend for them
        _ => Err(EINVAL),
    }
}

pub fn ioctl(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    const TCGETS: u32 = 0x5401;
    const TCSETS: u32 = 0x5402;
    const TCSETSF: u32 = 0x5404;
    const TIOCGPGRP: u32 = 0x540f;
    const TIOCSPGRP: u32 = 0x5410;
    const TIOCGWINSZ: u32 = 0x5413;
    const TIOCSWINSZ: u32 = 0x5414;
    const FIONREAD: u32 = 0x541b;
    const FIONBIO: u32 = 0x5421;
    const FIONCLEX: u32 = 0x5450;
    const FIOCLEX: u32 = 0x5451;

    let (fd, req, arg) = (f.r0, f.r1, f.r2);
    let pid = h.pid();
    let file = file(h, fd)?;
    match req {
        FIOCLEX | FIONCLEX => {
            fds(h).set_cloexec(fd, req == FIOCLEX)?;
            Ok(0)
        }
        FIONBIO => {
            let on: u32 = uaccess::read(arg)?;
            file.flags = if on != 0 { file.flags | O_NONBLOCK } else { file.flags & !O_NONBLOCK };
            Ok(0)
        }
        FIONREAD => {
            uaccess::write(arg, file.ready_to_read() as u32)?;
            Ok(0)
        }
        _ if !file.is_tty() => Err(ENOTTY),
        TCGETS => {
            uaccess::write(arg, Termios::CONSOLE)?;
            Ok(0)
        }
        TCSETS..=TCSETSF | TIOCSWINSZ | TIOCSPGRP => Ok(0),
        TIOCGWINSZ => {
            uaccess::write(arg, [24u16, 80, 0, 0])?; // rows, cols, xpixel, ypixel
            Ok(0)
        }
        TIOCGPGRP => {
            uaccess::write(arg, pid)?;
            Ok(0)
        }
        _ => Err(ENOTTY),
    }
}

#[repr(C)]
struct Termios {
    iflag: u32,
    oflag: u32,
    cflag: u32,
    lflag: u32,
    line: u8,
    cc: [u8; 19],
}

impl Termios {
    /// What a cooked 38400-baud serial console reports.
    const CONSOLE: Termios = Termios {
        iflag: 0o400,              // ICRNL
        oflag: 0o5,                // OPOST | ONLCR
        cflag: 0o17 | 0o60 | 0o200, // B38400 | CS8 | CREAD
        lflag: 0o1 | 0o2 | 0o10 | 0o20 | 0o40 | 0o100000, // ISIG ICANON ECHO ECHOE ECHOK IEXTEN
        line: 0,
        // VINTR VQUIT VERASE VKILL VEOF VTIME VMIN ...
        cc: [3, 0x1c, 0x7f, 0x15, 4, 0, 1, 0, 0x11, 0x13, 0x1a, 0, 0x12, 0x0f, 0x17, 0x16, 0, 0, 0],
    };
}

pub fn pipe(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    make_pipe(h, f.r0, 0)
}

pub fn pipe2(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    make_pipe(h, f.r0, f.r1 & (O_NONBLOCK | O_CLOEXEC))
}

fn make_pipe(h: &mut OSHolder, out: u32, flags: u32) -> SysResult {
    let (r, w) = KernelFile::pipe_pair(flags)?;
    let cloexec = flags & O_CLOEXEC != 0;
    let fds = fds(h);
    let rfd = match fds.install(r, cloexec) {
        Ok(fd) => fd,
        Err(e) => {
            w.discard();
            return Err(e);
        }
    };
    let wfd = match fds.install(w, cloexec) {
        Ok(fd) => fd,
        Err(e) => {
            let _ = fds.close(rfd);
            return Err(e);
        }
    };
    if let Err(e) = uaccess::write(out, [rfd, wfd]) {
        let _ = fds.close(rfd);
        let _ = fds.close(wfd);
        return Err(e);
    }
    Ok(0)
}

// ---- poll / select ---------------------------------------------------------

/// Nothing ready yet: give up immediately, at `deadline`, or never.
fn wait_for_ready(h: &mut OSHolder, timeout_us: Option<u64>) -> SysResult {
    match timeout_us {
        Some(0) => Ok(0),
        Some(us) => time::sleep_until(h, time::now_us() + us),
        None => Err(ERESTART),
    }
}

fn do_poll(h: &mut OSHolder, fds_va: u32, nfds: u32, timeout_us: Option<u64>) -> SysResult {
    const POLLIN: u16 = 0x1;
    const POLLOUT: u16 = 0x4;
    const POLLNVAL: u16 = 0x20;

    let mut ready = 0;
    for i in 0..nfds {
        let entry = fds_va + i * 8; // struct pollfd { int fd; short events; short revents; }
        let fd: i32 = uaccess::read(entry)?;
        let events: u16 = uaccess::read(entry + 4)?;
        let revents = if fd < 0 {
            0
        } else {
            match file(h, fd as u32) {
                Err(_) => POLLNVAL,
                Ok(file) => (events & POLLOUT) | if file.ready_to_read() { events & POLLIN } else { 0 },
            }
        };
        uaccess::write(entry + 6, revents)?;
        ready += (revents != 0) as u32;
    }
    if ready > 0 {
        h.current().wake_at = 0;
        return Ok(ready);
    }
    wait_for_ready(h, timeout_us)
}

pub fn poll(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let ms = f.r2 as i32;
    do_poll(h, f.r0, f.r1, (ms >= 0).then(|| ms as u64 * 1000))
}

pub fn ppoll(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let timeout = time::read_timeout(f.r2, false)?;
    do_poll(h, f.r0, f.r1, timeout)
}

pub fn ppoll_time64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let timeout = time::read_timeout(f.r2, true)?;
    do_poll(h, f.r0, f.r1, timeout)
}

/// select over fd_sets; every descriptor is always writable, and readable
/// unless it's an empty pipe.
fn do_select(h: &mut OSHolder, n: u32, rset: u32, wset: u32, eset: u32, timeout_us: Option<u64>) -> SysResult {
    // NUM_FILE_DESCRIPTORS <= 32, so only the first word of each set matters
    let n = n.min(NUM_FILE_DESCRIPTORS as u32);
    let mut rbits: u32 = if rset != 0 { uaccess::read(rset)? } else { 0 };
    let wbits: u32 = if wset != 0 { uaccess::read(wset)? } else { 0 };
    let mut ready = 0;
    for fd in 0..n {
        let bit = 1 << fd;
        if (rbits | wbits) & bit == 0 {
            continue;
        }
        let file = file(h, fd)?;
        if rbits & bit != 0 {
            if file.ready_to_read() { ready += 1 } else { rbits &= !bit }
        }
        if wbits & bit != 0 {
            ready += 1;
        }
    }
    if ready == 0 {
        // returns before touching the sets, so a restart sees the same request
        wait_for_ready(h, timeout_us)?;
    } else {
        h.current().wake_at = 0;
    }
    uaccess::write_opt(rset, rbits)?;
    uaccess::write_opt(wset, wbits)?;
    uaccess::write_opt(eset, 0u32)?;
    Ok(ready)
}

// _newselect(n, rset, wset, eset, struct timeval *timeout)
pub fn newselect(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let timeout = if f.r4 == 0 {
        None
    } else {
        let [sec, usec]: [i32; 2] = uaccess::read(f.r4)?;
        Some(sec.max(0) as u64 * 1_000_000 + usec.max(0) as u64)
    };
    do_select(h, f.r0, f.r1, f.r2, f.r3, timeout)
}

pub fn pselect6(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let timeout = time::read_timeout(f.r4, false)?;
    do_select(h, f.r0, f.r1, f.r2, f.r3, timeout)
}

pub fn pselect6_time64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let timeout = time::read_timeout(f.r4, true)?;
    do_select(h, f.r0, f.r1, f.r2, f.r3, timeout)
}

// ---- stat ------------------------------------------------------------------

fn stat_path(h: &mut OSHolder, dirfd: u32, path_va: u32, flags: u32) -> Result<Statx, Errno> {
    let s = uaccess::cstr(path_va)?;
    if s.is_empty() {
        if flags & AT_EMPTY_PATH == 0 {
            return Err(ENOENT);
        }
        if dirfd != AT_FDCWD {
            return Ok(file(h, dirfd)?.stat());
        }
    }
    let path = path_at(h, dirfd, if s.is_empty() { "." } else { s })?;
    if let Some(dev) = device(&path, O_RDONLY) {
        return Ok(dev.stat());
    }
    Ok(fat32::get_file_stat(&vfs::lookup(&path)?.dirent))
}

fn write_stat64(va: u32, st: &Statx) -> SysResult {
    uaccess::write(va, fat32::stat64_from_statx(st))?;
    Ok(0)
}

// stat64/lstat64(path, buf): there are no symlinks, so lstat is stat
pub fn stat64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let st = stat_path(h, AT_FDCWD, f.r0, 0)?;
    write_stat64(f.r1, &st)
}

pub fn fstat64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let st = file(h, f.r0)?.stat();
    write_stat64(f.r1, &st)
}

// fstatat64(dirfd, path, buf, flags)
pub fn fstatat64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let st = stat_path(h, f.r0, f.r1, f.r3)?;
    write_stat64(f.r2, &st)
}

// statx(dirfd, path, flags, mask, buf)
pub fn statx(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let st = stat_path(h, f.r0, f.r1, f.r2)?;
    uaccess::write(f.r4, st)?;
    Ok(0)
}

// access/faccessat: everything is readable, writable and executable by root
pub fn access(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    stat_path(h, AT_FDCWD, f.r0, 0)?;
    Ok(0)
}

pub fn faccessat(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    stat_path(h, f.r0, f.r1, f.r3)?;
    Ok(0)
}

/// chmod, chown, utimensat and friends: FAT32 has nowhere to store any of
/// it, so just check that the file exists.
pub fn attr_path(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    stat_path(h, AT_FDCWD, f.r0, 0)?;
    Ok(0)
}

pub fn attr_path_at(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    if f.r1 == 0 {
        file(h, f.r0)?; // utimensat(fd, NULL, ...) is futimens
        return Ok(0);
    }
    stat_path(h, f.r0, f.r1, 0)?;
    Ok(0)
}

pub fn attr_fd(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    file(h, f.r0)?;
    Ok(0)
}

// ---- directories and names -------------------------------------------------

pub fn getcwd(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let cwd = h.current().cwd;
    let len = cwd.as_str().len() + 1;
    if (f.r1 as usize) < len {
        return Err(ERANGE);
    }
    let buf = uaccess::slice_mut(f.r0, len)?;
    buf[..len - 1].copy_from_slice(cwd.as_str().as_bytes());
    buf[len - 1] = 0;
    Ok(len as u32)
}

pub fn chdir(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let path = user_path_at(h, AT_FDCWD, f.r0)?;
    if !vfs::lookup(&path)?.is_dir() {
        return Err(ENOTDIR);
    }
    h.current().cwd = path;
    Ok(0)
}

pub fn fchdir(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let dir = file(h, f.r0)?;
    if dir.kind != FileKind::Dir {
        return Err(ENOTDIR);
    }
    h.current().cwd = dir.path;
    Ok(0)
}

fn mkdir_at(h: &mut OSHolder, dirfd: u32, path_va: u32) -> SysResult {
    let path = user_path_at(h, dirfd, path_va)?;
    if vfs::lookup(&path).is_ok() {
        return Err(EEXIST);
    }
    vfs::create(&path, true)?;
    Ok(0)
}

pub fn mkdir(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    mkdir_at(h, AT_FDCWD, f.r0)
}

pub fn mkdirat(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    mkdir_at(h, f.r0, f.r1)
}

pub fn unlink(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    vfs::remove(&user_path_at(h, AT_FDCWD, f.r0)?, false)?;
    Ok(0)
}

pub fn rmdir(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    vfs::remove(&user_path_at(h, AT_FDCWD, f.r0)?, true)?;
    Ok(0)
}

pub fn unlinkat(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    vfs::remove(&user_path_at(h, f.r0, f.r1)?, f.r2 & AT_REMOVEDIR != 0)?;
    Ok(0)
}

pub fn rename(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let from = user_path_at(h, AT_FDCWD, f.r0)?;
    let to = user_path_at(h, AT_FDCWD, f.r1)?;
    vfs::rename(&from, &to)?;
    Ok(0)
}

// renameat/renameat2(olddirfd, old, newdirfd, new[, flags])
pub fn renameat(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let from = user_path_at(h, f.r0, f.r1)?;
    let to = user_path_at(h, f.r2, f.r3)?;
    vfs::rename(&from, &to)?;
    Ok(0)
}

fn readlink_at(h: &mut OSHolder, dirfd: u32, path_va: u32, buf: u32, size: u32) -> SysResult {
    let path = user_path_at(h, dirfd, path_va)?;
    if path.as_str() == "/proc/self/exe" {
        let exe = h.current().exe;
        let n = exe.as_str().len().min(size as usize);
        uaccess::slice_mut(buf, n)?.copy_from_slice(&exe.as_str().as_bytes()[..n]);
        return Ok(n as u32);
    }
    vfs::lookup(&path)?;
    Err(EINVAL) // exists, but isn't a symlink
}

pub fn readlink(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    readlink_at(h, AT_FDCWD, f.r0, f.r1, f.r2)
}

pub fn readlinkat(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    readlink_at(h, f.r0, f.r1, f.r2, f.r3)
}

pub fn umask(_h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Ok(0o022)
}
