use crate::fat32::{self, fat32_fs_t, pi_dirent_t};
use core::slice;
use crate::kmalloc;
use crate::println;

pub fn load_matrix_from_file(
    fs: &fat32_fs_t,
    root: &pi_dirent_t,
    filename: &str,
    expected_f32: usize,
) -> &'static [f32] {
    assert!(cfg!(target_endian = "little"), "requires little-endian target");

    println!("Reading {}", filename);
    let read_back = fat32::fat32_read(fs, root, filename);
    assert!(!read_back.is_null());

    let read_ref = unsafe { &*read_back };
    assert!(read_ref.n_data % 4 == 0, "{} size is not a multiple of 4", filename);

    let n_f32 = read_ref.n_data / 4;
    assert!(n_f32 == expected_f32, "{} has {} floats, expected {}", filename, n_f32, expected_f32);

    let src_ptr = read_ref.data as *const u8;
    let got = unsafe { slice::from_raw_parts(src_ptr as *const f32, n_f32) };
    println!("First 4 elements: {:?}", &got[..4.min(got.len())]);
    got
}
