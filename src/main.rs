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
mod softmax;
mod watchdog;
mod timer;
mod kmalloc;
mod fat32;
mod fast_hash;
mod crc;
mod gpt;
mod ckalloc;
mod bit_utils;
mod programs {
    pub mod gpu_test;
    pub mod mandelbrot;
    pub mod fat32_test;
    pub mod matrix_load_test;
    pub mod derive_jit;
    pub mod ir;
    pub mod ckmalloc_test;
    pub mod vm_test;
}
mod os {
    pub mod interrupts;
    pub mod virtmem;
    pub mod elf_loader;
    pub mod threads;
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

unsafe fn enable_caches() {
    let mut r: u32;
    unsafe {
        core::arch::asm!(
            "mrc p15, 0, {reg}, c1, c0, 0",
            reg = out(reg) r,
            options(nostack, nomem)
        );
    }

    r |= 1 << 12; // L1 instruction cache
    r |= 1 << 11; // branch prediction

    unsafe {
        core::arch::asm!(
            "mcr p15, 0, {reg}, c1, c0, 0",
            reg = in(reg) r,
            options(nostack, nomem)
        );
    }
}

fn main() {    
    uart::init();
    println!("Hello from Rust on the Pi!");

    programs::vm_test::vm_test();
    // programs::ckmalloc_test::test_ckmalloc();
    // programs::ir::ir_main();
    // programs::derive_jit::derive_main();
    // unsafe {enable_fpu()};
    // unsafe {enable_caches();}
    // fat32::pi_sd_init();
    // softmax::softmax_func_test();
    // softmax::exp_func_test();
    // programs::gpu_test::test_gpu();
    // gpt::gpt_demo();
    // programs::mandelbrot::mandelbrot();
    // programs::fat32_test::fat32_test();
    // programs::matrix_load_test::matrix_load_test();
    // gpt::model::infer_model();


    done();
}

fn done(){
    println!("DONE!!!");
}
