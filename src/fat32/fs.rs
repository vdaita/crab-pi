#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use super::helpers::*;
use super::sd;
use crate::kmalloc;
use core::ptr::{self, copy_nonoverlapping};

const NBYTES_PER_SECTOR: u32 = 512;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct pi_file_t {
    pub data: *mut u8,
    pub n_alloc: usize,
    pub n_data: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct pi_dirent_t {
    pub name: [u8; 16],
    pub raw_name: [u8; 16],
    pub cluster_id: u32,
    pub is_dir_p: u32,
    pub nbytes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct pi_directory_t {
    pub dirents: *mut pi_dirent_t,
    pub ndirents: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct fat32_fs_t {
    pub lba_start: u32,
    pub fat_begin_lba: u32,
    pub cluster_begin_lba: u32,
    pub sectors_per_cluster: u32,
    pub root_dir_first_cluster: u32,
    pub fat: *mut u32,
    pub n_entries: u32,
}

static mut INIT_P: bool = false;
static mut BOOT_SECTOR: fat32_boot_sec_t = fat32_boot_sec_t::zeroed();
static mut READ_PROGRESS_P: bool = false;
static mut READ_PROGRESS_EVERY: u32 = 1;

pub fn fat32_read_progress(on_p: bool) -> bool {
    let old = unsafe { READ_PROGRESS_P };
    unsafe { READ_PROGRESS_P = on_p; }
    old
}

pub fn fat32_read_progress_every(mut n: u32) -> u32 {
    if n == 0 {
        n = 1;
    }
    let old = unsafe { READ_PROGRESS_EVERY };
    unsafe { READ_PROGRESS_EVERY = n; }
    old
}

fn trace_enabled() -> bool {
    sd::trace_enabled()
}
pub fn fat32_mk(partition: &mbr_partition_ent_t) -> fat32_fs_t {
    demand(unsafe { !INIT_P }, "the fat32 module is already in use\n");

    unsafe {
        let boot_ptr = core::ptr::addr_of_mut!(BOOT_SECTOR);
        let res = sd::pi_sd_read(
            boot_ptr as *mut u8,
            partition.lba_start,
            1,
        );
        if res != 1 {
            panic!("failed to read boot sector");
        }
        let boot = *core::ptr::addr_of!(BOOT_SECTOR);
        fat32_volume_id_check(&boot);

        assert!(boot.info_sec_num == 1);
        let mut info = fsinfo {
            sig1: 0,
            _reserved0: [0; 480],
            sig2: 0,
            free_cluster_count: 0,
            next_free_cluster: 0,
            _reserved1: [0; 12],
            sig3: 0,
        };
        let ok = sd::pi_sd_read(&mut info as *mut _ as *mut u8, partition.lba_start + 1, 1);
        if ok != 1 {
            panic!("failed to read fsinfo");
        }
        fat32_fsinfo_check(&info);
        fat32_volume_id_print("volume id", &boot);
        fat32_fsinfo_print("fsinfo", &info);

        let lba_start = partition.lba_start;
        let fat_begin_lba = partition.lba_start + boot.reserved_area_nsec as u32;
        let cluster_begin_lba =
            fat_begin_lba + (boot.nfats as u32) * boot.nsec_per_fat;
        let sec_per_cluster = boot.sec_per_cluster as u32;
        let root_first_cluster = boot.first_cluster;
        let n_entries = (boot.nsec_per_fat * boot.bytes_per_sec as u32) / 4;

        let fat = kmalloc::kmalloc_t::<u32>(n_entries as usize);
        let ok = sd::pi_sd_read(fat as *mut u8, fat_begin_lba, boot.nsec_per_fat);
        if ok != 1 {
            panic!("failed to read FAT");
        }

        let fs = fat32_fs_t {
            lba_start,
            fat_begin_lba,
            cluster_begin_lba,
            sectors_per_cluster: sec_per_cluster,
            root_dir_first_cluster: root_first_cluster,
            fat,
            n_entries,
        };

        if trace_enabled() {
            crate::println!("begin lba = {}", fs.fat_begin_lba);
            crate::println!("cluster begin lba = {}", fs.cluster_begin_lba);
            crate::println!("sectors per cluster = {}", fs.sectors_per_cluster);
            crate::println!("root dir first cluster = {}", fs.root_dir_first_cluster);
        }

        INIT_P = true;
        fs
    }
}

fn cluster_to_lba(f: &fat32_fs_t, cluster_num: u32) -> u32 {
    assert!(cluster_num >= 2);
    let lba = f.cluster_begin_lba + (cluster_num - 2) * f.sectors_per_cluster;
    if trace_enabled() {
        crate::println!("cluster {} to lba: {}", cluster_num, lba);
    }
    lba
}

pub fn fat32_get_root(fs: &fat32_fs_t) -> pi_dirent_t {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    let dirent = pi_dirent_t {
        name: [0; 16],
        raw_name: [0; 16],
        cluster_id: fs.root_dir_first_cluster,
        is_dir_p: 1,
        nbytes: 0,
    };
    dirent
}

fn get_cluster_chain_length(fs: &fat32_fs_t, start_cluster: u32) -> u32 {
    let mut count = 0;
    let mut curr = start_cluster;
    loop {
        assert!(curr < fs.n_entries);
        count += 1;
        let next = unsafe { *fs.fat.add(curr as usize) };
        let t = fat32_fat_entry_type(next);
        if t == LAST_CLUSTER {
            break;
        }
        if t != USED_CLUSTER {
            panic!("FAT entry type unexpected {}", fat32_fat_entry_type_str(t));
        }
        curr = next;
    }
    count
}

fn read_cluster_chain(fs: &fat32_fs_t, start_cluster: u32, data: *mut u8) {
    let mut curr = start_cluster;
    let bytes_per_cluster = fs.sectors_per_cluster * NBYTES_PER_SECTOR;
    let mut off = 0u32;
    let progress = unsafe { READ_PROGRESS_P };
    let progress_every = unsafe { READ_PROGRESS_EVERY };
    let total_clusters = if progress {
        get_cluster_chain_length(fs, start_cluster)
    } else {
        0
    };
    let mut cluster_idx = 0u32;

    loop {
        let lba = cluster_to_lba(fs, curr);
        if progress
            && (cluster_idx == 0
                || cluster_idx + 1 == total_clusters
                || ((cluster_idx + 1) % progress_every) == 0)
        {
            crate::println!(
                "[fat32.read] cluster {}/{}: id={}, lba={}, off={}B",
                cluster_idx + 1,
                total_clusters,
                curr,
                lba,
                off
            );
        }
        let ok = sd::pi_sd_read(
            unsafe { data.add(off as usize) },
            lba,
            fs.sectors_per_cluster,
        );
        demand(ok == 1, "FAT read failed\n");
        off += bytes_per_cluster;
        cluster_idx += 1;

        let next = unsafe { *fs.fat.add(curr as usize) };
        let t = fat32_fat_entry_type(next);
        if t == LAST_CLUSTER {
            break;
        }
        if t != USED_CLUSTER {
            panic!("unexpected FAT entry type");
        }
        curr = next;
    }

    if progress {
        crate::println!("[fat32.read] done: {} clusters, {} bytes", cluster_idx, off);
    }
}

fn dirent_convert(d: &fat32_dirent_t) -> pi_dirent_t {
    let mut e = pi_dirent_t {
        name: [0; 16],
        raw_name: [0; 16],
        cluster_id: fat32_cluster_id(d),
        is_dir_p: if d.attr == FAT32_DIR { 1 } else { 0 },
        nbytes: d.file_nbytes,
    };
    e.raw_name[..11].copy_from_slice(&d.filename);
    let mut name = [0u8; 13];
    let len = fat32_dirent_name(d, &mut name);
    let copy_len = core::cmp::min(len, e.name.len() - 1);
    e.name[..copy_len].copy_from_slice(&name[..copy_len]);
    e.name[copy_len] = 0;
    e
}

fn get_dirents(fs: &fat32_fs_t, cluster_start: u32, dir_n: &mut u32) -> *mut fat32_dirent_t {
    let n_clusters = get_cluster_chain_length(fs, cluster_start);
    let bytes_per_cluster = fs.sectors_per_cluster * NBYTES_PER_SECTOR;
    let total_bytes = n_clusters * bytes_per_cluster;
    let buf = unsafe { kmalloc::kmalloc(total_bytes as usize) };
    read_cluster_chain(fs, cluster_start, buf);
    *dir_n = total_bytes / (size_of::<fat32_dirent_t>() as u32);
    buf as *mut fat32_dirent_t
}

pub fn fat32_readdir(fs: &fat32_fs_t, dirent: &pi_dirent_t) -> pi_directory_t {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    demand(dirent.is_dir_p != 0, "tried to readdir a file!");

    let mut n_dirents = 0u32;
    let dirents = get_dirents(fs, dirent.cluster_id, &mut n_dirents);

    let mut count = 0usize;
    for i in 0..n_dirents as usize {
        let d = unsafe { &*dirents.add(i) };
        if fat32_dirent_free(d) || fat32_dirent_is_lfn(d) || (d.attr & FAT32_VOLUME_LABEL) != 0 {
            continue;
        }
        count += 1;
    }

    let out = unsafe { kmalloc::kmalloc_t::<pi_dirent_t>(count) };
    let mut j = 0usize;
    for i in 0..n_dirents as usize {
        let d = unsafe { &*dirents.add(i) };
        if fat32_dirent_free(d) || fat32_dirent_is_lfn(d) || (d.attr & FAT32_VOLUME_LABEL) != 0 {
            continue;
        }
        unsafe {
            *out.add(j) = dirent_convert(d);
        }
        j += 1;
    }

    pi_directory_t {
        dirents: out,
        ndirents: count,
    }
}

fn find_dirent_with_name(dirents: *const fat32_dirent_t, n: u32, filename: &str) -> isize {
    let target = filename.as_bytes();
    for i in 0..n as usize {
        let d = unsafe { &*dirents.add(i) };
        if fat32_dirent_free(d) || fat32_dirent_is_lfn(d) || (d.attr & FAT32_VOLUME_LABEL) != 0 {
            continue;
        }
        let mut name = [0u8; 13];
        let len = fat32_dirent_name(d, &mut name);
        let name_slice = &name[..len.saturating_sub(1)];
        if name_slice == target {
            return i as isize;
        }
    }
    -1
}

pub fn fat32_stat(fs: &fat32_fs_t, directory: &pi_dirent_t, filename: &str) -> *mut pi_dirent_t {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    demand(
        directory.is_dir_p != 0,
        "tried to use a file as a directory",
    );

    let mut n_dirents = 0u32;
    let dirents = get_dirents(fs, directory.cluster_id, &mut n_dirents);

    let idx = find_dirent_with_name(dirents, n_dirents, filename);
    if idx < 0 {
        return ptr::null_mut();
    }

    let dirent = unsafe { kmalloc::kmalloc_t::<pi_dirent_t>(1) };
    unsafe {
        *dirent = dirent_convert(&*dirents.add(idx as usize));
    }
    dirent
}

pub fn fat32_read(fs: &fat32_fs_t, directory: &pi_dirent_t, filename: &str) -> *mut pi_file_t {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    demand(
        directory.is_dir_p != 0,
        "tried to use a file as a directory!",
    );

    let d = fat32_stat(fs, directory, filename);
    if d.is_null() {
        return ptr::null_mut();
    }
    let d_ref = unsafe { &*d };
    if d_ref.nbytes == 0 {
        let file = unsafe { kmalloc::kmalloc_t::<pi_file_t>(1) };
        unsafe {
            *file = pi_file_t {
                data: ptr::null_mut(),
                n_alloc: 0,
                n_data: 0,
            };
        }
        return file;
    }

    let n_clusters = get_cluster_chain_length(fs, d_ref.cluster_id);
    let bytes_per_cluster = fs.sectors_per_cluster * NBYTES_PER_SECTOR;
    let total_bytes = (n_clusters * bytes_per_cluster) as usize;
    let buf = unsafe { kmalloc::kmalloc(total_bytes) };
    read_cluster_chain(fs, d_ref.cluster_id, buf);

    let file = unsafe { kmalloc::kmalloc_t::<pi_file_t>(1) };
    unsafe {
        *file = pi_file_t {
            data: buf,
            n_alloc: total_bytes,
            n_data: d_ref.nbytes as usize,
        };
    }
    file
}

pub fn fat32_read_from_dirent(fs: &fat32_fs_t, dirent: &pi_dirent_t) -> *mut pi_file_t {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    demand(dirent.is_dir_p == 0, "fat32_read_from_dirent: expected a file dirent");

    if dirent.nbytes == 0 {
        let file = unsafe { kmalloc::kmalloc_t::<pi_file_t>(1) };
        unsafe {
            *file = pi_file_t {
                data: ptr::null_mut(),
                n_alloc: 0,
                n_data: 0,
            };
        }
        return file;
    }

    let n_clusters = get_cluster_chain_length(fs, dirent.cluster_id);
    let bytes_per_cluster = fs.sectors_per_cluster * NBYTES_PER_SECTOR;
    let total_bytes = (n_clusters * bytes_per_cluster) as usize;
    let buf = unsafe { kmalloc::kmalloc(total_bytes) };
    read_cluster_chain(fs, dirent.cluster_id, buf);

    let file = unsafe { kmalloc::kmalloc_t::<pi_file_t>(1) };
    unsafe {
        *file = pi_file_t {
            data: buf,
            n_alloc: total_bytes,
            n_data: dirent.nbytes as usize,
        };
    }
    file
}

fn find_free_cluster(fs: &fat32_fs_t, mut start_cluster: u32) -> u32 {
    if start_cluster < 3 {
        start_cluster = 3;
    }
    for i in start_cluster..fs.n_entries {
        let val = unsafe { *fs.fat.add(i as usize) };
        let t = fat32_fat_entry_type(val);
        if t == FREE_CLUSTER {
            return i;
        }
    }
    if trace_enabled() {
        crate::println!("failed to find free cluster from {}", start_cluster);
    }
    panic!("No more clusters on the disk!\n");
}

fn write_fat_to_disk(fs: &fat32_fs_t) {
    if trace_enabled() {
        crate::println!("syncing FAT");
    }
    let ok = sd::pi_sd_write(fs.fat as *const u8, fs.fat_begin_lba, unsafe {
        BOOT_SECTOR.nsec_per_fat
    });
    if ok != 1 {
        panic!("write FAT failed");
    }
}

fn free_cluster_chain(fs: &fat32_fs_t, start_cluster: u32) {
    if start_cluster < 2 {
        return;
    }

    let mut curr = start_cluster;
    loop {
        demand(curr < fs.n_entries, "cluster index out of range\n");
        let next = unsafe { *fs.fat.add(curr as usize) };
        let t = fat32_fat_entry_type(next);

        unsafe {
            *fs.fat.add(curr as usize) = FREE_CLUSTER;
        }

        if t == LAST_CLUSTER || t == FREE_CLUSTER {
            break;
        }
        if t != USED_CLUSTER {
            break;
        }
        curr = next;
    }
}

fn write_cluster_chain(fs: &fat32_fs_t, start_cluster: u32, data: *const u8, nbytes: u32) {
    if nbytes == 0 {
        return;
    }
    demand(
        start_cluster >= 2,
        "write_cluster_chain: invalid start cluster\n",
    );

    let bytes_per_cluster = fs.sectors_per_cluster * NBYTES_PER_SECTOR;
    let mut bytes_left = nbytes;
    let mut curr = start_cluster;
    let mut last_used = start_cluster;
    let mut data_ptr = data;

    while bytes_left > 0 && curr >= 2 {
        let lba = cluster_to_lba(fs, curr);
        let n = if bytes_left < bytes_per_cluster {
            bytes_left
        } else {
            bytes_per_cluster
        };

        if n == bytes_per_cluster {
            let ok = sd::pi_sd_write(data_ptr, lba, fs.sectors_per_cluster);
            demand(ok == 1, "write_cluster_chain: SD write failed\n");
        } else {
            let need = n as usize;
            let buf = unsafe { kmalloc::kmalloc(bytes_per_cluster as usize) };
            unsafe {
                ptr::write_bytes(buf, 0, bytes_per_cluster as usize);
                copy_nonoverlapping(data_ptr, buf, need);
            }
            let ok = sd::pi_sd_write(buf as *const u8, lba, fs.sectors_per_cluster);
            demand(ok == 1, "write_cluster_chain: SD write failed (partial)\n");
        }

        unsafe { data_ptr = data_ptr.add(n as usize) };
        bytes_left -= n;
        last_used = curr;

        if bytes_left == 0 {
            break;
        }

        let next = unsafe { *fs.fat.add(curr as usize) };
        let t = fat32_fat_entry_type(next);
        if t == USED_CLUSTER {
            curr = next;
            continue;
        }
        break;
    }

    while bytes_left > 0 {
        let new_cluster = find_free_cluster(fs, last_used + 1);
        unsafe {
            *fs.fat.add(last_used as usize) = new_cluster;
        }

        let lba = cluster_to_lba(fs, new_cluster);
        let n = if bytes_left < bytes_per_cluster {
            bytes_left
        } else {
            bytes_per_cluster
        };

        if n == bytes_per_cluster {
            let ok = sd::pi_sd_write(data_ptr, lba, fs.sectors_per_cluster);
            demand(ok == 1, "write_cluster_chain: SD write failed (extend)\n");
        } else {
            let need = n as usize;
            let buf = unsafe { kmalloc::kmalloc(bytes_per_cluster as usize) };
            unsafe {
                ptr::write_bytes(buf, 0, bytes_per_cluster as usize);
                copy_nonoverlapping(data_ptr, buf, need);
            }
            let ok = sd::pi_sd_write(buf as *const u8, lba, fs.sectors_per_cluster);
            demand(
                ok == 1,
                "write_cluster_chain: SD write failed (extend partial)\n",
            );
        }

        unsafe { data_ptr = data_ptr.add(n as usize) };
        bytes_left -= n;
        last_used = new_cluster;
    }

    let next = unsafe { *fs.fat.add(last_used as usize) };
    let t = fat32_fat_entry_type(next);
    if t == USED_CLUSTER {
        free_cluster_chain(fs, next);
    }

    unsafe {
        *fs.fat.add(last_used as usize) = LAST_CLUSTER;
    }
}

pub fn fat32_rename(fs: &fat32_fs_t, directory: &pi_dirent_t, oldname: &str, newname: &str) -> i32 {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    if trace_enabled() {
        crate::println!("renaming {} to {}", oldname, newname);
    }
    if !fat32_is_valid_name(newname) {
        return 0;
    }

    let mut n_dirents = 0u32;
    let dirents = get_dirents(fs, directory.cluster_id, &mut n_dirents);
    let old_idx = find_dirent_with_name(dirents, n_dirents, oldname);
    if old_idx < 0 {
        return 0;
    }
    let new_idx = find_dirent_with_name(dirents, n_dirents, newname);
    if new_idx >= 0 {
        return 0;
    }

    unsafe {
        fat32_dirent_set_name(&mut *dirents.add(old_idx as usize), newname);
    }
    write_cluster_chain(
        fs,
        directory.cluster_id,
        dirents as *const u8,
        n_dirents * size_of::<fat32_dirent_t>() as u32,
    );
    1
}

pub fn fat32_create(
    fs: &fat32_fs_t,
    directory: &pi_dirent_t,
    filename: &str,
    is_dir: i32,
) -> *mut pi_dirent_t {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    if trace_enabled() {
        crate::println!("creating {}", filename);
    }
    if !fat32_is_valid_name(filename) {
        return ptr::null_mut();
    }

    let mut n_dirents = 0u32;
    let dirents = get_dirents(fs, directory.cluster_id, &mut n_dirents);
    if find_dirent_with_name(dirents, n_dirents, filename) >= 0 {
        return ptr::null_mut();
    }

    let mut free_idx: isize = -1;
    for i in 0..n_dirents as usize {
        let d = unsafe { &*dirents.add(i) };
        if fat32_dirent_free(d) {
            free_idx = i as isize;
            break;
        }
    }
    if free_idx < 0 {
        return ptr::null_mut();
    }

    let d = unsafe { &mut *dirents.add(free_idx as usize) };
    unsafe {
        ptr::write_bytes(d as *mut _ as *mut u8, 0, size_of::<fat32_dirent_t>());
    }
    fat32_dirent_set_name(d, filename);
    d.attr = if is_dir != 0 {
        FAT32_DIR
    } else {
        FAT32_ARCHIVE
    };
    d.hi_start = 0;
    d.lo_start = 0;
    d.file_nbytes = 0;

    write_cluster_chain(
        fs,
        directory.cluster_id,
        dirents as *const u8,
        n_dirents * size_of::<fat32_dirent_t>() as u32,
    );

    let dirent = unsafe { kmalloc::kmalloc_t::<pi_dirent_t>(1) };
    unsafe {
        *dirent = dirent_convert(d);
    }
    dirent
}

pub fn fat32_delete(fs: &fat32_fs_t, directory: &pi_dirent_t, filename: &str) -> i32 {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    if trace_enabled() {
        crate::println!("deleting {}", filename);
    }
    if !fat32_is_valid_name(filename) {
        return 0;
    }

    let mut n_dirents = 0u32;
    let dirents = get_dirents(fs, directory.cluster_id, &mut n_dirents);
    let idx = find_dirent_with_name(dirents, n_dirents, filename);
    if idx < 0 {
        return 0;
    }

    let d = unsafe { &mut *dirents.add(idx as usize) };
    let start_cluster = fat32_cluster_id(d);
    d.filename[0] = 0xE5;

    if start_cluster >= 2 {
        write_cluster_chain(
            fs,
            directory.cluster_id,
            dirents as *const u8,
            n_dirents * size_of::<fat32_dirent_t>() as u32,
        );
        free_cluster_chain(fs, start_cluster);
        write_fat_to_disk(fs);
    } else {
        write_cluster_chain(
            fs,
            directory.cluster_id,
            dirents as *const u8,
            n_dirents * size_of::<fat32_dirent_t>() as u32,
        );
    }
    1
}

pub fn fat32_truncate(
    fs: &fat32_fs_t,
    directory: &pi_dirent_t,
    filename: &str,
    length: u32,
) -> i32 {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    if trace_enabled() {
        crate::println!("truncating {}", filename);
    }

    let mut n_dirents = 0u32;
    let dirents = get_dirents(fs, directory.cluster_id, &mut n_dirents);
    let idx = find_dirent_with_name(dirents, n_dirents, filename);
    if idx < 0 {
        return 0;
    }

    let d = unsafe { &mut *dirents.add(idx as usize) };
    let start_cluster = fat32_cluster_id(d);

    if length == 0 {
        d.file_nbytes = 0;
        let old_start = start_cluster;
        d.hi_start = 0;
        d.lo_start = 0;

        write_cluster_chain(
            fs,
            directory.cluster_id,
            dirents as *const u8,
            n_dirents * size_of::<fat32_dirent_t>() as u32,
        );
        if old_start >= 2 {
            free_cluster_chain(fs, old_start);
            write_fat_to_disk(fs);
        }
        return 1;
    }

    let f = fat32_read(fs, directory, filename);
    if f.is_null() {
        return 0;
    }
    let f_ref = unsafe { &*f };

    let mut new_data_ptr = f_ref.data;
    let mut new_alloc = f_ref.n_alloc;
    if length as usize > f_ref.n_data {
        let buf = unsafe { kmalloc::kmalloc(length as usize) };
        unsafe {
            copy_nonoverlapping(f_ref.data, buf, f_ref.n_data);
            ptr::write_bytes(buf.add(f_ref.n_data), 0, (length as usize) - f_ref.n_data);
        }
        new_data_ptr = buf;
        new_alloc = length as usize;
    }

    let new_file = pi_file_t {
        data: new_data_ptr,
        n_alloc: new_alloc,
        n_data: length as usize,
    };

    fat32_write(fs, directory, filename, &new_file)
}

pub fn fat32_write(
    fs: &fat32_fs_t,
    directory: &pi_dirent_t,
    filename: &str,
    file: &pi_file_t,
) -> i32 {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    demand(
        directory.is_dir_p != 0,
        "tried to use a file as a directory!",
    );

    let mut n_dirents = 0u32;
    let dirents = get_dirents(fs, directory.cluster_id, &mut n_dirents);
    let idx = find_dirent_with_name(dirents, n_dirents, filename);
    if idx < 0 {
        return 0;
    }

    let d = unsafe { &mut *dirents.add(idx as usize) };
    let mut start_cluster = fat32_cluster_id(d);

    if file.n_data == 0 {
        d.file_nbytes = 0;
        let old_start = start_cluster;
        d.hi_start = 0;
        d.lo_start = 0;

        write_cluster_chain(
            fs,
            directory.cluster_id,
            dirents as *const u8,
            n_dirents * size_of::<fat32_dirent_t>() as u32,
        );
        if old_start >= 2 {
            free_cluster_chain(fs, old_start);
            write_fat_to_disk(fs);
        }
        return 1;
    }

    if start_cluster < 2 {
        let new_cluster = find_free_cluster(fs, 3);
        d.hi_start = (new_cluster >> 16) as u16;
        d.lo_start = (new_cluster & 0xFFFF) as u16;
        start_cluster = new_cluster;
        unsafe {
            *fs.fat.add(start_cluster as usize) = LAST_CLUSTER;
        }
    }

    write_cluster_chain(
        fs,
        start_cluster,
        file.data as *const u8,
        file.n_data as u32,
    );
    d.file_nbytes = file.n_data as u32;
    write_cluster_chain(
        fs,
        directory.cluster_id,
        dirents as *const u8,
        n_dirents * size_of::<fat32_dirent_t>() as u32,
    );
    write_fat_to_disk(fs);
    1
}

pub fn fat32_flush(_fs: &fat32_fs_t) -> i32 {
    demand(unsafe { INIT_P }, "fat32 not initialized!");
    0
}
