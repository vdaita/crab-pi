use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE};
use crate::println;
use crate::print;

pub fn test_gpu() {    
    unsafe {
        let gpu_ptr = GpuKernel::init(DEADBEEF_GPU_CODE);
        let gpu = &mut *gpu_ptr;

        println!("Memory before running code: {:x} {:x} {:x} {:x}", gpu.data[0][0][0], gpu.data[0][0][16], gpu.data[0][0][32], gpu.data[0][0][48]);
        gpu.execute();
        println!("Memory after running code: {:x} {:x} {:x} {:x}", gpu.data[0][0][0], gpu.data[0][0][16], gpu.data[0][0][32], gpu.data[0][0][48]);

        gpu.release();

        println!("Finished releasing test_gpu");
    }
}