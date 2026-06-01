use crate::println;
use crate::arch::dev_barrier;

pub const KUSER_ADDR: usize = 0x1600_0000;

unsafe fn kuser_memory_barrier() {
    core::arch::asm!(
        "mcr p15, 0, {r0}, c7, c10, 5",
        r0 = in(reg) 0u32,
        options(nostack)
    );
}

unsafe fn kuser_version() -> u32 {
    return 5;
}

unsafe fn kuser_get_tls() -> u32 {
    let tls: u32;
    core::arch::asm!(
        "mrc p15, 0, {tls}, c13, c0, 3",
        tls = out(reg) tls,
        options(nostack)
    );
    tls
}

unsafe fn kuser_cmpxchg(newval: u32, ptr: *mut u32) -> u32 {
    // Simple swap: load old value, store new value, return old value
    let old: u32;
    core::arch::asm!(
        "ldr {old}, [{ptr}]",
        "str {newval}, [{ptr}]",
        ptr = in(reg) ptr,
        newval = in(reg) newval,
        old = out(reg) old,
        options(nostack)
    );
    old
}


pub fn install_kuser_helpers() {
    unsafe {
        dev_barrier();
        println!("About to copy Kuser helpers");

        // __kernel_helper_version at VA 0xFFFF0FFC
        core::ptr::write_volatile(
            (KUSER_ADDR + 0x00FF0FFC) as *mut u32, kuser_version());

        // __kernel_get_tls at VA 0xFFFF0FA0
        core::ptr::copy_nonoverlapping(
            kuser_get_tls as *const u32,
            (KUSER_ADDR + 0x00FF0FA0) as *mut u32, 4);

        // __kernel_cmpxchg at VA 0xFFFF0FC0
        core::ptr::copy_nonoverlapping(
            kuser_cmpxchg as *const u32,
            (KUSER_ADDR + 0x00FF0FC0) as *mut u32, 8);

        // __kernel_memory_barrier at VA 0xFFFF0FE0
        core::ptr::copy_nonoverlapping(
            kuser_memory_barrier as *const u32,
            (KUSER_ADDR + 0x00FF0FE0) as *mut u32, 2);

        println!("Finished copying KUSER");
    }
}