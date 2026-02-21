use crate::gpu::{DeadbeefGpu, AddVectorGpu};
use crate::println;
use crate::print;

pub fn test_deadbeef() {
    unsafe {
        let gpu_ptr = DeadbeefGpu::init();
        let gpu = &mut *gpu_ptr;

        // let output_addr = gpu_ptr as u32 + 
            ((&gpu.output as *const _ as u32) - (gpu_ptr as u32));
        // gpu.unif[0][0] = crate::gpu::GPU_BASE + output_addr;

        println!("Memory before running code: {:x} {:x} {:x} {:x}", gpu.output[0], gpu.output[16], gpu.output[32], gpu.output[48]);
        gpu.execute(1);
        println!("Memory after running code: {:x} {:x} {:x} {:x}", gpu.output[0], gpu.output[16], gpu.output[32], gpu.output[48]);

        gpu.release();

        println!("Finished releasing test_gpu");
    }
}

pub fn test_add_vec() {
    unsafe {
        let gpu_ptr = AddVectorGpu::init();
        let gpu = &mut *gpu_ptr;
        let n = 10;
        let size = n * 4;
        let a = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let b = [10, 9, 8, 7, 6, 5, 4, 3, 2, 1];
        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0].as_mut_ptr(), size);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[1].as_mut_ptr(), size);
        println!("Memory before running code: ");
        
        println!("A:");
        for i in 0..n {
            print!("{} ", *(gpu.unif[0][0] as *mut u32));
        }
        println!();
        
        println!("B: ");
        for i in 0..n {
            print!("{} ", *(gpu.unif[0][1] as *mut u32));
        }
        println!();

        println!("C: ");
        for i in 0..n {
            print!("{} ", gpu.output[i]);
        }
        println!();
        
        gpu.execute(1);
        
        println!("Memory after running code: ");
        println!("A:");
        for i in 0..n {
            print!("{} ", *(gpu.unif[0][0] as *mut u32));
        }
        println!();
        
        println!("B: ");
        for i in 0..n {
            print!("{} ", *(gpu.unif[0][1] as *mut u32));
        }
        println!();

        println!("C: ");
        for i in 0..n {
            print!("{} ", gpu.output[i]);
        }
        println!();

        gpu.release();
        println!("Finished releasing add_vector test");
    }
}

pub fn test_gpu() {    
    // test_add_vec();
    test_deadbeef();
}