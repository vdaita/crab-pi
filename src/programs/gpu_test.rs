use crate::gpu::DeadbeefGpu;
use crate::println;
use crate::print;

pub fn test_gpu() {    
    unsafe {
        let gpu_ptr = DeadbeefGpu::init();
        let mut output: u32 = 0;
        let gpu = &mut *gpu_ptr;

        gpu.unif[0][0] = crate::gpu::GPU_BASE + (&mut output as *mut u32) as u32;

        println!("Memory before running code: {:x} {:x} {:x} {:x}", gpu.output[0], gpu.output[1], gpu.output[2], gpu.output[3]);
        gpu.execute();
        println!("Memory after running code: {:x} {:x} {:x} {:x}", gpu.output[0], gpu.output[1], gpu.output[2], gpu.output[3]);

        gpu.release();
    }
}