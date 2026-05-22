use crate::fat32::fs;
use crate::fat32;
use crate::kmalloc;
use crate::println;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct StatxTimestamp {
    pub tv_sec: i64,
    pub tv_nsec: u32,
    pub __reserved: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct Statx {
    pub stx_mask: u32,
    pub stx_blksize: u32,
    pub stx_attributes: u64,

    pub stx_nlink: u32,
    pub stx_uid: u32,
    pub stx_gid: u32,
    pub stx_mode: u16,

    pub __spare0: [u16; 1],

    pub stx_ino: u64,
    pub stx_size: u64,
    pub stx_blocks: u64,
    pub stx_attributes_mask: u64,

    pub stx_atime: StatxTimestamp,
    pub stx_btime: StatxTimestamp,
    pub stx_ctime: StatxTimestamp,
    pub stx_mtime: StatxTimestamp,

    pub stx_rdev_major: u32,
    pub stx_rdev_minor: u32,
    pub stx_dev_major: u32,
    pub stx_dev_minor: u32,

    pub stx_mnt_id: u64,

    pub stx_dio_mem_align: u32,
    pub stx_dio_offset_align: u32,

    pub __spare3: [u64; 12],
}

const MAX_FDS: usize = 32;


#[derive(Copy, Clone, Default)]
pub struct FileDescriptor {
    pub cluster_id: u32,
    pub is_dir: bool,
    pub file_size: u32,
}

pub struct Fat32Manager {
    pub fs: fs::fat32_fs_t,
    pub root: fs::pi_dirent_t,
    pub fds: [Option<FileDescriptor>; MAX_FDS],
}

impl Fat32Manager {
    pub fn new() -> Fat32Manager {
        fat32::pi_sd_init();
        let partition = fat32::first_fat32_partition_from_mbr().expect("valid first FAT32 partition");
        let fs = fat32::fat32_mk(&partition);
        let root = fat32::fat32_get_root(&fs);
        
        return Fat32Manager {
            fs,
            root,
            fds: [None; MAX_FDS]
        }
    }

    pub fn read_file(&self, filename: &str) -> *mut fat32::pi_file_t {
        return fat32::fat32_read(&self.fs, &self.root, filename);
    }
     
    pub fn write_file_bytes(&self, filename: &str, data: *const u8, size: usize) -> i32 {
        let file = fs::pi_file_t {
            data: data as *mut u8,
            n_alloc: size,
            n_data: size,
        };
        
        let _ = fat32::fat32_delete(&self.fs, &self.root, filename);
        
        let file_ptr = fat32::fat32_create(&self.fs, &self.root, filename, 0);
        if file_ptr.is_null() {
            return -1;
        }
        
        let result = fat32::fat32_write(&self.fs, &self.root, filename, &file);
        match result {
            0 => 0, // success
            _ => -1, // error
        }
    }

    pub fn get_file_stat(&self, filename: &str) -> Statx {
        let mut statx = Statx::default();
        
        let stat_ptr = fat32::fat32_stat(&self.fs, &self.root, filename);
        if stat_ptr.is_null() {
            statx.stx_mask = 0;
            return statx;
        }
        
        let dirent = unsafe { &*stat_ptr };
        
        statx.stx_mask = 0x7ff;
        statx.stx_blksize = 512;
        statx.stx_attributes = 0;
        
        statx.stx_nlink = if dirent.is_dir_p != 0 { 2 } else { 1 };        
        statx.stx_uid = 0;
        statx.stx_gid = 0;
        
        // Mode: S_IFREG (0o100000) or S_IFDIR (0o40000)
        let mode = if dirent.is_dir_p != 0 {
            0o40755 // S_IFDIR | 0755
        } else {
            0o100755 // S_IFREG | 0755
        };
        statx.stx_mode = mode;
        
        statx.stx_ino = dirent.cluster_id as u64;
        
        statx.stx_size = dirent.nbytes as u64;
        statx.stx_blocks = (dirent.nbytes as u64 + 511) / 512;
        
        statx.stx_atime = StatxTimestamp::default();
        statx.stx_btime = StatxTimestamp::default();
        statx.stx_ctime = StatxTimestamp::default();
        statx.stx_mtime = StatxTimestamp::default();
        
        statx.stx_dev_major = 0;
        statx.stx_dev_minor = 1; // SD card
        statx.stx_rdev_major = 0;
        statx.stx_rdev_minor = 0;
        
        statx.stx_mnt_id = 0;
        statx.stx_dio_mem_align = 1;
        statx.stx_dio_offset_align = 1;
        
        statx
    }

    pub fn lsdir(&self) -> *const fs::pi_directory_t {
        let dir_ptr = unsafe { crate::kmalloc::kmalloc_t::<fs::pi_directory_t>(1) };
        if dir_ptr.is_null() {
            return core::ptr::null();
        }
        
        let dir = fat32::fat32_readdir(&self.fs, &self.root);
        unsafe {
            *dir_ptr = dir;
        }
        
        dir_ptr as *const fs::pi_directory_t
    }
}

static mut manager: *mut Fat32Manager = core::ptr::null_mut();

pub unsafe fn get_fat32_manager() -> *mut Fat32Manager {
    if manager.is_null() {
        manager = kmalloc::kmalloc(size_of::<Fat32Manager>()) as *mut Fat32Manager;
        *manager = Fat32Manager::new();
        println!("Created new manager");
    }

    println!("Getting fat32 manager");

    manager.as_mut().unwrap() as *mut _
}