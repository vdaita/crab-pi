use crate::fat32::fs;
use crate::kmalloc;

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
    // worst case per record: 19-byte header + 15-byte name + NUL, rounded up to 8
    let alloc_size = dir.ndirents * 40;

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
