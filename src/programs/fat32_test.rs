use crate::fat32::{self, pi_file_t};
use crate::println;
use core::slice;
use core::str;

pub fn fat32_test() {
    fat32::pi_sd_init();

    println!("Reading the MBR.");
    let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");

    println!("Loading the FAT.");
    let fs = fat32::fat32_mk(&partition);

    println!("Loading the root directory.");
    let root = fat32::fat32_get_root(&fs);
    let _dir = fat32::fat32_readdir(&fs, &root);

    println!("Creating HELLO.TXT");
    let _ = fat32::fat32_delete(&fs, &root, "HELLO.TXT");
    let created = fat32::fat32_create(&fs, &root, "HELLO.TXT", 0);
    assert!(!created.is_null());

    let hello = b"hello world\n";
    let file = pi_file_t {
        data: hello.as_ptr() as *mut u8,
        n_alloc: hello.len(),
        n_data: hello.len(),
    };

    println!("Writing HELLO.TXT");
    let wrote = fat32::fat32_write(&fs, &root, "HELLO.TXT", &file);
    assert!(wrote == 1);

    println!("Reading HELLO.TXT");
    let read_back = fat32::fat32_read(&fs, &root, "HELLO.TXT");
    assert!(!read_back.is_null());

    let read_ref = unsafe { &*read_back };
    assert!(read_ref.n_data == hello.len());
    let got = unsafe { slice::from_raw_parts(read_ref.data as *const u8, read_ref.n_data) };
    match str::from_utf8(got) {
        Ok(s) => println!("HELLO.TXT contents: {}", s),
        Err(_) => println!("HELLO.TXT contents are non-UTF8 ({} bytes)", got.len()),
    }
    assert!(got == hello);

    println!("PASS: {}", file!());
}
