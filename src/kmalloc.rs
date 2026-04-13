#![allow(dead_code)]

use core::cmp;

unsafe extern "C" {
    static __heap_start__: u8;
    static __dram_end__: u8;
}

pub static mut HEAP_CURR: usize = 0;
pub static mut HEAP_END: usize = 0;

#[inline(always)]
const fn align_up(x: usize, align: usize) -> usize {
    (x + align - 1) & !(align - 1)
}

unsafe fn ensure_init() {
    if HEAP_CURR == 0 {
        let start = &__heap_start__ as *const u8 as usize;
        let end = &__dram_end__ as *const u8 as usize;
        HEAP_CURR = start;
        HEAP_END = end;
    }
}

pub unsafe fn kmalloc_init_mb(mb: usize) {
    let start = &__heap_start__ as *const u8 as usize;
    let end = &__dram_end__ as *const u8 as usize;
    HEAP_CURR = start;
    let requested_end = start.saturating_add(mb.saturating_mul(1024 * 1024));
    HEAP_END = cmp::min(requested_end, end);
}

pub unsafe fn kmalloc_init_bytes(bytes: usize) {
    let start = &__heap_start__ as *const u8 as usize;
    let end = &__dram_end__ as *const u8 as usize;
    HEAP_CURR = start;
    let requested_end = start.saturating_add(bytes);
    HEAP_END = cmp::min(requested_end, end);
}

pub unsafe fn kmalloc(size: usize) -> *mut u8 {
    ensure_init();
    let size = align_up(size, 8);
    let ptr = HEAP_CURR;
    let next = ptr.saturating_add(size);
    if next > HEAP_END {
        panic!("kmalloc: out of memory (requested {} bytes)", size);
    }
    HEAP_CURR = next;
    ptr as *mut u8
}

pub unsafe fn kmalloc_t<T>(count: usize) -> *mut T {
    let bytes = count.saturating_mul(core::mem::size_of::<T>());
    kmalloc(bytes) as *mut T
}
