#![feature(sync_unsafe_cell)]
#![no_std]
#![no_main]

mod llvm_infra;
mod panic_infra;
mod start;

mod arch;     // CPU: barriers, MMIO access
mod devices;  // GPIO, UART, timers, watchdog
mod fs;       // SD card + FAT32
mod mm;       // kernel allocators
mod util;     // printing, checksums, bit helpers
mod os;       // the Linux-compatible process layer

// keep the old flat paths (crate::uart, crate::kmalloc, ...) working
use arch::mem;
use devices::{gpio, timer, uart, watchdog};
use fs::fat32;
use mm::kmalloc;
use util::{crc, fast_hash, print};

// demos, run by hand from main
#[allow(dead_code)]
mod programs {
    pub mod dma;
}

unsafe fn enable_fpu() {
    unsafe {
        core::arch::asm!(
            // Read CPACR
            "mrc p15, 0, r0, c1, c0, 2",
            // Enable single precision (bits 20-21) and double precision (bits 22-23)
            "orr r0, r0, #0x300000",   // single precision
            "orr r0, r0, #0xC00000",   // double precision
            // Write back CPACR
            "mcr p15, 0, r0, c1, c0, 2",
            // Enable FPU
            "mov r0, #0x40000000",
            "fmxr fpexc, r0",
        );
    }
}



pub fn main() {
    unsafe { enable_fpu(); }
    uart::init();
    println!("Hello from Rust on the Pi!");

    // programs::dma::dma_test();
    os::run_busybox();
    done();
}

fn done(){
    println!("DONE!!!");
}
