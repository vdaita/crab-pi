use crate::fat32::fs;
use crate::fat32;
use crate::kmalloc;
use crate::println;
use core::ffi::CStr;

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

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct Stat64 {
    pub st_dev: u64,
    pub __pad1: u32,

    pub __st_ino: u32,

    pub st_mode: u32,
    pub st_nlink: u32,

    pub st_uid: u32,
    pub st_gid: u32,

    pub st_rdev: u64,
    pub __pad2: u32,

    pub st_size: i64,
    pub st_blksize: u32,
    pub st_blocks: u64,

    pub st_atime: u32,
    pub st_atime_nsec: u32,

    pub st_mtime: u32,
    pub st_mtime_nsec: u32,

    pub st_ctime: u32,
    pub st_ctime_nsec: u32,

    pub st_ino: u64,
}

pub fn get_file_stat(dirent: &fat32::pi_dirent_t) -> Statx {
        let mut statx = Statx::default();
        
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

pub fn stat64_from_statx(stx: &Statx) -> Stat64 {
    let mut stat = Stat64::default();

    stat.st_dev =
        ((stx.stx_dev_major as u64) << 32) |
        (stx.stx_dev_minor as u64);

    stat.st_mode = stx.stx_mode as u32;
    stat.st_nlink = stx.stx_nlink;
    stat.st_uid = stx.stx_uid;
    stat.st_gid = stx.stx_gid;

    stat.st_rdev =
        ((stx.stx_rdev_major as u64) << 32) |
        (stx.stx_rdev_minor as u64);

    stat.st_size = stx.stx_size as i64;
    stat.st_blksize = stx.stx_blksize;
    stat.st_blocks = stx.stx_blocks;

    stat.st_atime = stx.stx_atime.tv_sec as u32;
    stat.st_atime_nsec = stx.stx_atime.tv_nsec;

    stat.st_mtime = stx.stx_mtime.tv_sec as u32;
    stat.st_mtime_nsec = stx.stx_mtime.tv_nsec;

    stat.st_ctime = stx.stx_ctime.tv_sec as u32;
    stat.st_ctime_nsec = stx.stx_ctime.tv_nsec;

    stat.st_ino = stx.stx_ino;
    stat.__st_ino = stx.stx_ino as u32;

    stat
}

pub fn get_file_stat64(dirent: &fat32::pi_dirent_t) -> Stat64 {
    stat64_from_statx(&get_file_stat(dirent))
}