use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE, MATMUL_KERNEL_CODE, EXP_MAX_GPU_CODE, DMA_TEST_CODE};
use crate::matmul::{print_matrix};
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


pub fn exp_max_kernel() {
    
    
    unsafe {
        let gpu_ptr = GpuKernel::init(EXP_MAX_GPU_CODE);
        let gpu = &mut *gpu_ptr;

        let a: [f32; 64] = core::array::from_fn(|i| i as f32);
        let n = a.len(); // copy_nonoverlapping expects element count
        
        for (i, &f) in a.iter().enumerate() {
            // gpu.data[0][i] = f.to_bits();
            let f_u32: u32 = f.to_bits();
            gpu.data[0][i] = f_u32;
        }
        core::ptr::copy_nonoverlapping(a.as_ptr() as *const u32, gpu.data[0].as_mut_ptr(), n);

        print!("Input: a[0..64] = ");
        for i in 0..64 {
            print!(" {}", f32::from_bits(gpu.data[0][i]));
        }
        println!("");
        
        print!("Before: out[0..64] =");
        for i in 0..64 {
            print!(" {}", f32::from_bits(gpu.data[1][i]));
        }
        println!("");

        gpu.execute(1);

        print!("After: out[0..64] =");
        for i in 0..64 {
            print!(" {}", f32::from_bits(gpu.data[1][i]));
        }
        println!("");

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

pub fn test_dma() {
    unsafe {
        let gpu_ptr = GpuKernel::init(DMA_TEST_CODE);
        let gpu = &mut *gpu_ptr;

        let a: [u32; 1024] = core::array::from_fn(|i| i as u32); // arrange this as a 
        let b: [u32; 1024] = core::array::from_fn(|i| (i as u32) + 6);
        let n = 1024;

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0].as_mut_ptr(), n);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[1].as_mut_ptr(), n);

        gpu.unif[0][3] = 32 * 4 as u32;
        gpu.unif[0][4] = 32 * 4 as u32;
        gpu.unif[0][5] = 32 * 4 as u32;
        gpu.unif[0][6] = 2 as u32;
        gpu.unif[0][7] = 2 as u32;

        print!("A: ");
        print_matrix(&a, 32, 32);

        print!("B: ");
        print_matrix(&b, 32, 32);

        print!("Before: out[0..512] =");
        print_matrix(&gpu.data[2], 32, 32);
        println!("");

        gpu.execute(1);

        print!("After: out[0..512] =");
        print_matrix(&gpu.data[2], 32, 32);
        println!("");

        gpu.release();
        println!("Finished releasing test_gpu");
    }
}

pub fn test_gpu() {
    test_dma();
    // crate::matmul::matmul_func_test();
    // exp_max_kernel();
    // add_kernel();
}