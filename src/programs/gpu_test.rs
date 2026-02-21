use crate::gpu::DeadbeefGpu;
use crate::println;
use crate::print;

pub fn test_gpu() {    
    unsafe {
        let gpu_ptr = DeadbeefGpu::init();
        let gpu = &mut *gpu_ptr;

        let output_addr = gpu_ptr as u32 + 
            ((&gpu.output as *const _ as u32) - (gpu_ptr as u32));
        gpu.unif[0][0] = crate::gpu::GPU_BASE + output_addr;

        println!("Memory before running code: {:x} {:x} {:x} {:x}", gpu.output[0], gpu.output[16], gpu.output[32], gpu.output[48]);
        gpu.execute();
        println!("Memory after running code: {:x} {:x} {:x} {:x}", gpu.output[0], gpu.output[16], gpu.output[32], gpu.output[48]);

        gpu.release();

        println!("Finished releasing test_gpu");
    }
}