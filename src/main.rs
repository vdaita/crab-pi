#![no_std]
#![no_main]

mod arch;
mod llvm_infra;
mod panic_infra;
mod print;
mod circular;
mod mem;
mod gpio;
mod start;
mod uart;
mod gpu;
mod matmul;
mod watchdog;
mod timer;
mod kmalloc;
mod fat32;
mod fast_hash;
mod crc;
mod gpt;
mod programs {
    pub mod gpu_test;
    pub mod mandelbrot;
    pub mod fat32_test;
    pub mod matrix_load_test;
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
            options(nostack, nomem)
        );
    }
}

fn main() {    
    uart::init();
    println!("Hello from Rust on the Pi!");

    unsafe {enable_fpu();}
    fat32::pi_sd_init();

    // programs::gpu_test::test_gpu();
    // gpt::gpt_demo();
    // programs::mandelbrot::mandelbrot();
    // programs::fat32_test::fat32_test();
    // programs::matrix_load_test::matrix_load_test();
    gpt::model::load_model();

    done();
}

fn done(){
    println!("DONE!!!");
}
