use crate::gpu::{GpuKernel, DEADBEEF_GPU_CODE, ADD_KERNEL_CODE, MATMUL_KERNEL_CODE};
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

        let a: [u32; 128] = core::array::from_fn(|i| i as u32);
        let b: [u32; 128] = core::array::from_fn(|i| (i as u32) + 6);
        let n = a.len(); // copy_nonoverlapping expects element count

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0][0].as_mut_ptr(), n);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[0][1].as_mut_ptr(), n);
        gpu.unif[0][3] = n as u32;

        print!("Before: out[0..128] =");
        for i in 0..128 {
            print!(" {}", gpu.data[0][2][i]);
        }
        println!("");

        gpu.execute();

        print!("After: out[0..128] =");
        for i in 0..128 {
            print!(" {}", gpu.data[0][2][i]);
        }
        println!("");

        gpu.release();
        println!("Finished releasing test_gpu");
    }
}

fn matrix_transpose(data: &[u32], out: &mut [u32], n: usize, m: usize) {
    if data.len() != n * m || out.len() != n * m {
        return;
    }

    for i in 0..n {
        for j in 0..m {
            out[j * n + i] = data[i * m + j];
        }
    }
}

fn print_matrix(data: &[u32], n: usize, m: usize) {
    for i in 0..n {
        for j in 0..m {
            print!("{} ", data[i * m + j]);
        }
        print!("\n");
    }
}

pub fn matmul_kernel() {
    unsafe {
        let gpu_ptr = GpuKernel::init(MATMUL_KERNEL_CODE);
        let gpu = &mut *gpu_ptr;

        const NUM_ROWS: usize = 16;
        const NUM_COLS: usize = 16;
        let a: [u32; NUM_ROWS * NUM_COLS] = core::array::from_fn(|i| i as u32);
        let b: [u32; NUM_COLS * NUM_ROWS] = core::array::from_fn(|i| (i as u32) + 6);
        
        let mut b_t: [u32; NUM_ROWS * NUM_COLS] = [0; NUM_ROWS * NUM_COLS];
        matrix_transpose(&b, &mut b_t, NUM_COLS, NUM_ROWS);
        
        println!("Matrix a ({} x {}):", NUM_ROWS, NUM_COLS);
        print_matrix(&a, NUM_ROWS, NUM_COLS);
        println!("Matrix b ({} x {}):", NUM_COLS, NUM_ROWS);
        print_matrix(&b, NUM_COLS, NUM_ROWS);
        println!("Matrix b, transposed ({} x {}):", NUM_ROWS, NUM_COLS);
        print_matrix(&b_t, NUM_ROWS, NUM_COLS);

        let mut c: [u32; NUM_ROWS * NUM_ROWS] = [0; NUM_ROWS * NUM_ROWS];
        for i in 0..NUM_ROWS {
            for j in 0..NUM_ROWS {
                let mut sum: u32 = 0;
                for k in 0..NUM_COLS {
                    sum = sum.wrapping_add(a[i * NUM_COLS + k].wrapping_mul(b_t[j * NUM_COLS + k]));
                }
                c[i * NUM_ROWS + j] = sum;
            }
        }
        println!("Expected matmul result ({} x {}):", NUM_ROWS, NUM_ROWS);
        print_matrix(&c, NUM_ROWS, NUM_ROWS);

        core::ptr::copy_nonoverlapping(a.as_ptr(), gpu.data[0][0].as_mut_ptr(), NUM_ROWS * NUM_COLS);
        core::ptr::copy_nonoverlapping(b.as_ptr(), gpu.data[0][1].as_mut_ptr(), NUM_ROWS * NUM_COLS);
        gpu.unif[0][3] = (NUM_ROWS * NUM_ROWS) as u32;

        print!("Before out:\n");
        print_matrix(&gpu.data[0][2], NUM_ROWS, NUM_ROWS);

        gpu.execute();

        print!("After out:\n");
        print_matrix(&gpu.data[0][2], NUM_ROWS, NUM_ROWS);

        gpu.release();
        println!("Finished releasing matmul test");
    }
}


pub fn test_gpu() {    
    // add_kernel();
    matmul_kernel();
}