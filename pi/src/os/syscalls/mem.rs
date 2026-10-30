//! brk and mmap, both carved out of the program's fixed heap window: brk
//! grows up from the bottom, mmap takes pages from the top down (one bit per
//! page in `Program::mmap_pages`). File mappings are private copies.

use super::uaccess;
use crate::os::errno::*;
use crate::os::holder::{OSHolder, Program, HEAP_PAGES, PAGE_SIZE};
use crate::os::interrupts::InterruptFrame;

type Frame = InterruptFrame;

const MAP_FIXED: u32 = 0x10;
const MAP_ANONYMOUS: u32 = 0x20;

fn page_used(p: &Program, page: usize) -> bool {
    p.mmap_pages[page / 32] & (1 << (page % 32)) != 0
}

fn mark_pages(p: &mut Program, first: usize, count: usize, used: bool) {
    for page in first..(first + count).min(HEAP_PAGES) {
        let bit = 1 << (page % 32);
        if used { p.mmap_pages[page / 32] |= bit } else { p.mmap_pages[page / 32] &= !bit }
    }
}

/// Lowest address mmap has handed out; brk can't grow past it.
fn mmap_floor(p: &Program) -> usize {
    let (lo, hi) = Program::heap_range();
    (0..HEAP_PAGES).find(|&i| page_used(p, i)).map_or(hi, |i| lo + i * PAGE_SIZE)
}

pub fn brk(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let p = h.current();
    let (lo, _) = Program::heap_range();
    let want = f.r0 as usize;
    // out-of-range requests (including brk(0)) just report the current break
    if want < lo || want > mmap_floor(p) {
        return Ok(p.heap_ptr as u32);
    }
    if want > p.heap_ptr {
        uaccess::zero(p.heap_ptr as u32, want - p.heap_ptr)?;
    }
    p.heap_ptr = want;
    Ok(want as u32)
}

// mmap2(addr, len, prot, flags, fd, pgoff)
pub fn mmap2(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let (addr, len, flags, fd, pgoff) = (f.r0 as usize, f.r1 as usize, f.r3, f.r4, f.r5 as usize);
    if len == 0 {
        return Err(EINVAL);
    }
    let p = h.current();
    // (ptr, len) rather than a slice so it doesn't hold a borrow of the program
    let source = match flags & MAP_ANONYMOUS {
        0 => Some(p.fds.get(fd)?.contents().map(|d| (d.as_ptr(), d.len()))?),
        _ => None,
    };

    let (lo, hi) = Program::heap_range();
    let pages = len.div_ceil(PAGE_SIZE);
    let first = if flags & MAP_FIXED != 0 {
        // only within the heap window: anywhere else would clobber the program or the kernel's state
        if addr % PAGE_SIZE != 0 || addr < lo || addr + pages * PAGE_SIZE > hi {
            return Err(EINVAL);
        }
        (addr - lo) / PAGE_SIZE
    } else {
        let lowest = (p.heap_ptr - lo).div_ceil(PAGE_SIZE);
        let fits = |start: usize| (start..start + pages).all(|i| !page_used(p, i));
        (lowest..=HEAP_PAGES.checked_sub(pages).ok_or(ENOMEM)?).rev().find(|&s| fits(s)).ok_or(ENOMEM)?
    };
    mark_pages(p, first, pages, true);

    let va = (lo + first * PAGE_SIZE) as u32;
    uaccess::zero(va, pages * PAGE_SIZE)?;
    if let Some((ptr, file_len)) = source {
        let data = unsafe { core::slice::from_raw_parts(ptr, file_len) };
        let offset = pgoff * PAGE_SIZE;
        if offset < data.len() {
            let n = len.min(data.len() - offset);
            uaccess::slice_mut(va, n)?.copy_from_slice(&data[offset..offset + n]);
        }
    }
    Ok(va)
}

pub fn munmap(h: &mut OSHolder, f: &mut Frame) -> SysResult {
    let (addr, len) = (f.r0 as usize, f.r1 as usize);
    let (lo, hi) = Program::heap_range();
    if addr % PAGE_SIZE != 0 {
        return Err(EINVAL);
    }
    if addr >= lo && addr < hi {
        mark_pages(h.current(), (addr - lo) / PAGE_SIZE, len.div_ceil(PAGE_SIZE), false);
    }
    Ok(0)
}

/// Callers (musl's realloc) fall back to allocate-and-copy.
pub fn mremap(_h: &mut OSHolder, _f: &mut Frame) -> SysResult {
    Err(ENOMEM)
}
