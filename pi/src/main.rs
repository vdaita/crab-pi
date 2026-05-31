#![feature(sync_unsafe_cell)]
#![no_std]
#![no_main]

use core::arch::global_asm;

mod arch;
mod llvm_infra;
mod panic_infra;
mod print;
mod circular;
mod mem;
mod gpio;
mod start;
mod uart;
mod watchdog;
mod timer;
mod kmalloc;
mod fat32;
mod fast_hash;
mod crc;
// mod gpt;
// mod gpu;
// mod matmul;
// mod softmax;
mod ckalloc;
mod profiler;
mod pmu_profiler;
mod bit_utils;
// mod programs {
//     pub mod gpu_test;
//     pub mod mandelbrot;
//     pub mod fat32_test;
//     pub mod matrix_load_test;
//     pub mod derive_jit;
//     pub mod ir;
//     pub mod ckmalloc_test;
//     pub mod vm_test;
//     pub mod imu;
//     pub mod lightstrip;
//     pub mod memtrace;
//     pub mod stepper_motor;
//     pub mod oled_display;
// }
mod os {
    pub mod holder;
    pub mod interrupts;
    pub mod virtmem;
    // pub mod elf_loader;
    pub mod threads;
    pub mod utils;
    pub mod elf_file;
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
    unsafe { 
        enable_fpu(); 
        // os::utils::enable_l1_instruction_cache();
        // os::utils::enable_branch_prediction();
    }
    uart::init();
    println!("Hello from Rust on the Pi!");

    // os::interrupts::test_interrupts();
    // os::interrupts::test_interrupts_vbar();

    // unsafe {
    //     os::holder::mmu_identity_map_test();
    // }


    // unsafe {
    //     os::holder::OSHolder::init();
    //     os::holder::OSHolder::test_swi();
    // }

    // unsafe {
        // os::holder::mmu_identity_map_test();
    //     os::holder::OSHolder::init();    
    //     let busybox_program_index = os::holder::OSHolder::os_holder_mut().load_elf("BUSYBOX");
    //     println!("Program index: {}", busybox_program_index);
    //     os::holder::OSHolder::os_holder_mut().run_elf(busybox_program_index, "BUSYBOX");
    //     // let _ = hello_program_index;
    // }

    os::holder::test_elf_holder();

    // programs::oled_display::test_oled_display();
    // programs::stepper_motor::run_stepper_motor();
    // memtrace::test_memtrace_with_ckalloc();
    // memtrace::test_memtrace();
    // lightstrip::basic_run();
    // lightstrip::use_imu_to_color();
    // pmu_profiler::test_pmu_profiler();
    // profiler::test_profiler();
    // fat32::pi_sd_init();
    // programs::fat32_test::fat32_test();
    // os::elf_loader::test_elf_loader();
    // os::threads::test_threads();
    // programs::imu::imu_test();
    // os::interrupts::test_interrupts();
    // programs::vm_test::vm_test();
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
