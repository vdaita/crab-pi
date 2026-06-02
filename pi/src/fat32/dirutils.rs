use crate::fat32::fs;
use crate::fat32;
use crate::kmalloc;
use crate::println;
use core::ffi::CStr;

#[repr(C, packed)]
pub struct LinuxDirent64 {
    pub d_ino: u64, // inode number
    pub d_off: i64, // offset to next dirent
    pub d_reclen: u16, // length of this record
    pub d_type: u8 // file type
}

fn cstr_len(buf: &[u8]) -> usize {
    let mut len = 0usize;
    while len < buf.len() && buf[len] != 0 {
        len += 1;
    }
    len
}

pub fn get_dirents64_as_file(dir: &fs::pi_directory_t) -> fs::pi_file_t {
    let mut curr_index = 0;
    let mut buffer_offset = 0;
    let alloc_size = 8192;

    let buffer = unsafe { kmalloc::kmalloc(alloc_size) }; // save this in the kernel buffer

    while curr_index < dir.ndirents {
        let entry = unsafe { &*dir.dirents.add(curr_index) };
        let name_len = cstr_len(&entry.name);

        let base_dirent_size = core::mem::size_of::<LinuxDirent64>();
        let record_size = ((base_dirent_size + name_len + 1) + 7) & !7;

        unsafe {
            let curr_ptr = buffer.add(buffer_offset);
            let dirent = LinuxDirent64 {
                d_ino: entry.cluster_id as u64,
                d_off: (curr_index + 1) as i64,
                d_reclen: record_size as u16,
                d_type: if entry.is_dir_p != 0 { 4 } else { 8 } // 4=dir, 8=reg
            };
            core::ptr::write_unaligned(curr_ptr as *mut LinuxDirent64, dirent);
            core::ptr::copy_nonoverlapping(entry.name.as_ptr(), curr_ptr.add(base_dirent_size), name_len);
            core::ptr::write_bytes(curr_ptr.add(base_dirent_size + name_len), 0, record_size - (base_dirent_size + name_len));
        }

        buffer_offset += record_size;
        curr_index += 1;
    }

    fs::pi_file_t {
        data: buffer,
        n_data: buffer_offset,
        n_alloc: alloc_size
    }
}

pub fn get_dir_listing_as_file(dir: &fs::pi_directory_t) -> fs::pi_file_t {
    let mut total_size = 0usize;

    for i in 0..dir.ndirents {
        let entry = unsafe { &*dir.dirents.add(i) };
        let name_len = cstr_len(&entry.name);
        if name_len > 0 {
            total_size += name_len + 1; // name + '\n'
        }
    }

    if total_size == 0 {
        return fs::pi_file_t {
            data: core::ptr::null_mut(),
            n_data: 0,
            n_alloc: 0,
        };
    }

    let buffer = unsafe { kmalloc::kmalloc(total_size) as *mut u8 };
    let mut offset = 0usize;

    for i in 0..dir.ndirents {
        let entry = unsafe { &*dir.dirents.add(i) };
        let name_len = cstr_len(&entry.name);

        if name_len > 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    entry.name.as_ptr(),
                    buffer.add(offset),
                    name_len
                );
                offset += name_len;

                *buffer.add(offset) = b'\n';
                offset += 1;
            }
        }
    }

    fs::pi_file_t {
        data: buffer,
        n_data: total_size,
        n_alloc: total_size,
    }
}