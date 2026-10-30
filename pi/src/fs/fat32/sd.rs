#![allow(dead_code)]
use crate::crc;
use crate::fast_hash;
use crate::kmalloc;
use super::emmc;
use super::helpers;

static mut SD_TRACE_P: bool = false;
static mut SD_INIT_P: bool = false;

pub fn pi_sd_trace(on_p: bool) -> bool {
    let old = unsafe { SD_TRACE_P };
    unsafe { SD_TRACE_P = on_p; }
    old
}

pub(crate) fn trace_enabled() -> bool {
    unsafe { SD_TRACE_P }
}

pub fn pi_sd_init() -> i32 {
    if !emmc::emmc_init() {
        panic!("sd_init failed");
    }
    unsafe { SD_INIT_P = true; }
    1
}

pub fn pi_sd_read(data: *mut u8, lba: u32, nsec: u32) -> i32 {
    helpers::demand(unsafe { SD_INIT_P }, "SD card not initialized!\n");
    let res = emmc::emmc_read(lba, data, nsec * 512);
    if res != (512 * nsec) as i32 {
        panic!("could not read from sd card: result = {}", res);
    }
    if unsafe { SD_TRACE_P } {
        let cksum = unsafe { fast_hash::fast_hash(data, nsec * 512) };
        crate::println!("sd_read: lba=<{:x}>, cksum={:x}", lba, cksum);
    }
    1
}

pub fn pi_sec_read(lba: u32, nsec: u32) -> *mut u8 {
    helpers::demand(unsafe { SD_INIT_P }, "SD card not initialized!\n");
    let bytes = (nsec as usize) * 512;
    let data = unsafe { kmalloc::kmalloc(bytes) };
    if pi_sd_read(data, lba, nsec) != 1 {
        panic!("could not read from sd card");
    }
    data
}

pub fn pi_sd_write(data: *const u8, lba: u32, nsec: u32) -> i32 {
    helpers::demand(unsafe { SD_INIT_P }, "SD card not initialized!\n");
    let res = emmc::emmc_write(lba, data as *mut u8, nsec * 512);
    if res != (512 * nsec) as i32 {
        panic!("could not write to sd card: result = {}", res);
    }
    if unsafe { SD_TRACE_P } {
        let cksum = unsafe { crc::our_crc32(data, nsec * 512) };
        crate::println!("sd_write: lba=<{:x}>, cksum={:x}", lba, cksum);
    }
    1
}
