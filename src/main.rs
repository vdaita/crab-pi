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
mod programs {
    pub mod gpu_test;
}

fn main() {
    uart::init();
    println!("Hello from Rust on the Pi!");

    programs::gpu_test::test_gpu();

    done();
}

fn done(){
    println!("DONE!!!");
}