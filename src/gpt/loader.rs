#![allow(dead_code)]

use crate::fat32::{self, mbr_partition_ent_t, pi_dirent_t};

use super::model::Tokenizer;

pub struct LoadedModelFiles {
    pub tokenizer: Tokenizer,
    pub weights: &'static [u8],
    pub weights_len: usize,
    pub tokenizer_len: usize,
    pub seed: u32,
}

const SD_WEIGHTS_NAME: &str = "GPTW.BIN";
const SD_TOKENIZER_NAME: &str = "GPTTOK.TXT";

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

fn dirent_name_ascii(d: &pi_dirent_t, out: &mut [u8; 16]) -> usize {
    let mut n = 0usize;
    while n < d.name.len() && d.name[n] != 0 {
        out[n] = d.name[n];
        n += 1;
    }
    n
}

fn ascii_upper(c: u8) -> u8 {
    if c >= b'a' && c <= b'z' {
        c - 32
    } else {
        c
    }
}

fn contains_upper_ascii(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > hay.len() {
        return false;
    }
    for i in 0..=(hay.len() - needle.len()) {
        let mut ok = true;
        for j in 0..needle.len() {
            if ascii_upper(hay[i + j]) != needle[j] {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

fn ends_with_upper_ascii(hay: &[u8], suffix: &[u8]) -> bool {
    if suffix.len() > hay.len() {
        return false;
    }
    let off = hay.len() - suffix.len();
    for i in 0..suffix.len() {
        if ascii_upper(hay[off + i]) != suffix[i] {
            return false;
        }
    }
    true
}

fn find_file_name(root_dir: *const pi_dirent_t, n: usize, want_tokenizer: bool, out_name: &mut [u8; 16]) -> Option<usize> {
    let mut fallback = None;

    for i in 0..n {
        let d = unsafe { &*root_dir.add(i) };
        if d.is_dir_p != 0 {
            continue;
        }

        let mut name_buf = [0u8; 16];
        let len = dirent_name_ascii(d, &mut name_buf);
        if len == 0 {
            continue;
        }
        let name = &name_buf[..len];

        if want_tokenizer {
            if ends_with_upper_ascii(name, b".TXT") {
                if contains_upper_ascii(name, b"TOKEN") || contains_upper_ascii(name, b"VOCAB") {
                    out_name[..len].copy_from_slice(name);
                    if len < out_name.len() {
                        out_name[len] = 0;
                    }
                    return Some(len);
                }
                if fallback.is_none() {
                    fallback = Some((len, name_buf));
                }
            }
        } else if ends_with_upper_ascii(name, b".BIN") {
            if contains_upper_ascii(name, b"GPT") || contains_upper_ascii(name, b"WEIGHT") {
                out_name[..len].copy_from_slice(name);
                if len < out_name.len() {
                    out_name[len] = 0;
                }
                return Some(len);
            }
            if fallback.is_none() {
                fallback = Some((len, name_buf));
            }
        }
    }

    if let Some((len, bytes)) = fallback {
        out_name[..len].copy_from_slice(&bytes[..len]);
        if len < out_name.len() {
            out_name[len] = 0;
        }
        Some(len)
    } else {
        None
    }
}

fn find_exact_file_name(root_dir: *const pi_dirent_t, n: usize, exact: &str, out_name: &mut [u8; 16]) -> Option<usize> {
    let e = exact.as_bytes();
    for i in 0..n {
        let d = unsafe { &*root_dir.add(i) };
        if d.is_dir_p != 0 {
            continue;
        }
        let mut name_buf = [0u8; 16];
        let len = dirent_name_ascii(d, &mut name_buf);
        if len != e.len() {
            continue;
        }
        let mut match_p = true;
        for j in 0..len {
            if ascii_upper(name_buf[j]) != ascii_upper(e[j]) {
                match_p = false;
                break;
            }
        }
        if match_p {
            out_name[..len].copy_from_slice(&name_buf[..len]);
            if len < out_name.len() {
                out_name[len] = 0;
            }
            return Some(len);
        }
    }
    None
}

fn find_dirent_by_name(root_dir: *const pi_dirent_t, n: usize, name: &[u8]) -> Option<pi_dirent_t> {
    for i in 0..n {
        let d = unsafe { &*root_dir.add(i) };
        if d.is_dir_p != 0 {
            continue;
        }
        let mut name_buf = [0u8; 16];
        let len = dirent_name_ascii(d, &mut name_buf);
        if len == name.len() && &name_buf[..len] == name {
            return Some(*d);
        }
    }
    None
}

fn read_file_bytes_from_dirent(fs: &fat32::fat32_fs_t, d: &pi_dirent_t) -> Option<&'static [u8]> {
    let f = fat32::fat32_read_from_dirent(fs, d);
    if f.is_null() {
        return None;
    }
    let f_ref = unsafe { &*f };
    if f_ref.n_data == 0 {
        return Some(&[]);
    }
    if f_ref.data.is_null() {
        return None;
    }
    let out = unsafe { core::slice::from_raw_parts(f_ref.data as *const u8, f_ref.n_data) };
    Some(out)
}

fn seed_from_bytes(data: &[u8]) -> u32 {
    let mut s = 0x811c9dc5u32;
    for &b in data.iter() {
        s ^= b as u32;
        s = s.wrapping_mul(0x01000193);
    }
    s
}

pub fn load_from_fat32() -> Option<LoadedModelFiles> {
    // Trace logging hashes every SD read/write and can dominate load latency.
    fat32::pi_sd_trace(false);
    crate::println!("[gpt.loader] init SD + FAT32");
    fat32::pi_sd_init();

    let partition = first_fat32_partition_from_mbr()?;
    crate::println!(
        "[gpt.loader] FAT32 partition: lba_start={}, nsectors={}",
        partition.lba_start,
        partition.nsectors
    );
    let fs = fat32::fat32_mk(&partition);
    let root = fat32::fat32_get_root(&fs);
    let dir = fat32::fat32_readdir(&fs, &root);

    if dir.dirents.is_null() || dir.ndirents == 0 {
        crate::println!("[gpt.loader] root directory empty or unavailable");
        return None;
    }
    crate::println!("[gpt.loader] root entries: {}", dir.ndirents);

    let mut w_name = [0u8; 16];
    let mut t_name = [0u8; 16];
    let w_len = find_exact_file_name(dir.dirents, dir.ndirents, SD_WEIGHTS_NAME, &mut w_name)
        .or_else(|| find_file_name(dir.dirents, dir.ndirents, false, &mut w_name))?;
    let t_len = find_exact_file_name(dir.dirents, dir.ndirents, SD_TOKENIZER_NAME, &mut t_name)
        .or_else(|| find_file_name(dir.dirents, dir.ndirents, true, &mut t_name))?;

    let w_dirent = find_dirent_by_name(dir.dirents, dir.ndirents, &w_name[..w_len])?;
    let t_dirent = find_dirent_by_name(dir.dirents, dir.ndirents, &t_name[..t_len])?;

    if let Ok(w_name_s) = core::str::from_utf8(&w_name[..w_len]) {
        crate::println!("[gpt.loader] weights file: {}", w_name_s);
    }
    if let Ok(t_name_s) = core::str::from_utf8(&t_name[..t_len]) {
        crate::println!("[gpt.loader] tokenizer file: {}", t_name_s);
    }

    crate::println!("[gpt.loader] streaming weights clusters...");
    fat32::fat32_read_progress_every(64);
    fat32::fat32_read_progress(true);
    let weights = read_file_bytes_from_dirent(&fs, &w_dirent)?;
    fat32::fat32_read_progress(false);

    crate::println!("[gpt.loader] streaming tokenizer clusters...");
    fat32::fat32_read_progress_every(16);
    fat32::fat32_read_progress(true);
    let tok_bytes = read_file_bytes_from_dirent(&fs, &t_dirent)?;
    fat32::fat32_read_progress(false);

    crate::println!(
        "[gpt.loader] file sizes: weights={} bytes, tokenizer={} bytes",
        weights.len(),
        tok_bytes.len()
    );

    let tokenizer = Tokenizer::from_tokenizer_txt(tok_bytes)?;
    let seed = seed_from_bytes(weights);
    crate::println!("[gpt.loader] tokenizer entries loaded: {}", tokenizer.len());
    crate::println!("[gpt.loader] checksum seed: 0x{:08x}", seed);

    Some(LoadedModelFiles {
        tokenizer,
        weights,
        weights_len: weights.len(),
        tokenizer_len: tok_bytes.len(),
        seed,
    })
}
