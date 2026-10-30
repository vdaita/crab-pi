//! The Linux syscall layer.
//!
//! `handle_software_interrupt` is the SVC entry point: it saves the caller's
//! registers into its `Program`, runs the handler for the syscall number, and
//! resumes whichever program should run next. Handlers are grouped by area in
//! the submodules and all have the same shape, so the whole ABI surface is
//! the one table in `handler`.

mod fs;
mod mem;
mod nr;
mod proc;
mod sys;
mod time;
pub mod uaccess;

use core::arch::{asm, global_asm};
use crate::arch::dev_barrier;
use crate::os::errno::*;
use crate::os::holder::{self, OSHolder, Program};
use crate::os::interrupts::InterruptFrame;
use crate::os::virtmem::{mmu_disable, mmu_enable};
use crate::println;

/// Log every syscall, strace-style. Unimplemented ones are always logged.
const TRACE: bool = false;

type Handler = fn(&mut OSHolder, &mut InterruptFrame) -> SysResult;

/// Accepted and ignored: there's no state behind these to change or report.
fn nop(_h: &mut OSHolder, _f: &mut InterruptFrame) -> SysResult {
    Ok(0)
}

/// Deliberately unsupported, and callers are known to cope (so don't log it).
fn unsupported(_h: &mut OSHolder, _f: &mut InterruptFrame) -> SysResult {
    Err(ENOSYS)
}

fn not_permitted(_h: &mut OSHolder, _f: &mut InterruptFrame) -> SysResult {
    Err(EPERM)
}

fn handler(number: u32) -> Option<Handler> {
    use nr::*;
    Some(match number {
        // processes
        EXIT | EXIT_GROUP => proc::exit,
        FORK | VFORK => proc::fork,
        CLONE => proc::clone,
        EXECVE => proc::execve,
        WAIT4 => proc::wait4,
        KILL => proc::kill,
        TKILL => proc::tkill,
        TGKILL => proc::tgkill,
        GETPID | GETTID => proc::getpid,
        GETPPID => proc::getppid,
        GETPGRP | GETPGID | GETSID | SETSID => proc::own_pid,
        SETPGID => nop,
        SET_TID_ADDRESS => proc::set_tid_address,
        ARM_SET_TLS => proc::set_tls,
        ARM_GET_TLS => proc::get_tls,
        SET_ROBUST_LIST => nop,
        GET_ROBUST_LIST | RSEQ | MEMBARRIER | CLONE3 => unsupported,

        // users and groups: everyone is root
        GETUID | GETUID32 | GETEUID | GETEUID32 | GETGID | GETGID32 | GETEGID | GETEGID32 => nop,
        SETUID | SETUID32 | SETGID | SETGID32 | SETREUID | SETREUID32 | SETREGID | SETREGID32 => nop,
        SETRESUID | SETRESUID32 | SETRESGID | SETRESGID32 | SETGROUPS | SETGROUPS32 => nop,
        GETRESUID | GETRESUID32 | GETRESGID | GETRESGID32 => proc::getresid,
        GETGROUPS | GETGROUPS32 => nop,

        // signals
        RT_SIGACTION => proc::rt_sigaction,
        RT_SIGPROCMASK => proc::rt_sigprocmask,
        RT_SIGPENDING => proc::rt_sigpending,
        SIGALTSTACK => proc::sigaltstack,
        PAUSE | RT_SIGSUSPEND => proc::wait_forever,
        RT_SIGRETURN | SIGRETURN => unsupported,

        // memory
        BRK => mem::brk,
        MMAP2 => mem::mmap2,
        MUNMAP => mem::munmap,
        MREMAP => mem::mremap,
        MPROTECT | MADVISE | MSYNC | MLOCK | MUNLOCK | MINCORE => nop,
        ARM_CACHEFLUSH => nop,

        // files
        OPEN => fs::open,
        OPENAT => fs::openat,
        CREAT => fs::creat,
        CLOSE => fs::close,
        READ => fs::read,
        WRITE => fs::write,
        READV => fs::readv,
        WRITEV => fs::writev,
        PREAD64 => fs::pread64,
        PWRITE64 => fs::pwrite64,
        LSEEK => fs::lseek,
        _LLSEEK => fs::llseek,
        GETDENTS64 => fs::getdents64,
        FSYNC | FDATASYNC => fs::fsync,
        SYNC => nop,
        FTRUNCATE => fs::ftruncate,
        FTRUNCATE64 => fs::ftruncate64,
        TRUNCATE64 => fs::truncate64,
        DUP => fs::dup,
        DUP2 => fs::dup2,
        DUP3 => fs::dup3,
        FCNTL | FCNTL64 => fs::fcntl,
        IOCTL => fs::ioctl,
        PIPE => fs::pipe,
        PIPE2 => fs::pipe2,
        POLL => fs::poll,
        PPOLL => fs::ppoll,
        PPOLL_TIME64 => fs::ppoll_time64,
        _NEWSELECT => fs::newselect,
        PSELECT6 => fs::pselect6,
        PSELECT6_TIME64 => fs::pselect6_time64,
        // callers (busybox's copyfd) fall back to read/write
        SENDFILE | SENDFILE64 => unsupported,
        FLOCK | FADVISE64_64 => nop,

        // metadata
        STAT64 | LSTAT64 => fs::stat64,
        FSTAT64 => fs::fstat64,
        FSTATAT64 => fs::fstatat64,
        STATX => fs::statx,
        ACCESS => fs::access,
        FACCESSAT | FACCESSAT2 => fs::faccessat,
        CHMOD | CHOWN | CHOWN32 | LCHOWN | LCHOWN32 | UTIMES => fs::attr_path,
        FCHMODAT | FCHOWNAT | UTIMENSAT | UTIMENSAT_TIME64 => fs::attr_path_at,
        FCHMOD | FCHOWN | FCHOWN32 => fs::attr_fd,
        UMASK => fs::umask,

        // directories and names
        GETCWD => fs::getcwd,
        CHDIR => fs::chdir,
        FCHDIR => fs::fchdir,
        MKDIR => fs::mkdir,
        MKDIRAT => fs::mkdirat,
        RMDIR => fs::rmdir,
        UNLINK => fs::unlink,
        UNLINKAT => fs::unlinkat,
        RENAME => fs::rename,
        RENAMEAT | RENAMEAT2 => fs::renameat,
        READLINK => fs::readlink,
        READLINKAT => fs::readlinkat,
        LINK | LINKAT | SYMLINK | SYMLINKAT | MKNOD | MKNODAT => not_permitted,
        MOUNT | UMOUNT2 | CHROOT | REBOOT | SETHOSTNAME => not_permitted,

        // time
        CLOCK_GETTIME => time::clock_gettime,
        CLOCK_GETTIME64 => time::clock_gettime64,
        CLOCK_GETRES => time::clock_getres,
        CLOCK_GETRES_TIME64 => time::clock_getres_time64,
        CLOCK_SETTIME => time::clock_settime,
        CLOCK_SETTIME64 => time::clock_settime64,
        GETTIMEOFDAY => time::gettimeofday,
        SETTIMEOFDAY => time::settimeofday,
        TIMES => time::times,
        NANOSLEEP => time::nanosleep,
        CLOCK_NANOSLEEP => time::clock_nanosleep,
        CLOCK_NANOSLEEP_TIME64 => time::clock_nanosleep_time64,

        // system
        UNAME => sys::uname,
        SYSINFO => sys::sysinfo,
        GETRANDOM => sys::getrandom,
        PRLIMIT64 => sys::prlimit64,
        UGETRLIMIT => sys::ugetrlimit,
        SETRLIMIT => nop,
        GETRUSAGE => sys::getrusage,
        SCHED_YIELD => sys::sched_yield,
        SCHED_GETAFFINITY => sys::sched_getaffinity,
        SCHED_SETAFFINITY | SCHED_SETPARAM | SCHED_SETSCHEDULER => nop,
        SCHED_GETPARAM => sys::sched_getparam,
        SCHED_GETSCHEDULER | SCHED_GET_PRIORITY_MAX | SCHED_GET_PRIORITY_MIN => nop,
        GETPRIORITY => sys::getpriority,
        SETPRIORITY | NICE => nop,
        GETCPU => sys::getcpu,
        FUTEX | FUTEX_TIME64 => sys::futex,
        PRCTL | PERSONALITY | SYSLOG => nop,

        _ => return None,
    })
}

const SPSR_THUMB: u32 = 1 << 5;

fn decode_syscall_number(frame: &InterruptFrame, svc_lr: u32, spsr: u32) -> u32 {
    if spsr & SPSR_THUMB != 0 {
        return frame.r7; // Thumb svc is EABI-only
    }
    let instr = unsafe { core::ptr::read_volatile(svc_lr.wrapping_sub(4) as *const u32) };
    let imm = instr & 0x00ff_ffff;
    if imm == 0 {
        frame.r7 // EABI: svc #0, number in r7
    } else if imm & 0x00ff_0000 == 0x0090_0000 {
        imm - 0x0090_0000 // OABI: svc #0x900000 + number
    } else {
        imm
    }
}

fn dispatch(holder: &mut OSHolder, frame: &mut InterruptFrame, number: u32) -> u32 {
    let args = [frame.r0, frame.r1, frame.r2, frame.r3];
    let result = match handler(number) {
        Some(h) => h(holder, frame),
        None => {
            println!("[pid {}] unimplemented syscall {} ({:#x})", holder.pid(), nr::name(number), number);
            Err(ENOSYS)
        }
    };

    if TRACE && result != Err(ERESTART) {
        println!("[pid {}] {}({:#x}, {:#x}, {:#x}, {:#x}) = {:?}",
            holder.pid(), nr::name(number), args[0], args[1], args[2], args[3], result);
    }

    match result {
        Ok(value) => value,
        Err(ERESTART) => {
            // re-run the svc when this program is next scheduled; r0 is still the first argument
            let thumb = holder.current().spsr & SPSR_THUMB != 0;
            frame.lr = frame.lr.wrapping_sub(if thumb { 2 } else { 4 });
            holder.should_cswitch = true;
            frame.r0
        }
        Err(Errno(e)) => (e as i32).wrapping_neg() as u32,
    }
}

fn set_tls(tls: u32) {
    unsafe { asm!("mcr p15, 0, {tls}, c13, c0, 3", tls = in(reg) tls) };
}

/// Everything needed to drop back into a user program; `resume_user` reads
/// it by offset.
#[repr(C)]
struct ResumeState {
    frame: InterruptFrame, // r0-r12, pc
    sp: u32,               // 56
    lr: u32,               // 60
    spsr: u32,             // 64
    fpscr: u32,            // 68
    vfp: [u64; 16],        // 72
}

global_asm!(r#"
.fpu vfp
.globl resume_user
resume_user:                    @ r0 = &ResumeState
    add r1, r0, #72
    vldmia r1, {{d0-d15}}
    ldr r1, [r0, #68]
    vmsr fpscr, r1
    ldr r1, [r0, #64]
    msr spsr_cxsf, r1
    ldr r1, [r0, #56]
    ldr r2, [r0, #60]
    cps #0x1f                   @ system mode shares the user sp/lr
    mov sp, r1
    mov lr, r2
    cps #0x13
    ldr lr, [r0, #52]
    ldmia r0, {{r0-r12}}
    movs pc, lr
"#);

unsafe extern "C" {
    fn resume_user(state: *const ResumeState) -> !;
}

/// Save what the entry stub didn't: the banked user sp/lr, SPSR, and the VFP
/// registers (another program, or the kernel, may use them before we return).
fn save_user_state(p: &mut Program) {
    p.sp = holder::get_user_sp() as usize;
    unsafe {
        asm!("stm {p}, {{lr}}^", "nop", p = in(reg) &mut p.user_lr);
        asm!("mrs {}, spsr", out(reg) p.spsr);
        asm!("vstmia {p}, {{d0-d15}}", "vmrs {s}, fpscr", p = in(reg) p.vfp.as_mut_ptr(), s = out(reg) p.fpscr);
    }
}

#[inline(never)]
pub fn handle_software_interrupt(frame: *mut InterruptFrame, svc_lr: u32) -> u32 {
    dev_barrier();
    let frame = unsafe { &mut *frame };
    let holder = unsafe { OSHolder::os_holder_mut() };

    // a syscall from kernel test code, with no user program running
    if !holder.active[holder.current_program] {
        let spsr: u32;
        unsafe { asm!("mrs {}, spsr", out(reg) spsr) };
        let number = decode_syscall_number(frame, svc_lr, spsr);
        return dispatch(holder, frame, number);
    }

    // the svc instruction has to be read through the user mapping, before the MMU goes off
    let spsr: u32;
    unsafe { asm!("mrs {}, spsr", out(reg) spsr) };
    let number = decode_syscall_number(frame, svc_lr, spsr);
    dev_barrier();

    mmu_disable();
    let program = holder.current();
    program.frame = *frame;
    save_user_state(program);

    frame.r0 = dispatch(holder, frame, number);

    // keep the result (and execve's new registers); execve and clone set sp directly
    if holder.active[holder.current_program] {
        holder.current().frame = *frame;
    }

    if holder.should_cswitch {
        holder.current_program = unsafe { holder.get_next_active_program_index(holder.current_program) };
        holder.should_cswitch = false;
    }

    // copy the resume state onto the kernel stack while the program is still at its physical address
    let next = holder.current();
    let resume = ResumeState {
        frame: next.frame,
        sp: next.sp as u32,
        lr: next.user_lr,
        spsr: next.spsr,
        fpscr: next.fpscr,
        vfp: next.vfp,
    };
    let tls = next.thread_pointer;

    holder.map_program_mmu(holder.current_program);
    mmu_enable();
    set_tls(tls);
    unsafe { resume_user(&resume) }
}
