use crate::fat32::{self, fat32_fs_t, pi_dirent_t, load_matrix_from_file};
use crate::kmalloc;
use crate::println;
use crate::matmul::{matmul_with_gpu,print_float_matrix};
use crate::gpu::GpuKernel;
use crate::gpu::{DMA_TEST_CODE};

pub fn matrix_load_test() {
    unsafe {
        let a_row = 16;
        let a_col = 32;
        let b_row = 32;
        let b_col = 16;

        println!("Reading the MBR.");
        let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");

        println!("Loading the FAT.");
        let fs = fat32::fat32_mk(&partition);

        println!("Loading the root directory.");
        let root = fat32::fat32_get_root(&fs);

        let a_data = load_matrix_from_file(&fs, &root, "A_TEST.BIN", a_row * a_col);
        let b_data = load_matrix_from_file(&fs, &root, "B_TEST.BIN", b_row * b_col);

        let gpu_ptr = GpuKernel::new();
        let gpu = &mut *gpu_ptr;
        gpu.load_code(DMA_TEST_CODE);

        let mut c: [f32; 16 * 16] = [0.0; 16 * 16];
        matmul_with_gpu(gpu, a_data, b_data, &mut c, a_row, b_col, a_col);

        print_float_matrix(&c, a_row, b_col);

        println!("PASS: {}", file!());
    }
}
