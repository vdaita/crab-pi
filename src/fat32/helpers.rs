#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use core::mem::size_of;

pub fn demand(cond: bool, msg: &str) {
    assert!(cond, "{}", msg);
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct fat32_boot_sec_t {
    pub asm_code: [u8; 3],
    pub oem: [u8; 8],
    pub bytes_per_sec: u16,
    pub sec_per_cluster: u8,
    pub reserved_area_nsec: u16,
    pub nfats: u8,
    pub max_files: u16,
    pub fs_nsec: u16,
    pub media_type: u8,
    pub zero: u16,
    pub sec_per_track: u16,
    pub n_heads: u16,
    pub hidden_secs: u32,
    pub nsec_in_fs: u32,
    pub nsec_per_fat: u32,
    pub mirror_flags: u16,
    pub version: u16,
    pub first_cluster: u32,
    pub info_sec_num: u16,
    pub backup_boot_loc: u16,
    pub reserved: [u8; 12],
    pub logical_drive_num: u8,
    pub reserved1: u8,
    pub extended_sig: u8,
    pub serial_num: u32,
    pub volume_label: [u8; 11],
    pub fs_type: [u8; 8],
    pub ignore: [u8; 420],
    pub sig: u16,
}

impl fat32_boot_sec_t {
    pub const fn zeroed() -> Self {
        Self {
            asm_code: [0; 3],
            oem: [0; 8],
            bytes_per_sec: 0,
            sec_per_cluster: 0,
            reserved_area_nsec: 0,
            nfats: 0,
            max_files: 0,
            fs_nsec: 0,
            media_type: 0,
            zero: 0,
            sec_per_track: 0,
            n_heads: 0,
            hidden_secs: 0,
            nsec_in_fs: 0,
            nsec_per_fat: 0,
            mirror_flags: 0,
            version: 0,
            first_cluster: 0,
            info_sec_num: 0,
            backup_boot_loc: 0,
            reserved: [0; 12],
            logical_drive_num: 0,
            reserved1: 0,
            extended_sig: 0,
            serial_num: 0,
            volume_label: [0; 11],
            fs_type: [0; 8],
            ignore: [0; 420],
            sig: 0,
        }
    }
}

const _: [(); 512] = [(); size_of::<fat32_boot_sec_t>()];

#[repr(C)]
#[derive(Clone, Copy)]
pub struct fsinfo {
    pub sig1: u32,
    pub _reserved0: [u8; 480],
    pub sig2: u32,
    pub free_cluster_count: u32,
    pub next_free_cluster: u32,
    pub _reserved1: [u8; 12],
    pub sig3: u32,
}

const _: [(); 512] = [(); size_of::<fsinfo>()];

#[repr(C)]
#[derive(Clone, Copy)]
pub struct mbr_partition_ent_t {
    pub status: u8,
    pub chs_first: [u8; 3],
    pub partition_type: u8,
    pub chs_last: [u8; 3],
    pub lba_start: u32,
    pub nsectors: u32,
}

pub const FAT32_RO: u8 = 0x01;
pub const FAT32_HIDDEN: u8 = 0x02;
pub const FAT32_SYSTEM_FILE: u8 = 0x04;
pub const FAT32_VOLUME_LABEL: u8 = 0x08;
pub const FAT32_LONG_FILE_NAME: u8 = 0x0f;
pub const FAT32_DIR: u8 = 0x10;
pub const FAT32_ARCHIVE: u8 = 0x20;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct fat32_dirent_t {
    pub filename: [u8; 11],
    pub attr: u8,
    pub reserved0: u8,
    pub ctime_tenths: u8,
    pub ctime: u16,
    pub create_date: u16,
    pub access_date: u16,
    pub hi_start: u16,
    pub mod_time: u16,
    pub mod_date: u16,
    pub lo_start: u16,
    pub file_nbytes: u32,
}

const _: [(); 32] = [(); size_of::<fat32_dirent_t>()];

pub const NDIR_PER_SEC: usize = 512 / size_of::<fat32_dirent_t>();
const _: [(); 512] = [(); size_of::<fat32_dirent_t>() * NDIR_PER_SEC];

#[repr(C)]
#[derive(Clone, Copy)]
pub struct lfn_dir_t {
    pub seqno: u8,
    pub name1_5: [u8; 10],
    pub attr: u8,
    pub reserved: u8,
    pub cksum: u8,
    pub name6_11: [u8; 12],
    pub reserved1: u16,
    pub name12_13: [u8; 4],
}

const _: [(); 32] = [(); size_of::<lfn_dir_t>()];

pub const FREE_CLUSTER: u32 = 0;
pub const RESERVED_CLUSTER: u32 = 0x1;
pub const BAD_CLUSTER: u32 = 0x0fff_fff7;
pub const LAST_CLUSTER: u32 = 0x0fff_fff8;
pub const USED_CLUSTER: u32 = 0x0fff_fff9;

#[inline]
pub fn fat32_cluster_id(d: &fat32_dirent_t) -> u32 {
    ((d.hi_start as u32) << 16) | d.lo_start as u32
}

#[inline]
pub fn fat32_is_dir(d: &fat32_dirent_t) -> bool {
    d.attr == FAT32_DIR
}

#[inline]
pub fn fat32_is_attr(x: u8, flag: u8) -> bool {
    if x == FAT32_LONG_FILE_NAME {
        x == flag
    } else {
        (x & flag) == flag
    }
}

#[inline]
pub fn fat32_dirent_is_lfn(d: &fat32_dirent_t) -> bool {
    d.attr == FAT32_LONG_FILE_NAME
}

pub fn fat32_volume_id_check(b: &fat32_boot_sec_t) {
    assert!(b.bytes_per_sec == 512);
    assert!(b.nfats == 2);
    assert!(b.sig == 0xAA55);

    assert!(b.sec_per_cluster != 0 && b.sec_per_cluster.is_power_of_two());
    let n = b.bytes_per_sec;
    assert!(n == 512 || n == 1024 || n == 2048 || n == 4096);
    assert!(b.max_files == 0);
    assert!(b.fs_nsec == 0);
    assert!(b.zero == 0);
    assert!(b.nsec_in_fs != 0);

    assert!(b.info_sec_num == 1);
    assert!(b.backup_boot_loc == 6);
    assert!(b.extended_sig == 0x29);
}

fn ascii_copy(bytes: &[u8], out: &mut [u8]) {
    let mut i = 0;
    while i < bytes.len() && i + 1 < out.len() {
        let b = bytes[i];
        out[i] = if (0x20..=0x7e).contains(&b) { b } else { b'.' };
        i += 1;
    }
    if !out.is_empty() {
        out[out.len() - 1] = 0;
    }
}

fn cstr_len(bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0 {
            return i;
        }
        i += 1;
    }
    bytes.len()
}

pub fn fat32_volume_id_print(msg: &str, b: &fat32_boot_sec_t) {
    let mut oem = [0u8; 9];
    let mut label = [0u8; 12];
    let mut fstype = [0u8; 9];
    ascii_copy(&b.oem, &mut oem);
    ascii_copy(&b.volume_label, &mut label);
    ascii_copy(&b.fs_type, &mut fstype);

    let bytes_per_sec = b.bytes_per_sec;
    let sec_per_cluster = b.sec_per_cluster;
    let reserved_area_nsec = b.reserved_area_nsec;
    let nfats = b.nfats;
    let max_files = b.max_files;
    let fs_nsec = b.fs_nsec;
    let media_type = b.media_type;
    let sec_per_track = b.sec_per_track;
    let n_heads = b.n_heads;
    let hidden_secs = b.hidden_secs;
    let nsec_in_fs = b.nsec_in_fs;
    let nsec_per_fat = b.nsec_per_fat;
    let mirror_flags = b.mirror_flags;
    let version = b.version;
    let first_cluster = b.first_cluster;
    let info_sec_num = b.info_sec_num;
    let backup_boot_loc = b.backup_boot_loc;
    let logical_drive_num = b.logical_drive_num;
    let extended_sig = b.extended_sig;
    let serial_num = b.serial_num;
    let sig = b.sig;

    crate::println!("{}:", msg);
    crate::println!("\toem                = <{}>", core::str::from_utf8(&oem[..cstr_len(&oem)]).unwrap_or("?"));
    crate::println!("\tbytes_per_sec      = {}", bytes_per_sec);
    crate::println!("\tsec_per_cluster    = {}", sec_per_cluster);
    crate::println!("\treserved size      = {}", reserved_area_nsec);
    crate::println!("\tnfats              = {}", nfats);
    crate::println!("\tmax_files          = {}", max_files);
    crate::println!("\tfs n sectors       = {}", fs_nsec);
    crate::println!("\tmedia type         = {:#x}", media_type);
    crate::println!("\tsec per track      = {}", sec_per_track);
    crate::println!("\tn heads            = {}", n_heads);
    crate::println!("\tn hidden secs      = {}", hidden_secs);
    crate::println!("\tn nsec in FS       = {}", nsec_in_fs);
    crate::println!("\tn nsec per fat     = {}", nsec_per_fat);
    crate::println!("\tn mirror flags     = {:#x}", mirror_flags);
    crate::println!("\tn version          = {}", version);
    crate::println!("\tn first_cluster    = {}", first_cluster);
    crate::println!("\tn info_sec_num     = {}", info_sec_num);
    crate::println!("\tn back_boot_loc    = {}", backup_boot_loc);
    crate::println!("\tn logical_drive_num= {}", logical_drive_num);
    crate::println!("\tn extended sig     = {:#x}", extended_sig);
    crate::println!("\tn serial_num       = {:#x}", serial_num);
    crate::println!("\tn volume label     = <{}>", core::str::from_utf8(&label[..cstr_len(&label)]).unwrap_or("?"));
    crate::println!("\tn fs_type          = <{}>", core::str::from_utf8(&fstype[..cstr_len(&fstype)]).unwrap_or("?"));
    crate::println!("\tn sig              = {:#x}", sig);
}

pub fn fat32_fsinfo_print(msg: &str, f: &fsinfo) {
    crate::println!("{}:", msg);
    crate::println!("\tsig1              = {:#x}", f.sig1);
    crate::println!("\tsig2              = {:#x}", f.sig2);
    crate::println!("\tsig3              = {:#x}", f.sig3);
    crate::println!("\tfree cluster cnt  = {}", f.free_cluster_count);
    crate::println!("\tnext free cluster = {:#x}", f.next_free_cluster);
}

pub fn fat32_fsinfo_check(info: &fsinfo) {
    assert!(info.sig1 == 0x4161_5252);
    assert!(info.sig2 == 0x6141_7272);
    assert!(info.sig3 == 0xaa55_0000);
}

pub fn fat32_fat_entry_type_str(x: u32) -> &'static str {
    match x {
        FREE_CLUSTER => "FREE_CLUSTER",
        RESERVED_CLUSTER => "RESERVED_CLUSTER",
        BAD_CLUSTER => "BAD_CLUSTER",
        LAST_CLUSTER => "LAST_CLUSTER",
        USED_CLUSTER => "USED_CLUSTER",
        _ => panic!("bad FAT entry type value: {:#x}", x),
    }
}

pub fn fat32_fat_entry_type(mut x: u32) -> u32 {
    x &= 0x0fff_ffff;
    match x {
        FREE_CLUSTER | RESERVED_CLUSTER | BAD_CLUSTER => return x,
        _ => {}
    }
    if (0x2..=0x0fff_ffef).contains(&x) {
        return USED_CLUSTER;
    }
    if (0x0fff_fff0..=0x0fff_fff6).contains(&x) {
        panic!("reserved FAT value: {:#x}", x);
    }
    if (0x0fff_fff8..=0x0fff_ffff).contains(&x) {
        return LAST_CLUSTER;
    }
    panic!("impossible FAT type value: {:#x}", x);
}

pub fn fat32_dirent_is_deleted_lfn(d: &fat32_dirent_t) -> bool {
    d.filename[0] == 0xe5
}

pub fn fat32_dirent_free(d: &fat32_dirent_t) -> bool {
    let x = d.filename[0];
    if d.attr == FAT32_LONG_FILE_NAME {
        return fat32_dirent_is_deleted_lfn(d);
    }
    x == 0 || x == 0xe5
}

pub fn fat32_dir_attr_str(attr: u8) -> &'static str {
    if attr == FAT32_LONG_FILE_NAME {
        return "LONG FILE NAME";
    }
    if attr == FAT32_SYSTEM_FILE {
        return "SYSTEM FILE";
    }
    if attr == FAT32_VOLUME_LABEL {
        return "VOLUME LABEL";
    }
    if attr == FAT32_DIR {
        return "DIR";
    }
    if attr == FAT32_ARCHIVE {
        return "ARCHIVE";
    }
    "UNKNOWN"
}

fn parse_name_8dot3(name: &str) -> Option<([u8; 8], [u8; 3])> {
    if name.is_empty() {
        return None;
    }

    let bytes = name.as_bytes();
    let mut dot = None;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'.' {
            if dot.is_some() {
                return None;
            }
            dot = Some(i);
        }
    }

    let (stem, ext) = if let Some(di) = dot {
        (&bytes[..di], &bytes[di + 1..])
    } else {
        (bytes, &[][..])
    };

    if stem.is_empty() || stem.len() > 8 || ext.len() > 3 {
        return None;
    }

    fn valid(c: u8) -> bool {
        c.is_ascii_uppercase() || c.is_ascii_digit()
    }

    if !stem.iter().all(|b| valid(*b)) || !ext.iter().all(|b| valid(*b)) {
        return None;
    }

    let mut stem_out = [b' '; 8];
    let mut ext_out = [b' '; 3];
    stem_out[..stem.len()].copy_from_slice(stem);
    if !ext.is_empty() {
        ext_out[..ext.len()].copy_from_slice(ext);
    }
    Some((stem_out, ext_out))
}

pub fn fat32_is_valid_name(name: &str) -> bool {
    parse_name_8dot3(name).is_some()
}

pub fn fat32_dirent_set_name(d: &mut fat32_dirent_t, name: &str) {
    let (stem, ext) = parse_name_8dot3(name).expect("invalid FAT32 8.3 name");
    d.filename[..8].copy_from_slice(&stem);
    d.filename[8..11].copy_from_slice(&ext);
}

pub fn fat32_dirent_name(d: &fat32_dirent_t, name_out: &mut [u8; 13]) -> usize {
    let mut stem_len = 8;
    while stem_len > 0 && d.filename[stem_len - 1] == b' ' {
        stem_len -= 1;
    }

    let mut ext_len = 3;
    while ext_len > 0 && d.filename[8 + ext_len - 1] == b' ' {
        ext_len -= 1;
    }

    let mut i = 0usize;
    name_out.fill(0);

    for j in 0..stem_len {
        if i + 1 >= name_out.len() {
            break;
        }
        name_out[i] = d.filename[j];
        i += 1;
    }

    if ext_len > 0 && i + 1 < name_out.len() {
        name_out[i] = b'.';
        i += 1;
        for j in 0..ext_len {
            if i + 1 >= name_out.len() {
                break;
            }
            name_out[i] = d.filename[8 + j];
            i += 1;
        }
    }

    name_out[i] = 0;
    i + 1
}

pub fn fat32_dir_lookup(raw_name: &[u8; 11], dirs: *const fat32_dirent_t, n: usize) -> isize {
    for i in 0..n {
        let d = unsafe { &*dirs.add(i) };
        if fat32_dirent_free(d) || fat32_dirent_is_lfn(d) {
            continue;
        }
        if d.filename == *raw_name {
            return i as isize;
        }
    }
    -1
}

pub fn fat32_dirent_print_helper(d: &fat32_dirent_t) {
    if fat32_dirent_free(d) {
        crate::println!("\tdirent is not allocated");
        return;
    }
    if d.attr == FAT32_LONG_FILE_NAME {
        crate::println!("\tdirent is an LFN");
        return;
    }

    let mut pretty = [0u8; 13];
    let n = fat32_dirent_name(d, &mut pretty);
    let name_len = n.saturating_sub(1);
    let name = core::str::from_utf8(&pretty[..name_len]).unwrap_or("?");
    crate::println!("\tfilename      = <{}>", name);
    crate::println!("\tattr          = {:#x} ({})", d.attr, fat32_dir_attr_str(d.attr));
    crate::println!("\thi_start      = {:#x}", d.hi_start);
    crate::println!("\tlo_start      = {:#x}", d.lo_start);
    crate::println!("\tfile_nbytes   = {}", d.file_nbytes);
}

pub fn fat32_dirent_print(msg: &str, d: &fat32_dirent_t) {
    crate::println!("{}:", msg);
    fat32_dirent_print_helper(d);
}

pub fn fat32_lfn_print(_msg: &str, _d: &fat32_dirent_t, _left: i32) -> i32 {
    0
}

pub fn print_as_string(msg: &str, p: *const u8, n: usize) {
    crate::println!("{}", msg);
    for i in 0..n {
        let c = unsafe { *p.add(i) };
        crate::print!("{}", c as char);
    }
    crate::println!("");
}

pub fn print_bytes(msg: &str, p: *const u8, n: usize) {
    crate::println!("{}", msg);
    for i in 0..n {
        if i % 16 == 0 {
            crate::print!("\n\t");
        }
        let b = unsafe { *p.add(i) };
        crate::print!("{:02x}, ", b);
    }
    crate::println!("");
}

pub fn print_words(msg: &str, p: *const u32, n: usize) {
    crate::println!("{}", msg);
    for i in 0..n {
        if i % 16 == 0 {
            crate::print!("\n\t");
        }
        let w = unsafe { *p.add(i) };
        crate::print!("0x{:08x}, ", w);
    }
    crate::println!("");
}

