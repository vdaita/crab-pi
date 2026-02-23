use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE, MATMUL_KERNEL_CODE};
use crate::{print, println};

pub fn deadbeef_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::init(DEADBEEF_GPU_CODE);
        let gpu = &mut *gpu_ptr;

        println!("Memory before running code: {:x} {:x} {:x} {:x}", gpu.data[0][0], gpu.data[0][16], gpu.data[0][32], gpu.data[0][48]);
        gpu.execute(1);
        println!("Memory after running code: {:x} {:x} {:x} {:x}", gpu.data[0][0], gpu.data[0][16], gpu.data[0][32], gpu.data[0][48]);

        gpu.release();

        println!("Finished releasing test_gpu");
    }
}

pub fn add_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::init(ADD_KERNEL_CODE);
        let gpu = &mut *gpu_ptr;

        let a: [u32; 128] = core::array::from_fn(|i| i as u32);
        let b: [u32; 128] = core::array::from_fn(|i| (i as u32) + 6);
        let n = a.len(); // copy_nonoverlapping expects element count

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0].as_mut_ptr(), n);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[1].as_mut_ptr(), n);
        gpu.unif[0][3] = n as u32;

        print!("A: ");
        // print_matrix(&a, 1, 128); ->

        print!("B: ");
        // print_matrix(&b, 1, 128);

        print!("Before: out[0..128] =");
        for i in 0..128 {
            print!(" {}", gpu.data[2][i]);
        }
        println!("");

        gpu.execute(1);

        print!("After: out[0..128] =");
        for i in 0..128 {
            print!(" {}", gpu.data[2][i]);
        }
        println!("");

        gpu.release();
        println!("Finished releasing test_gpu");
    }
}

pub fn test_gpu() {
    crate::matmul::matmul_func_test();
}