use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE, EXP_MAX_GPU_CODE, DMA_TEST_CODE};
use crate::matmul::{print_matrix, cpu_matmul};
use crate::{print, println};

pub fn deadbeef_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DEADBEEF_GPU_CODE);

        println!("Memory before running code: {:x} {:x} {:x} {:x}", gpu.data[0][0], gpu.data[0][16], gpu.data[0][32], gpu.data[0][48]);
        gpu.execute(1);
        println!("Memory after running code: {:x} {:x} {:x} {:x}", gpu.data[0][0], gpu.data[0][16], gpu.data[0][32], gpu.data[0][48]);

        gpu.release();

        println!("Finished releasing test_gpu");
    }
}


pub fn exp_max_kernel() {
    
    
    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(EXP_MAX_GPU_CODE);

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
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(ADD_KERNEL_CODE);

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
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DMA_TEST_CODE);

        let a: [u32; 1024] = core::array::from_fn(|i| i as u32); // arrange this as a 
        let b: [u32; 1024] = core::array::from_fn(|i| (i as u32) + 6);
        let n = 1024;

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0].as_mut_ptr(), n);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[1].as_mut_ptr(), n);

        gpu.unif[0][3] = 2 as u32;
        gpu.unif[0][4] = 0 as u32;
        gpu.unif[0][5] = 0 as u32;
        gpu.unif[0][6] = 2 as u32;
        gpu.unif[0][7] = 2 as u32;
        gpu.unif[0][8] = 2 as u32;
        gpu.unif[0][9] = 0 as u32;

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


pub fn test_mini_matmul() {
    unsafe {
        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DMA_TEST_CODE);
        
        const N: usize = 32;

        let a: [f32; N * N] = core::array::from_fn(|i| i as f32);
        let b: [f32; N * N] = core::array::from_fn(|i| (i as f32) + 6.0);
        let n = N * N;

        gpu.data_slot_as_mut_f32(0)[..n].copy_from_slice(&a);
        gpu.data_slot_as_mut_f32(1)[..n].copy_from_slice(&b);

        gpu.unif[0][3] = (N / 16) as u32;
        gpu.unif[0][4] = 0 as u32;
        gpu.unif[0][5] = 0 as u32;
        gpu.unif[0][6] = (N / 16) as u32;
        gpu.unif[0][7] = (N / 16) as u32;
        gpu.unif[0][8] = (N / 16) as u32;
        gpu.unif[0][9] = 0 as u32;

        print!("A[0..16]:");
        for i in 0..16 {
            print!(" {:.2}", a[i]);
        }
        println!("");

        print!("B[0..16]:");
        for i in 0..16 {
            print!(" {:.2}", b[i]);
        }
        println!("");
        
        let mut c_cpu: [f32; N * N] = [0.0; N * N];
        cpu_matmul(&a, &b, &mut c_cpu, N, N, N);


        print!("Before: out[0..512] =");
        print_matrix(&gpu.data[2], N, N);
        println!("");

        gpu.execute(1);

        print!("After: out[0..16] =");
        for i in 0..16 {
            print!(" {:.2}", gpu.data_slot_as_f32(2)[i]);
        }
        println!("");
        
        print!("Baseline[0..16]: ");
        for i in 0..16 {
            print!(" {:.2}", c_cpu[i]);
        }
        println!("");
        
        let mut matches = true;
        for i in 0..(N * N) {
                if (gpu.data_slot_as_f32(2)[i] - c_cpu[i]).abs() > 1e-3 {
                println!(
                    "Discrepancy at index {}: expected {}, observed {}",
                    i, c_cpu[i], gpu.data_slot_as_f32(2)[i]
                );
                matches = false;
                break;
            }
        }
        
        if matches {
            println!("Matrix outputs match: YES");
        } else {
            println!("Matrix outputs match: NO");
        }

        gpu.release();
        println!("Finished releasing test_gpu");
    }
}


pub fn test_gpu() {
    // test_mini_matmul();
    // test_dma();
    crate::matmul::matmul_func_test();
    // exp_max_kernel();
    // add_kernel();
}