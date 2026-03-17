use crate::fat32::{self, fat32_fs_t, pi_dirent_t};
use crate::kmalloc;
use crate::println;
use core::slice;
use crate::matmul::{matmul_with_gpu,print_float_matrix};
use crate::gpu::GpuKernel;
use crate::gpu::{DMA_TEST_CODE};

fn load_matrix_from_file(
    fs: &fat32_fs_t,
    root: &pi_dirent_t,
    filename: &str,
    expected_f32: usize,
) -> &'static [f32] {
    println!("Reading {}", filename);
    let read_back = fat32::fat32_read(fs, root, filename);
    assert!(!read_back.is_null());

    let read_ref = unsafe { &*read_back };
    assert!(read_ref.n_data % 4 == 0, "{} size is not a multiple of 4", filename);

    let n_f32 = read_ref.n_data / 4;
    assert!(n_f32 == expected_f32, "{} has {} floats, expected {}", filename, n_f32, expected_f32);

    let src = unsafe { slice::from_raw_parts(read_ref.data as *const u8, read_ref.n_data) };
    let out = unsafe { kmalloc::kmalloc_t::<f32>(n_f32) };
    for i in 0..n_f32 {
        let off = i * 4;
        let bits = [src[off], src[off + 1], src[off + 2], src[off + 3]];
        unsafe {
            *out.add(i) = f32::from_le_bytes(bits);
        }
    }
    let got = unsafe { slice::from_raw_parts(out as *const f32, n_f32) };

    println!("First 4 elements: {:?}", &got[..4.min(got.len())]);

    got
}

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
        let _dir = fat32::fat32_readdir(&fs, &root);

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
