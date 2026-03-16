use crate::fat32::{self, mbr_partition_ent_t};
use crate::println;

fn parse_partition_entry(mbr: *const u8, idx: usize) -> mbr_partition_ent_t {
    let base = 446 + idx * 16;

    let status = unsafe { *mbr.add(base) };
    let chs_first = [
        unsafe { *mbr.add(base + 1) },
        unsafe { *mbr.add(base + 2) },
        unsafe { *mbr.add(base + 3) },
    ];
    let partition_type = unsafe { *mbr.add(base + 4) };
    let chs_last = [
        unsafe { *mbr.add(base + 5) },
        unsafe { *mbr.add(base + 6) },
        unsafe { *mbr.add(base + 7) },
    ];

    let lba_start = u32::from_le_bytes([
        unsafe { *mbr.add(base + 8) },
        unsafe { *mbr.add(base + 9) },
        unsafe { *mbr.add(base + 10) },
        unsafe { *mbr.add(base + 11) },
    ]);
    let nsectors = u32::from_le_bytes([
        unsafe { *mbr.add(base + 12) },
        unsafe { *mbr.add(base + 13) },
        unsafe { *mbr.add(base + 14) },
        unsafe { *mbr.add(base + 15) },
    ]);

    mbr_partition_ent_t {
        status,
        chs_first,
        partition_type,
        chs_last,
        lba_start,
        nsectors,
    }
}

fn first_fat32_partition_from_mbr() -> Option<mbr_partition_ent_t> {
    let mbr = fat32::pi_sec_read(0, 1) as *const u8;

    let sig_lo = unsafe { *mbr.add(510) };
    let sig_hi = unsafe { *mbr.add(511) };
    if sig_lo != 0x55 || sig_hi != 0xAA {
        return None;
    }

    for i in 0..4 {
        let p = parse_partition_entry(mbr, i);
        if p.partition_type == 0x0B || p.partition_type == 0x0C {
            return Some(p);
        }
    }

    None
}

pub fn fat32_test() {
    fat32::pi_sd_init();

    println!("Reading the MBR.");
    let partition = first_fat32_partition_from_mbr().unwrap_or(mbr_partition_ent_t {
        status: 0,
        chs_first: [0; 3],
        partition_type: 0x0C,
        chs_last: [0; 3],
        lba_start: 0,
        nsectors: 0,
    });

    println!("Loading the FAT.");
    let fs = fat32::fat32_mk(&partition);

    println!("Loading the root directory.");
    let root = fat32::fat32_get_root(&fs);
    let _dir = fat32::fat32_readdir(&fs, &root);

    println!("Creating TEMP.TXT");
    let _ = fat32::fat32_delete(&fs, &root, "TEMP.TXT");
    let created = fat32::fat32_create(&fs, &root, "TEMP.TXT", 0);
    assert!(!created.is_null());

    println!("PASS: {}", file!());
}