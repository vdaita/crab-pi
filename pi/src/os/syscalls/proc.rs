//! Process lifecycle (fork/exec/exit/wait), ids, and signals.
//!
//! A pid is the program's slot index + 1. Signals are never delivered to a
//! handler: kill either terminates the target or is ignored, according to
//! the default action and whether the program set SIG_IGN.

use core::cell::SyncUnsafeCell;
use super::uaccess;
use crate::os::errno::*;
use crate::os::holder::{self, OSHolder, Program, NUM_PROGRAMS};
use crate::os::interrupts::InterruptFrame;
use crate::os::vfs::{self, Path};
use crate::println;

type Frame = InterruptFrame;

// ---- exit / wait -----------------------------------------------------------

/// End program `index` with a wait4-style `status`, waking its parent's wait4.
/// If nothing is left running, this returns to whoever called `run_elf`.
fn terminate(h: &mut OSHolder, index: usize, status: u32) {
    let program = unsafe { h.get_program_mut(index) };
    if index == h.current_program && program.clear_child_tid != 0 {
        let _ = uaccess::write(program.clear_child_tid, 0u32);
    }
    program.fds.close_all();

    h.active[index] = false;
    h.exit_status[index] = status;
    h.zombie[index] = h.parent[index].is_some_and(|p| h.active[p]);
    // orphans: nobody will reap them, so they're freed as soon as they exit
    for child in 0..NUM_PROGRAMS {
        if h.parent[child] == Some(index) {
            h.parent[child] = None;
            h.zombie[child] = false;
        }
    }
    if index == h.current_program {
        h.should_cswitch = true;
    }

    if !h.active.iter().any(|&a| a) {
        println!("\x02 all ELF finished, exiting \x03");
        unsafe { holder::elf_loader_return(program.return_sp, program.return_lr) };
    }
}

pub fn exit(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    terminate(h, h.current_program, (f.r0 & 0xff) << 8);
    Ok(0)
}

// wait4(pid, status*, options, rusage*)
pub fn wait4(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    const WNOHANG: u32 = 1;
    let (pid, status, options, rusage) = (f.r0 as i32, f.r1, f.r2, f.r3);

    let me = h.current_program;
    let mut waiting_on_live_child = false;
    for child in 0..NUM_PROGRAMS {
        // pid <= 0 (any child / process groups) all mean "any child" here
        if h.parent[child] != Some(me) || (pid > 0 && child != pid as usize - 1) {
            continue;
        }
        if h.zombie[child] {
            uaccess::write_opt(status, h.exit_status[child])?;
            if rusage != 0 {
                uaccess::zero(rusage, 72)?;
            }
            h.zombie[child] = false;
            h.parent[child] = None;
            return Ok(child as u32 + 1);
        }
        waiting_on_live_child |= h.active[child];
    }

    match (waiting_on_live_child, options & WNOHANG != 0) {
        (false, _) => Err(ECHILD),
        (true, true) => Ok(0),
        (true, false) => Err(ERESTART),
    }
}

// ---- fork / clone ----------------------------------------------------------

/// Copy the current program into a free slot. The child resumes from the
/// same syscall with a return value of 0.
fn fork_current(h: &mut OSHolder) -> Result<usize, Errno> {
    let child = h.free_slot().ok_or(EAGAIN)?;
    unsafe { core::ptr::copy_nonoverlapping(h.programs[h.current_program], h.programs[child], 1) };

    let c = unsafe { h.get_program_mut(child) };
    c.fds.retain_all();
    c.frame.r0 = 0;
    c.clear_child_tid = 0;

    h.active[child] = true;
    h.zombie[child] = false;
    h.parent[child] = Some(h.current_program);
    Ok(child)
}

pub fn fork(h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Ok(fork_current(h)? as u32 + 1)
}

// clone(flags, stack, parent_tid*, tls, child_tid*): processes only. Without
// shared memory, CLONE_VM spawns (posix_spawn, vfork-style) still work since
// the child gets its own copy of everything the parent set up for it.
pub fn clone(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    const CLONE_THREAD: u32 = 0x10000;
    const CLONE_SETTLS: u32 = 0x80000;
    const CLONE_PARENT_SETTID: u32 = 0x100000;
    const CLONE_CHILD_CLEARTID: u32 = 0x200000;

    let (flags, stack, parent_tid, tls, child_tid) = (f.r0, f.r1, f.r2, f.r3, f.r4);
    if flags & CLONE_THREAD != 0 {
        return Err(ENOSYS); // no threads: pthread_create reports EAGAIN
    }

    let child = fork_current(h)?;
    let pid = child as u32 + 1;
    let c = unsafe { h.get_program_mut(child) };
    if stack != 0 {
        c.sp = stack as usize;
    }
    if flags & CLONE_SETTLS != 0 {
        c.thread_pointer = tls;
    }
    if flags & CLONE_CHILD_CLEARTID != 0 {
        c.clear_child_tid = child_tid;
    }
    if flags & CLONE_PARENT_SETTID != 0 {
        uaccess::write(parent_tid, pid)?;
    }
    Ok(pid)
}

// ---- execve ----------------------------------------------------------------

const MAX_ARGS: usize = 64;

/// Holds execve's argv/envp strings: they live in the old image, so they
/// have to be copied out before the new one is loaded over it.
struct ArgBuf {
    bytes: [u8; 8192],
    len: usize,
}

static EXEC_ARGS: SyncUnsafeCell<ArgBuf> = SyncUnsafeCell::new(ArgBuf { bytes: [0; 8192], len: 0 });

impl ArgBuf {
    fn stash(&mut self, s: &str) -> Result<&'static str, Errno> {
        let start = self.len;
        let end = start + s.len();
        if end > self.bytes.len() {
            return Err(E2BIG);
        }
        self.bytes[start..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(unsafe { core::str::from_utf8_unchecked(&*(&self.bytes[start..end] as *const [u8])) })
    }

    /// Copy a NULL-terminated user array of strings into `out`.
    fn stash_list(&mut self, list_va: u32, out: &mut [&'static str; MAX_ARGS]) -> Result<usize, Errno> {
        if list_va == 0 {
            return Ok(0);
        }
        for i in 0..MAX_ARGS {
            let ptr: u32 = uaccess::read(list_va + 4 * i as u32)?;
            if ptr == 0 {
                return Ok(i);
            }
            out[i] = self.stash(uaccess::cstr(ptr)?)?;
        }
        Err(E2BIG)
    }
}

fn read_image(path: &Path) -> Result<&'static [u8], Errno> {
    let entry = vfs::lookup(path)?;
    if entry.is_dir() {
        return Err(EACCES);
    }
    let file = vfs::read_file(&entry.dirent);
    if file.data.is_null() {
        return Err(ENOEXEC);
    }
    Ok(unsafe { core::slice::from_raw_parts(file.data, file.n_data) })
}

/// `#!interpreter [arg]` from the start of a script.
fn parse_shebang(image: &[u8]) -> Option<(&str, Option<&str>)> {
    let rest = image.strip_prefix(b"#!")?;
    let line = &rest[..rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len()).min(127)];
    let line = core::str::from_utf8(line).ok()?.trim();
    match line.split_once([' ', '\t']) {
        Some((interp, arg)) => Some((interp, Some(arg.trim()))),
        None if !line.is_empty() => Some((line, None)),
        None => None,
    }
}

// execve(path, argv, envp)
pub fn execve(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let program: &mut Program = h.current();
    let path_arg = uaccess::cstr(f.r0)?;

    let args = unsafe { &mut *EXEC_ARGS.get() };
    args.len = 0;
    let path_arg = args.stash(path_arg)?;
    let mut argv = [""; MAX_ARGS];
    let mut argc = args.stash_list(f.r1, &mut argv)?;
    let mut envp = [""; MAX_ARGS];
    let envc = args.stash_list(f.r2, &mut envp)?;

    let mut path = match path_arg {
        "/proc/self/exe" => program.exe,
        _ => Path::resolve(&program.cwd, path_arg)?,
    };
    let mut image = read_image(&path)?;

    if let Some((interp, interp_arg)) = parse_shebang(image) {
        // argv becomes: interp [interp_arg] script original-argv[1..]
        let extra = 2 + interp_arg.is_some() as usize;
        if argc.max(1) - 1 + extra > MAX_ARGS {
            return Err(E2BIG);
        }
        let mut new_argv = [""; MAX_ARGS];
        let mut n = 0;
        new_argv[n] = args.stash(interp)?;
        n += 1;
        if let Some(arg) = interp_arg {
            new_argv[n] = args.stash(arg)?;
            n += 1;
        }
        new_argv[n] = path_arg;
        n += 1;
        for &arg in argv.iter().take(argc).skip(1) {
            new_argv[n] = arg;
            n += 1;
        }
        argv = new_argv;
        argc = n;
        path = Path::resolve(&program.cwd, interp)?;
        image = read_image(&path)?;
    }

    let old_exe = program.exe;
    program.exe = path; // exec reads it for AT_EXECFN
    if let Err(e) = h.exec_elf(h.current_program, image.as_ptr(), image.len(), &argv[..argc], &envp[..envc]) {
        program.exe = old_exe;
        return Err(e);
    }
    program.fds.close_on_exec();

    // return "into" the new image; the dispatcher saves this frame and leaves the new sp alone
    *f = program.frame;
    Ok(0)
}

// ---- ids -------------------------------------------------------------------

pub fn getpid(h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Ok(h.pid())
}

pub fn getppid(h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Ok(h.parent[h.current_program].map_or(0, |p| p as u32 + 1))
}

/// setsid/getpgrp/getsid etc.: every program is its own session and group.
pub fn own_pid(h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Ok(h.pid())
}

pub fn set_tid_address(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    h.current().clear_child_tid = f.r0;
    Ok(h.pid())
}

pub fn set_tls(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    // the TLS register is loaded from here when the program resumes
    h.current().thread_pointer = f.r0;
    Ok(0)
}

pub fn get_tls(h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Ok(h.current().thread_pointer)
}

// getresuid32/getresgid32(r*, e*, s*): everyone is root
pub fn getresid(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    for va in [f.r0, f.r1, f.r2] {
        uaccess::write_opt(va, 0u32)?;
    }
    Ok(0)
}

// ---- signals ---------------------------------------------------------------

const SIGKILL: u32 = 9;
const SIGSTOP: u32 = 19;
const SIG_IGN: u32 = 1;

/// Signals whose default action is to do nothing (or to stop/continue, which we don't model).
fn ignored_by_default(sig: u32) -> bool {
    matches!(sig, 17 | 18 | 19 | 20 | 21 | 22 | 23 | 28) // CHLD CONT STOP TSTP TTIN TTOU URG WINCH
}

fn send_signal(h: &mut OSHolder, pid: i32, sig: u32) -> SysResult {
    let index = match pid {
        0 => h.current_program,
        p if p > 0 => p as usize - 1,
        p if p < -1 => (-p) as usize - 1,
        _ => return Err(EPERM), // -1: everything
    };
    if sig > 64 {
        return Err(EINVAL);
    }
    if index >= NUM_PROGRAMS || !h.active[index] {
        return Err(ESRCH);
    }
    let ignored = unsafe { h.get_program(index) }.sig_ignored & (1 << (sig.max(1) - 1)) != 0;
    if sig == 0 || ignored_by_default(sig) || (ignored && sig != SIGKILL) {
        return Ok(0);
    }
    terminate(h, index, sig);
    Ok(0)
}

pub fn kill(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    send_signal(h, f.r0 as i32, f.r1)
}

pub fn tkill(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    send_signal(h, f.r0 as i32, f.r1)
}

pub fn tgkill(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    send_signal(h, f.r1 as i32, f.r2)
}

// rt_sigaction(sig, act*, oldact*, sigsetsize). Only SIG_IGN vs. everything
// else is remembered, since handlers are never run.
pub fn rt_sigaction(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let (sig, act, oldact) = (f.r0, f.r1, f.r2);
    if sig == 0 || sig > 64 || (act != 0 && (sig == SIGKILL || sig == SIGSTOP)) {
        return Err(EINVAL);
    }
    let program = h.current();
    let bit = 1u64 << (sig - 1);
    if oldact != 0 {
        // struct k_sigaction { handler, flags, restorer, mask[2] }
        let handler = if program.sig_ignored & bit != 0 { SIG_IGN } else { 0 };
        uaccess::write(oldact, [handler, 0, 0, 0, 0])?;
    }
    if act != 0 {
        let handler: u32 = uaccess::read(act)?;
        if handler == SIG_IGN { program.sig_ignored |= bit } else { program.sig_ignored &= !bit }
    }
    Ok(0)
}

// rt_sigprocmask(how, set*, oldset*, size), rt_sigpending(set*): nothing is ever blocked or pending
pub fn rt_sigprocmask(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    uaccess::write_opt(f.r2, 0u64)?;
    Ok(0)
}

pub fn rt_sigpending(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    uaccess::write(f.r0, 0u64)?;
    Ok(0)
}

pub fn sigaltstack(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    const SS_DISABLE: u32 = 2;
    uaccess::write_opt(f.r1, [0, SS_DISABLE, 0])?;
    Ok(0)
}

/// pause/sigsuspend: wait for a signal, which never comes.
pub fn wait_forever(_h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Err(ERESTART)
}
