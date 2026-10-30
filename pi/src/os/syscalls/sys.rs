//! System information, limits, scheduling, and other odds and ends.

use super::{time, uaccess};
use crate::kmalloc;
use crate::os::errno::*;
use crate::os::file::NUM_FILE_DESCRIPTORS;
use crate::os::holder::OSHolder;
use crate::os::interrupts::InterruptFrame;
use crate::os::utils;

type Frame = InterruptFrame;

const RLIM_INFINITY: u64 = u64::MAX;
const RLIMIT_NOFILE: u32 = 7;

pub fn uname(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    const FIELD: usize = 65;
    let fields = ["Linux", "crab-pi", "6.1.0-crab-pi", "#1", "armv6l", "(none)"];
    let buf = uaccess::slice_mut(f.r0, fields.len() * FIELD)?;
    buf.fill(0);
    for (i, s) in fields.iter().enumerate() {
        buf[i * FIELD..][..s.len()].copy_from_slice(s.as_bytes());
    }
    Ok(0)
}

pub fn sysinfo(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    // struct sysinfo, all 32-bit fields: uptime, loads[3], totalram, freeram,
    // sharedram, bufferram, totalswap, freeswap, procs|pad, totalhigh, freehigh, mem_unit, pad[2]
    let mut info = [0u32; 16];
    info[0] = (time::now_us() / 1_000_000) as u32;
    info[4] = 512 * 1024 * 1024;
    info[5] = unsafe { kmalloc::HEAP_END - kmalloc::HEAP_CURR } as u32;
    info[10] = h.active.iter().filter(|&&a| a).count() as u32;
    info[13] = 1;
    uaccess::write(f.r0, info)?;
    Ok(0)
}

pub fn getrandom(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    utils::fill_random(uaccess::slice_mut(f.r0, f.r1 as usize)?);
    Ok(f.r1)
}

fn rlimit(resource: u32) -> u64 {
    if resource == RLIMIT_NOFILE { NUM_FILE_DESCRIPTORS as u64 } else { RLIM_INFINITY }
}

// prlimit64(pid, resource, new*, old*): limits are reported but never enforced or changed
pub fn prlimit64(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let limit = rlimit(f.r1);
    uaccess::write_opt(f.r3, [limit, limit])?;
    Ok(0)
}

// ugetrlimit(resource, struct rlimit *)
pub fn ugetrlimit(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let limit = rlimit(f.r0).min(u32::MAX as u64) as u32;
    uaccess::write(f.r1, [limit, limit])?;
    Ok(0)
}

// getrusage(who, struct rusage *): no accounting
pub fn getrusage(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    uaccess::zero(f.r1, 72)?;
    Ok(0)
}

pub fn sched_yield(h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    h.should_cswitch = true;
    Ok(0)
}

// sched_getaffinity(pid, size, mask*): one CPU. Returns the bytes written.
pub fn sched_getaffinity(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    if f.r1 < 4 {
        return Err(EINVAL);
    }
    uaccess::write(f.r2, 1u32)?;
    Ok(4)
}

// sched_getparam(pid, struct sched_param *)
pub fn sched_getparam(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    uaccess::write(f.r1, 0u32)?;
    Ok(0)
}

// getcpu(cpu*, node*, cache)
pub fn getcpu(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    uaccess::write_opt(f.r0, 0u32)?;
    uaccess::write_opt(f.r1, 0u32)?;
    Ok(0)
}

/// getpriority returns 20 - nice
pub fn getpriority(_h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Ok(20)
}

// futex(uaddr, op, val, ...): with no threads there's never anyone to wait
// for or wake, so a wait that would block just returns (a spurious wakeup).
pub fn futex(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    const FUTEX_WAIT: u32 = 0;
    const FUTEX_WAKE: u32 = 1;
    const FUTEX_WAIT_BITSET: u32 = 9;
    const FUTEX_WAKE_BITSET: u32 = 10;
    const FUTEX_CMD_MASK: u32 = 0x7f;

    let (uaddr, op, val) = (f.r0, f.r1 & FUTEX_CMD_MASK, f.r2);
    match op {
        FUTEX_WAIT | FUTEX_WAIT_BITSET => {
            if uaccess::read::<u32>(uaddr)? != val { Err(EAGAIN) } else { Ok(0) }
        }
        FUTEX_WAKE | FUTEX_WAKE_BITSET => Ok(0),
        _ => Err(ENOSYS),
    }
}
