use crate::println;
use crate::arch::dev_barrier;
use core::arch::global_asm;

pub const KUSER_ADDR: usize = 0x1600_0000;

global_asm!(r#"
    .align 4

    .global __kuser_get_tls_start
    .global __kuser_get_tls_end
__kuser_get_tls_start:
    mrc p15, 0, r0, c13, c0, 3
    bx lr
__kuser_get_tls_end:

    .global __kuser_memory_barrier_start
    .global __kuser_memory_barrier_end
__kuser_memory_barrier_start:
    mcr p15, 0, r0, c7, c10, 5
    bx lr
__kuser_memory_barrier_end:

    .global __kuser_cmpxchg_start
    .global __kuser_cmpxchg_end
__kuser_cmpxchg_start:                @ r0 = oldval, r1 = newval, r2 = ptr; r0 = 0 (and C set) on success
    ldr r3, [r2]
    subs r3, r3, r0
    streq r1, [r2]
    rsbs r0, r3, #0
    bx lr
__kuser_cmpxchg_end:
"#);

unsafe extern "C" {
    static __kuser_get_tls_start: u8;
    static __kuser_get_tls_end: u8;

    static __kuser_memory_barrier_start: u8;
    static __kuser_memory_barrier_end: u8;

    static __kuser_cmpxchg_start: u8;
    static __kuser_cmpxchg_end: u8;
}

unsafe fn copy_blob(start: *const u8, end: *const u8, dst: *mut u8) {
    unsafe {
        let size = end.offset_from(start) as usize;

        core::ptr::copy_nonoverlapping(
            start,
            dst,
            size,
        );

        println!("Copied {} bytes from {:x}->{:x} to {:x}->{:x}", size, start as usize, start as usize + size, dst as usize, dst as usize + size);
    }
}

pub fn install_kuser_helpers() {
    unsafe {
        dev_barrier();
        println!("About to copy Kuser helpers");

        // __kernel_helper_version at VA 0xFFFF0FFC
        let helper_version = (KUSER_ADDR + 0x00FF0FFC) as *mut u32;
        *helper_version = 5;

        // __kernel_get_tls at VA 0xFFFF0FE0
        copy_blob(
            &__kuser_get_tls_start,
            &__kuser_get_tls_end,
            (KUSER_ADDR + 0x00FF0FE0) as *mut u8,
        );

        // __kernel_cmpxchg at VA 0xFFFF0FC0
        copy_blob(
            &__kuser_cmpxchg_start,
            &__kuser_cmpxchg_end,
            (KUSER_ADDR + 0x00FF0FC0) as *mut u8,
        );

        // __kernel_memory_barrier at VA 0xFFFF0FA0
        copy_blob(
            &__kuser_memory_barrier_start,
            &__kuser_memory_barrier_end,
            (KUSER_ADDR + 0x00FF0FA0) as *mut u8,
        );

        println!("Finished copying KUSER");
    }
}