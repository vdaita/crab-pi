//! Clocks and sleeping, backed by the BCM2835's free-running 1MHz system timer.
//!
//! The realtime clock starts at the epoch on boot; settimeofday/clock_settime
//! move it. Every other clock is time since boot.

use super::uaccess;
use crate::os::errno::*;
use crate::os::holder::OSHolder;
use crate::os::interrupts::InterruptFrame;

type Frame = InterruptFrame;

const TIMER_CLO: usize = 0x2000_3004;
const TIMER_CHI: usize = 0x2000_3008;

const CLOCK_REALTIME: u32 = 0;
const CLOCK_REALTIME_COARSE: u32 = 5;
const CLOCK_TAI: u32 = 11;
const TIMER_ABSTIME: u32 = 1;

static mut REALTIME_OFFSET_US: i64 = 0;

/// Microseconds since boot.
pub fn now_us() -> u64 {
    loop {
        let hi = unsafe { core::ptr::read_volatile(TIMER_CHI as *const u32) };
        let lo = unsafe { core::ptr::read_volatile(TIMER_CLO as *const u32) };
        if hi == unsafe { core::ptr::read_volatile(TIMER_CHI as *const u32) } {
            return (hi as u64) << 32 | lo as u64;
        }
    }
}

/// Offset from boot time to `clock`.
fn clock_offset(clock: u32) -> Result<i64, Errno> {
    match clock {
        CLOCK_REALTIME | CLOCK_REALTIME_COARSE | CLOCK_TAI => Ok(unsafe { REALTIME_OFFSET_US }),
        1..=4 | 6 | 7 => Ok(0), // monotonic, cputime, raw, boottime: all just time since boot
        _ => Err(EINVAL),
    }
}

fn clock_us(clock: u32) -> Result<u64, Errno> {
    Ok((now_us() as i64 + clock_offset(clock)?) as u64)
}

/// Finish once `deadline` (us since boot) has passed; until then, yield.
/// The first call records the deadline so restarts don't keep pushing it out.
pub fn sleep_until(h: &mut OSHolder, deadline: u64) -> SysResult {
    let program = h.current();
    if program.wake_at == 0 {
        program.wake_at = deadline.max(1);
    }
    if now_us() < program.wake_at {
        return Err(ERESTART);
    }
    program.wake_at = 0;
    Ok(0)
}

/// Read a struct timespec (32- or 64-bit fields) as microseconds.
pub fn read_timespec(va: u32, time64: bool) -> Result<u64, Errno> {
    let (sec, nsec) = if time64 {
        let [sec, nsec]: [i64; 2] = uaccess::read(va)?;
        (sec, nsec)
    } else {
        let [sec, nsec]: [i32; 2] = uaccess::read(va)?;
        (sec as i64, nsec as i64)
    };
    if sec < 0 || !(0..1_000_000_000).contains(&nsec) {
        return Err(EINVAL);
    }
    Ok(sec as u64 * 1_000_000 + nsec as u64 / 1000)
}

/// A timeout argument: a null pointer means wait forever.
pub fn read_timeout(va: u32, time64: bool) -> Result<Option<u64>, Errno> {
    if va == 0 { Ok(None) } else { read_timespec(va, time64).map(Some) }
}

fn write_timespec(va: u32, us: u64, time64: bool) -> SysResult {
    let (sec, nsec) = (us / 1_000_000, us % 1_000_000 * 1000);
    if time64 {
        uaccess::write(va, [sec as i64, nsec as i64])?;
    } else {
        uaccess::write(va, [sec as i32, nsec as i32])?;
    }
    Ok(0)
}

fn set_realtime(us: u64) {
    unsafe { REALTIME_OFFSET_US = us as i64 - now_us() as i64 };
}

pub fn clock_gettime(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    write_timespec(f.r1, clock_us(f.r0)?, false)
}

pub fn clock_gettime64(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    write_timespec(f.r1, clock_us(f.r0)?, true)
}

pub fn clock_getres(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    clock_offset(f.r0)?;
    if f.r1 == 0 { Ok(0) } else { write_timespec(f.r1, 1, false) }
}

pub fn clock_getres_time64(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    clock_offset(f.r0)?;
    if f.r1 == 0 { Ok(0) } else { write_timespec(f.r1, 1, true) }
}

fn clock_settime_impl(f: &Frame, time64: bool) -> SysResult {
    if f.r0 != CLOCK_REALTIME {
        return Err(EINVAL);
    }
    set_realtime(read_timespec(f.r1, time64)?);
    Ok(0)
}

pub fn clock_settime(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    clock_settime_impl(f, false)
}

pub fn clock_settime64(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    clock_settime_impl(f, true)
}

// gettimeofday(struct timeval *tv, struct timezone *tz)
pub fn gettimeofday(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let us = clock_us(CLOCK_REALTIME)?;
    uaccess::write_opt(f.r0, [(us / 1_000_000) as i32, (us % 1_000_000) as i32])?;
    uaccess::write_opt(f.r1, [0i32; 2])?;
    Ok(0)
}

pub fn settimeofday(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    if f.r0 != 0 {
        let [sec, usec]: [i32; 2] = uaccess::read(f.r0)?;
        set_realtime(sec as u64 * 1_000_000 + usec as u64);
    }
    Ok(0)
}

/// times(struct tms *): no per-process accounting, so all zeros; returns clock ticks (100Hz).
pub fn times(_h: &mut OSHolder, f: &mut Frame) -> SysResult {
    uaccess::write_opt(f.r0, [0u32; 4])?;
    Ok((now_us() / 10_000) as u32)
}

// nanosleep(req, rem)
pub fn nanosleep(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let us = read_timespec(f.r0, false)?;
    sleep_until(h, now_us() + us)
}

fn clock_nanosleep_impl(h: &mut OSHolder, f: &Frame, time64: bool) -> SysResult {
    let (clock, flags, req) = (f.r0, f.r1, f.r2);
    let us = read_timespec(req, time64)?;
    let deadline = if flags & TIMER_ABSTIME != 0 {
        (us as i64 - clock_offset(clock)?) as u64
    } else {
        clock_offset(clock)?;
        now_us() + us
    };
    sleep_until(h, deadline)
}

// clock_nanosleep(clock, flags, req, rem)
pub fn clock_nanosleep(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    clock_nanosleep_impl(h, f, false)
}

pub fn clock_nanosleep_time64(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    clock_nanosleep_impl(h, f, true)
}
