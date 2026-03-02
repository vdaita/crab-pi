use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE, MATMUL_KERNEL_CODE, EXP_MAX_GPU_CODE};
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
        
        let a: [f32; 16] = core::array::from_fn(|i| i as f32);
        let n = a.len();
    
        print!("A:\n");
        for i in 0..n {
            print!("{} ", a[i]);
        }
        print!("\n");
        
        // Cast f32 pointer to u32 pointer
        core::ptr::copy_nonoverlapping(
            a.as_ptr() as *const u32,
            gpu.data[0].as_mut_ptr(),
            n
        );
        
        gpu.unif[0][0] = gpu.get_data_ptr(0) as u32;
        gpu.unif[0][1] = gpu.get_data_ptr(1) as u32;
        
        let mut target: [f32; 16] = [0.0f32; 16]; 
        for i in 0..n {
            target[i] = 1.0 + a[i] + (a[i] * a[i] / 2.0);
        }
        
        print!("target:\n");
        for i in 0..n {
            print!("{} ", target[i]);
        }
        print!("\n");
        
        gpu.execute(1);
        
        print!("result:\n");
        for i in 0..n {
            let float_res = f32::from_bits(gpu.data[1][i]);
            print!("{} ", float_res);
        }
        print!("\n");
        
        for i in 0..n {
            let float_res = f32::from_bits(gpu.data[1][i]);
            if (target[i] - float_res).abs() > 1e-5 {
                println!("Discrepancy at position {}, target={}, result={}", i, target[i], float_res);
            }
        }
        
        gpu.release();
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
    // crate::matmul::matmul_func_test();
    exp_max_kernel();
}