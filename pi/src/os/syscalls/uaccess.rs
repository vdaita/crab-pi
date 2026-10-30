//! Every access to user memory goes through here.
//!
//! Syscalls run with the MMU off, so user virtual addresses are translated by
//! hand: the current program's slot is mapped at VA 0, and the kernel region
//! from `KERNEL_BASE` up is identity mapped. When the page-table layout
//! changes, this is the file that has to follow.

use core::mem::size_of;
use crate::os::errno::*;
use crate::os::holder::OSHolder;

const KERNEL_BASE: usize = 0x1000_0000;
/// Size of the per-program window at VA 0 (two 16MB sections, see `map_program_mmu`).
const USER_SPAN: usize = 0x0200_0000;
const MAX_CSTR: usize = 4096;

/// Kernel-usable pointer to `len` bytes of user memory at `va`.
fn translate(va: u32, len: usize) -> Result<*mut u8, Errno> {
    let va = va as usize;
    if va == 0 {
        return Err(EFAULT);
    }
    if va >= KERNEL_BASE {
        return Ok(va as *mut u8);
    }
    if va.checked_add(len).is_none_or(|end| end > USER_SPAN) {
        return Err(EFAULT);
    }
    let holder = unsafe { OSHolder::os_holder_mut() };
    Ok(unsafe { (holder.programs[holder.current_program] as *mut u8).add(va) })
}

pub fn slice<'a>(va: u32, len: usize) -> Result<&'a [u8], Errno> {
    if len == 0 {
        return Ok(&[]);
    }
    Ok(unsafe { core::slice::from_raw_parts(translate(va, len)?, len) })
}

pub fn slice_mut<'a>(va: u32, len: usize) -> Result<&'a mut [u8], Errno> {
    if len == 0 {
        return Ok(&mut []);
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(translate(va, len)?, len) })
}

pub fn read<T: Copy>(va: u32) -> Result<T, Errno> {
    Ok(unsafe { (translate(va, size_of::<T>())? as *const T).read_unaligned() })
}

pub fn write<T>(va: u32, value: T) -> Result<(), Errno> {
    unsafe { (translate(va, size_of::<T>())? as *mut T).write_unaligned(value) };
    Ok(())
}

/// `write` for optional out-parameters: a null `va` is skipped.
pub fn write_opt<T>(va: u32, value: T) -> Result<(), Errno> {
    if va == 0 { Ok(()) } else { write(va, value) }
}

pub fn zero(va: u32, len: usize) -> Result<(), Errno> {
    slice_mut(va, len)?.fill(0);
    Ok(())
}

/// A NUL-terminated user string.
pub fn cstr<'a>(va: u32) -> Result<&'a str, Errno> {
    let start = translate(va, 1)?;
    let limit = if (va as usize) >= KERNEL_BASE { MAX_CSTR } else { MAX_CSTR.min(USER_SPAN - va as usize) };
    let bytes = unsafe { core::slice::from_raw_parts(start, limit) };
    let len = bytes.iter().position(|&b| b == 0).ok_or(ENAMETOOLONG)?;
    core::str::from_utf8(&bytes[..len]).map_err(|_| EINVAL)
}
