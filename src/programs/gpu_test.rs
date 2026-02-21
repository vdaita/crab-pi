use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE};
use crate::{print, println};

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

        let a: [u32; 64] = core::array::from_fn(|i| i as u32);
        let b: [u32; 64] = core::array::from_fn(|i| (i as u32) + 6);
        let n = a.len(); // copy_nonoverlapping expects element count

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0][0].as_mut_ptr(), n);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[0][1].as_mut_ptr(), n);

        print!("Before: out[0..16] =");
        for i in 0..16 {
            print!(" {}", gpu.data[0][2][i]);
        }
        println!("");

        gpu.execute();

        print!("After: out[0..16] =");
        for i in 0..16 {
            print!(" {}", gpu.data[0][2][i]);
        }
        println!("");
        println!(
            "Check: out[63] = {} (expected {})",
            gpu.data[0][2][63],
            a[63] + b[63]
        );

        gpu.release();
        println!("Finished releasing test_gpu");
    }
}

pub fn test_gpu() {    
    add_kernel();
}