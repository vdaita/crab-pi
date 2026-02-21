use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE};
use crate::println;

pub fn deadbeef_kernel() {
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

pub fn add_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::init(ADD_KERNEL_CODE);
        let gpu = &mut *gpu_ptr;

        let a = [0, 1, 2, 3];
        let b = [6, 7, 8, 9];
        let len = core::mem::size_of_val(&a);

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0][0].as_mut_ptr(), len);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[0][1].as_mut_ptr(), len);

        for i in 0..3 {
            println!("Memory in array {} before running code: {} {} {} {}", i, gpu.data[0][i][0], gpu.data[0][i][1], gpu.data[0][i][2], gpu.data[0][i][3]);
        }

        gpu.execute();

        for i in 0..3 {
            println!("Memory in array {} before running code: {} {} {} {}", i, gpu.data[0][i][0], gpu.data[0][i][1], gpu.data[0][i][2], gpu.data[0][i][3]);
        }
        gpu.release();

        println!("Finished releasing test_gpu");
    }
}

pub fn test_gpu() {    
    add_kernel();
}